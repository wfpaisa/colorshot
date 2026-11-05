//! Aparición de las ventanas y su colocación en el monitor.
use egui::Rect;
use std::time::{Duration, Instant};

/// Desaceleración suave al final (ease-out cúbica).
pub fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Duración del fundido con que aparecen las ventanas.
pub const APPEAR: Duration = Duration::from_millis(160);

/// Opacidad 0..=1 de la ventana mientras aparece, contando desde la primera vez que se pide
/// en este contexto. Pide cuadros hasta terminar.
pub fn appear(ctx: &egui::Context) -> f32 {
    let id = egui::Id::new("rapidshot-appear");
    let now = Instant::now();
    let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    let t = now.duration_since(start).as_secs_f32() / APPEAR.as_secs_f32();
    if t < 1. {
        ctx.request_repaint();
    }
    ease_out(t)
}

/// Coloca un rectángulo de `size` dentro de `area` según la alineación (0..1 por eje).
pub fn place(area: Rect, size: egui::Vec2, align: (f32, f32), margin: f32) -> Rect {
    let room = (area.size() - size - egui::Vec2::splat(2. * margin)).max(egui::Vec2::ZERO);
    Rect::from_min_size(
        area.min + egui::vec2(margin + room.x * align.0, margin + room.y * align.1),
        size,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eases_and_places() {
        assert_eq!(ease_out(0.), 0.);
        assert_eq!(ease_out(1.), 1.);
        assert!(ease_out(0.5) > 0.5);
        let area = Rect::from_min_size(egui::pos2(100., 0.), egui::vec2(1000., 500.));
        let size = egui::vec2(300., 100.);
        assert_eq!(
            place(area, size, (1., 1.), 20.).max,
            egui::pos2(1080., 480.)
        );
        assert_eq!(place(area, size, (0.5, 0.5), 20.).center(), area.center());
    }
}
