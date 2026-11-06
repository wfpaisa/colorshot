//! Miniatura flotante, acciones y cierre temporizado después de capturar.
use crate::theme::Tokens;
use egui::{self, TextureHandle};
use egui_phosphor::regular;
use colorshot_core::i18n::t;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewAction {
    Edit,
    Pin,
    Close,
}

pub struct Preview {
    texture: TextureHandle,
    /// Tamaño de la captura en píxeles; la textura puede estar reducida.
    size: egui::Vec2,
    pub path: PathBuf,
    started: Instant,
    hovered: bool,
    status: Status,
    lifetime: Option<f32>,
}

/// Lo que ya pasó con la captura; guardar y copiar corren en segundo plano.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Working,
    Done { saved: bool, copied: bool },
    Failed(String),
}

/// Opacidad plena y desvanecimiento durante el último segundo de `lifetime`
/// (en segundos; `None` = se cierra a mano).
pub fn fade(elapsed: Duration, hovered: bool, lifetime: Option<f32>) -> Option<f32> {
    let Some(lifetime) = lifetime else {
        return Some(1.);
    };
    if hovered {
        Some(1.)
    } else if elapsed.as_secs_f32() >= lifetime {
        None
    } else {
        Some((lifetime - elapsed.as_secs_f32()).min(1.))
    }
}

/// Lados máximo y mínimo de la ventana de la miniatura.
pub const MAX_WINDOW: egui::Vec2 = egui::vec2(360., 270.);
pub const MIN_WINDOW: egui::Vec2 = egui::vec2(160., 100.);

/// La ventana toma la forma de la imagen (en puntos): reducida hasta caber en `MAX_WINDOW`, sin agrandarla,
/// y nunca menor que `MIN_WINDOW`; lo que sobra queda como fondo de la tarjeta.
pub fn window_size(image: egui::Vec2) -> egui::Vec2 {
    let image = image.max(egui::Vec2::splat(1.));
    let fit = image * (MAX_WINDOW / image).min_elem().min(1.);
    fit.clamp(MIN_WINDOW, MAX_WINDOW).round()
}

/// Donde se pinta una imagen de `size` puntos dentro de la tarjeta, sin deformarla ni agrandarla.
pub fn image_rect(card: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let size = size.max(egui::Vec2::splat(1.));
    let scale = (card.size() / size).min_elem().min(1.);
    egui::Rect::from_center_size(card.center(), size * scale)
}

impl Preview {
    pub fn new(ctx: &egui::Context, image: &image::RgbaImage, path: PathBuf) -> Self {
        Self {
            texture: thumbnail_texture(ctx, image),
            size: egui::vec2(image.width() as f32, image.height() as f32),
            path,
            started: Instant::now(),
            hovered: false,
            status: Status::Working,
            lifetime: Some(7.),
        }
    }

    pub fn texture(&self) -> &TextureHandle {
        &self.texture
    }

