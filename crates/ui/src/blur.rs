//! Desenfoque del fondo detrás de la ventana con `ext_background_effect_v1` (GNOME 51+),
//! como en plane-llama. El compositor decide cuánto desenfoca; la app solo marca el área.
//
//!
//! winit no expone este protocolo, así que se usa la conexión Wayland de la ventana
//! (`wl_display` y `wl_surface` de raw-window-handle) desde una cola de eventos propia.
//! El área desenfocada es estado de la superficie y se aplica en el próximo commit, que
//! hace el renderer al presentar el siguiente frame.
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::{Backend, ObjectId},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry, wl_surface::WlSurface,
    },
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, Capability, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

#[derive(Default)]
struct State {
    blur: bool,
}

pub struct Blur {
    conn: Connection,
    queue: EventQueue<State>,
    state: State,
    compositor: WlCompositor,
    effect: ExtBackgroundEffectSurfaceV1,
    /// Última área aplicada (rectángulos en coordenadas lógicas), para no repetirla.
    applied: Option<Vec<[i32; 4]>>,
}

impl Blur {
    /// Prepara el efecto sobre la ventana; `None` fuera de Wayland o sin el protocolo.
    pub fn new(window: &(impl HasWindowHandle + HasDisplayHandle)) -> Option<Self> {
        let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
            return None;
        };
        let RawWindowHandle::Wayland(surface) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: el display y la superficie son de la ventana viva de winit, que los
        // mantiene mientras exista el contexto; `Blur` se descarta antes que la ventana.
        let conn = Connection::from_backend(unsafe {
            Backend::from_foreign_display(display.display.as_ptr().cast())
        });
        let surface =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) }
                .ok()
                .and_then(|id| WlSurface::from_id(&conn, id).ok())?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 1..=4, ()).ok()?;
        let manager: ExtBackgroundEffectManagerV1 = globals.bind(&qh, 1..=1, ()).ok()?;
        let effect = manager.get_background_effect(&surface, &qh, ());
        let mut state = State::default();
        // Las capacidades llegan nada más enlazar el global.
        queue.roundtrip(&mut state).ok()?;
        Some(Self {
            conn,
            queue,
            state,
            compositor,
            effect,
            applied: None,
        })
    }
    /// El compositor puede desenfocar ahora mismo.
    pub fn supported(&mut self) -> bool {
        let _ = self.queue.dispatch_pending(&mut self.state);
        self.state.blur
    }
    /// Desenfoca toda la ventana (con las esquinas del marco) o nada.
    pub fn cover(&mut self, ctx: &egui::Context, on: bool) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let zoom = ctx.zoom_factor();
        let rect = ctx.content_rect();
        let rect = egui::Rect::from_min_max(
            (rect.min.to_vec2() * zoom).to_pos2(),
            (rect.max.to_vec2() * zoom).to_pos2(),
        );
        let radius = if maximized {
            0.
        } else {
            crate::frame::RADIUS as f32 * zoom
        };
        self.set(on.then_some((rect, radius)));
    }
    /// Desenfoca el rectángulo redondeado (puntos lógicos) o lo quita con `None`.
    pub fn set(&mut self, area: Option<(egui::Rect, f32)>) {
        let rects = area.map(|(rect, radius)| region(rect, radius));
        if rects == self.applied {
            return;
        }
        match &rects {
            Some(rects) => {
                let qh = self.queue.handle();
                let region = self.compositor.create_region(&qh, ());
                for [x, y, w, h] in rects {
                    region.add(*x, *y, *w, *h);
                }
                self.effect.set_blur_region(Some(&region));
                region.destroy();
            }
            None => self.effect.set_blur_region(None),
        }
        let _ = self.conn.flush();
        self.applied = rects;
    }
}

/// Rectángulos que cubren el rectángulo redondeado: el centro entero y franjas de 2 px
/// en las esquinas que siguen la curva.
fn region(rect: egui::Rect, radius: f32) -> Vec<[i32; 4]> {
    let (x, y) = (rect.left().round() as i32, rect.top().round() as i32);
    let (w, h) = (rect.width().round() as i32, rect.height().round() as i32);
    let r = (radius.round() as i32).clamp(0, w.min(h) / 2);
    let mut rects = vec![[x, y + r, w, h - 2 * r]];
    let mut dy = 0;
    while dy < r {
        let band = 2.min(r - dy);
        let from_center = (r - dy - band) as f32;
        let inset = r - ((r * r) as f32 - from_center * from_center).max(0.).sqrt() as i32;
        rects.push([x + inset, y + dy, w - 2 * inset, band]);
        rects.push([x + inset, y + h - dy - band, w - 2 * inset, band]);
        dy += band;
    }
    rects
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
impl Dispatch<ExtBackgroundEffectManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities {
            flags: WEnum::Value(flags),
        } = event
        {
            state.blur = flags.contains(Capability::Blur);
        }
    }
}
wayland_client::delegate_noop!(State: ignore WlCompositor);
wayland_client::delegate_noop!(State: ignore WlRegion);
wayland_client::delegate_noop!(State: ignore ExtBackgroundEffectSurfaceV1);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn region_covers_rounded_rect() {
        let rects = region(
            egui::Rect::from_min_size(egui::pos2(14., 14.), egui::vec2(400., 300.)),
            12.,
        );
        assert_eq!(rects[0], [14, 26, 400, 276]);
        // Arriba y abajo, las franjas suman el radio y se estrechan hacia el borde.
        let top: Vec<_> = rects[1..].iter().step_by(2).collect();
        assert_eq!(top.iter().map(|r| r[3]).sum::<i32>(), 12);
        assert!(top[0][2] < top[top.len() - 1][2]);
        assert_eq!(top[0][1], 14);
        assert!(rects.iter().all(|r| r[0] >= 14 && r[0] + r[2] <= 414));
    }
}
