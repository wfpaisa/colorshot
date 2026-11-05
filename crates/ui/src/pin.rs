//! Imagen fijada encima de todo, con esquinas redondeadas y borde fino. Al pasar el puntero
//! aparece la barra: cerrar, asa para mover, editar, copiar y el zoom, que sigue al tamaño
//! de la ventana. Arrastrar mueve, los bordes redimensionan, la rueda cambia la opacidad y
//! el clic derecho abre el menú.
use crate::icons;
use egui::{
    Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui,
    Vec2, pos2, vec2,
};
use rapidshot_core::i18n::t;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinAction {
    Copy,
    Edit,
    Close,
    /// Nuevo tamaño de ventana en puntos.
    Resize(Vec2),
}

const RADIUS: u8 = 10;
const BAR: f32 = 36.;
const ITEM: f32 = 28.;
const MENU_SCALES: [(&str, f32); 4] = [("50%", 0.5), ("100%", 1.), ("150%", 1.5), ("200%", 2.)];

pub struct Pin {
    texture: TextureHandle,
    size: Vec2,
    opacity: f32,
    /// Cuándo se copió, para mostrar la marca un momento.
    copied: Option<Instant>,
}

/// Tamaño en puntos para una escala dada, sin pasar de `max` por lado.
pub fn scaled(size: Vec2, scale: f32, max: f32) -> Vec2 {
    let wanted = size * scale;
    wanted * (max / wanted.max_elem()).min(1.)
}

/// Rectángulo de la imagen dentro de la ventana, sin deformarla.
fn fit(size: Vec2, window: Rect) -> Rect {
    let scale = (window.width() / size.x.max(1.)).min(window.height() / size.y.max(1.));
    Rect::from_center_size(window.center(), size * scale)
}

