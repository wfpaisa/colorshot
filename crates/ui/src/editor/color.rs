//! Formatos para copiar un color con Tab, como un menú «Color Format».
use egui::{Color32, Pos2, Rect};
use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Format {
    #[default]
    Hex,
    HexBare,
    Css,
    Unit,
    Android,
    UiColor,
    CSharp,
    /// CSS `oklch()`.
    Oklch,
    /// CSS `lab()`: CIE Lab con blanco D50.
    Lab,
    /// CSS `lch()`: la forma polar de `lab()`.
    Lch,
}
impl Format {
    pub const ALL: [Format; 10] = [
        Format::Hex,
        Format::HexBare,
        Format::Css,
        Format::Unit,
        Format::Android,
        Format::UiColor,
        Format::CSharp,
        Format::Oklch,
        Format::Lab,
        Format::Lch,
    ];
    /// Nombre corto, para la lista de Preferencias.
    pub fn name(self) -> &'static str {
        match self {
            Format::Hex => "HEX #FFFFFF",
            Format::HexBare => "HEX FFFFFF",
            Format::Css => "CSS rgba()",
            Format::Unit => "RGB 0–1",
            Format::Android => "Android 0xFF…",
            Format::UiColor => "UIColor",
            Format::CSharp => "C# Color.FromArgb()",
            Format::Oklch => "CSS oklch()",
            Format::Lab => "CSS lab()",
            Format::Lch => "CSS lch()",
        }
    }
    /// Entrada del menú «Color Format»: el nombre con la muestra.
    pub fn label(self, c: Color32) -> String {
        let text = self.text(c);
        let label = match self {
            Format::Hex | Format::HexBare => format!("HEX {text}"),
            Format::Css => format!("CSS {text}"),
            Format::Unit => format!("RGB ({text})"),
            Format::Android => format!("Android {text}"),
            Format::UiColor => text.replacen('(', " (", 1),
            Format::CSharp => "C# Color.FromArgb()".to_owned(),
            Format::Oklch | Format::Lab | Format::Lch => text,
        };
        // Las largas se cortan: «UIColor (red: 1.00, gre…».
        match label.char_indices().nth(24) {
            Some((at, _)) => format!("{}…", &label[..at]),
            None => label,
        }
    }
    pub fn text(self, c: Color32) -> String {
        let (r, g, b) = (c.r(), c.g(), c.b());
        let unit = |v: u8| format!("{:.2}", v as f32 / 255.);
        match self {
            Format::Hex => format!("#{r:02X}{g:02X}{b:02X}"),
            Format::HexBare => format!("{r:02X}{g:02X}{b:02X}"),
            Format::Css => format!("rgba({r}, {g}, {b}, 1)"),
            Format::Unit => format!("{}, {}, {}", unit(r), unit(g), unit(b)),
            Format::Android => format!("0xFF{r:02X}{g:02X}{b:02X}"),
            Format::UiColor => format!(
                "UIColor(red: {}, green: {}, blue: {}, alpha: 1)",
                unit(r),
                unit(g),
                unit(b)
            ),
            Format::CSharp => format!("Color.FromArgb({r}, {g}, {b})"),
            Format::Oklch => {
                let [l, a, b] = oklab(c);
                let (chroma, hue) = polar(a, b, 0.0005);
                format!(
                    "oklch({}% {} {})",
                    fixed(l * 100., 1),
                    fixed(chroma, 3),
                    fixed(hue, 1)
                )
            }
            Format::Lab => {
                let [l, a, b] = lab(c);
                format!("lab({}% {} {})", fixed(l, 2), fixed(a, 2), fixed(b, 2))
            }
            Format::Lch => {
                let [l, a, b] = lab(c);
                let (chroma, hue) = polar(a, b, 0.005);
                format!(
                    "lch({}% {} {})",
                    fixed(l, 2),
                    fixed(chroma, 2),
                    fixed(hue, 1)
                )
            }
        }
    }
}

/// `v` con `decimals` decimales, sin «-0».
fn fixed(v: f32, decimals: usize) -> String {
    let text = format!("{v:.decimals$}");
    match text.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_owned(),
        _ => text,
    }
}

/// Croma y tono en grados; sin croma (grises), el tono es 0.
fn polar(a: f32, b: f32, gray: f32) -> (f32, f32) {
    let chroma = a.hypot(b);
    let hue = if chroma < gray {
        0.
    } else {
        b.atan2(a).to_degrees().rem_euclid(360.)
    };
    (chroma, hue)
}

