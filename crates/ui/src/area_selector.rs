//! Ventana a pantalla completa por monitor: marca una zona sobre el escritorio vivo (la captura
//! se toma al soltar) o elige un color sobre una captura congelada.
use crate::selection;
use egui::{Color32, Context, CursorIcon, Pos2, Rect as UiRect, Stroke, StrokeKind, Vec2};
use image::RgbaImage;
use rapidshot_core::capture::Rect;
use rapidshot_core::i18n::t;

#[derive(Clone, Copy)]
pub enum SelectorResult {
    /// Zona en píxeles físicos de la ventana, con el tamaño de esta.
    Area {
        rect: Rect,
        size: (u32, u32),
    },
    /// Píxel elegido con el cuentagotas.
    Color([u8; 3]),
    /// Todos los monitores (A al elegir pantalla).
    All,
    Cancelled,
}
/// Qué hacer con cada color que se guarda con clic derecho, sin cerrar el cuentagotas.
pub type Keep = std::sync::Arc<dyn Fn([u8; 3]) + Send + Sync>;

#[derive(Clone, Copy)]
struct Drag {
    anchor: (u32, u32),
    last: (u32, u32),
}
enum Mode {
    // Ventana transparente: se ve el escritorio tal cual.
    Area,
    Color {
        image: RgbaImage,
        texture: Option<egui::TextureHandle>,
        // Nivel de zoom de la lupa (índice de `LOUPE`) y rueda acumulada hacia el siguiente.
        zoom: usize,
        scroll: f32,
        // Punto que se elige, en puntos: sigue al puntero, a un cuarto de su paso con Shift,
        // y las flechas lo mueven de a un píxel.
        aim: Option<Pos2>,
        // Clic derecho: guarda el color y sigue; el aro del último dura un momento.
        keep: Option<Keep>,
        kept: Option<std::time::Instant>,
    },
    // Monitor entero: el que recibe el puntero o, con `pick`, el que se pulsa.
    Screen {
        pick: bool,
        primary: bool,
        born: std::time::Instant,
    },
    // Ventanas de este monitor, de arriba abajo, en puntos del selector.
    Window(Vec<UiRect>),
}

