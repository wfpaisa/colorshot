//! Figuras ya dibujadas: selección, tiradores, mover y estilo, en píxeles de imagen.
use super::model::{Fill, Shape, Style, marker_width, text_size, text_width};
use egui::{Pos2, Rect, Vec2};

/// Caja que ocupa la figura.
pub fn bounds(shape: &Shape) -> Rect {
    match shape {
        Shape::Arrow { from, to, .. } | Shape::Line { from, to, .. } => {
            Rect::from_two_pos(*from, *to)
        }
        Shape::Rect { rect, .. }
        | Shape::Oval { rect, .. }
        | Shape::Blur { rect, .. }
        | Shape::Spotlight { rect, .. } => *rect,
        Shape::Freehand { points, .. } => Rect::from_points(points),
        Shape::Highlighter { points, style } => {
            Rect::from_points(points).expand(marker_width(*style) / 2.)
        }
        Shape::Counter { at, style, .. } => {
            Rect::from_center_size(*at, Vec2::splat(radius(*style) * 2.))
        }
        Shape::Text { at, text, style } => {
            let size = text_size(*style);
            let lines = text.lines().count().max(1) as f32;
            let width = text.lines().map(|l| text_width(l, size)).fold(0., f32::max);
            Rect::from_min_size(*at, egui::vec2(width, lines * size * 1.2))
        }
    }
}

/// Radio del círculo del contador.
pub fn radius(style: Style) -> f32 {
    (style.width * 3.).max(12.)
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(f32::EPSILON)).clamp(0., 1.);
    p.distance(a + ab * t)
}

/// ¿Toca `p` la figura? Las figuras huecas solo por su trazo, para poder dibujar dentro.
pub fn hit(shape: &Shape, p: Pos2, tolerance: f32) -> bool {
    match shape {
        Shape::Arrow { from, to, style } | Shape::Line { from, to, style } => {
            segment_distance(p, *from, *to) <= tolerance + style.width / 2.
        }
        Shape::Freehand { points, style } => points
            .windows(2)
            .any(|w| segment_distance(p, w[0], w[1]) <= tolerance + style.width / 2.),
        Shape::Highlighter { points, style } => points
            .windows(2)
            .any(|w| segment_distance(p, w[0], w[1]) <= tolerance + marker_width(*style) / 2.),
        Shape::Rect { rect, style } if style.fill != Fill::Outline => {
            rect.expand(tolerance + style.width / 2.).contains(p)
        }
        Shape::Rect { rect, style } => {
            let reach = tolerance + style.width / 2.;
            rect.expand(reach).contains(p) && !rect.shrink(reach).contains(p)
        }
        Shape::Oval { rect, style } => {
            let r = rect.size() / 2.;
            let d = p - rect.center();
            // Distancia aproximada al borde de la elipse.
            let k = (d.x / r.x.max(1.)).hypot(d.y / r.y.max(1.));
            (k - 1.).abs() * r.min_elem().max(1.) <= tolerance + style.width / 2.
        }
        Shape::Counter { at, style, .. } => p.distance(*at) <= radius(*style) + tolerance,
        _ => bounds(shape).expand(tolerance).contains(p),
    }
}

/// Puntos que se pueden arrastrar: extremos de líneas y esquinas de rectángulos.
pub fn handles(shape: &Shape) -> Vec<Pos2> {
    match shape {
        Shape::Arrow { from, to, .. } | Shape::Line { from, to, .. } => vec![*from, *to],
        Shape::Rect { rect, .. }
        | Shape::Oval { rect, .. }
        | Shape::Blur { rect, .. }
        | Shape::Spotlight { rect, .. } => vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ],
        _ => Vec::new(),
    }
}

/// Lleva el tirador `index` a `p`; en un rectángulo la esquina opuesta queda fija.
pub fn set_handle(shape: &mut Shape, index: usize, p: Pos2) {
    match shape {
        Shape::Arrow { from, to, .. } | Shape::Line { from, to, .. } => {
            *if index == 0 { from } else { to } = p;
        }
        Shape::Rect { rect, .. }
        | Shape::Oval { rect, .. }
        | Shape::Blur { rect, .. }
        | Shape::Spotlight { rect, .. } => {
            let opposite = [
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
                rect.right_top(),
            ][index % 4];
            *rect = Rect::from_two_pos(opposite, p);
        }
        _ => {}
    }
}

pub fn translate(shape: &mut Shape, d: Vec2) {
    match shape {
        Shape::Arrow { from, to, .. } | Shape::Line { from, to, .. } => {
            *from += d;
            *to += d;
        }
        Shape::Rect { rect, .. }
        | Shape::Oval { rect, .. }
        | Shape::Blur { rect, .. }
        | Shape::Spotlight { rect, .. } => *rect = rect.translate(d),
        Shape::Freehand { points, .. } | Shape::Highlighter { points, .. } => {
            points.iter_mut().for_each(|p| *p += d)
        }
        Shape::Counter { at, .. } | Shape::Text { at, .. } => *at += d,
    }
}

pub fn style_mut(shape: &mut Shape) -> Option<&mut Style> {
    match shape {
        Shape::Arrow { style, .. }
        | Shape::Line { style, .. }
        | Shape::Rect { style, .. }
        | Shape::Oval { style, .. }
        | Shape::Freehand { style, .. }
        | Shape::Highlighter { style, .. }
        | Shape::Counter { style, .. }
        | Shape::Text { style, .. } => Some(style),
        Shape::Blur { .. } | Shape::Spotlight { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hollow_shapes_hit_on_stroke_and_handles_resize() {
        let style = Style::default();
        let mut rect = Shape::Rect {
            rect: Rect::from_min_max(Pos2::ZERO, egui::pos2(100., 50.)),
            style,
        };
        assert!(hit(&rect, egui::pos2(100., 25.), 3.));
        assert!(!hit(&rect, egui::pos2(50., 25.), 3.));
        // La esquina inferior derecha pasa al otro lado de la opuesta: el rectángulo se voltea.
        set_handle(&mut rect, 2, egui::pos2(-20., -10.));
        assert_eq!(
            bounds(&rect),
            Rect::from_min_max(egui::pos2(-20., -10.), Pos2::ZERO)
        );
        let mut line = Shape::Line {
            from: Pos2::ZERO,
            to: egui::pos2(100., 0.),
            style,
        };
        assert!(hit(&line, egui::pos2(50., 3.), 2.));
        assert!(!hit(&line, egui::pos2(50., 10.), 2.));
        translate(&mut line, egui::vec2(5., 5.));
        assert_eq!(
            handles(&line),
            vec![egui::pos2(5., 5.), egui::pos2(105., 5.)]
        );
    }
}
