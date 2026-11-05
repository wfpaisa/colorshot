//! Marco propio de ventana: una sola barra con las herramientas y los botones de GNOME,
//! borde con brillo interior, esquinas redondeadas y tiradores para redimensionar.
//!
//! La ventana va sin decoración y transparente (`clear_color` a cero); `round_window` redondea
//! todo lo que toca una esquina para que nada asome fuera de la curva.
use crate::theme::Gnome;
use egui::{
    CursorIcon, Rect, ResizeDirection, Sense, Stroke, StrokeKind, Ui, ViewportCommand as Cmd, pos2,
    vec2,
};

pub const RADIUS: u8 = 12;
const BUTTON: f32 = 22.;
const BUTTON_STEP: f32 = 30.;
const EDGE: f32 = 8.;
const CORNER: f32 = 16.;

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Botones de GNOME (minimizar, maximizar, cerrar): círculos grises pequeños con el
/// símbolo claro, alineados a la derecha de `right` y centrados en `y`. Devuelve dónde
/// empiezan, para que lo demás de la barra no los pise.
pub fn window_buttons(ui: &mut Ui, g: &Gnome, right: f32, y: f32, resizable: bool) -> f32 {
    let painter = ui.painter().clone();
    let buttons: &[(&str, Cmd)] = if resizable {
        &[
            ("Minimize", Cmd::Minimized(true)),
            ("Maximize", Cmd::Maximized(!maximized(ui.ctx()))),
            ("Close", Cmd::Close),
        ]
    } else {
        &[("Minimize", Cmd::Minimized(true)), ("Close", Cmd::Close)]
    };
    let count = buttons.len() as f32;
    for (i, (label, command)) in buttons.iter().enumerate() {
        let center = pos2(
            right - BUTTON / 2. - (count - 1. - i as f32) * BUTTON_STEP,
            y,
        );
        let response = ui.interact(
            Rect::from_center_size(center, vec2(BUTTON, BUTTON)),
            ui.id().with(*label),
            Sense::click(),
        );
        let fill = if response.is_pointer_button_down_on() {
            g.field_hover.gamma_multiply(1.3)
        } else if response.hovered() {
            g.field_hover
        } else {
            g.field
        };
        painter.circle_filled(center, BUTTON / 2., fill);
        let stroke = Stroke::new(1.5, g.text);
        match *label {
            "Minimize" => {
                painter.line_segment([center + vec2(-3.5, 2.5), center + vec2(3.5, 2.5)], stroke);
            }
            "Maximize" => {
                painter.rect_stroke(
                    Rect::from_center_size(center, vec2(6., 6.)),
                    1,
                    stroke,
                    StrokeKind::Inside,
                );
            }
            _ => {
                for sign in [-1., 1.] {
                    painter.line_segment(
                        [center + vec2(-3., -3. * sign), center + vec2(3., 3. * sign)],
                        stroke,
                    );
                }
            }
        }
        if response.clicked() {
            ui.ctx().send_viewport_cmd(command.clone());
        }
    }
    right - count * BUTTON_STEP + (BUTTON_STEP - BUTTON)
}

/// Marco propio: la barra de herramientas es la barra de título, con los botones de
/// GNOME a la derecha. Devuelve el hueco de la barra para las herramientas y el resto de la
/// ventana.
pub fn toolbar(ui: &mut Ui, g: &Gnome, height: f32, resizable: bool) -> (Rect, Rect) {
    let r = ui.ctx().content_rect();
    let radius = if maximized(ui.ctx()) { 0 } else { RADIUS };
    let painter = ui.painter().clone();
    painter.rect_filled(r, radius, g.window);
    let bar = Rect::from_min_size(r.min, vec2(r.width(), height));
    painter.rect_filled(
        bar,
        egui::CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        g.header,
    );
    // Toda la barra arrastra la ventana; los botones que se añadan después quedan encima.
    drag_area(ui, bar, resizable);
    let left = window_buttons(ui, g, r.right() - 12., bar.center().y, resizable);
    let tools = Rect::from_min_max(
        pos2(r.left() + 12., bar.top()),
        pos2(left - 12., bar.bottom()),
    );
    (
        tools,
        Rect::from_min_max(pos2(r.left(), bar.bottom()), r.max),
    )
}

/// Zona que mueve la ventana al arrastrarla y, si `resizable`, la maximiza con doble clic.
/// Lo que se añada después encima sigue recibiendo sus clics.
pub fn drag_area(ui: &mut Ui, rect: Rect, resizable: bool) {
    let drag = ui.interact(rect, ui.id().with("title-bar"), Sense::click_and_drag());
    if resizable && drag.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(Cmd::Maximized(!maximized(ui.ctx())));
    } else if drag.drag_started() {
        grab(ui.ctx(), Cmd::StartDrag);
    }
}

