//! Paletas de colores en `~/.config/rapidshot/palettes.ron`: las guardadas, los últimos
//! colores del cuentagotas y las cuentas de tonos (más claro, más oscuro, gama de un color).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Rgb = [u8; 3];

/// Colores del cuentagotas que se recuerdan.
pub const PICKED_MAX: usize = 10;
/// Columnas que caben en una paleta.
pub const COLORS_MAX: usize = 24;
/// Tonos de la vista «View shades».
pub const SHADES: usize = 24;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Rgb>,
}

/// Lo que se ve bajo el código HEX de cada color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Secondary {
    #[default]
    Name,
    Rgb,
    Hsl,
    Hsb,
    Cmyk,
    Oklch,
    Lab,
    Lch,
}
impl Secondary {
    pub const ALL: [Secondary; 8] = [
        Secondary::Name,
        Secondary::Rgb,
        Secondary::Hsl,
        Secondary::Hsb,
        Secondary::Cmyk,
        Secondary::Oklch,
        Secondary::Lab,
        Secondary::Lch,
    ];
}

/// Vista del selector de color: el cuadro con el tono, o deslizadores de un modelo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    #[default]
    Picker,
    Hsb,
    Hsl,
    Rgb,
    Cmyk,
    Lab,
    Oklch,
}
impl View {
    pub const ALL: [View; 7] = [
        View::Picker,
        View::Hsb,
        View::Hsl,
        View::Rgb,
        View::Cmyk,
        View::Lab,
        View::Oklch,
    ];
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palettes {
    /// Últimos colores del cuentagotas, el más reciente a la derecha.
    pub picked: Vec<Rgb>,
    pub saved: Vec<Palette>,
    pub secondary: Secondary,
    /// `view` y no `input`: los archivos con el campo viejo se siguen leyendo (se ignora).
    pub view: View,
}

pub fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("rapidshot").join("palettes.ron"))
}

impl Palettes {
    /// Si falta o no se puede leer, vacías.
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), crate::save::SaveError> {
        let path = path().ok_or(crate::save::SaveError::NoConfig)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text)?;
        Ok(())
    }
    /// Añade un color del cuentagotas; si ya estaba, pasa al final.
    pub fn add_picked(&mut self, rgb: Rgb) {
        self.picked.retain(|c| *c != rgb);
        self.picked.push(rgb);
        let extra = self.picked.len().saturating_sub(PICKED_MAX);
        self.picked.drain(..extra);
    }
    /// «Palette N» con el primer número libre.
    pub fn next_name(&self, word: &str) -> String {
        (1..)
            .map(|n| format!("{word} {n}"))
            .find(|name| self.saved.iter().all(|p| &p.name != name))
            .unwrap_or_default()
    }
}

/// HEX sin almohadilla, como en la ventana de paletas.
pub fn hex(c: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// `#RRGGBB`, `RRGGBB` o `RGB`.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let text = text.trim().trim_start_matches('#');
    let digits: Vec<u8> = text
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    match digits[..] {
        [r, g, b] => Some([r * 17, g * 17, b * 17]),
        [r1, r2, g1, g2, b1, b2] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2]),
        _ => None,
    }
}

/// Canales de `c` en las unidades de los deslizadores de `view` (`Picker`, como HSB):
/// HSB y HSL en grados y %, RGB de 0 a 255, CMYK en %, LAB con L de 0 a 100 y OKLCH con
/// L de 0 a 1. El cuarto solo lo usa CMYK.
pub fn channels(c: Rgb, view: View) -> [f32; 4] {
    let pct = |[h, s, v]: [f32; 3]| [h, s * 100., v * 100., 0.];
    match view {
        View::Picker | View::Hsb => pct(to_hsb(c)),
        View::Hsl => pct(to_hsl(c)),
        View::Rgb => [c[0] as f32, c[1] as f32, c[2] as f32, 0.],
        View::Cmyk => {
            let [r, g, b] = c.map(|v| v as f32 / 255.);
            let k = 1. - r.max(g).max(b);
            let part = |v: f32| {
                if k >= 1. {
                    0.
                } else {
                    (1. - v - k) / (1. - k) * 100.
                }
            };
            [part(r), part(g), part(b), k * 100.]
        }
        View::Lab => {
            let [l, a, b] = to_lab(c);
            [l, a, b, 0.]
        }
        View::Oklch => {
            let [l, chroma, hue] = to_oklch(c);
            [l, chroma, hue, 0.]
        }
    }
}

