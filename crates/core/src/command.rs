//! Órdenes que llegan por la línea de comandos, la bandeja o el socket de la instancia.
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Arranca (o deja viva) la instancia con la bandeja.
    Daemon,
    Area,
    Full,
    Repeat,
    Delayed,
    /// Ventana (u otra zona) elegida con la interfaz de captura de GNOME.
    Window,
    /// Reconoce texto o QR en una zona y lo copia.
    Ocr,
    /// Cuentagotas: copia el color del píxel elegido.
    Color,
    Edit(PathBuf),
    /// Siempre en un proceso propio: un pin no debe bloquear la instancia.
    Pin(PathBuf),
    /// Miniatura con su transición; la lanza el proceso que capturó (ver `flows::deliver`).
    Preview(PathBuf),
    /// Lectura OCR animada sobre la zona elegida (la lanza `flows::ocr`).
    Scan(PathBuf),
    /// Ventana para crear y editar paletas de colores.
    Palettes,
    OpenFile,
    Clipboard,
    Preferences,
    Quit,
}

pub const USAGE: &str = "Usage: rapidshot [area | full | window | ocr | color | palettes | repeat | delayed | clipboard | open | prefs | edit <file> | pin <file> | quit]";

impl Command {
    pub fn parse(args: &[String]) -> Option<Self> {
        Some(match args {
            [] => Self::Daemon,
            [c] => match c.as_str() {
                "area" => Self::Area,
                "full" => Self::Full,
                "repeat" => Self::Repeat,
                "delayed" => Self::Delayed,
                "window" => Self::Window,
                "ocr" => Self::Ocr,
                "color" => Self::Color,
                "palettes" => Self::Palettes,
                "open" => Self::OpenFile,
                "clipboard" => Self::Clipboard,
                "prefs" => Self::Preferences,
                "quit" => Self::Quit,
                _ => return None,
            },
            [c, path] if c == "edit" => Self::Edit(PathBuf::from(path)),
            [c, path] if c == "pin" => Self::Pin(PathBuf::from(path)),
            [c, path] if c == "preview" => Self::Preview(PathBuf::from(path)),
            [c, path] if c == "scan" => Self::Scan(PathBuf::from(path)),
            _ => return None,
        })
    }
    /// Una línea para el socket; las rutas van absolutas porque la instancia tiene otro cwd.
    pub fn to_line(&self) -> String {
        match self {
            Self::Daemon => "show".into(),
            Self::Area => "area".into(),
            Self::Full => "full".into(),
            Self::Repeat => "repeat".into(),
            Self::Delayed => "delayed".into(),
            Self::Window => "window".into(),
            Self::Ocr => "ocr".into(),
            Self::Color => "color".into(),
            Self::Palettes => "palettes".into(),
            Self::OpenFile => "open".into(),
            Self::Clipboard => "clipboard".into(),
            Self::Preferences => "prefs".into(),
            Self::Quit => "quit".into(),
            Self::Edit(path) | Self::Pin(path) | Self::Preview(path) | Self::Scan(path) => {
                let verb = match self {
                    Self::Pin(_) => "pin",
                    Self::Preview(_) => "preview",
                    Self::Scan(_) => "scan",
                    _ => "edit",
                };
                let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
                format!("{verb} {}", path.display())
            }
        }
    }
    pub fn from_line(line: &str) -> Option<Self> {
        let line = line.trim_end_matches('\n');
        if line == "show" {
            return Some(Self::Daemon);
        }
        match line.split_once(' ') {
            Some(("edit", path)) => Some(Self::Edit(PathBuf::from(path))),
            Some(("pin", path)) => Some(Self::Pin(PathBuf::from(path))),
            Some(("preview", path)) => Some(Self::Preview(PathBuf::from(path))),
            Some(("scan", path)) => Some(Self::Scan(PathBuf::from(path))),
            _ => Self::parse(&[line.to_string()]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_through_socket_line() {
        for command in [
            Command::Daemon,
            Command::Area,
            Command::Full,
            Command::Repeat,
            Command::Delayed,
            Command::Window,
            Command::Ocr,
            Command::Color,
            Command::Palettes,
            Command::OpenFile,
            Command::Clipboard,
            Command::Preferences,
            Command::Quit,
            Command::Edit("/tmp/a b.png".into()),
            Command::Pin("/tmp/p.png".into()),
            Command::Preview("/tmp/v.png".into()),
            Command::Scan("/tmp/s.png".into()),
        ] {
            assert_eq!(Command::from_line(&command.to_line()), Some(command));
        }
        assert_eq!(Command::parse(&["bogus".into()]), None);
        assert_eq!(
            Command::parse(&["edit".into(), "x.png".into()]),
            Some(Command::Edit("x.png".into()))
        );
    }
}
