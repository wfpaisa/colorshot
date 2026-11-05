//! Exportar e importar paletas en archivos de muestras.
//!
//! ASE (Adobe Swatch Exchange) es el formato más extendido: lo leen Photoshop, Illustrator,
//! Affinity, Inkscape y Krita, y guarda varias paletas en un archivo (un grupo por paleta).
//! Al importar se acepta también GPL (GIMP), que guarda una sola.
use crate::palettes::{self, COLORS_MAX, Palette, Rgb};

const GROUP_START: u16 = 0xC001;
const GROUP_END: u16 = 0xC002;
const COLOR: u16 = 0x0001;
/// Tipo de color «normal» (ni global ni tinta plana).
const NORMAL: u16 = 2;

/// Las paletas como ASE: un grupo por paleta y cada color en RGB, con su HEX de nombre.
pub fn to_ase(list: &[Palette]) -> Vec<u8> {
    let mut blocks: Vec<(u16, Vec<u8>)> = Vec::new();
    for palette in list {
        blocks.push((GROUP_START, utf16(&palette.name)));
        for &c in &palette.colors {
            let mut data = utf16(&format!("#{}", palettes::hex(c)));
            data.extend_from_slice(b"RGB ");
            for v in c {
                data.extend_from_slice(&(f32::from(v) / 255.).to_be_bytes());
            }
            data.extend_from_slice(&NORMAL.to_be_bytes());
            blocks.push((COLOR, data));
        }
        blocks.push((GROUP_END, Vec::new()));
    }
    let mut out = b"ASEF".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(blocks.len() as u32).to_be_bytes());
    for (kind, data) in blocks {
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&data);
    }
    out
}

/// Lee un archivo de muestras (ASE o GPL); `fallback` nombra la paleta que no trae nombre.
pub fn import(bytes: &[u8], fallback: &str) -> Result<Vec<Palette>, String> {
    let list = if bytes.starts_with(b"ASEF") {
        from_ase(bytes, fallback)?
    } else if bytes.starts_with(b"GIMP Palette") {
        vec![from_gpl(&String::from_utf8_lossy(bytes), fallback)]
    } else {
        return Err("Unknown palette file".into());
    };
    let list: Vec<Palette> = list
        .into_iter()
        .filter(|p| !p.colors.is_empty())
        .map(|mut p| {
            p.colors.truncate(COLORS_MAX);
            p
        })
        .collect();
    if list.is_empty() {
        return Err("The file has no colors".into());
    }
    Ok(list)
}

/// Grupos de ASE como paletas; los colores sueltos van a una con el nombre `fallback`.
fn from_ase(bytes: &[u8], fallback: &str) -> Result<Vec<Palette>, String> {
    let mut reader = Reader { bytes, at: 4 };
    let invalid = || "Invalid ASE file".to_string();
    reader.u16().ok_or_else(invalid)?;
    reader.u16().ok_or_else(invalid)?;
    let count = reader.u32().ok_or_else(invalid)?;
    let mut loose = Palette {
        name: fallback.into(),
        colors: Vec::new(),
    };
    let mut list = Vec::new();
    let mut group: Option<Palette> = None;
    for _ in 0..count {
        let kind = reader.u16().ok_or_else(invalid)?;
        let len = reader.u32().ok_or_else(invalid)? as usize;
        let data = reader.take(len).ok_or_else(invalid)?;
        match kind {
            GROUP_START => {
                let mut block = Reader { bytes: data, at: 0 };
                let name = block.name().unwrap_or_default();
                group = Some(Palette {
                    name: if name.is_empty() {
                        fallback.into()
                    } else {
                        name
                    },
                    colors: Vec::new(),
                });
            }
            GROUP_END => list.extend(group.take()),
            COLOR => {
                // Un color que no se entiende (modelo raro) se salta sin estropear el resto.
                let Some(c) = ase_color(data) else { continue };
                match &mut group {
                    Some(g) => g.colors.push(c),
                    None => loose.colors.push(c),
                }
            }
            _ => {}
        }
    }
    // Un grupo sin cierre también cuenta.
    list.extend(group);
    if !loose.colors.is_empty() {
        list.insert(0, loose);
    }
    Ok(list)
}