/// El color de unos canales de `view`; `None` si cae fuera de sRGB (LAB y OKLCH).
pub fn from_channels(view: View, [x, y, z, w]: [f32; 4]) -> Option<Rgb> {
    let unit = |v: f32| (v / 100.).clamp(0., 1.);
    match view {
        View::Picker | View::Hsb => Some(from_hsb([x.rem_euclid(360.), unit(y), unit(z)])),
        View::Hsl => Some(from_hsl([x.rem_euclid(360.), unit(y), unit(z)])),
        View::Rgb => Some([x, y, z].map(|v| v.round().clamp(0., 255.) as u8)),
        View::Cmyk => {
            let k = unit(w);
            Some([x, y, z].map(|v| ((1. - unit(v)) * (1. - k) * 255.).round() as u8))
        }
        View::Lab => from_lab([x, y, z]),
        View::Oklch => from_oklch([x, y, z]),
    }
}

fn to_linear(v: u8) -> f32 {
    let v = v as f32 / 255.;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB lineal a 8 bits; `None` si algún canal se sale de 0..1 (fuera de la gama).
fn from_linear(rgb: [f32; 3]) -> Option<Rgb> {
    const SLACK: f32 = 0.001;
    if rgb.iter().any(|v| !(-SLACK..=1. + SLACK).contains(v)) {
        return None;
    }
    Some(rgb.map(|v| {
        let v = v.clamp(0., 1.);
        let v = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        };
        (v * 255.).round() as u8
    }))
}

/// OKLCH de Björn Ottosson: L de 0 a 1, croma y tono en grados (0 en los grises).
pub fn to_oklch(c: Rgb) -> [f32; 3] {
    let [r, g, b] = c.map(to_linear);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let lightness = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let b = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    let (chroma, hue) = polar(a, b, 0.0005);
    [lightness, chroma, hue]
}

pub fn from_oklch([lightness, chroma, hue]: [f32; 3]) -> Option<Rgb> {
    let (a, b) = cartesian(chroma, hue);
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    from_linear([
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ])
}