/// Canales sRGB a lineales, de 0 a 1.
fn linear(c: Color32) -> [f32; 3] {
    [c.r(), c.g(), c.b()].map(|v| {
        let v = v as f32 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// OKLab de Björn Ottosson: L de 0 a 1.
fn oklab(c: Color32) -> [f32; 3] {
    let [r, g, b] = linear(c);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// CIE Lab con blanco D50, como `lab()` de CSS: L de 0 a 100.
fn lab(c: Color32) -> [f32; 3] {
    let [r, g, b] = linear(c);
    // sRGB lineal a XYZ ya adaptado a D50 (Bradford), dividido por el blanco D50.
    let x = (0.436_074_7 * r + 0.385_064_9 * g + 0.143_080_4 * b) / 0.964_22;
    let y = 0.222_504_5 * r + 0.716_878_6 * g + 0.060_616_9 * b;
    let z = (0.013_932_2 * r + 0.097_104_5 * g + 0.714_173_3 * b) / 0.825_21;
    let f = |t: f32| {
        if t > 216. / 24389. {
            t.cbrt()
        } else {
            (24389. / 27. * t + 16.) / 116.
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116. * fy - 16., 500. * (fx - fy), 200. * (fy - fz)]
}

/// Radio en el que ⇧Tab busca el color más oscuro alrededor del puntero.
pub const DARK_RADIUS: i64 = 20;

/// El píxel más oscuro a `DARK_RADIUS` px de `(x, y)`: el del texto bajo el puntero.
pub fn darkest(image: &RgbaImage, x: u32, y: u32) -> Option<Color32> {
    let (w, h) = (image.width() as i64, image.height() as i64);
    let mut best: Option<(u32, Color32)> = None;
    for py in (y as i64 - DARK_RADIUS).max(0)..(y as i64 + DARK_RADIUS + 1).min(h) {
        for px in (x as i64 - DARK_RADIUS).max(0)..(x as i64 + DARK_RADIUS + 1).min(w) {
            let p = image.get_pixel(px as u32, py as u32);
            let light = p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114;
            if best.is_none_or(|(l, _)| light < l) {
                best = Some((light, Color32::from_rgb(p[0], p[1], p[2])));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Color medio de la zona.
pub fn average(image: &RgbaImage, zone: Rect) -> Option<Color32> {
    let full = Rect::from_min_size(
        Pos2::ZERO,
        egui::vec2(image.width() as f32, image.height() as f32),
    );
    let zone = zone.intersect(full);
    let (x0, y0) = (zone.left().floor() as u32, zone.top().floor() as u32);
    let (x1, y1) = (zone.right().ceil() as u32, zone.bottom().ceil() as u32);
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    for y in y0..y1.min(image.height()) {
        for x in x0..x1.min(image.width()) {
            let p = image.get_pixel(x, y);
            for c in 0..3 {
                sum[c] += p[c] as u64;
            }
            n += 1;
        }
    }
    (n > 0).then(|| Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn formats() {
        let c = Color32::from_rgb(240, 60, 40);
        assert_eq!(Format::Hex.text(c), "#F03C28");
        assert_eq!(Format::HexBare.text(c), "F03C28");
        assert_eq!(Format::Css.text(c), "rgba(240, 60, 40, 1)");
        assert_eq!(Format::Unit.text(c), "0.94, 0.24, 0.16");
        assert_eq!(Format::Android.text(c), "0xFFF03C28");
        assert_eq!(Format::CSharp.text(c), "Color.FromArgb(240, 60, 40)");
        assert_eq!(Format::Unit.label(c), "RGB (0.94, 0.24, 0.16)");
        assert_eq!(Format::UiColor.label(c), "UIColor (red: 0.94, gree…");
        let red = Color32::from_rgb(255, 0, 0);
        assert_eq!(Format::Oklch.text(red), "oklch(62.8% 0.258 29.2)");
        assert_eq!(Format::Lab.text(red), "lab(54.29% 80.81 69.89)");
        assert_eq!(Format::Lch.text(red), "lch(54.29% 106.84 40.9)");
        assert_eq!(
            Format::Oklch.text(Color32::WHITE),
            "oklch(100.0% 0.000 0.0)"
        );
        assert_eq!(Format::Lab.text(Color32::BLACK), "lab(0.00% 0.00 0.00)");
    }
    #[test]
    fn darkest_and_average() {
        let mut image = RgbaImage::from_pixel(60, 60, Rgba([200, 200, 200, 255]));
        image.put_pixel(15, 12, Rgba([10, 20, 30, 255]));
        image.put_pixel(50, 50, Rgba([0, 0, 0, 255]));
        assert_eq!(darkest(&image, 10, 10), Some(Color32::from_rgb(10, 20, 30)));
        let mut image = RgbaImage::from_pixel(4, 4, Rgba([100, 100, 100, 255]));
        image.put_pixel(0, 0, Rgba([200, 200, 200, 255]));
        let zone = Rect::from_min_max(Pos2::ZERO, Pos2::new(2., 1.));
        assert_eq!(average(&image, zone), Some(Color32::from_gray(150)));
    }
}
