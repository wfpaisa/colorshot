//! Animación de lectura del OCR: un barrido de luz recorre la imagen y termina en «Done».
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, vec2};
use egui_phosphor::regular;
use std::time::{Duration, Instant};

/// Una vuelta completa del barrido (bajar y subir).
const SWEEP: f32 = 1.8;
/// Lo que dura la medalla del resultado antes de desvanecerse.
pub const RESULT: Duration = Duration::from_millis(1600);
const OK: Color32 = Color32::from_rgb(46, 194, 126);
const WARN: Color32 = Color32::from_rgb(229, 165, 10);

pub struct Scan {
    started: Instant,
    finished: Option<(Instant, bool, String)>,
}

impl Default for Scan {
    fn default() -> Self {
        Self::new()
    }
}

impl Scan {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            finished: None,
        }
    }

    /// Resultado: `ok` = algo copiado; `message` va bajo la medalla.
    pub fn finish(&mut self, ok: bool, message: impl Into<String>) {
        self.finished = Some((Instant::now(), ok, message.into()));
    }

    /// ¿Terminó ya la medalla?
    pub fn over(&self, now: Instant) -> bool {
        self.finished
            .as_ref()
            .is_some_and(|(at, ..)| now - *at > RESULT)
    }

    /// Pinta sobre `rect`; devuelve `false` cuando ya no queda nada que mostrar.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        accent: Color32,
        now: Instant,
    ) -> bool {
        if self.over(now) {
            return false;
        }
        // El barrido se apaga en el primer cuarto de segundo tras el resultado.
        let since_done = self
            .finished
            .as_ref()
            .map(|(at, ..)| (now - *at).as_secs_f32());
        let scan_alpha = since_done.map_or(1., |s| 1. - (s / 0.25).min(1.));
        let fade_out = since_done.map_or(1., |s| {
            let left = RESULT.as_secs_f32() - s;
            (left / 0.35).clamp(0., 1.)
        });
        if scan_alpha > 0. {
            self.paint_sweep(painter, rect, accent, now, scan_alpha);
        }
        if let (Some(since), Some((_, ok, message))) = (since_done, &self.finished) {
            paint_badge(painter, rect, *ok, message, since, fade_out);
        }
        true
    }

    fn paint_sweep(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        accent: Color32,
        now: Instant,
        alpha: f32,
    ) {
        let t = (now - self.started).as_secs_f32() / SWEEP;
        // Ida y vuelta con suavizado en los extremos.
        let phase = t.fract();
        let down = phase < 0.5;
        let k = if down { phase * 2. } else { 2. - phase * 2. };
        let k = k * k * (3. - 2. * k);
        let y = egui::lerp(rect.top()..=rect.bottom(), k);
        painter.rect_filled(rect, 0., Color32::from_black_alpha((70. * alpha) as u8));
        // Estela detrás de la línea, que se borra hacia atrás.
        let trail = (rect.height() * 0.28).clamp(16., 140.);
        let steps = 24;
        for i in 0..steps {
            let f = i as f32 / steps as f32;
            let offset = trail * f;
            let band_y = if down { y - offset } else { y + offset };
            let band = Rect::from_min_max(
                Pos2::new(rect.left(), band_y.min(band_y + trail / steps as f32)),
                Pos2::new(rect.right(), band_y.max(band_y + trail / steps as f32)),
            )
            .intersect(rect);
            if band.height() > 0. {
                painter.rect_filled(
                    band,
                    0.,
                    accent.gamma_multiply(0.22 * (1. - f) * (1. - f) * alpha),
                );
            }
        }
        // La línea, con un halo.
        for (width, a) in [(14., 0.10), (6., 0.25), (2., 1.)] {
            painter.hline(
                rect.x_range(),
                y,
                Stroke::new(width, accent.gamma_multiply(a * alpha)),
            );
        }
        painter.hline(
            rect.x_range(),
            y,
            Stroke::new(1., Color32::from_white_alpha((200. * alpha) as u8)),
        );
        // Esquinas de visor.
        let len = (rect.width().min(rect.height()) * 0.12).clamp(8., 26.);
        let stroke = Stroke::new(2.5, Color32::from_white_alpha((230. * alpha) as u8));
        let inset = rect.shrink(6.);
        for (corner, dx, dy) in [
            (inset.left_top(), 1., 1.),
            (inset.right_top(), -1., 1.),
            (inset.left_bottom(), 1., -1.),
            (inset.right_bottom(), -1., -1.),
        ] {
            painter.line_segment([corner, corner + vec2(len * dx, 0.)], stroke);
            painter.line_segment([corner, corner + vec2(0., len * dy)], stroke);
        }
        if rect.height() > 60. && rect.width() > 120. {
            let dots = ".".repeat(1 + ((now - self.started).as_secs_f32() * 3.) as usize % 3);
            chip(
                painter,
                rect.center_bottom() - vec2(0., 22.),
                &format!(
                    "{}  {}{dots}",
                    regular::SCAN,
                    rapidshot_core::i18n::t("Reading text / QR")
                ),
                alpha,
            );
        }
    }
}