fn ase_color(data: &[u8]) -> Option<Rgb> {
    let mut block = Reader { bytes: data, at: 0 };
    block.name()?;
    let model = block.take(4)?;
    let mut floats = |n: usize| -> Option<Vec<f32>> { (0..n).map(|_| block.f32()).collect() };
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    match model {
        b"RGB " => {
            let v = floats(3)?;
            Some([byte(v[0]), byte(v[1]), byte(v[2])])
        }
        b"Gray" => {
            let v = byte(floats(1)?[0]);
            Some([v, v, v])
        }
        b"CMYK" => {
            let v = floats(4)?;
            let k = 1. - v[3];
            Some([
                byte((1. - v[0]) * k),
                byte((1. - v[1]) * k),
                byte((1. - v[2]) * k),
            ])
        }
        b"LAB " => {
            // L suele ir de 0 a 1 (Adobe), pero algunos programas la escriben de 0 a 100.
            let v = floats(3)?;
            let l = if v[0] <= 1. { v[0] * 100. } else { v[0] };
            Some(palettes::from_lab([l, v[1], v[2]]).unwrap_or_else(|| {
                // Fuera de sRGB: el gris de la misma luminosidad antes que perder el color.
                let gray = byte(l / 100.);
                [gray, gray, gray]
            }))
        }
        _ => None,
    }
}

/// GPL: cabecera, `Name:` opcional, comentarios con `#` y una línea `R G B nombre` por color.
fn from_gpl(text: &str, fallback: &str) -> Palette {
    let mut palette = Palette {
        name: fallback.into(),
        colors: Vec::new(),
    };
    for line in text.lines().skip(1) {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("Name:") {
            let name = name.trim();
            if !name.is_empty() {
                palette.name = name.into();
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let numbers: Vec<u8> = line
            .split_whitespace()
            .take(3)
            .map_while(|n| n.parse().ok())
            .collect();
        if let [r, g, b] = numbers[..] {
            palette.colors.push([r, g, b]);
        }
    }
    palette
}

/// Nombre de ASE: longitud en unidades UTF-16 (con el cero final) y el texto en UTF-16BE.
fn utf16(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let mut out = (units.len() as u16).to_be_bytes().to_vec();
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(slice)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn name(&mut self) -> Option<String> {
        let len = self.u16()? as usize;
        let units: Vec<u16> = (0..len).map(|_| self.u16()).collect::<Option<_>>()?;
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        Some(String::from_utf16_lossy(&units[..end]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ase_round_trip() {
        let list = vec![
            Palette {
                name: "Sunset ñ".into(),
                colors: vec![[255, 94, 58], [39, 39, 66]],
            },
            Palette {
                name: "Mono".into(),
                colors: vec![[0, 0, 0]],
            },
        ];
        let bytes = to_ase(&list);
        assert_eq!(import(&bytes, "Imported").unwrap(), list);
    }

    #[test]
    fn reads_gpl_and_rejects_garbage() {
        let gpl =
            "GIMP Palette\nName: Warm\nColumns: 4\n# comment\n255   0   0\tRed\n  0 128 255 Blue\n";
        let list = import(gpl.as_bytes(), "Imported").unwrap();
        assert_eq!(list[0].name, "Warm");
        assert_eq!(list[0].colors, vec![[255, 0, 0], [0, 128, 255]]);
        assert!(import(b"hello", "Imported").is_err());
        assert!(import(b"ASEF\0\x01", "Imported").is_err());
    }

    #[test]
    fn ase_cmyk_and_loose_colors() {
        let mut data = utf16("Cyan");
        data.extend_from_slice(b"CMYK");
        for v in [1f32, 0., 0., 0.] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        data.extend_from_slice(&NORMAL.to_be_bytes());
        let mut bytes = b"ASEF\0\x01\0\0\0\0\0\x01".to_vec();
        bytes.extend_from_slice(&COLOR.to_be_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&data);
        let list = import(&bytes, "Imported").unwrap();
        assert_eq!(list[0].name, "Imported");
        assert_eq!(list[0].colors, vec![[0, 255, 255]]);
    }
}
