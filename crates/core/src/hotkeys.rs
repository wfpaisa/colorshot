//! Atajos globales: en GNOME se registran como atajos personalizados que llaman a la CLI.
use std::process::Command;

/// Nombre visible, subcomando y combinación: Alt+Shift y la inicial de la acción.
pub const HOTKEYS: [(&str, &str, &str); 8] = [
    (crate::i18n::n("Capture Screen"), "full", "<Alt><Shift>f"),
    (crate::i18n::n("Capture Area"), "area", "<Alt><Shift>a"),
    (
        crate::i18n::n("Repeat Area Capture"),
        "repeat",
        "<Alt><Shift>r",
    ),
    (
        crate::i18n::n("Capture in 3 seconds"),
        "delayed",
        "<Alt><Shift>d",
    ),
    (
        crate::i18n::n("Open from Clipboard"),
        "clipboard",
        "<Alt><Shift>v",
    ),
    (crate::i18n::n("Read Text / QR"), "ocr", "<Alt><Shift>t"),
    (crate::i18n::n("Capture Window"), "window", "<Alt><Shift>w"),
    (crate::i18n::n("Color picker"), "color", "<Alt><Shift>c"),
];

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const ITEM: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("gsettings failed: {0}")]
    Gsettings(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn gsettings(args: &[&str]) -> Result<String, HotkeyError> {
    let output = Command::new("gsettings").args(args).output()?;
    if !output.status.success() {
        return Err(HotkeyError::Gsettings(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

/// Lista de rutas en formato GVariant (`@as []` o `['/a/', '/b/']`).
fn parse_list(text: &str) -> Vec<String> {
    text.trim_start_matches("@as")
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|s| s.trim().trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn format_list(items: &[String]) -> String {
    let quoted: Vec<_> = items.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", quoted.join(", "))
}

fn path(command: &str) -> String {
    format!("{BASE}/colorshot-{command}/")
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('\'').to_string()
}

/// Combinación registrada en GNOME para cada entrada de `HOTKEYS` (`None` si falta).
pub fn installed() -> Result<Vec<Option<String>>, HotkeyError> {
    let list = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    HOTKEYS
        .iter()
        .map(|(_, command, _)| {
            let path = path(command);
            if !list.contains(&path) {
                return Ok(None);
            }
            let binding = gsettings(&["get", &format!("{ITEM}:{path}"), "binding"])?;
            Ok(Some(unquote(&binding)))
        })
        .collect()
}

/// Crea o actualiza las entradas `colorshot-*` pedidas, cada una con su combinación
/// (`<Control><Alt>s`), sin tocar los demás atajos.
pub fn install(binary: &str, bindings: &[(&str, String)]) -> Result<(), HotkeyError> {
    let mut list = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    for (command, binding) in bindings {
        let Some((name, ..)) = HOTKEYS.iter().find(|h| h.1 == *command) else {
            continue;
        };
        let path = path(command);
        let item = format!("{ITEM}:{path}");
        gsettings(&["set", &item, "name", &format!("colorshot: {name}")])?;
        gsettings(&["set", &item, "command", &format!("{binary} {command}")])?;
        gsettings(&["set", &item, "binding", binding.as_str()])?;
        if !list.contains(&path) {
            list.push(path);
        }
    }
    gsettings(&["set", SCHEMA, "custom-keybindings", &format_list(&list)])?;
    Ok(())
}

/// Todas con su combinación de fábrica.
pub fn install_all(binary: &str) -> Result<(), HotkeyError> {
    let bindings: Vec<_> = HOTKEYS.iter().map(|h| (h.1, h.2.to_string())).collect();
    install(binary, &bindings)
}

/// Quita una entrada `colorshot-*` de la lista de GNOME y borra sus claves.
pub fn remove(command: &str) -> Result<(), HotkeyError> {
    let path = path(command);
    let mut list = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    list.retain(|p| *p != path);
    gsettings(&["set", SCHEMA, "custom-keybindings", &format_list(&list)])?;
    let item = format!("{ITEM}:{path}");
    for key in ["name", "command", "binding"] {
        gsettings(&["reset", &item, key])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_formats_gvariant_lists() {
        assert!(parse_list("@as []").is_empty());
        let list = parse_list("['/a/', '/b/']");
        assert_eq!(list, vec!["/a/", "/b/"]);
        assert_eq!(format_list(&list), "['/a/', '/b/']");
        assert_eq!(unquote("'<Alt><Shift>f'"), "<Alt><Shift>f");
    }

    // Toca los atajos reales de GNOME: `cargo test -- --ignored gnome_round_trip`.
    #[test]
    #[ignore]
    fn gnome_round_trip() {
        let before = gsettings(&["get", SCHEMA, "custom-keybindings"]).unwrap();
        install("/bin/true", &[("window", "<Control><Alt>w".into())]).unwrap();
        assert_eq!(installed().unwrap()[6].as_deref(), Some("<Control><Alt>w"));
        remove("window").unwrap();
        assert_eq!(installed().unwrap()[6], None);
        assert_eq!(
            gsettings(&["get", SCHEMA, "custom-keybindings"]).unwrap(),
            before
        );
    }
}