/// Lo que se espera a que un monitor reciba el puntero (≈200 ms en GNOME) antes de quedarse
/// con el principal.
const FIND_CURSOR: std::time::Duration = std::time::Duration::from_millis(350);
pub struct AreaSelector {
    mode: Mode,
    selection: Option<Rect>,
    drag: Option<Drag>,
    // El monitor dentro de la ventana si no la llena entera; si no, toda la ventana.
    screen: Option<UiRect>,
}
impl AreaSelector {
    /// Selección de zona sobre el escritorio vivo.
    pub fn area() -> Self {
        Self {
            mode: Mode::Area,
            selection: None,
            drag: None,
            screen: None,
        }
    }
    /// Cuentagotas sobre la captura del monitor.
    pub fn color(image: RgbaImage) -> Self {
        Self {
            mode: Mode::Color {
                image,
                texture: None,
                zoom: 0,
                scroll: 0.,
                aim: None,
                keep: None,
                kept: None,
            },
            selection: None,
            drag: None,
            screen: None,
        }
    }
    /// Con clic derecho, el cuentagotas pasa el color a `keep` y sigue abierto.
    pub fn keeping(mut self, f: Keep) -> Self {
        if let Mode::Color { keep, .. } = &mut self.mode {
            *keep = Some(f);
        }
        self
    }
    /// Monitor entero: el del puntero o, con `pick`, el que se pulse. Sin puntero, se queda
    /// el principal (`primary`).
    pub fn screen(pick: bool, primary: bool) -> Self {
        Self {
            mode: Mode::Screen {
                pick,
                primary,
                born: std::time::Instant::now(),
            },
            selection: None,
            drag: None,
            screen: None,
        }
    }
    /// Elegir una ventana con un clic; `windows` va de la de arriba a la de abajo, en puntos
    /// de este monitor.
    pub fn window(windows: Vec<UiRect>) -> Self {
        Self {
            mode: Mode::Window(windows),
            selection: None,
            drag: None,
            screen: None,
        }
    }
    pub fn transparent(&self) -> bool {
        !matches!(self.mode, Mode::Color { .. })
    }
    /// Zona de la ventana que ocupa el monitor, en puntos.
    pub fn set_screen(&mut self, screen: UiRect) {
        self.screen = Some(screen);
    }
    fn screen_rect(&self, ctx: &Context) -> UiRect {
        self.screen.unwrap_or_else(|| ctx.content_rect())
    }
    /// Descarta la zona (otro monitor empezó una nueva).
    pub fn clear(&mut self) {
        self.selection = None;
        self.drag = None;
    }
    pub fn ui(&mut self, ctx: &Context) -> Option<SelectorResult> {
        ctx.set_cursor_icon(match self.mode {
            Mode::Screen { .. } | Mode::Window(_) => CursorIcon::Default,
            _ => CursorIcon::Crosshair,
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(SelectorResult::Cancelled);
        }
        let screen = self.screen_rect(ctx);
        match self.mode {
            Mode::Area => self.area_ui(ctx, screen),
            Mode::Color { .. } => self.color_ui(ctx, screen),
            Mode::Screen {
                pick,
                primary,
                born,
            } => screen_ui(ctx, screen, pick, primary, born),
            Mode::Window(ref windows) => window_ui(ctx, screen, windows),
        }
    }
    fn area_ui(&mut self, ctx: &Context, screen: UiRect) -> Option<SelectorResult> {
        let ppp = ctx.pixels_per_point();
        let physical = screen.size() * ppp;
        let bounds = (physical.x.round() as u32, physical.y.round() as u32);
        if bounds.0 == 0 || bounds.1 == 0 {
            return Some(SelectorResult::Cancelled);
        }
        let to_pixel = |p: Pos2| {
            let v = (p - screen.min) * ppp;
            (
                v.x.round().clamp(0., bounds.0 as f32) as u32,
                v.y.round().clamp(0., bounds.1 as f32) as u32,
            )
        };
        let (pointer, shift, space) = ctx.input(|i| {
            (
                i.pointer.clone(),
                i.modifiers.shift,
                i.key_down(egui::Key::Space),
            )
        });
        if let Some(pos) = pointer.hover_pos() {
            let p = to_pixel(pos);
            if pointer.button_pressed(egui::PointerButton::Primary) {
                self.selection = None;
                self.drag = Some(Drag { anchor: p, last: p });
            }
            if let Some(Drag { mut anchor, last }) = self.drag {
                // Con Espacio pulsado la zona se desplaza en vez de crecer.
                match self.selection.filter(|_| space) {
                    Some(r) => {
                        let moved = selection::translate(
                            r,
                            (p.0 as i64 - last.0 as i64, p.1 as i64 - last.1 as i64),
                            bounds,
                        );
                        anchor = (
                            (anchor.0 as i64 + moved.x as i64 - r.x as i64) as u32,
                            (anchor.1 as i64 + moved.y as i64 - r.y as i64) as u32,
                        );
                        self.selection = Some(moved);
                    }
                    None => self.selection = Some(selection::create(anchor, p, bounds, shift)),
                }
                self.drag = Some(Drag { anchor, last: p });
            }
        }
        // Soltar captura: la zona marcada o, con un clic sin arrastrar, el monitor entero.
        if pointer.button_released(egui::PointerButton::Primary) && self.drag.take().is_some() {
            let rect = self
                .selection
                .take()
                .filter(|r| r.w >= 3 && r.h >= 3)
                .unwrap_or(Rect {
                    x: 0,
                    y: 0,
                    w: bounds.0,
                    h: bounds.1,
                });
            return Some(SelectorResult::Area { rect, size: bounds });
        }
        let r = self.selection?;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("selector"),
        ));
        let zone = UiRect::from_min_size(
            screen.min + Vec2::new(r.x as f32, r.y as f32) / ppp,
            Vec2::new(r.w as f32, r.h as f32) / ppp,
        );
        let shade = Color32::from_black_alpha(115);
        for outside in [
            UiRect::from_min_max(screen.min, egui::pos2(screen.right(), zone.top())),
            UiRect::from_min_max(egui::pos2(screen.left(), zone.bottom()), screen.max),
            UiRect::from_min_max(egui::pos2(screen.left(), zone.top()), zone.left_bottom()),
            UiRect::from_min_max(zone.right_top(), egui::pos2(screen.right(), zone.bottom())),
        ] {
            painter.rect_filled(outside, 0., shade);
        }
        painter.rect_stroke(
            zone.expand(1.),
            0.,
            Stroke::new(1., Color32::BLACK),
            StrokeKind::Outside,
        );
        painter.rect_stroke(
            zone,
            0.,
            Stroke::new(1., Color32::WHITE),
            StrokeKind::Outside,
        );
        let size = Vec2::new(110., 26.);
        let y = if zone.bottom() + 30. <= screen.bottom() {
            zone.bottom() + 4.
        } else {
            zone.top() - 30.
        };
        let label = fit_rect(screen, egui::pos2(zone.center().x - 55., y), size);
        painter.rect_filled(label, 6., crate::theme::Tokens::BAR);
        painter.text(
            label.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} × {}", r.w, r.h),
            egui::FontId::proportional(13.),
            Color32::WHITE,
        );
        None
    }
    fn color_ui(&mut self, ctx: &Context, screen: UiRect) -> Option<SelectorResult> {
        let Mode::Color {
            image,
            texture,
            zoom,
            scroll,
            aim,
            keep,
            kept,
        } = &mut self.mode
        else {
            return None;
        };
        let bounds = image.dimensions();
        if bounds.0 == 0 || bounds.1 == 0 {
            return Some(SelectorResult::Cancelled);
        }
        // La razón física compensa HiDPI y el tamaño real del viewport por eje.
        let physical = screen.size() * ctx.pixels_per_point();
        let scale = Vec2::new(bounds.0 as f32 / physical.x, bounds.1 as f32 / physical.y)
            * ctx.pixels_per_point();
        let to_pixel = |p: Pos2| {
            let v = (p - screen.min) * scale;
            (
                (v.x.max(0.) as u32).min(bounds.0 - 1),
                (v.y.max(0.) as u32).min(bounds.1 - 1),
            )
        };
        let (pointer, shift, wheel, step, enter) = ctx.input(|i| {
            let key = |k, v: Vec2| if i.key_pressed(k) { v } else { Vec2::ZERO };
            (
                i.pointer.clone(),
                i.modifiers.shift,
                // En muescas de rueda: una línea es una muesca y unos 40 puntos de touchpad.
                i.events
                    .iter()
                    .map(|e| match e {
                        egui::Event::MouseWheel { unit, delta, .. } => match unit {
                            egui::MouseWheelUnit::Line => delta.y,
                            egui::MouseWheelUnit::Point => delta.y / 40.,
                            egui::MouseWheelUnit::Page => delta.y * 3.,
                        },
                        _ => 0.,
                    })
                    .sum::<f32>(),
                key(egui::Key::ArrowLeft, Vec2::new(-1., 0.))
                    + key(egui::Key::ArrowRight, Vec2::new(1., 0.))
                    + key(egui::Key::ArrowUp, Vec2::new(0., -1.))
                    + key(egui::Key::ArrowDown, Vec2::new(0., 1.)),
                i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space),
            )
        });
        let hover = pointer.hover_pos();
        // Rueda: un nivel por muesca, también con el desplazamiento suave del touchpad. Hacia
        // abajo acerca y hacia arriba aleja.
        *scroll += wheel;
        if scroll.abs() >= 1. {
            *zoom = if *scroll < 0. {
                (*zoom + 1).min(LOUPE.len() - 1)
            } else {
                zoom.saturating_sub(1)
            };
            *scroll = 0.;
        }
        let delta = pointer.delta();
        *aim = match (*aim, hover) {
            (Some(at), Some(_)) if shift => Some(at + delta * 0.25),
            (Some(at), Some(_)) if delta == Vec2::ZERO => Some(at),
            (_, hover) => hover,
        };
        if let Some(at) = aim.as_mut() {
            // Las flechas, de a un píxel de la captura, al centro de ese píxel.
            if step != Vec2::ZERO {
                let (x, y) = to_pixel(*at);
                let pixel = Vec2::new(x as f32, y as f32) + step + Vec2::splat(0.5);
                *at = screen.min + pixel / scale;
            }
            *at = at.clamp(screen.min, screen.max);
        }
        if let Some(at) = *aim
            && (pointer.button_released(egui::PointerButton::Primary) || enter)
        {
            let (x, y) = to_pixel(at);
            let c = image.get_pixel(x, y).0;
            return Some(SelectorResult::Color([c[0], c[1], c[2]]));
        }
        if let (Some(at), Some(keep)) = (*aim, keep.as_ref())
            && pointer.button_released(egui::PointerButton::Secondary)
        {
            let (x, y) = to_pixel(at);
            let c = image.get_pixel(x, y).0;
            keep([c[0], c[1], c[2]]);
            *kept = Some(std::time::Instant::now());
        }
        let id = frozen_texture(ctx, image, texture);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("selector"),
        ));
        painter.image(
            id,
            screen,
            UiRect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        if let Some(at) = *aim {
            // Lejos del puntero (Shift o flechas), una cruz marca el punto elegido.
            if hover.is_some_and(|h| (h - at).length() > 1.5) {
                for (stroke, width) in [(Color32::BLACK, 3.), (Color32::WHITE, 1.)] {
                    let stroke = Stroke::new(width, stroke);
                    painter.line_segment([at - Vec2::X * 7., at + Vec2::X * 7.], stroke);
                    painter.line_segment([at - Vec2::Y * 7., at + Vec2::Y * 7.], stroke);
                }
            }
            magnifier(image, &painter, screen, at, to_pixel(at), *zoom);
            // Aviso de que el clic derecho guardó el color.
            if let Some(since) = *kept {
                let t = since.elapsed().as_secs_f32() / 0.45;
                if t < 1. {
                    let alpha = ((1. - t) * 255.) as u8;
                    let color = Color32::from_rgba_unmultiplied(53, 132, 228, alpha);
                    painter.circle_stroke(at, 10. + t * 26., Stroke::new(3., color));
                    ctx.request_repaint();
                } else {
                    *kept = None;
                }
            }
        }
        None
    }
}
fn screen_ui(
    ctx: &Context,
    screen: UiRect,
    pick: bool,
    primary: bool,
    born: std::time::Instant,
) -> Option<SelectorResult> {
    let physical = screen.size() * ctx.pixels_per_point();
    let size = (physical.x.round() as u32, physical.y.round() as u32);
    let whole = SelectorResult::Area {
        rect: Rect {
            x: 0,
            y: 0,
            w: size.0,
            h: size.1,
        },
        size,
    };
    let (hover, clicked, all) = ctx.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_clicked(),
            i.key_pressed(egui::Key::A),
        )
    });
    if !pick {
        if hover.is_some() || (primary && born.elapsed() >= FIND_CURSOR) {
            return Some(whole);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        return None;
    }
    if all {
        return Some(SelectorResult::All);
    }
    if hover.is_some() && clicked {
        return Some(whole);
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("selector"),
    ));
    if hover.is_some() {
        painter.rect_filled(
            screen,
            0.,
            Color32::from_rgba_unmultiplied(53, 132, 228, 28),
        );
        painter.rect_stroke(
            screen,
            0.,
            Stroke::new(4., crate::theme::Tokens::SELECTION),
            StrokeKind::Inside,
        );
    } else {
        painter.rect_filled(screen, 0., Color32::from_black_alpha(110));
    }
    let card = UiRect::from_center_size(screen.center(), Vec2::new(340., 92.));
    painter.rect_filled(card, 14., crate::theme::Tokens::BAR);
    painter.text(
        card.center() - Vec2::new(0., 14.),
        egui::Align2::CENTER_CENTER,
        if hover.is_some() {
            t("Click to capture this screen")
        } else {
            t("Move pointer here to capture")
        },
        egui::FontId::proportional(16.),
        Color32::WHITE,
    );
    painter.text(
        card.center() + Vec2::new(0., 16.),
        egui::Align2::CENTER_CENTER,
        t("A  all screens   ·   Esc  cancel"),
        egui::FontId::proportional(12.5),
        crate::theme::Tokens::DIM,
    );
    None
}

