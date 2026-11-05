//! Lápiz de tableta en Wayland con `zwp_tablet_v2`.
//!
//! winit no implementa el protocolo y GNOME imita el lápiz como un ratón, pero esa imitación
//! se pierde al cambiar de ventana (Alt+Tab) y el lápiz deja de dibujar. Aquí se escucha el
//! lápiz directamente, como en `blur`, sobre la conexión de la ventana: un hilo propio
//! despacha sus eventos y los deja como eventos de puntero de egui para `raw_input_hook`.
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::sync::{Arc, Mutex};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::{Backend, ObjectId},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat::WlSeat, wl_surface::WlSurface},
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ButtonState, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};

/// Botones laterales del lápiz (`BTN_STYLUS`, `BTN_STYLUS2` de Linux).
const BTN_STYLUS: u32 = 0x14b;
const BTN_STYLUS2: u32 = 0x14c;

/// Lo que hizo el lápiz, en puntos lógicos de la superficie; los modificadores se ponen al
/// pasarlo a egui.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pen {
    Move(egui::Pos2),
    Button(egui::PointerButton, bool),
    Gone,
}

/// Cursor del lápiz sobre la ventana: el de egui, puesto al entrar y cuando cambia.
#[derive(Default)]
struct Cursor {
    // Dispositivo de la herramienta que está encima y el `serial` con que entró.
    device: Option<(WpCursorShapeDeviceV1, u32)>,
    shape: Option<Shape>,
}
impl Cursor {
    fn apply(&self) {
        if let Some((device, serial)) = &self.device {
            device.set_shape(*serial, self.shape.unwrap_or(Shape::Default));
        }
    }
}

struct State {
    surface: u32,
    ctx: egui::Context,
    events: Arc<Mutex<Vec<Pen>>>,
    shapes: Option<WpCursorShapeManagerV1>,
    // Por herramienta, su dispositivo de cursor; se crea al aparecer la herramienta.
    cursors: Vec<(ObjectId, WpCursorShapeDeviceV1)>,
    cursor: Arc<Mutex<Cursor>>,
    // Herramienta sobre la ventana.
    inside: Option<ObjectId>,
}

impl State {
    fn push(&mut self, pen: Pen) {
        if let Ok(mut events) = self.events.lock() {
            events.push(pen);
        }
        self.ctx.request_repaint();
    }
}

pub struct Tablet {
    conn: Connection,
    events: Arc<Mutex<Vec<Pen>>>,
    cursor: Arc<Mutex<Cursor>>,
    // Último sitio del lápiz, para los botones.
    at: Option<egui::Pos2>,
}

impl Tablet {
    /// Escucha el lápiz sobre la ventana; `None` fuera de Wayland o sin el protocolo.
    pub fn new(
        window: &(impl HasWindowHandle + HasDisplayHandle),
        ctx: &egui::Context,
    ) -> Option<Self> {
        let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
            return None;
        };
        let RawWindowHandle::Wayland(surface) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: como en `blur`: display y superficie son de la ventana viva de winit. El
        // hilo vive lo que el proceso, que termina al cerrar la ventana.
        let conn = Connection::from_backend(unsafe {
            Backend::from_foreign_display(display.display.as_ptr().cast())
        });
        let surface =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) }
                .ok()?
                .protocol_id();
        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=5, ()).ok()?;
        let manager: ZwpTabletManagerV2 = globals.bind(&qh, 1..=1, ()).ok()?;
        let shapes: Option<WpCursorShapeManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        manager.get_tablet_seat(&seat, &qh, ());
        let events = Arc::new(Mutex::new(Vec::new()));
        let cursor = Arc::new(Mutex::new(Cursor::default()));
        let mut state = State {
            surface,
            ctx: ctx.clone(),
            events: events.clone(),
            shapes,
            cursors: Vec::new(),
            cursor: cursor.clone(),
            inside: None,
        };
        std::thread::Builder::new()
            .name("tablet".into())
            .spawn(move || while queue.blocking_dispatch(&mut state).is_ok() {})
            .ok()?;
        Some(Self {
            conn,
            events,
            cursor,
            at: None,
        })
    }

    /// Al final de cada cuadro: el lápiz lleva el mismo cursor que pidió egui (la cruz del
    /// cuentagotas, la mano…). Sin esto GNOME lo deja sin cursor al rato.
    pub fn cursor(&self, ctx: &egui::Context) {
        let shape = shape(ctx.output(|o| o.cursor_icon));
        let Ok(mut cursor) = self.cursor.lock() else {
            return;
        };
        if cursor.shape != Some(shape) {
            cursor.shape = Some(shape);
            cursor.apply();
            let _ = self.conn.flush();
        }
    }

    /// Para `App::raw_input_hook`: añade lo que hizo el lápiz desde el último cuadro.
    pub fn feed(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Ok(mut events) = self.events.lock() else {
            return;
        };
        let zoom = ctx.zoom_factor();
        for pen in events.drain(..) {
            match pen {
                Pen::Move(pos) => {
                    let pos = pos / zoom;
                    self.at = Some(pos);
                    raw.events.push(egui::Event::PointerMoved(pos));
                }
                Pen::Button(button, pressed) => {
                    if let Some(pos) = self.at {
                        raw.events.push(egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers: ctx.input(|i| i.modifiers),
                        });
                    }
                }
                Pen::Gone => {
                    self.at = None;
                    raw.events.push(egui::Event::PointerGone);
                }
            }
        }
    }
}