    /// Segundos hasta ocultarse; 0 = solo a mano.
    pub fn with_lifetime(mut self, seconds: u32) -> Self {
        self.lifetime = (seconds > 0).then_some(seconds as f32);
        self
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// Reinicia la cuenta atrás (al terminar la transición de entrada).
    pub fn restart(&mut self) {
        self.started = Instant::now();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PreviewAction> {
        let ctx = ui.ctx().clone();
        // `content_rect` toma el candado del contexto: llamarlo dentro de `input`, que ya lo
        // tiene, bloquea la ventana para siempre en cuanto el puntero entra.
        let content = ctx.content_rect();
        let hovered = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| content.contains(p));
        let now = Instant::now();
        if self.hovered && !hovered {
            self.started = now;
        }
        if self.hovered != hovered {
            ctx.request_repaint();
        }
        self.hovered = hovered;
        let elapsed = now.duration_since(self.started);
        let Some(alpha) = fade(elapsed, hovered, self.lifetime) else {
            return Some(PreviewAction::Close);
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(PreviewAction::Close);
        }
        if !hovered && let Some(lifetime) = self.lifetime {
            let solid = Duration::from_secs_f32((lifetime - 1.).max(0.));
            let delay = if elapsed < solid {
                solid - elapsed
            } else {
                Duration::from_millis(16)
            };
            ctx.request_repaint_after(delay);
        }
        ui.multiply_opacity(alpha);
        let mut action = None;
        // La tarjeta ocupa toda la ventana: sin hueco transparente no queda un segundo borde.
        let card = content;
        let response = ui
            .interact(card, ui.id().with("card"), egui::Sense::click_and_drag())
            .on_hover_text(t("Click to edit"));
        let painter = ui.painter().clone();
        painter.rect_filled(card, RADIUS, Tokens::BAR);
        let image = image_rect(card, self.size / ctx.pixels_per_point());
        painter.add(
            egui::epaint::RectShape::filled(image, image_radius(card, image), egui::Color32::WHITE)
                .with_texture(
                    self.texture.id(),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                ),
        );
        // Arrastrar mueve la miniatura y un clic la abre en el editor.
        if response.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if response.clicked() {
            action = Some(PreviewAction::Edit);
        }
        let hover = ctx.animate_bool_with_time(ui.id().with("hover"), hovered, 0.12);
        if hover > 0. {
            painter.rect_filled(
                card,
                RADIUS,
                egui::Color32::from_black_alpha((90. * hover) as u8),
            );
        }
        self.status_chip(ui, card, 1. - hover);
        if hovered {
            ui.multiply_opacity(hover);
            if let Some(command) = self.actions(ui, card) {
                action = Some(command);
            }
        }
        // El borde de las demás ventanas (`frame::finish`): un píxel del fondo y el brillo
        // dentro, para que la miniatura se distinga sobre un escritorio del mismo color.
        painter.rect_stroke(
            card,
            RADIUS,
            egui::Stroke::new(1., Tokens::BAR),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            card.shrink(1.),
            RADIUS - 1.,
            egui::Stroke::new(1., crate::theme::Gnome::new(true).highlight),
            egui::StrokeKind::Inside,
        );
        action
    }

    // Estado abajo a la izquierda sobre un degradado, oculto mientras el puntero está encima.
    fn status_chip(&self, ui: &egui::Ui, card: egui::Rect, alpha: f32) {
        if alpha <= 0. {
            return;
        }
        let (icon, text) = match &self.status {
            Status::Working => (regular::CIRCLE_NOTCH, t("Saving…").to_string()),
            Status::Done { copied: true, .. } => (regular::CHECK, t("Copied").to_string()),
            Status::Done { saved: true, .. } => (regular::CHECK, t("Saved").to_string()),
            Status::Done { .. } => (regular::IMAGE, t("Not saved").to_string()),
            Status::Failed(error) => (regular::WARNING, error.clone()),
        };
        let painter = ui.painter();
        let font = egui::FontId::proportional(11.);
        let galley = painter.layout_no_wrap(
            format!("{icon}  {text}"),
            font,
            Tokens::TEXT.gamma_multiply(alpha),
        );
        let chip = egui::Rect::from_min_size(
            card.left_bottom() + egui::vec2(8., -8. - 20.),
            egui::vec2(galley.size().x + 16., 20.),
        );
        painter.rect_filled(
            chip,
            10.,
            egui::Color32::from_black_alpha((150. * alpha) as u8),
        );
        painter.galley(
            chip.left_center() + egui::vec2(8., -galley.size().y / 2.),
            galley,
            Tokens::TEXT,
        );
    }

    // Fijar y cerrar arriba a la derecha; el resto se hace en el editor.
    fn actions(&self, ui: &mut egui::Ui, card: egui::Rect) -> Option<PreviewAction> {
        let mut action = None;
        let buttons = egui::Rect::from_min_max(
            card.right_top() + egui::vec2(-8. - 22. * 2. - 4., 6.),
            card.right_top() + egui::vec2(-8., 6. + 22.),
        );
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(buttons)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 4.;
        for (icon, tooltip, command) in [
            (regular::PUSH_PIN, t("Pin"), PreviewAction::Pin),
            (regular::X, t("Close (Esc)"), PreviewAction::Close),
        ] {
            if pill_button(&mut row, icon, tooltip).clicked() {
                action = Some(command);
            }
        }
        action
    }
}

const RADIUS: f32 = 12.;

// La imagen toca el borde de la tarjeta por algún eje: allí hereda el redondeo.
fn image_radius(card: egui::Rect, image: egui::Rect) -> f32 {
    if (card.width() - image.width()).abs() < 1. || (card.height() - image.height()).abs() < 1. {
        RADIUS - 1.
    } else {
        4.
    }
}

fn pill_button(ui: &mut egui::Ui, icon: &str, tooltip: &str) -> egui::Response {
    let side = 22.;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        egui::Color32::from_white_alpha(46)
    } else if response.hovered() {
        egui::Color32::from_white_alpha(28)
    } else {
        egui::Color32::from_black_alpha(150)
    };
    ui.painter().circle_filled(rect.center(), side / 2., fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(12.),
        Tokens::TEXT,
    );
    response.on_hover_text(tooltip)
}

