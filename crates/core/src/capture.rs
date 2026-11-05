//! Captura el escritorio y recorta regiones en píxeles físicos.
use image::RgbaImage;
use std::{os::unix::fs::DirBuilderExt, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
impl Rect {
    pub fn normalized(a: (u32, u32), b: (u32, u32)) -> Self {
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: a.0.abs_diff(b.0),
            h: a.1.abs_diff(b.1),
        }
    }
}
pub struct Capture {
    pub image: RgbaImage,
}
impl Capture {
    pub fn crop(&self, rect: Rect) -> RgbaImage {
        let x = rect.x.min(self.image.width());
        let y = rect.y.min(self.image.height());
        image::imageops::crop_imm(
            &self.image,
            x,
            y,
            rect.w.min(self.image.width() - x),
            rect.h.min(self.image.height() - y),
        )
        .to_image()
    }
}
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Screenshot portal failed: {0}")]
    Portal(#[from] ashpd::Error),
    #[error("Screenshot I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid screenshot: {0}")]
    Image(#[from] image::ImageError),
    #[error("Screenshot portal returned an invalid file URI")]
    InvalidUri,
    #[error("Screenshot portal failed ({0}); neither gnome-screenshot nor grim succeeded")]
    Unavailable(String),
    #[error("GNOME extension capture failed: {0}")]
    Extension(#[from] ashpd::zbus::Error),
}
fn load_and_remove(path: &Path) -> Result<Capture, CaptureError> {
    let image = image::open(path).map(|i| Capture {
        image: i.into_rgba8(),
    });
    let cleanup = std::fs::remove_file(path);
    let capture = image?;
    cleanup?;
    Ok(capture)
}
async fn portal(interactive: bool) -> Result<Capture, CaptureError> {
    let started = std::time::Instant::now();
    let response = ashpd::desktop::screenshot::Screenshot::request()
        .interactive(interactive)
        .modal(false)
        .send()
        .await?
        .response()?;
    let path = url::Url::parse(response.uri().as_str())
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .ok_or(CaptureError::InvalidUri)?;
    log::info!("portal: answered in {:?}", started.elapsed());
    load_and_remove(&path)
}
pub async fn capture_full() -> Result<Capture, CaptureError> {
    let error = match portal(false).await {
        Ok(capture) => return Ok(capture),
        Err(error) => error,
    };
    for tool in ["gnome-screenshot", "grim"] {
        let dir = private_dir()?;
        let path = dir.join("capture.png");
        let mut command = tokio::process::Command::new(tool);
        if tool == "gnome-screenshot" {
            command.arg("-f");
        }
        let status = command.arg(&path).status().await;
        let result = if status.is_ok_and(|s| s.success()) {
            Some(load_and_remove(&path))
        } else {
            None
        };
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        if let Some(Ok(capture)) = result {
            return Ok(capture);
        }
    }
    Err(CaptureError::Unavailable(error.to_string()))
}
/// Solo una zona, en coordenadas lógicas del escritorio, con la extensión de GNOME
/// `rapidshot@wfelipe.com`. El portal codifica todos los monitores en un PNG (más de un
/// segundo con dos monitores HiDPI); así GNOME codifica solo la zona. La extensión solo
/// atiende al proceso con el selector abierto.
pub async fn capture_area(x: i32, y: i32, w: u32, h: u32) -> Result<Capture, CaptureError> {
    let started = std::time::Instant::now();
    let dir = private_dir()?;
    let path = dir.join("area.png");
    let result = async {
        let name = path.to_str().ok_or(CaptureError::InvalidUri)?;
        ashpd::zbus::Connection::session()
            .await?
            .call_method(
                Some("com.wfelipe.RapidShot"),
                "/com/wfelipe/RapidShot",
                Some("com.wfelipe.RapidShot"),
                "Capture Area",
                &(x, y, w as i32, h as i32, name),
            )
            .await?;
        log::info!("extension: area in {:?}", started.elapsed());
        load_and_remove(&path)
    }
    .await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
    result
}
/// Un directorio privado evita colisiones y enlaces sobre el archivo temporal.
fn private_dir() -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "rapidshot-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}
/// Interfaz de captura del escritorio (en GNOME permite elegir una ventana); sin respaldo:
/// cancelarla devuelve error.
pub async fn capture_interactive() -> Result<Capture, CaptureError> {
    portal(true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_both_directions() {
        assert_eq!(
            Rect::normalized((9, 2), (3, 8)),
            Rect {
                x: 3,
                y: 2,
                w: 6,
                h: 6
            }
        );
        assert_eq!(
            Rect::normalized((3, 8), (9, 2)),
            Rect::normalized((9, 2), (3, 8))
        );
    }
    #[test]
    fn crop_clamps_and_preserves_pixels() {
        let capture = Capture {
            image: RgbaImage::from_fn(4, 3, |x, y| image::Rgba([x as u8, y as u8, 0, 255])),
        };
        let cropped = capture.crop(Rect {
            x: 2,
            y: 1,
            w: u32::MAX,
            h: u32::MAX,
        });
        assert_eq!(cropped.dimensions(), (2, 2));
        assert_eq!(cropped.get_pixel(0, 0).0, [2, 1, 0, 255]);
        assert_eq!(
            capture
                .crop(Rect {
                    x: 9,
                    y: 9,
                    w: 2,
                    h: 2
                })
                .dimensions(),
            (0, 0)
        );
    }
}
