//! Acciones sobre la zona seleccionada: ajustar al contenido, agrandar y contraste.
use egui::{Pos2, Rect};
use image::{Rgba, RgbaImage};

/// Diferencia máxima por canal para tratar dos píxeles como el mismo fondo.
const TOLERANCE: u8 = 10;
/// Lo que agranda la zona cada pulsación de ⇧+], por lado.
pub const GROW: f32 = 20.;

fn same(a: &Rgba<u8>, b: &Rgba<u8>) -> bool {
    (0..3).all(|c| a[c].abs_diff(b[c]) <= TOLERANCE)
}

/// Zona en píxeles enteros dentro de la imagen: (x0, y0, x1, y1) con x1, y1 exclusivos.
fn pixels(image: &RgbaImage, zone: Rect) -> (u32, u32, u32, u32) {
    let (w, h) = image.dimensions();
    let x0 = (zone.left().round().max(0.) as u32).min(w);
    let y0 = (zone.top().round().max(0.) as u32).min(h);
    let x1 = (zone.right().round().max(0.) as u32).clamp(x0, w);
    let y1 = (zone.bottom().round().max(0.) as u32).clamp(y0, h);
    (x0, y0, x1, y1)
}

/// Encoge cada lado mientras su línea de píxeles sea del color del fondo (el de la esquina),
/// como «Autoadjust».
pub fn autoadjust(image: &RgbaImage, zone: Rect) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = pixels(image, zone);
    if x1 == x0 || y1 == y0 {
        return zone;
    }
    let background = *image.get_pixel(x0, y0);
    let row = |y: u32, x0: u32, x1: u32| (x0..x1).all(|x| same(image.get_pixel(x, y), &background));
    let column =
        |x: u32, y0: u32, y1: u32| (y0..y1).all(|y| same(image.get_pixel(x, y), &background));
    while y1 - y0 > 1 && x1 > x0 && row(y0, x0, x1) {
        y0 += 1;
    }
    while y1 - y0 > 1 && x1 > x0 && row(y1 - 1, x0, x1) {
        y1 -= 1;
    }
    while x1 - x0 > 1 && y1 > y0 && column(x0, y0, y1) {
        x0 += 1;
    }
    while x1 - x0 > 1 && y1 > y0 && column(x1 - 1, y0, y1) {
        x1 -= 1;
    }
    Rect::from_min_max(
        Pos2::new(x0 as f32, y0 as f32),
        Pos2::new(x1 as f32, y1 as f32),
    )
}

/// Agranda la zona `GROW` px por lado sin salir de la imagen.
pub fn grow(image: &RgbaImage, zone: Rect) -> Rect {
    let full = Rect::from_min_size(
        Pos2::ZERO,
        egui::vec2(image.width() as f32, image.height() as f32),
    );
    zone.expand(GROW).intersect(full)
}

/// Luminancia relativa de WCAG.
fn luminance(color: [f32; 3]) -> f32 {
    let channel = |v: f32| {
        let v = v / 255.;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
}

/// Razón de contraste entre los dos colores que más se repiten en la zona (texto y fondo),
/// o `None` si la zona es de un solo color.
pub fn contrast(image: &RgbaImage, zone: Rect) -> Option<f32> {
    let (x0, y0, x1, y1) = pixels(image, zone);
    let area = (x1 - x0) as u64 * (y1 - y0) as u64;
    if area == 0 {
        return None;
    }
    // Basta una muestra de unos 40 000 píxeles para zonas grandes.
    let stride = ((area as f64 / 40_000.).sqrt().ceil() as usize).max(1);
    let mut buckets = std::collections::HashMap::<[u8; 3], ([u64; 3], u64)>::new();
    for y in (y0..y1).step_by(stride) {
        for x in (x0..x1).step_by(stride) {
            let p = image.get_pixel(x, y);
            let entry = buckets
                .entry([p[0] >> 4, p[1] >> 4, p[2] >> 4])
                .or_default();
            for c in 0..3 {
                entry.0[c] += p[c] as u64;
            }
            entry.1 += 1;
        }
    }
    let mut ranked: Vec<_> = buckets.into_values().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let mean = |(sum, n): &([u64; 3], u64)| sum.map(|s| s as f32 / *n as f32);
    let (a, b) = (
        luminance(mean(ranked.first()?)),
        luminance(mean(ranked.get(1)?)),
    );
    Some((a.max(b) + 0.05) / (a.min(b) + 0.05))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn picture() -> RgbaImage {
        // Fondo blanco con un bloque negro en (10, 5)–(20, 15).
        let mut image = RgbaImage::from_pixel(40, 30, Rgba([255; 4]));
        for y in 5..15 {
            for x in 10..20 {
                image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        image
    }
    #[test]
    fn autoadjust_tightens_to_content() {
        let image = picture();
        let zone = Rect::from_min_max(Pos2::new(2., 1.), Pos2::new(35., 28.));
        assert_eq!(
            autoadjust(&image, zone),
            Rect::from_min_max(Pos2::new(10., 5.), Pos2::new(20., 15.))
        );
    }
    #[test]
    fn grow_stays_inside_image() {
        let image = picture();
        let zone = Rect::from_min_max(Pos2::new(10., 5.), Pos2::new(20., 15.));
        assert_eq!(
            grow(&image, zone),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(40., 30.))
        );
    }
    #[test]
    fn black_on_white_is_21() {
        let image = picture();
        let zone = Rect::from_min_max(Pos2::new(5., 0.), Pos2::new(25., 20.));
        let ratio = contrast(&image, zone).unwrap();
        assert!((ratio - 21.).abs() < 0.01);
        let flat = Rect::from_min_max(Pos2::new(25., 0.), Pos2::new(40., 30.));
        assert_eq!(contrast(&image, flat), None);
    }
}
