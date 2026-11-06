//! Reconoce texto (tesseract) y códigos QR o de barras (zbarimg) en una imagen.
use image::RgbaImage;
use std::process::Command;

#[derive(Debug, PartialEq)]
pub enum Recognized {
    /// Contenido de los códigos encontrados, uno por línea.
    Code(String),
    Text(String),
    Nothing,
}

#[derive(Debug, thiserror::Error)]
pub enum RecognizeError {
    #[error("{}", crate::i18n::tf(
        "Text recognition needs tesseract. Install it with: {}",
        &[&install_hint()],
    ))]
    NoTesseract,
    #[error("tesseract failed: {0}")]
    Tesseract(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

/// Primero busca códigos (son exactos); si no hay, reconoce texto.
pub fn recognize(image: &RgbaImage) -> Result<Recognized, RecognizeError> {
    let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("colorshot-ocr-{}.png", std::process::id()));
    // Texto pequeño se reconoce mejor ampliado; tesseract prefiere letras de ~30 px.
    let scaled;
    let image = if image.height() < 400 {
        scaled = image::imageops::resize(
            image,
            image.width() * 2,
            image.height() * 2,
            image::imageops::FilterType::CatmullRom,
        );
        &scaled
    } else {
        image
    };
    image.save(&path)?;
    let result = codes(&path)
        .map(Recognized::Code)
        .map(Ok)
        .unwrap_or_else(|| {
            text(&path).map(|t| {
                if t.is_empty() {
                    Recognized::Nothing
                } else {
                    Recognized::Text(t)
                }
            })
        });
    let _ = std::fs::remove_file(&path);
    result
}

fn codes(path: &std::path::Path) -> Option<String> {
    let output = Command::new("zbarimg")
        .args(["--quiet", "--raw"])
        .arg(path)
        .output()
        .ok()?;
    let found = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !found.is_empty()).then_some(found)
}

fn text(path: &std::path::Path) -> Result<String, RecognizeError> {
    let output = Command::new("tesseract")
        .arg(path)
        .args(["stdout", "-l", &languages(), "--psm", "3"])
        .output()
        .map_err(|_| RecognizeError::NoTesseract)?;
    if !output.status.success() {
        return Err(RecognizeError::Tesseract(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    Ok(tidy(&String::from_utf8_lossy(&output.stdout)))
}

/// Si cada motor está instalado: (tesseract, zbarimg).
pub fn engines() -> (bool, bool) {
    let runs = |program: &str| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    (runs("tesseract"), runs("zbarimg"))
}

/// Orden para instalar tesseract con el gestor de paquetes que haya.
pub fn install_hint() -> &'static str {
    let has = |path: &str| std::path::Path::new(path).exists();
    if has("/usr/bin/pacman") {
        "sudo pacman -S tesseract tesseract-data-eng"
    } else if has("/usr/bin/apt") {
        "sudo apt install tesseract-ocr"
    } else if has("/usr/bin/dnf") {
        "sudo dnf install tesseract"
    } else if has("/usr/bin/zypper") {
        "sudo zypper install tesseract-ocr"
    } else {
        "your package manager (tesseract)"
    }
}

/// Inglés más el idioma del sistema, si sus datos están instalados.
fn languages() -> String {
    let installed = Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let system = crate::i18n::current().code();
    pick_languages(&installed, system)
}

fn pick_languages(installed: &str, system: &str) -> String {
    let have: Vec<&str> = installed.lines().skip(1).map(str::trim).collect();
    let local = match system.get(..2).unwrap_or("") {
        "es" => "spa",
        "pt" => "por",
        "fr" => "fra",
        "de" => "deu",
        "it" => "ita",
        "ru" => "rus",
        "zh" => "chi_sim",
        "ja" => "jpn",
        _ => "",
    };
    let mut chosen: Vec<&str> = ["eng", local]
        .into_iter()
        .filter(|l| have.contains(l))
        .collect();
    if chosen.is_empty() {
        chosen = have.into_iter().filter(|l| *l != "osd").take(1).collect();
    }
    if chosen.is_empty() {
        "eng".into()
    } else {
        chosen.join("+")
    }
}

/// Quita líneas vacías repetidas y espacios sobrantes de la salida de tesseract.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picks_english_and_system_language() {
        let list =
            "List of available languages in \"/usr/share/tessdata/\" (3):\nafr\neng\nosd\nspa\n";
        assert_eq!(pick_languages(list, "es_CO.UTF-8"), "eng+spa");
        assert_eq!(pick_languages(list, "C"), "eng");
        assert_eq!(pick_languages("x\nosd\nafr\n", "C"), "afr");
        assert_eq!(pick_languages("", "C"), "eng");
    }
    #[test]
    fn tidy_collapses_blank_lines() {
        assert_eq!(tidy("\n a  \n\n\n b\n\x0c"), " a\n\n b");
    }
}
