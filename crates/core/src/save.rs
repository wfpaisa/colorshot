//! Guarda capturas PNG en la carpeta elegida.
use std::path::{Path, PathBuf};
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("Pictures directory is unavailable")]
    NoPictures,
    #[error("Configuration directory is unavailable")]
    NoConfig,
    #[error(transparent)]
    Serialize(#[from] ron::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}
/// Carpeta de las capturas temporales con que se pasan la imagen las ventanas (miniatura,
/// editor, pin): está en el directorio de la sesión, en memoria, y se vacía al reiniciar.
pub fn temp_dir() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("colorshot-images")
}

pub fn is_temp(path: &Path) -> bool {
    path.parent() == Some(temp_dir().as_path())
}

/// Cuántas capturas temporales hay y cuánto ocupan.
pub fn temp_usage() -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(temp_dir()) else {
        return (0, 0);
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .fold((0, 0), |(n, bytes), m| (n + 1, bytes + m.len()))
}

/// Borra las capturas temporales con más de `age` de antigüedad (las que dejó un cierre
/// forzado); con `Duration::ZERO`, todas.
pub fn purge_temp(age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|elapsed| elapsed >= age);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// La imagen más reciente entre `dirs` (la carpeta de capturas y la temporal), para
/// «Editar último».
pub fn latest(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter(|e| {
            let ext = e.path().extension().map(|x| x.to_ascii_lowercase());
            ext.is_some_and(|x| ["png", "jpg", "jpeg", "webp"].contains(&x.to_str().unwrap_or("")))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(time, _)| *time)
        .map(|(_, path)| path)
}

pub fn default_dir() -> Result<PathBuf, SaveError> {
    Ok(dirs::picture_dir()
        .ok_or(SaveError::NoPictures)?
        .join("Screenshots"))
}
pub fn save_png(image: &image::RgbaImage, dir: impl AsRef<Path>) -> Result<PathBuf, SaveError> {
    std::fs::create_dir_all(dir.as_ref())?;
    let path = dir.as_ref().join(
        chrono::Local::now()
            .format("SCR-%Y%m%d-%H%M%S.png")
            .to_string(),
    );
    std::fs::write(
        &path,
        encode_png(image, image::codecs::png::CompressionType::Default)?,
    )?;
    Ok(path)
}
/// Guarda según el formato elegido; Auto usa JPEG solo si el PNG pesa más de 1 MB y el
/// JPEG ocupa menos de la mitad (capturas con fotos).
pub fn save_with(
    image: &image::RgbaImage,
    dir: impl AsRef<Path>,
    format: crate::settings::Format,
) -> Result<PathBuf, SaveError> {
    let (bytes, ext) = encode(image, format)?;
    save_bytes(&bytes, ext, dir)
}

/// Codifica una sola vez: el mismo PNG sirve para el archivo y para el portapapeles.
pub fn encode(
    image: &image::RgbaImage,
    format: crate::settings::Format,
) -> Result<(Vec<u8>, &'static str), SaveError> {
    let png = encode_png(image, image::codecs::png::CompressionType::Default)?;
    let mut jpeg = Vec::new();
    if format == crate::settings::Format::Auto && png.len() > 1_000_000 {
        let rgb = image::DynamicImage::ImageRgba8(image.clone()).into_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90).encode_image(&rgb)?;
    }
    Ok(if !jpeg.is_empty() && jpeg.len() * 2 < png.len() {
        (jpeg, "jpg")
    } else {
        (png, "png")
    })
}

/// PNG rápido (poca compresión) para pasar la imagen a otro proceso sin esperar.
pub fn encode_fast(image: &image::RgbaImage) -> Result<Vec<u8>, SaveError> {
    encode_png(image, image::codecs::png::CompressionType::Fast)
}

// Con filtro adaptativo: el codificador por defecto no filtra y las capturas pesan el triple.
fn encode_png(
    image: &image::RgbaImage,
    compression: image::codecs::png::CompressionType,
) -> Result<Vec<u8>, SaveError> {
    use image::codecs::png::{FilterType, PngEncoder};
    let mut out = Vec::new();
    image::ImageEncoder::write_image(
        PngEncoder::new_with_quality(&mut out, compression, FilterType::Adaptive),
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

pub fn save_bytes(bytes: &[u8], ext: &str, dir: impl AsRef<Path>) -> Result<PathBuf, SaveError> {
    std::fs::create_dir_all(dir.as_ref())?;
    let path = dir.as_ref().join(
        chrono::Local::now()
            .format(&format!("SCR-%Y%m%d-%H%M%S.{ext}"))
            .to_string(),
    );
    std::fs::write(&path, bytes)?;
    Ok(path)
}
/// Conserva la zona física para el futuro comando de repetición.
pub fn save_last_area(rect: crate::capture::Rect) -> Result<(), SaveError> {
    let dir = dirs::config_dir()
        .ok_or(SaveError::NoConfig)?
        .join("colorshot");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("last-area.ron"), ron::to_string(&rect)?)?;
    Ok(())
}

/// Última zona confirmada, para «Repeat Area Capture».
pub fn load_last_area() -> Option<crate::capture::Rect> {
    let path = dirs::config_dir()?.join("colorshot").join("last-area.ron");
    ron::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
        let path = super::save_png(&image, dir.path().join("nested")).unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("SCR-")
        );
        assert_eq!(image::open(path).unwrap().into_rgba8(), image);
    }

    #[test]
    fn latest_picks_the_newest_image() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("a.png");
        let new = dir.path().join("b.jpg");
        std::fs::write(&old, b"").unwrap();
        std::fs::write(dir.path().join("c.txt"), b"").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&new, b"").unwrap();
        let dirs = [dir.path().to_path_buf(), dir.path().join("missing")];
        assert_eq!(super::latest(&dirs), Some(new));
    }
}
