//! Copia imágenes al portapapeles del escritorio.
use arboard::SetExtLinux;
use image::RgbaImage;
use std::{io::Write, process::Stdio};

pub fn copy_image(image: &RgbaImage) -> Result<(), arboard::Error> {
    // En Wayland el contenido muere con el proceso; wl-copy queda sirviéndolo en segundo plano.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && copy_with_wl_copy(image).is_ok() {
        return Ok(());
    }
    arboard::Clipboard::new()?
        .set()
        .wait_until(std::time::Instant::now() + std::time::Duration::from_millis(500))
        .image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        })
}

fn copy_with_wl_copy(image: &RgbaImage) -> std::io::Result<()> {
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(std::io::Error::other)?;
    copy_png(&png)
}

/// Copia un PNG ya codificado; en Wayland con wl-copy, si no, decodificándolo para arboard.
pub fn copy_png(png: &[u8]) -> std::io::Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        let image = image::load_from_memory(png)
            .map_err(std::io::Error::other)?
            .into_rgba8();
        return arboard::Clipboard::new()
            .and_then(|mut c| {
                c.set_image(arboard::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: std::borrow::Cow::Borrowed(image.as_raw()),
                })
            })
            .map_err(std::io::Error::other);
    }
    let mut child = std::process::Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("wl-copy stdin"))?
        .write_all(png)?;
    // wl-copy se bifurca y el padre sale enseguida.
    let status = wait_briefly(&mut child)?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("wl-copy failed"))
    }
}

/// Copia texto; en Wayland con wl-copy para que sobreviva al proceso.
pub fn copy_text(text: &str) -> Result<(), arboard::Error> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        && let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(text.as_bytes()).is_ok()
    {
        drop(stdin);
        if wait_briefly(&mut child).is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }
    arboard::Clipboard::new()?
        .set()
        .wait_until(std::time::Instant::now() + std::time::Duration::from_millis(500))
        .text(text)
}

/// wl-copy se bifurca en cuanto tiene la selección. En GNOME antes necesita el foco: si otra
/// ventana se lo quita, espera para siempre («wl-clipboard is ready»). Se corta a los 2 s.
fn wait_briefly(child: &mut std::process::Child) -> std::io::Result<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("wl-copy could not get the clipboard"));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