/// Resalta la ventana de más arriba bajo el puntero y la devuelve al pulsarla, recortada al
/// monitor.
fn window_ui(ctx: &Context, screen: UiRect, windows: &[UiRect]) -> Option<SelectorResult> {
    let ppp = ctx.pixels_per_point();
    let physical = screen.size() * ppp;
    let size = (physical.x.round() as u32, physical.y.round() as u32);
    let (hover, clicked) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked()));
    let target = hover.and_then(|p| {
        windows
            .iter()
            .map(|w| w.translate(screen.min.to_vec2()).intersect(screen))
            .find(|w| w.contains(p) && w.is_positive())
    });
    if let (Some(zone), true) = (target, clicked) {
        let px = |v: f32, limit: u32| ((v * ppp).round().max(0.) as u32).min(limit);
        let x = px(zone.left() - screen.left(), size.0);
        let y = px(zone.top() - screen.top(), size.1);
        return Some(SelectorResult::Area {
            rect: Rect {
                x,
                y,
                w: px(zone.right() - screen.left(), size.0) - x,
                h: px(zone.bottom() - screen.top(), size.1) - y,
            },
            size,
        });
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("selector"),
    ));
    let shade = Color32::from_black_alpha(110);
    let Some(zone) = target else {
        painter.rect_filled(screen, 0., shade);
        return None;
    };
    for outside in [
        UiRect::from_min_max(screen.min, egui::pos2(screen.right(), zone.top())),
        UiRect::from_min_max(egui::pos2(screen.left(), zone.bottom()), screen.max),
        UiRect::from_min_max(egui::pos2(screen.left(), zone.top()), zone.left_bottom()),
        UiRect::from_min_max(zone.right_top(), egui::pos2(screen.right(), zone.bottom())),
    ] {
        painter.rect_filled(outside, 0., shade);
    }
    painter.rect_filled(zone, 0., Color32::from_rgba_unmultiplied(53, 132, 228, 28));
    painter.rect_stroke(
        zone,
        0.,
        Stroke::new(3., crate::theme::Tokens::SELECTION),
        StrokeKind::Inside,
    );
    let label = UiRect::from_center_size(zone.center(), Vec2::new(250., 56.));
    painter.rect_filled(label, 12., crate::theme::Tokens::BAR);
    painter.text(
        label.center() - Vec2::new(0., 9.),
        egui::Align2::CENTER_CENTER,
        t("Click to capture this window"),
        egui::FontId::proportional(14.),
        Color32::WHITE,
    );
    painter.text(
        label.center() + Vec2::new(0., 12.),
        egui::Align2::CENTER_CENTER,
        format!(
            "{} × {}",
            (zone.width() * ppp).round(),
            (zone.height() * ppp).round()
        ),
        egui::FontId::proportional(12.),
        crate::theme::Tokens::DIM,
    );
    None
}

