//! Geometría de selección en píxeles físicos, independiente del contexto gráfico.
use colorshot_core::capture::Rect;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}
pub const HANDLES: [Handle; 8] = [
    Handle::Nw,
    Handle::N,
    Handle::Ne,
    Handle::E,
    Handle::Se,
    Handle::S,
    Handle::Sw,
    Handle::W,
];
pub fn point(r: Rect, h: Handle) -> (f32, f32) {
    let (x, y, w, t) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    match h {
        Handle::Nw => (x, y),
        Handle::N => (x + w / 2., y),
        Handle::Ne => (x + w, y),
        Handle::E => (x + w, y + t / 2.),
        Handle::Se => (x + w, y + t),
        Handle::S => (x + w / 2., y + t),
        Handle::Sw => (x, y + t),
        Handle::W => (x, y + t / 2.),
    }
}
pub fn hit_test(r: Rect, p: (f32, f32), tolerance: (f32, f32)) -> Option<Handle> {
    HANDLES.into_iter().find(|h| {
        let q = point(r, *h);
        (p.0 - q.0).abs() <= tolerance.0 && (p.1 - q.1).abs() <= tolerance.1
    })
}
pub fn contains(r: Rect, p: (u32, u32)) -> bool {
    p.0 >= r.x && p.1 >= r.y && p.0 <= r.x.saturating_add(r.w) && p.1 <= r.y.saturating_add(r.h)
}
pub fn translate(r: Rect, delta: (i64, i64), bounds: (u32, u32)) -> Rect {
    let w = r.w.min(bounds.0);
    let h = r.h.min(bounds.1);
    Rect {
        x: (r.x as i64 + delta.0).clamp(0, (bounds.0 - w) as i64) as u32,
        y: (r.y as i64 + delta.1).clamp(0, (bounds.1 - h) as i64) as u32,
        w,
        h,
    }
}
pub fn create(a: (u32, u32), b: (u32, u32), bounds: (u32, u32), square: bool) -> Rect {
    let a = (a.0.min(bounds.0), a.1.min(bounds.1));
    let mut b = (b.0.min(bounds.0), b.1.min(bounds.1));
    if square {
        let size =
            a.0.abs_diff(b.0)
                .max(a.1.abs_diff(b.1))
                .min(if b.0 < a.0 { a.0 } else { bounds.0 - a.0 })
                .min(if b.1 < a.1 { a.1 } else { bounds.1 - a.1 });
        b = (
            if b.0 < a.0 { a.0 - size } else { a.0 + size },
            if b.1 < a.1 { a.1 - size } else { a.1 + size },
        );
    }
    Rect::normalized(a, b)
}
pub fn resize(r: Rect, h: Handle, p: (u32, u32), bounds: (u32, u32), square: bool) -> Rect {
    let (left, right, top, bottom) = (r.x, r.x + r.w, r.y, r.y + r.h);
    let a = (
        match h {
            Handle::Nw | Handle::W | Handle::Sw => right,
            _ => left,
        },
        match h {
            Handle::Nw | Handle::N | Handle::Ne => bottom,
            _ => top,
        },
    );
    let b = (
        if matches!(h, Handle::N | Handle::S) {
            right
        } else {
            p.0
        },
        if matches!(h, Handle::E | Handle::W) {
            bottom
        } else {
            p.1
        },
    );
    create(a, b, bounds, square)
}
#[cfg(test)]
mod tests {
    use super::*;
    const R: Rect = Rect {
        x: 10,
        y: 20,
        w: 30,
        h: 40,
    };
    #[test]
    fn moving_clamps_all_edges() {
        assert_eq!(
            translate(R, (-100, 200), (100, 100)),
            Rect { x: 0, y: 60, ..R }
        );
    }
    #[test]
    fn handles_match_all_positions() {
        for h in HANDLES {
            assert_eq!(hit_test(R, point(R, h), (1., 1.)), Some(h));
        }
        assert_eq!(hit_test(R, (25., 40.), (1., 1.)), None);
    }
    #[test]
    fn square_stays_inside_bounds() {
        let r = create((80, 80), (99, 50), (100, 100), true);
        assert_eq!(r.w, r.h);
        assert!(r.x + r.w <= 100);
        assert!(r.y + r.h <= 100);
    }
    #[test]
    fn resize_crosses_anchor_and_preserves_other_axis() {
        assert_eq!(
            resize(R, Handle::W, (60, 0), (100, 100), false),
            Rect {
                x: 40,
                y: 20,
                w: 20,
                h: 40
            }
        );
        for h in HANDLES {
            let r = resize(R, h, (95, 95), (100, 100), true);
            assert_eq!(r.w, r.h);
        }
    }
}
