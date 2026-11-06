//! Iconos SVG de `assets/icons`, instalados con `just icons` en ~/.local/share/colorshot/icons
//! para editarlos sin recompilar. Si faltan, cada botón usa su glifo de Phosphor.
use egui::{Button, RichText, TextureHandle};
use std::path::PathBuf;

/// Los iconos se dibujan varias veces más grandes de lo que se pintan: sin mipmaps, el filtro
/// lineal salta píxeles al reducirlos y los bordes quedan escalonados.
pub(crate) const SMOOTH: egui::TextureOptions = egui::TextureOptions {
    mipmap_mode: Some(egui::TextureFilter::Linear),
    ..egui::TextureOptions::LINEAR
};

pub fn dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("colorshot").join("icons"))
}

/// Icono de plantilla (`name.svg`) en blanco con su alfa, para teñirlo al pintar.
pub fn load(name: &str) -> Option<image::RgbaImage> {
    let mut image = svg(&dir()?.join(format!("{name}.svg")))?;
    for pixel in image.pixels_mut() {
        pixel.0[..3].fill(255);
    }
    Some(image)
}

/// SVG dibujado al doble de su tamaño lógico. Solo cuenta el alfa.
fn svg(path: &std::path::Path) -> Option<image::RgbaImage> {
    render(&std::fs::read(path).ok()?, 2.)
}

/// SVG dibujado a `scale` veces su tamaño, en RGBA sin premultiplicar.
fn render(data: &[u8], scale: f32) -> Option<image::RgbaImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let size = tree.size().to_int_size().scale_by(scale)?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let c = pixel.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    image::RgbaImage::from_raw(size.width(), size.height(), rgba)
}

/// Logo de la aplicación (`assets/logo.svg`) a 128 px, para el icono de las ventanas en X11.
pub fn logo() -> Option<egui::IconData> {
    let svg = include_bytes!("../../../assets/logo.svg");
    let image = render(svg, 128. / 512.)?;
    Some(egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

/// Logo pequeño (`assets/icons/logo.svg`, 24 px) a cuatro veces su tamaño, para la barra de
/// Preferencias. Va en color, sin teñir.
pub fn logo_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let id = egui::Id::new("colorshot-logo");
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return cached;
    }
    let texture = render(include_bytes!("../../../assets/icons/logo.svg"), 4.).map(|image| {
        ctx.load_texture(
            "colorshot-logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            SMOOTH,
        )
    });
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// Icono de plantilla teñido de `rgb` y codificado en PNG, para el menú de la bandeja.
pub fn png(name: &str, rgb: [u8; 3]) -> Option<Vec<u8>> {
    let mut image = load(name)?;
    for pixel in image.pixels_mut() {
        pixel.0[..3].copy_from_slice(&rgb);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
    Some(bytes.into_inner())
}

/// Glifo de Phosphor de 32 px teñido de `rgb` en PNG, para entradas sin icono propio.
pub fn glyph_png(glyph: &str, rgb: [u8; 3]) -> Option<Vec<u8>> {
    use ab_glyph::{Font, ScaleFont, point};
    let font =
        ab_glyph::FontRef::try_from_slice(egui_phosphor::Variant::Regular.font_bytes()).ok()?;
    let scaled = font.as_scaled(28.);
    let id = scaled.glyph_id(glyph.chars().next()?);
    let outline = font.outline_glyph(id.with_scale_and_position(28., point(0., 0.)))?;
    let bounds = outline.px_bounds();
    // Centrado en el lienzo.
    let offset = (
        ((32. - bounds.width()) / 2.).round() as i64,
        ((32. - bounds.height()) / 2.).round() as i64,
    );
    let mut image = image::RgbaImage::new(32, 32);
    outline.draw(|x, y, coverage| {
        let (px, py) = (x as i64 + offset.0, y as i64 + offset.1);
        if (0..32).contains(&px) && (0..32).contains(&py) {
            let alpha = (coverage.clamp(0., 1.) * 255.) as u8;
            image.put_pixel(
                px as u32,
                py as u32,
                image::Rgba([rgb[0], rgb[1], rgb[2], alpha]),
            );
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
    Some(bytes.into_inner())
}

/// Textura del icono, cargada una vez por contexto.
pub fn texture(ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
    let id = egui::Id::new(("colorshot-icon", name));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return cached;
    }
    let texture = load(name).map(|image| {
        ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            SMOOTH,
        )
    });
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// Botón sin marco con el icono `name`, o con `glyph` si no está instalado.
pub fn button<'a>(
    ui: &egui::Ui,
    name: &str,
    glyph: &str,
    size: f32,
    color: egui::Color32,
) -> Button<'a> {
    match texture(ui.ctx(), name) {
        Some(texture) => Button::image(
            egui::Image::new(&texture)
                .fit_to_exact_size(egui::vec2(size, size))
                .tint(color),
        ),
        None => Button::new(RichText::new(glyph).size(size).color(color)),
    }
    // Sin marco en reposo, pero con el fondo de hover y de pulsación de egui.
    .frame_when_inactive(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn svg_se_dibuja_al_doble() {
        let path = std::env::temp_dir().join("colorshot-icon-test.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect x="4" y="4" width="8" height="8"/></svg>"#,
        )
        .unwrap();
        let image = super::svg(&path).unwrap();
        assert_eq!(image.dimensions(), (32, 32));
        assert_eq!(image.get_pixel(16, 16).0[3], 255);
        assert_eq!(image.get_pixel(2, 2).0[3], 0);
    }

    #[test]
    fn logo_a_128() {
        let logo = super::logo().unwrap();
        assert_eq!((logo.width, logo.height), (128, 128));
        // La flama es opaca en el centro y la esquina queda transparente.
        let alpha = |x: usize, y: usize| logo.rgba[(y * 128 + x) * 4 + 3];
        assert_eq!(alpha(64, 64), 255);
        assert_eq!(alpha(0, 0), 0);
    }
}
