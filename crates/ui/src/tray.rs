//! Icono de la bandeja con el menú de la app; cada entrada envía una orden a la instancia.
use ksni::{
    MenuItem, TrayMethods,
    menu::{StandardItem, SubMenu},
};
use colorshot_core::command::Command;
use colorshot_core::i18n::{self, t};
use colorshot_core::settings::Settings;
use std::sync::mpsc::Sender;

struct Tray {
    commands: Sender<Command>,
    // Color de los iconos del menú según el tema del escritorio.
    tint: [u8; 3],
}

// El menú de GNOME Shell sigue la preferencia clara u oscura del escritorio.
fn menu_tint() -> [u8; 3] {
    let dark = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("dark"));
    if dark { [255; 3] } else { [40; 3] }
}

impl Tray {
    fn item(&self, label: &str, icon: &str, command: Command) -> MenuItem<Self> {
        StandardItem {
            icon_data: crate::icons::png(icon, self.tint).unwrap_or_default(),
            ..item(label, command)
        }
        .into()
    }

    // El icono `icon` si existe; si no, el glifo de Phosphor.
    fn glyph_item(&self, label: &str, icon: &str, glyph: &str, command: Command) -> MenuItem<Self> {
        StandardItem {
            icon_data: crate::icons::png(icon, self.tint)
                .or_else(|| crate::icons::glyph_png(glyph, self.tint))
                .unwrap_or_default(),
            ..item(label, command)
        }
        .into()
    }
}

/// Tiempo para que GNOME Shell cierre el menú con su animación.
const MENU_CLOSE: std::time::Duration = std::time::Duration::from_millis(400);

fn item(label: &str, command: Command) -> StandardItem<Tray> {
    StandardItem {
        label: label.into(),
        activate: Box::new(move |t: &mut Tray| {
            let (commands, command) = (t.commands.clone(), command.clone());
            // El menú tarda en cerrarse: capturar enseguida lo deja en la imagen.
            let captures = matches!(
                command,
                Command::Full
                    | Command::Area
                    | Command::Window
                    | Command::Ocr
                    | Command::Color
                    | Command::Repeat
            );
            std::thread::spawn(move || {
                if captures {
                    std::thread::sleep(MENU_CLOSE);
                }
                let _ = commands.send(command);
            });
        }),
        ..Default::default()
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "colorshot".into()
    }
    fn title(&self) -> String {
        "colorshot".into()
    }
    fn icon_name(&self) -> String {
        // Con nombre, AppIndicator ignora el pixmap.
        if crate::icons::load("16-menu-colorshot").is_some() {
            String::new()
        } else {
            "applets-screenshooter".into()
        }
    }
    // El icono propio si está instalado; si no, el del tema.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        crate::icons::load("16-menu-colorshot")
            .map(|image| {
                // ksni espera ARGB32 en orden de red.
                let data = image
                    .pixels()
                    .flat_map(|p| [p[3], p[0], p[1], p[2]])
                    .collect();
                vec![ksni::Icon {
                    width: image.width() as i32,
                    height: image.height() as i32,
                    data,
                }]
            })
            .unwrap_or_default()
    }
    // Clic en el icono = captura de zona, la acción más usada.
    fn activate(&mut self, _: i32, _: i32) {
        let _ = self.commands.send(Command::Area);
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            self.item(t("Capture Screen"), "16-menu-screen", Command::Full),
            self.item(t("Capture Area"), "16-menu-area", Command::Area),
            self.item(t("Capture Window"), "16-menu-window", Command::Window),
            self.item(t("Read Text / QR"), "16-menu-ocr", Command::Ocr),
            // Cuentagotas propio en `assets/icons/menu`; sin él, el de Phosphor.
            self.glyph_item(
                t("Pick Color"),
                "16-menu-color",
                egui_phosphor::regular::EYEDROPPER,
                Command::Color,
            ),
            self.glyph_item(
                t("Color Palettes"),
                "16-menu-palette",
                egui_phosphor::regular::PALETTE,
                Command::Palettes,
            ),
            SubMenu {
                label: t("More").into(),
                icon_data: crate::icons::png("16-menu-more", self.tint).unwrap_or_default(),
                submenu: vec![
                    self.item(
                        t("Repeat Area Capture"),
                        "lib-tool-28-repeat",
                        Command::Repeat,
                    ),
                    self.item(t("Capture in 3 seconds"), "16-menu-timer", Command::Delayed),
                    MenuItem::Separator,
                    self.item(t("Open File…"), "16-menu-file", Command::OpenFile),
                    self.item(
                        t("Open from Clipboard"),
                        "16-menu-clipboard",
                        Command::Clipboard,
                    ),
                    MenuItem::Separator,
                    self.glyph_item(
                        t("Welcome Guide"),
                        "16-menu-welcome",
                        egui_phosphor::regular::HAND_WAVING,
                        Command::Welcome,
                    ),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            self.item(t("Preferences…"), "16-settings", Command::Preferences),
            self.glyph_item(
                t("Quit"),
                "16-menu-quit",
                egui_phosphor::regular::POWER,
                Command::Quit,
            ),
        ]
    }
}

/// Publica el icono. Al iniciar sesión la instancia suele arrancar antes que la extensión
/// AppIndicator: el icono se registra cuando aparece la bandeja, sin rendirse al primer fallo.
pub fn spawn(runtime: &tokio::runtime::Handle, commands: Sender<Command>) {
    runtime.spawn(async move {
        let handle = match (Tray {
            commands,
            tint: menu_tint(),
        })
        .assume_sni_available(true)
        .spawn()
        .await
        {
            Ok(handle) => handle,
            Err(error) => {
                log::warn!("Tray unavailable: {error}");
                return;
            }
        };
        // Preferencias corre en otro proceso: el menú se rehace cuando cambia el idioma.
        let mut language = Settings::load().language;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let now = Settings::load().language;
            if now != language {
                language = now;
                i18n::set(now);
                handle.update(|_| {}).await;
            }
        }
    });
}