/// Textura reducida a 640 px como mucho: una captura completa supera el máximo de la GPU.
pub fn thumbnail_texture(ctx: &egui::Context, image: &image::RgbaImage) -> TextureHandle {
    let f = (640. / image.width().max(image.height()).max(1) as f32).min(1.);
    let thumb = image::imageops::thumbnail(
        image,
        ((image.width() as f32 * f) as u32).max(1),
        ((image.height() as f32 * f) as u32).max(1),
    );
    ctx.load_texture(
        "capture-preview",
        egui::ColorImage::from_rgba_unmultiplied(
            [thumb.width() as usize, thumb.height() as usize],
            thumb.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fade_boundaries_and_hover() {
        for seconds in [0., 5.9, 6.] {
            assert_eq!(
                fade(Duration::from_secs_f32(seconds), false, Some(7.)),
                Some(1.)
            );
        }
        assert_eq!(
            fade(Duration::from_millis(6500), false, Some(7.)),
            Some(0.5)
        );
        assert_eq!(fade(Duration::from_secs(7), false, Some(7.)), None);
        assert_eq!(fade(Duration::from_secs(20), false, Some(7.)), None);
        assert_eq!(fade(Duration::from_secs(20), true, Some(7.)), Some(1.));
        assert_eq!(fade(Duration::from_secs(99), false, None), Some(1.));
    }
    #[test]
    fn preview_fits_wide_and_tall_images() {
        let card = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(280., 210.));
        assert_eq!(
            image_rect(card, egui::vec2(100., 1000.)),
            egui::Rect::from_min_max(egui::pos2(129.5, 0.), egui::pos2(150.5, 210.))
        );
        assert_eq!(
            image_rect(card, egui::vec2(1600., 900.)),
            egui::Rect::from_min_max(egui::pos2(0., 26.25), egui::pos2(280., 183.75))
        );
        // Una imagen pequeña no se agranda.
        assert_eq!(
            image_rect(card, egui::vec2(40., 20.)),
            egui::Rect::from_center_size(card.center(), egui::vec2(40., 20.))
        );
    }
    #[test]
    fn window_follows_image() {
        assert_eq!(window_size(egui::vec2(300., 200.)), egui::vec2(300., 200.));
        assert_eq!(
            window_size(egui::vec2(1920., 1080.)),
            egui::vec2(360., 203.)
        );
        assert_eq!(window_size(egui::vec2(40., 20.)), MIN_WINDOW);
        assert_eq!(window_size(egui::vec2(100., 1000.)), egui::vec2(160., 270.));
    }
}