/// CIE Lab con blanco D50, como `lab()` de CSS: L de 0 a 100.
pub fn to_lab(c: Rgb) -> [f32; 3] {
    let [r, g, b] = c.map(to_linear);
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

pub fn from_lab([l, a, b]: [f32; 3]) -> Option<Rgb> {
    let fy = (l + 16.) / 116.;
    let (fx, fz) = (fy + a / 500., fy - b / 200.);
    let inverse = |t: f32| {
        let cube = t.powi(3);
        if cube > 216. / 24389. {
            cube
        } else {
            (116. * t - 16.) * 27. / 24389.
        }
    };
    let (x, y, z) = (inverse(fx) * 0.964_22, inverse(fy), inverse(fz) * 0.825_21);
    from_linear([
        3.134_136 * x - 1.617_386_3 * y - 0.490_662 * z,
        -0.978_768_5 * x + 1.916_141_6 * y + 0.033_425_09 * z,
        0.071_945_31 * x - 0.228_991_42 * y + 1.405_242_7 * z,
    ])
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

fn cartesian(chroma: f32, hue: f32) -> (f32, f32) {
    let h = hue.to_radians();
    (chroma * h.cos(), chroma * h.sin())
}

/// Tono en grados, saturación y luminosidad de 0 a 1.
pub fn to_hsl(c: Rgb) -> [f32; 3] {
    let [r, g, b] = c.map(|v| v as f32 / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    let d = max - min;
    if d == 0. {
        return [0., 0., l];
    }
    let s = d / (1. - (2. * l - 1.).abs());
    [hue(r, g, b, max, d), s, l]
}

pub fn from_hsl([h, s, l]: [f32; 3]) -> Rgb {
    let c = (1. - (2. * l - 1.).abs()) * s;
    from_chroma(h, c, l - c / 2.)
}

/// Tono en grados, saturación y brillo (HSB/HSV) de 0 a 1.
pub fn to_hsb(c: Rgb) -> [f32; 3] {
    let [r, g, b] = c.map(|v| v as f32 / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let s = if max == 0. { 0. } else { d / max };
    let h = if d == 0. { 0. } else { hue(r, g, b, max, d) };
    [h, s, max]
}

pub fn from_hsb([h, s, v]: [f32; 3]) -> Rgb {
    let c = v * s;
    from_chroma(h, c, v - c)
}

fn hue(r: f32, g: f32, b: f32, max: f32, d: f32) -> f32 {
    let h = if max == r {
        (g - b) / d
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60.).rem_euclid(360.)
}

fn from_chroma(h: f32, c: f32, m: f32) -> Rgb {
    let h = h.rem_euclid(360.) / 60.;
    let x = c * (1. - (h % 2. - 1.).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    [r, g, b].map(|v| ((v + m).clamp(0., 1.) * 255.).round() as u8)
}

/// Un paso más claro: acerca la luminosidad al blanco un tercio.
pub fn lighter(c: Rgb) -> Rgb {
    let [h, s, l] = to_hsl(c);
    from_hsl([h, s, l + (1. - l) * 0.33])
}

/// Un paso más oscuro: quita un tercio de la luminosidad.
pub fn darker(c: Rgb) -> Rgb {
    let [h, s, l] = to_hsl(c);
    from_hsl([h, s, l * 0.67])
}

/// Gama de `SHADES` tonos del color, de casi blanco a casi negro.
pub fn shades(c: Rgb) -> Vec<Rgb> {
    let [h, s, _] = to_hsl(c);
    (0..SHADES)
        .map(|i| {
            let l = 0.96 - 0.92 * i as f32 / (SHADES - 1) as f32;
            from_hsl([h, s, l])
        })
        .collect()
}

/// Luminancia relativa de 0 a 1, para elegir texto claro u oscuro encima.
pub fn luminance(c: Rgb) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.
}

/// Nombre del color con nombre de CSS más cercano.
pub fn name(c: Rgb) -> &'static str {
    // Distancia «redmean»: barata y bastante cercana a la percepción.
    let distance = |n: u32| {
        let o = [(n >> 16) as u8, (n >> 8) as u8, n as u8].map(i32::from);
        let [r, g, b] = c.map(i32::from);
        let mean = (r + o[0]) / 2;
        let (dr, dg, db) = (r - o[0], g - o[1], b - o[2]);
        (512 + mean) * dr * dr / 256 + 4 * dg * dg + (767 - mean) * db * db / 256
    };
    NAMES
        .iter()
        .min_by_key(|(_, n)| distance(*n))
        .map_or("", |(name, _)| name)
}

const NAMES: &[(&str, u32)] = &[
    ("Alice Blue", 0xF0F8FF),
    ("Antique White", 0xFAEBD7),
    ("Aqua", 0x00FFFF),
    ("Aquamarine", 0x7FFFD4),
    ("Azure", 0xF0FFFF),
    ("Beige", 0xF5F5DC),
    ("Bisque", 0xFFE4C4),
    ("Black", 0x000000),
    ("Blanched Almond", 0xFFEBCD),
    ("Blue", 0x0000FF),
    ("Blue Violet", 0x8A2BE2),
    ("Brown", 0xA52A2A),
    ("Burlywood", 0xDEB887),
    ("Cadet Blue", 0x5F9EA0),
    ("Chartreuse", 0x7FFF00),
    ("Chocolate", 0xD2691E),
    ("Coral", 0xFF7F50),
    ("Cornflower Blue", 0x6495ED),
    ("Cornsilk", 0xFFF8DC),
    ("Crimson", 0xDC143C),
    ("Dark Blue", 0x00008B),
    ("Dark Cyan", 0x008B8B),
    ("Dark Goldenrod", 0xB8860B),
    ("Dark Gray", 0xA9A9A9),
    ("Dark Green", 0x006400),
    ("Dark Khaki", 0xBDB76B),
    ("Dark Magenta", 0x8B008B),
    ("Dark Olive Green", 0x556B2F),
    ("Dark Orange", 0xFF8C00),
    ("Dark Orchid", 0x9932CC),
    ("Dark Red", 0x8B0000),
    ("Dark Salmon", 0xE9967A),
    ("Dark Sea Green", 0x8FBC8F),
    ("Dark Slate Blue", 0x483D8B),
    ("Dark Slate Gray", 0x2F4F4F),
    ("Dark Turquoise", 0x00CED1),
    ("Dark Violet", 0x9400D3),
    ("Deep Pink", 0xFF1493),
    ("Deep Sky Blue", 0x00BFFF),
    ("Dim Gray", 0x696969),
    ("Dodger Blue", 0x1E90FF),
    ("Firebrick", 0xB22222),
    ("Floral White", 0xFFFAF0),
    ("Forest Green", 0x228B22),
    ("Gainsboro", 0xDCDCDC),
    ("Ghost White", 0xF8F8FF),
    ("Gold", 0xFFD700),
    ("Goldenrod", 0xDAA520),
    ("Gray", 0x808080),
    ("Green", 0x008000),
    ("Green Yellow", 0xADFF2F),
    ("Honeydew", 0xF0FFF0),
    ("Hot Pink", 0xFF69B4),
    ("Indian Red", 0xCD5C5C),
    ("Indigo", 0x4B0082),
    ("Ivory", 0xFFFFF0),
    ("Khaki", 0xF0E68C),
    ("Lavender", 0xE6E6FA),
    ("Lavender Blush", 0xFFF0F5),
    ("Lawn Green", 0x7CFC00),
    ("Lemon Chiffon", 0xFFFACD),
    ("Light Blue", 0xADD8E6),
    ("Light Coral", 0xF08080),
    ("Light Cyan", 0xE0FFFF),
    ("Light Goldenrod", 0xFAFAD2),
    ("Light Gray", 0xD3D3D3),
    ("Light Green", 0x90EE90),
    ("Light Pink", 0xFFB6C1),
    ("Light Salmon", 0xFFA07A),
    ("Light Sea Green", 0x20B2AA),
    ("Light Sky Blue", 0x87CEFA),
    ("Light Slate Gray", 0x778899),
    ("Light Steel Blue", 0xB0C4DE),
    ("Light Yellow", 0xFFFFE0),
    ("Lime", 0x00FF00),
    ("Lime Green", 0x32CD32),
    ("Linen", 0xFAF0E6),
    ("Magenta", 0xFF00FF),
    ("Maroon", 0x800000),
    ("Medium Aquamarine", 0x66CDAA),
    ("Medium Blue", 0x0000CD),
    ("Medium Orchid", 0xBA55D3),
    ("Medium Purple", 0x9370DB),
    ("Medium Sea Green", 0x3CB371),
    ("Medium Slate Blue", 0x7B68EE),
    ("Medium Spring Green", 0x00FA9A),
    ("Medium Turquoise", 0x48D1CC),
    ("Medium Violet Red", 0xC71585),
    ("Midnight Blue", 0x191970),
    ("Mint Cream", 0xF5FFFA),
    ("Misty Rose", 0xFFE4E1),
    ("Moccasin", 0xFFE4B5),
    ("Navajo White", 0xFFDEAD),
    ("Navy", 0x000080),
    ("Old Lace", 0xFDF5E6),
    ("Olive", 0x808000),
    ("Olive Drab", 0x6B8E23),
    ("Orange", 0xFFA500),
    ("Orange Red", 0xFF4500),
    ("Orchid", 0xDA70D6),
    ("Pale Goldenrod", 0xEEE8AA),
    ("Pale Green", 0x98FB98),
    ("Pale Turquoise", 0xAFEEEE),
    ("Pale Violet Red", 0xDB7093),
    ("Papaya Whip", 0xFFEFD5),
    ("Peach Puff", 0xFFDAB9),
    ("Peru", 0xCD853F),
    ("Pink", 0xFFC0CB),
    ("Plum", 0xDDA0DD),
    ("Powder Blue", 0xB0E0E6),
    ("Purple", 0x800080),
    ("Rebecca Purple", 0x663399),
    ("Red", 0xFF0000),
    ("Rosy Brown", 0xBC8F8F),
    ("Royal Blue", 0x4169E1),
    ("Saddle Brown", 0x8B4513),
    ("Salmon", 0xFA8072),
    ("Sandy Brown", 0xF4A460),
    ("Sea Green", 0x2E8B57),
    ("Seashell", 0xFFF5EE),
    ("Sienna", 0xA0522D),
    ("Silver", 0xC0C0C0),
    ("Sky Blue", 0x87CEEB),
    ("Slate Blue", 0x6A5ACD),
    ("Slate Gray", 0x708090),
    ("Snow", 0xFFFAFA),
    ("Spring Green", 0x00FF7F),
    ("Steel Blue", 0x4682B4),
    ("Tan", 0xD2B48C),
    ("Teal", 0x008080),
    ("Thistle", 0xD8BFD8),
    ("Tomato", 0xFF6347),
    ("Turquoise", 0x40E0D0),
    ("Violet", 0xEE82EE),
    ("Wheat", 0xF5DEB3),
    ("White", 0xFFFFFF),
    ("White Smoke", 0xF5F5F5),
    ("Yellow", 0xFFFF00),
    ("Yellow Green", 0x9ACD32),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picked_keeps_the_last_ten_without_repeats() {
        let mut palettes = Palettes::default();
        for i in 0..15u8 {
            palettes.add_picked([i, 0, 0]);
        }
        palettes.add_picked([7, 0, 0]);
        assert_eq!(palettes.picked.len(), PICKED_MAX);
        assert_eq!(palettes.picked.first(), Some(&[5, 0, 0]));
        assert_eq!(palettes.picked.last(), Some(&[7, 0, 0]));
        let text = ron::to_string(&palettes).unwrap();
        assert_eq!(ron::from_str::<Palettes>(&text).unwrap(), palettes);
    }

    #[test]
    fn channels_round_trip() {
        let c = [0x30, 0x2C, 0x3F];
        for view in View::ALL {
            let back = from_channels(view, channels(c, view)).unwrap();
            let close = back.iter().zip(c).all(|(a, b)| a.abs_diff(b) <= 1);
            assert!(close, "{view:?}: {back:?}");
        }
        assert_eq!(channels([255, 0, 0], View::Cmyk), [0., 100., 100., 0.]);
        // Croma de sobra: fuera de sRGB.
        assert_eq!(from_oklch([0.5, 0.37, 140.]), None);
        assert_eq!(from_lab([50., 120., 0.]), None);
    }

    #[test]
    fn color_math() {
        assert_eq!(parse_hex("#dff6f6"), Some([0xDF, 0xF6, 0xF6]));
        assert_eq!(parse_hex("fff"), Some([255; 3]));
        assert_eq!(parse_hex("12345"), None);
        assert_eq!(hex([0x27, 0x27, 0x42]), "272742");
        for c in [
            [0x64, 0x45, 0x36],
            [0xBB, 0xD6, 0x86],
            [0, 0, 0],
            [255, 255, 255],
        ] {
            assert_eq!(from_hsl(to_hsl(c)), c);
            assert_eq!(from_hsb(to_hsb(c)), c);
        }
        let base = [0xB2, 0x67, 0x5E];
        assert!(luminance(lighter(base)) > luminance(base));
        assert!(luminance(darker(base)) < luminance(base));
        let shades = shades(base);
        assert_eq!(shades.len(), SHADES);
        assert!(luminance(shades[0]) > luminance(shades[SHADES - 1]));
        assert_eq!(name([0xE0, 0xFF, 0xFF]), "Light Cyan");
        assert_eq!(name([250, 2, 3]), "Red");
    }

    #[test]
    fn next_name_skips_used_numbers() {
        let mut palettes = Palettes::default();
        palettes.saved.push(Palette {
            name: "Palette 1".into(),
            colors: Vec::new(),
        });
        assert_eq!(palettes.next_name("Palette"), "Palette 2");
    }
}