/// Brillo del borde por encima de todo y tiradores. Va al final del cuadro.
pub fn finish(ui: &mut Ui, g: &Gnome, resizable: bool) {
    let r = ui.ctx().content_rect();
    if !maximized(ui.ctx()) {
        ui.ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("frame-edge"),
            ))
            .rect_stroke(
                r.shrink(1.),
                RADIUS - 1,
                Stroke::new(1., g.highlight),
                StrokeKind::Inside,
            );
    }
    if resizable {
        resize(ui, r);
    }
    round_window(ui.ctx());
}

/// Tiradores para redimensionar desde los bordes y las esquinas de `r`.
pub fn resize(ui: &mut Ui, r: Rect) {
    if maximized(ui.ctx()) {
        return;
    }
    use CursorIcon as C;
    use ResizeDirection as D;
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(r.min + vec2(x, y), vec2(w, h));
    let (w, h, e, c) = (r.width(), r.height(), EDGE, CORNER);
    let ui = &mut ui.new_child(
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("resize"),
            ))
            .max_rect(r),
    );
    for (i, (area, direction, cursor)) in [
        (at(0., 0., c, c), D::NorthWest, C::ResizeNorthWest),
        (at(w - c, 0., c, c), D::NorthEast, C::ResizeNorthEast),
        (at(0., h - c, c, c), D::SouthWest, C::ResizeSouthWest),
        (at(w - c, h - c, c, c), D::SouthEast, C::ResizeSouthEast),
        (at(c, 0., w - 2. * c, e / 2.), D::North, C::ResizeNorth),
        (at(c, h - e, w - 2. * c, e), D::South, C::ResizeSouth),
        (at(0., c, e, h - 2. * c), D::West, C::ResizeWest),
        (at(w - e, c, e, h - 2. * c), D::East, C::ResizeEast),
    ]
    .into_iter()
    .enumerate()
    {
        let response = ui
            .interact(area, ui.id().with(("resize", i)), Sense::drag())
            .on_hover_cursor(cursor);
        if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            grab(ui.ctx(), Cmd::BeginResize(direction));
        }
    }
}

fn grab_id() -> egui::Id {
    egui::Id::new("window-grab")
}

// Al arrastrar o redimensionar, el compositor se queda el ratón y egui nunca ve soltar el botón.
fn grab(ctx: &egui::Context, command: Cmd) {
    ctx.data_mut(|d| d.insert_temp(grab_id(), true));
    ctx.send_viewport_cmd(command);
}

/// Para `App::raw_input_hook`: suelta el botón que el compositor se quedó en el arrastre.
pub fn release_hook(ctx: &egui::Context, raw: &mut egui::RawInput) {
    if ctx.data_mut(|d| d.remove_temp::<bool>(grab_id())).is_some() {
        raw.events.push(egui::Event::PointerButton {
            pos: pos2(-10_000., -10_000.),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
    }
}

/// Redondea cada rectángulo que llega a una esquina de la ventana: paneles, lienzo y velos
/// siguen la curva transparente.
pub fn round_window(ctx: &egui::Context) {
    if maximized(ctx) {
        return;
    }
    let window = ctx.content_rect();
    let mut layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
    layers.push(egui::LayerId::background());
    layers.dedup();
    ctx.graphics_mut(|g| {
        for layer in layers {
            let Some(list) = g.get_mut(layer) else {
                continue;
            };
            for i in 0..list.next_idx().0 {
                list.mutate_shape(egui::layers::ShapeIdx(i), |clipped| {
                    round_corners(&mut clipped.shape, window)
                });
            }
        }
    });
}

fn round_corners(shape: &mut egui::Shape, window: Rect) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter_mut().for_each(|s| round_corners(s, window)),
        egui::Shape::Rect(rect) => {
            let r = rect.rect;
            let left = r.left() - window.left();
            let right = window.right() - r.right();
            let top = r.top() - window.top();
            let bottom = window.bottom() - r.bottom();
            // Una esquina metida `d` px sigue la curva con radio `RADIUS - d`.
            let fit = |a: f32, b: f32, current: u8| {
                let d = a.max(b);
                if d < 1.5 && a > -1. && b > -1. {
                    current.max(RADIUS.saturating_sub(d.max(0.).round() as u8))
                } else {
                    current
                }
            };
            let c = &mut rect.corner_radius;
            c.nw = fit(left, top, c.nw);
            c.ne = fit(right, top, c.ne);
            c.sw = fit(left, bottom, c.sw);
            c.se = fit(right, bottom, c.se);
        }
        _ => {}
    }
}