// La textura se crea en el primer cuadro: antes eframe aún no conoce el máximo de la GPU.
fn frozen_texture(
    ctx: &Context,
    image: &RgbaImage,
    texture: &mut Option<egui::TextureHandle>,
) -> egui::TextureId {
    if let Some(texture) = texture {
        return texture.id();
    }
    let max = ctx.input(|i| i.max_texture_side).max(1) as u32;
    let (w, h) = image.dimensions();
    let shown = if w > max || h > max {
        let f = max as f32 / w.max(h) as f32;
        std::borrow::Cow::Owned(image::imageops::resize(
            image,
            ((w as f32 * f) as u32).max(1),
            ((h as f32 * f) as u32).max(1),
            image::imageops::FilterType::Triangle,
        ))
    } else {
        std::borrow::Cow::Borrowed(image)
    };
    let handle = ctx.load_texture(
        "frozen-screen",
        egui::ColorImage::from_rgba_unmultiplied(
            [shown.width() as usize, shown.height() as usize],
            shown.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    );
    texture.insert(handle).id()
}
/// Píxeles por lado de la lupa en cada nivel de zoom; siempre impares, para tener centro.
const LOUPE: [i64; 3] = [15, 9, 5];
/// Lado de la cuadrícula de la lupa, en puntos.
const LOUPE_SIDE: f32 = 120.;
fn magnifier(
    image: &RgbaImage,
    painter: &egui::Painter,
    screen: UiRect,
    cursor: Pos2,
    p: (u32, u32),
    zoom: usize,
) {
    let desired = egui::pos2(
        if cursor.x + 148. > screen.right() {
            cursor.x - 144.
        } else {
            cursor.x + 24.
        },
        if cursor.y + 198. > screen.bottom() {
            cursor.y - 194.
        } else {
            cursor.y + 24.
        },
    );
    let frame = fit_rect(screen, desired, egui::vec2(LOUPE_SIDE, 172.));
    painter.rect_filled(frame, 4., crate::theme::Tokens::BAR);
    let (x, y) = p;
    let count = LOUPE[zoom.min(LOUPE.len() - 1)];
    let size = LOUPE_SIDE / count as f32;
    let half = count / 2;
    for row in 0..count {
        for col in 0..count {
            let sx = (x as i64 + col - half).clamp(0, image.width() as i64 - 1) as u32;
            let sy = (y as i64 + row - half).clamp(0, image.height() as i64 - 1) as u32;
            let c = image.get_pixel(sx, sy).0;
            let cell = UiRect::from_min_size(
                frame.min + egui::vec2(col as f32, row as f32) * size,
                Vec2::splat(size),
            );
            painter.rect_filled(cell, 0., Color32::from_rgb(c[0], c[1], c[2]));
            painter.rect_stroke(
                cell,
                0.,
                Stroke::new(0.5, Color32::from_black_alpha(40)),
                StrokeKind::Inside,
            );
        }
    }
    let center = UiRect::from_min_size(
        frame.min + Vec2::splat(half as f32 * size),
        Vec2::splat(size),
    );
    painter.rect_stroke(
        center.expand(1.),
        0.,
        Stroke::new(1., Color32::BLACK),
        StrokeKind::Outside,
    );
    painter.rect_stroke(
        center,
        0.,
        Stroke::new(1., Color32::WHITE),
        StrokeKind::Inside,
    );
    let c = image.get_pixel(x, y).0;
    painter.text(
        frame.min + egui::vec2(60., 130.),
        egui::Align2::CENTER_CENTER,
        format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
        egui::FontId::monospace(12.),
        Color32::WHITE,
    );
    painter.text(
        frame.min + egui::vec2(60., 147.),
        egui::Align2::CENTER_CENTER,
        format!("{x}, {y}"),
        egui::FontId::monospace(11.),
        crate::theme::Tokens::DIM,
    );
    // Nivel de zoom: un punto por nivel, el actual en blanco y más ancho.
    let mut left = frame.center().x - 16.;
    for level in 0..LOUPE.len() {
        let width = if level == zoom { 12. } else { 5. };
        let dot =
            UiRect::from_min_size(egui::pos2(left, frame.min.y + 161.), egui::vec2(width, 5.));
        let color = if level == zoom {
            Color32::WHITE
        } else {
            Color32::from_gray(100)
        };
        painter.rect_filled(dot, 2.5, color);
        left += width + 5.;
    }
}
fn fit_rect(screen: UiRect, pos: Pos2, size: Vec2) -> UiRect {
    UiRect::from_min_size(
        egui::pos2(
            pos.x
                .clamp(screen.left(), (screen.right() - size.x).max(screen.left())),
            pos.y
                .clamp(screen.top(), (screen.bottom() - size.y).max(screen.top())),
        ),
        size,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &Context,
        selector: &mut AreaSelector,
        events: Vec<egui::Event>,
    ) -> Option<SelectorResult> {
        let mut result = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(UiRect::from_min_size(Pos2::ZERO, egui::vec2(400., 300.))),
                events,
                ..Default::default()
            },
            |ui| {
                result = selector.ui(ui.ctx());
            },
        );
        output.textures_delta.clear();
        result
    }
    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn hidpi_release_confirms_the_zone() {
        let ctx = Context::default();
        ctx.set_pixels_per_point(2.);
        let mut selector = AreaSelector::area();
        frame(&ctx, &mut selector, vec![]);
        let start = egui::pos2(50., 50.);
        let end = egui::pos2(150., 120.);
        frame(
            &ctx,
            &mut selector,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        assert!(frame(&ctx, &mut selector, vec![egui::Event::PointerMoved(end)]).is_none());
        assert!(matches!(
            frame(&ctx, &mut selector, vec![button(end, false)]),
            Some(SelectorResult::Area {
                rect: Rect {
                    x: 100,
                    y: 100,
                    w: 200,
                    h: 140
                },
                size: (800, 600)
            })
        ));
    }
    #[test]
    fn click_without_drag_takes_the_monitor_and_escape_cancels() {
        let ctx = Context::default();
        let mut selector = AreaSelector::area();
        let p = egui::pos2(40., 40.);
        frame(
            &ctx,
            &mut selector,
            vec![egui::Event::PointerMoved(p), button(p, true)],
        );
        assert!(matches!(
            frame(&ctx, &mut selector, vec![button(p, false)]),
            Some(SelectorResult::Area {
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 400,
                    h: 300
                },
                ..
            })
        ));
        assert!(matches!(
            frame(
                &ctx,
                &mut selector,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default()
                }]
            ),
            Some(SelectorResult::Cancelled)
        ));
    }
    #[test]
    fn screen_takes_the_hovered_monitor_or_all() {
        let ctx = Context::default();
        let p = egui::pos2(10., 10.);
        let mut cursor = AreaSelector::screen(false, false);
        assert!(frame(&ctx, &mut cursor, vec![]).is_none());
        assert!(matches!(
            frame(&ctx, &mut cursor, vec![egui::Event::PointerMoved(p)]),
            Some(SelectorResult::Area {
                rect: Rect { w: 400, h: 300, .. },
                ..
            })
        ));
        let mut pick = AreaSelector::screen(true, false);
        assert!(frame(&ctx, &mut pick, vec![egui::Event::PointerMoved(p)]).is_none());
        assert!(matches!(
            frame(
                &ctx,
                &mut pick,
                vec![egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default()
                }]
            ),
            Some(SelectorResult::All)
        ));
    }
    #[test]
    fn color_click_picks_the_pixel() {
        let ctx = Context::default();
        let mut image = RgbaImage::new(400, 300);
        image.put_pixel(40, 30, image::Rgba([12, 34, 56, 255]));
        let mut selector = AreaSelector::color(image);
        let p = egui::pos2(40.5, 30.5);
        frame(
            &ctx,
            &mut selector,
            vec![egui::Event::PointerMoved(p), button(p, true)],
        );
        assert!(matches!(
            frame(&ctx, &mut selector, vec![button(p, false)]),
            Some(SelectorResult::Color([12, 34, 56]))
        ));
    }
}