impl Pin {
    pub fn new(ctx: &egui::Context, image: &image::RgbaImage) -> Self {
        // Igual que la miniatura: nunca más grande que el máximo de la GPU.
        let max = 4096.;
        let f = (max / image.width().max(image.height()).max(1) as f32).min(1.);
        let shown = image::imageops::thumbnail(
            image,
            ((image.width() as f32 * f) as u32).max(1),
            ((image.height() as f32 * f) as u32).max(1),
        );
        let texture = ctx.load_texture(
            "pin",
            egui::ColorImage::from_rgba_unmultiplied(
                [shown.width() as usize, shown.height() as usize],
                shown.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            texture,
            size: egui::vec2(image.width() as f32, image.height() as f32),
            opacity: 1.,
            copied: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<PinAction> {
        let window = ui.max_rect();
        // Un punto de margen para que el borde de fuera no quede cortado.
        let rect = fit(self.size, window.shrink(1.));
        let response = ui.interact(rect, ui.id().with("pin-image"), Sense::click_and_drag());
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if response.hovered() && scroll != 0. {
            self.opacity = (self.opacity + scroll * 0.002).clamp(0.2, 1.);
        }
        let painter = ui.painter().clone();
        painter.add(
            egui::epaint::RectShape::filled(
                rect,
                RADIUS,
                Color32::from_white_alpha((self.opacity * 255.) as u8),
            )
            .with_texture(
                self.texture.id(),
                Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            ),
        );
        // Borde fino: oscuro por fuera y un brillo por dentro.
        painter.rect_stroke(
            rect,
            RADIUS,
            Stroke::new(1., Color32::from_black_alpha(160)),
            StrokeKind::Outside,
        );
        painter.rect_stroke(
            rect,
            RADIUS,
            Stroke::new(1., Color32::from_white_alpha(36)),
            StrokeKind::Inside,
        );
        if response.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let mut action = None;
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || response.double_clicked() {
            action = Some(PinAction::Close);
        }
        let zoom = rect.width() / self.size.x.max(1.);
        // La barra solo con el puntero dentro o con su menú abierto.
        let menu_open = egui::Popup::is_any_open(ui.ctx());
        if ui.rect_contains_pointer(window) || menu_open {
            self.bar(ui, rect, zoom, &mut action);
        }
        response.context_menu(|ui| self.menu(ui, &mut action));
        crate::frame::resize(ui, window);
        if self
            .copied
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1200))
        {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else {
            self.copied = None;
        }
        action
    }

    fn bar(&mut self, ui: &mut Ui, image: Rect, zoom: f32, action: &mut Option<PinAction>) {
        let painter = ui.painter().clone();
        let label = format!("{:.0}%", zoom * 100.);
        let label_width = ui.fonts_mut(|f| {
            f.layout_no_wrap(label.clone(), FontId::proportional(15.), Color32::WHITE)
                .size()
                .x
        });
        // Cerrar · asa · editar · copiar · zoom.
        let width = 8. + ITEM * 4. + 14. + label_width + 12.;
        let bar = Rect::from_min_size(image.min + vec2(8., 8.), vec2(width, BAR));
        painter.rect_filled(bar, 9, Color32::from_rgba_unmultiplied(36, 36, 36, 225));
        painter.rect_stroke(
            bar,
            9,
            Stroke::new(1., Color32::from_white_alpha(24)),
            StrokeKind::Inside,
        );
        let y = bar.center().y;
        let slot = |i: usize| {
            Rect::from_center_size(
                pos2(bar.left() + 8. + ITEM * (i as f32 + 0.5), y),
                vec2(ITEM, ITEM),
            )
        };
        let item = |i: usize, id: &str, sense: Sense| {
            let response = ui
                .interact(slot(i), ui.id().with(id), sense)
                .on_hover_cursor(CursorIcon::PointingHand);
            if response.hovered() && i != 0 {
                painter.rect_filled(slot(i), 6, Color32::from_white_alpha(22));
            }
            response
        };
        // Cerrar: botón redondo de GNOME.
        let close = item(0, "pin-close", Sense::click()).on_hover_text(t("Close (Esc)"));
        let fill = if close.hovered() {
            Color32::from_white_alpha(70)
        } else {
            Color32::from_white_alpha(40)
        };
        let center = slot(0).center();
        painter.circle_filled(center, 10., fill);
        for sign in [-1., 1.] {
            painter.line_segment(
                [center + vec2(-3., -3. * sign), center + vec2(3., 3. * sign)],
                Stroke::new(1.5, Color32::WHITE),
            );
        }
        if close.clicked() {
            *action = Some(PinAction::Close);
        }
        let handle = item(1, "pin-drag", Sense::drag())
            .on_hover_text(t("Drag to move"))
            .on_hover_cursor(CursorIcon::Grab);
        if handle.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        icon(
            &painter,
            ui.ctx(),
            slot(1),
            "20-dragger-white",
            egui_phosphor::regular::DOTS_SIX_VERTICAL,
        );
        let edit = item(2, "pin-edit", Sense::click()).on_hover_text(t("Edit"));
        icon(
            &painter,
            ui.ctx(),
            slot(2),
            "20-edit-white",
            egui_phosphor::regular::PENCIL_SIMPLE,
        );
        if edit.clicked() {
            *action = Some(PinAction::Edit);
        }
        let copy = item(3, "pin-copy", Sense::click()).on_hover_text(t("Copy"));
        if self.copied.is_some() {
            icon(
                &painter,
                ui.ctx(),
                slot(3),
                "",
                egui_phosphor::regular::CHECK,
            );
        } else {
            icon(
                &painter,
                ui.ctx(),
                slot(3),
                "20-copy-white",
                egui_phosphor::regular::COPY,
            );
        }
        if copy.clicked() {
            self.copied = Some(Instant::now());
            *action = Some(PinAction::Copy);
        }
        // Zoom: cambia al redimensionar; con clic, tamaños fijos y opacidad.
        let zoom_rect = Rect::from_min_max(
            pos2(slot(3).right() + 4., bar.top() + 4.),
            pos2(bar.right() - 4., bar.bottom() - 4.),
        );
        let zoom_button = ui
            .interact(zoom_rect, ui.id().with("pin-zoom"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if zoom_button.hovered() {
            painter.rect_filled(zoom_rect, 6, Color32::from_white_alpha(22));
        }
        painter.text(
            zoom_rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(15.),
            Color32::WHITE,
        );
        egui::Popup::menu(&zoom_button).show(|ui| self.menu(ui, action));
    }

    fn menu(&mut self, ui: &mut Ui, action: &mut Option<PinAction>) {
        for (label, scale) in MENU_SCALES {
            if ui.button(label).clicked() {
                *action = Some(PinAction::Resize(scaled(self.size, scale, 4000.)));
            }
        }
        ui.separator();
        ui.add(egui::Slider::new(&mut self.opacity, 0.2..=1.).text(t("Opacity")));
        ui.separator();
        for (label, command) in [
            (t("Copy"), PinAction::Copy),
            (t("Edit"), PinAction::Edit),
            (t("Close"), PinAction::Close),
        ] {
            if ui.button(label).clicked() {
                *action = Some(command);
            }
        }
    }
}

/// Icono en blanco, o su glifo de Phosphor si no está instalado.
fn icon(painter: &egui::Painter, ctx: &egui::Context, rect: Rect, name: &str, glyph: &str) {
    match (!name.is_empty())
        .then(|| icons::texture(ctx, name))
        .flatten()
    {
        Some(texture) => {
            painter.image(
                texture.id(),
                Rect::from_center_size(rect.center(), vec2(20., 20.)),
                Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        None => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(18.),
                Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_fits_without_stretching() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(300., 300.));
        assert_eq!(
            fit(vec2(400., 200.), window),
            Rect::from_min_max(pos2(0., 75.), pos2(300., 225.))
        );
    }
    #[test]
    fn scale_keeps_ratio_and_limit() {
        assert_eq!(
            scaled(egui::vec2(400., 200.), 0.5, 4000.),
            egui::vec2(200., 100.)
        );
        assert_eq!(
            scaled(egui::vec2(3000., 1000.), 2., 3000.),
            egui::vec2(3000., 1000.)
        );
    }
}