/// Forma de `cursor-shape-v1` para el cursor de egui; las que no tiene, la flecha.
fn shape(icon: egui::CursorIcon) -> Shape {
    use egui::CursorIcon as C;
    match icon {
        C::PointingHand => Shape::Pointer,
        C::Crosshair => Shape::Crosshair,
        C::Text => Shape::Text,
        C::Move => Shape::Move,
        C::Grab => Shape::Grab,
        C::Grabbing => Shape::Grabbing,
        C::AllScroll => Shape::AllScroll,
        C::NotAllowed | C::NoDrop => Shape::NotAllowed,
        C::Wait => Shape::Wait,
        C::Progress => Shape::Progress,
        C::Help => Shape::Help,
        C::Cell => Shape::Cell,
        C::ResizeHorizontal | C::ResizeColumn => Shape::EwResize,
        C::ResizeVertical | C::ResizeRow => Shape::NsResize,
        C::ResizeNeSw => Shape::NeswResize,
        C::ResizeNwSe => Shape::NwseResize,
        C::ResizeEast => Shape::EResize,
        C::ResizeWest => Shape::WResize,
        C::ResizeNorth => Shape::NResize,
        C::ResizeSouth => Shape::SResize,
        C::ResizeNorthEast => Shape::NeResize,
        C::ResizeNorthWest => Shape::NwResize,
        C::ResizeSouthEast => Shape::SeResize,
        C::ResizeSouthWest => Shape::SwResize,
        C::ZoomIn => Shape::ZoomIn,
        C::ZoomOut => Shape::ZoomOut,
        _ => Shape::Default,
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for State {
    fn event(
        state: &mut Self,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use zwp_tablet_tool_v2::Event;
        let ours = state.inside.as_ref() == Some(&tool.id());
        match event {
            Event::ProximityIn {
                serial, surface, ..
            } => {
                if surface.id().protocol_id() != state.surface {
                    return;
                }
                state.inside = Some(tool.id());
                // Sin cursor propio el lápiz no muestra ninguno sobre la ventana.
                if let Some(shapes) = &state.shapes {
                    let id = tool.id();
                    if !state.cursors.iter().any(|(t, _)| *t == id) {
                        let device = shapes.get_tablet_tool_v2(tool, qh, ());
                        state.cursors.push((id.clone(), device));
                    }
                    if let Some((_, device)) = state.cursors.iter().find(|(t, _)| *t == id)
                        && let Ok(mut cursor) = state.cursor.lock()
                    {
                        cursor.device = Some((device.clone(), serial));
                        cursor.apply();
                    }
                }
            }
            Event::ProximityOut if ours => {
                state.inside = None;
                if let Ok(mut cursor) = state.cursor.lock() {
                    cursor.device = None;
                }
                state.push(Pen::Gone);
            }
            Event::Motion { x, y } if ours => {
                state.push(Pen::Move(egui::pos2(x as f32, y as f32)));
            }
            Event::Down { .. } if ours => {
                state.push(Pen::Button(egui::PointerButton::Primary, true));
            }
            Event::Up if ours => {
                state.push(Pen::Button(egui::PointerButton::Primary, false));
            }
            Event::Button {
                button,
                state: pressed,
                ..
            } if ours => {
                let button = match button {
                    BTN_STYLUS => egui::PointerButton::Secondary,
                    BTN_STYLUS2 => egui::PointerButton::Middle,
                    _ => return,
                };
                let pressed = pressed == wayland_client::WEnum::Value(ButtonState::Pressed);
                state.push(Pen::Button(button, pressed));
            }
            Event::Removed => {
                if ours {
                    state.inside = None;
                    if let Ok(mut cursor) = state.cursor.lock() {
                        cursor.device = None;
                    }
                    state.push(Pen::Gone);
                }
                let id = tool.id();
                state.cursors.retain(|(t, device)| {
                    let keep = *t != id;
                    if !keep {
                        device.destroy();
                    }
                    keep
                });
                tool.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletSeatV2,
        _: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    wayland_client::event_created_child!(State, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

// Las teclas de la tableta no se usan, pero sus objetos llegan igual.
impl Dispatch<ZwpTabletPadV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadV2,
        _: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    wayland_client::event_created_child!(State, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}
impl Dispatch<ZwpTabletPadGroupV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    wayland_client::event_created_child!(State, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
wayland_client::delegate_noop!(State: ignore WlSeat);
wayland_client::delegate_noop!(State: ignore ZwpTabletManagerV2);
wayland_client::delegate_noop!(State: ignore ZwpTabletV2);
wayland_client::delegate_noop!(State: ignore ZwpTabletPadRingV2);
wayland_client::delegate_noop!(State: ignore ZwpTabletPadStripV2);
wayland_client::delegate_noop!(State: WpCursorShapeManagerV1);
wayland_client::delegate_noop!(State: WpCursorShapeDeviceV1);
