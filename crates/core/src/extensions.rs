//! Extensiones de GNOME Shell que usa la app: la propia (Capture Window y capturas de zona) y
//! AppIndicator (el icono de la bandeja). Se consultan con `gdbus` y `gsettings`, como los atajos.
use std::path::PathBuf;
use std::process::Command;

pub const UUID: &str = "colorshot@wfelipe.com";
/// Extensión que muestra los iconos de bandeja (StatusNotifierItem) en GNOME.
pub const APPINDICATOR: &str = "appindicatorsupport@rgcjonas.gmail.com";
pub const APPINDICATOR_URL: &str = "https://extensions.gnome.org/extension/615/appindicator-support/";

// La extensión va dentro del binario: el asistente la instala aunque se haya compilado a mano.
const FILES: [(&str, &str); 2] = [
    (
        "extension.js",
        include_str!("../../../extension/colorshot@wfelipe.com/extension.js"),
    ),
    (
        "metadata.json",
        include_str!("../../../extension/colorshot@wfelipe.com/metadata.json"),
    ),
];

/// Nombre en D-Bus que publica cada extensión cuando GNOME la tiene cargada.
fn bus_name(uuid: &str) -> &'static str {
    if uuid == UUID {
        "com.wfelipe.ColorShot"
    } else {
        "org.kde.StatusNotifierWatcher"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Cargada y funcionando.
    Active,
    /// Activada, pero GNOME aún no la cargó: hace falta volver a iniciar sesión (Wayland).
    NeedsRelogin,
    /// Instalada pero desactivada.
    Disabled,
    Missing,
}

pub fn state(uuid: &str) -> State {
    if name_has_owner(bus_name(uuid)) {
        State::Active
    } else if !installed(uuid) {
        State::Missing
    } else if !enabled(uuid) {
        State::Disabled
    } else {
        State::NeedsRelogin
    }
}

fn name_has_owner(name: &str) -> bool {
    Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            name,
        ])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("true"))
}

fn user_dir(uuid: &str) -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("gnome-shell/extensions").join(uuid))
}

fn installed(uuid: &str) -> bool {
    user_dir(uuid).is_some_and(|d| d.join("metadata.json").is_file())
        || ["/usr/share", "/usr/local/share"]
            .iter()
            .any(|base| PathBuf::from(base).join("gnome-shell/extensions").join(uuid).is_dir())
}

fn gsettings(args: &[&str]) -> std::io::Result<String> {
    let output = Command::new("gsettings").args(args).output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

fn list(key: &str) -> Vec<String> {
    gsettings(&["get", "org.gnome.shell", key])
        .map(|text| crate::hotkeys::parse_list(&text))
        .unwrap_or_default()
}

fn set_list(key: &str, items: &[String]) -> std::io::Result<()> {
    gsettings(&["set", "org.gnome.shell", key, &crate::hotkeys::format_list(items)]).map(drop)
}

fn enabled(uuid: &str) -> bool {
    let off = gsettings(&["get", "org.gnome.shell", "disable-user-extensions"])
        .is_ok_and(|v| v == "true");
    !off && list("enabled-extensions").iter().any(|u| u == uuid)
        && !list("disabled-extensions").iter().any(|u| u == uuid)
}

/// La activa en GNOME. Con las claves de `org.gnome.shell` también vale para una extensión
/// recién copiada, que `gnome-extensions enable` aún no conoce.
pub fn enable(uuid: &str) -> std::io::Result<()> {
    let mut on = list("enabled-extensions");
    if !on.iter().any(|u| u == uuid) {
        on.push(uuid.into());
        set_list("enabled-extensions", &on)?;
    }
    let mut off = list("disabled-extensions");
    if off.iter().any(|u| u == uuid) {
        off.retain(|u| u != uuid);
        set_list("disabled-extensions", &off)?;
    }
    if gsettings(&["get", "org.gnome.shell", "disable-user-extensions"]).is_ok_and(|v| v == "true")
    {
        gsettings(&["set", "org.gnome.shell", "disable-user-extensions", "false"])?;
    }
    Ok(())
}

/// Copia la extensión de ColorShot a ~/.local/share/gnome-shell/extensions y la activa.
pub fn install() -> std::io::Result<()> {
    let dir = user_dir(UUID).ok_or_else(|| std::io::Error::other("No data folder"))?;
    std::fs::create_dir_all(&dir)?;
    for (name, text) in FILES {
        std::fs::write(dir.join(name), text)?;
    }
    enable(UUID)
}