fn chip(painter: &egui::Painter, center: Pos2, text: &str, alpha: f32) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::proportional(13.),
        Color32::from_white_alpha((240. * alpha) as u8),
    );
    let size = galley.size() + vec2(22., 10.);
    let rect = Rect::from_center_size(center, size);
    painter.rect_filled(
        rect,
        size.y / 2.,
        Color32::from_black_alpha((170. * alpha) as u8),
    );
    painter.galley(rect.center() - galley.size() / 2., galley, Color32::WHITE);
}

// Medalla que entra con un pequeño rebote: visto si se copió algo, aviso si no.
fn paint_badge(
    painter: &egui::Painter,
    rect: Rect,
    ok: bool,
    message: &str,
    since: f32,
    alpha: f32,
) {
    let t = (since / 0.32).min(1.);
    // Rebote (ease-out-back).
    let c = 1.9;
    let pop = 1. + (c + 1.) * (t - 1.).powi(3) + c * (t - 1.).powi(2);
    let radius = (rect.width().min(rect.height()) * 0.16).clamp(16., 40.) * pop.max(0.);
    let center = rect.center() - vec2(0., if rect.height() > 90. { 12. } else { 0. });
    let color = if ok { OK } else { WARN };
    painter.circle_filled(
        center + vec2(0., 3.),
        radius + 2.,
        Color32::from_black_alpha((60. * alpha) as u8),
    );
    painter.circle_filled(center, radius, color.gamma_multiply(alpha));
    if ok {
        // La marca se dibuja de izquierda a derecha.
        let draw = ((since - 0.12) / 0.25).clamp(0., 1.);
        let a = center + vec2(-0.42, 0.02) * radius;
        let b = center + vec2(-0.12, 0.32) * radius;
        let c = center + vec2(0.44, -0.30) * radius;
        let stroke = Stroke::new(
            (radius * 0.16).max(2.),
            Color32::WHITE.gamma_multiply(alpha),
        );
        let first = (draw / 0.4).min(1.);
        painter.line_segment([a, a + (b - a) * first], stroke);
        if draw > 0.4 {
            let second = (draw - 0.4) / 0.6;
            painter.line_segment([b, b + (c - b) * second], stroke);
        }
    } else {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            regular::WARNING,
            FontId::proportional(radius * 1.1),
            Color32::WHITE.gamma_multiply(alpha),
        );
    }
    if rect.height() > 90. && rect.width() > 120. {
        chip(painter, center + vec2(0., radius + 22.), message, alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_ends_after_result() {
        let mut scan = Scan::new();
        let now = Instant::now();
        assert!(!scan.over(now + Duration::from_secs(60)));
        scan.finish(true, rapidshot_core::i18n::t("Done"));
        assert!(!scan.over(Instant::now()));
        assert!(scan.over(Instant::now() + RESULT + Duration::from_millis(10)));
    }
}
