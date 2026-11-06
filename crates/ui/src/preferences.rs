//! Ventana de preferencias con el diseño de tablet-center.
//!
//! Una sola barra arriba, como en el editor: logo, secciones y botones de la ventana; debajo,
//! tarjetas con el título en versalitas dentro. El acento y el esquema salen de GNOME (`theme::Gnome`). Se guarda al
//! cambiar: no hay botón «Apply».
use crate::editor::{color::Format as ColorFormat, prefs::Prefs as EditorPrefs};
use crate::frame;
use crate::theme::Gnome;
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;
use colorshot_core::i18n::{self, Language, t, tf};
use colorshot_core::{
    hotkeys,
    monitors::Monitor,
    palettes::{Palettes, Secondary, View},
    save,
    settings::{
        self, AreaShows, EscAction, Format, Placement, ScreenCapture, Settings, ThumbnailMonitor,
    },
};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Hotkeys,
    Advanced,
}

pub const SIZE: egui::Vec2 = vec2(720., 660.);
/// Alto de la barra, el mismo que en el editor.
const BAR: f32 = 52.;
const PAD: f32 = 28.;
const INSET: f32 = 18.;
const ROW: f32 = 50.;
const RADIUS: u8 = 12;
const OK: Color32 = Color32::from_rgb(51, 209, 122);
/// Alto y radio de todos los controles de las filas.
const CONTROL: f32 = 32.;
const CONTROL_RADIUS: u8 = 8;

/// Opciones de «Hide thumbnail» en segundos; 0 = con el botón de cerrar.
const HIDE_AFTER: [u32; 6] = [0, 3, 5, 7, 10, 15];

pub struct Preferences {
    settings: Settings,
    tab: Tab,
    status: Option<String>,
    // Ruta del binario para los atajos de GNOME y el inicio automático.
    binary: String,
    monitors: Vec<Monitor>,
    // Colores de GNOME; `gnome` es esta paleta con o sin frosted glass.
    base: Gnome,
    gnome: Gnome,
    // Desenfoque del fondo; `None` fuera de Wayland o si el compositor no lo ofrece.
    blur: Option<crate::blur::Blur>,
    blur_supported: bool,
    // Combinación registrada en GNOME para cada `hotkeys::HOTKEYS` (`None` = sin añadir).
    installed: Vec<Option<String>>,
    // Atajo (índice de `hotkeys::HOTKEYS`) que espera la nueva combinación.
    recording: Option<usize>,
    // Tecla y modificadores pulsados mientras se graba; se asigna al soltarlo todo.
    pending: Option<(egui::Key, egui::Modifiers)>,
    // Si tesseract y zbarimg están instalados.
    engines: (bool, bool),
    // Formato con el que el editor copia los colores; vive en `editor.ron`, con el resto de lo
    // que recuerda el editor.
    color_format: ColorFormat,
    // Formatos de la ventana de paletas; viven en `palettes.ron`, con las paletas.
    palette_view: View,
    palette_secondary: Secondary,
}

impl Preferences {
    pub fn new(binary: String, monitors: Vec<Monitor>, gnome: Gnome) -> Self {
        let settings = Settings::load();
        let mut preferences = Self {
            gnome,
            settings,
            // `COLORSHOT_TAB=shortcuts|advanced` abre otra sección (para revisarla sin ratón).
            tab: match std::env::var("COLORSHOT_TAB").as_deref() {
                Ok("shortcuts") => Tab::Hotkeys,
                Ok("advanced") => Tab::Advanced,
                _ => Tab::General,
            },
            status: None,
            binary,
            monitors,
            base: gnome,
            blur: None,
            blur_supported: false,
            installed: Vec::new(),
            recording: None,
            pending: None,
            engines: colorshot_core::recognize::engines(),
            color_format: EditorPrefs::load().color_format,
            palette_view: View::default(),
            palette_secondary: Secondary::default(),
        };
        let palettes = Palettes::load();
        preferences.palette_view = palettes.view;
        preferences.palette_secondary = palettes.secondary;
        preferences.refresh_hotkeys();
        preferences
    }

    /// Desenfoque de la ventana, creado con su `CreationContext`.
    pub fn with_blur(mut self, blur: Option<crate::blur::Blur>) -> Self {
        self.blur = blur;
        self
    }

    fn refresh_hotkeys(&mut self) {
        match hotkeys::installed() {
            Ok(installed) => self.installed = installed,
            Err(error) => {
                self.installed = vec![None; hotkeys::HOTKEYS.len()];
                self.status = Some(tf("Could not read GNOME shortcuts: {}", &[&error]));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let before = self.settings.clone();
        self.blur_supported = self.blur.as_mut().is_some_and(|b| b.supported());
        let glass = self.settings.glass().filter(|_| self.blur_supported);
        self.gnome = self.base.frosted(glass);
        if let Some(blur) = &mut self.blur {
            blur.cover(ui.ctx(), glass.is_some());
        }
        let g = self.gnome;
        let (bar, body) = frame::toolbar(ui, &g, BAR, false);
        self.tabs(ui, bar);
        // Abajo deja sitio para que la barra de desplazamiento no toque la esquina redonda.
        let mut page =
            ui.new_child(egui::UiBuilder::new().max_rect(body.with_max_y(body.bottom() - 6.)));
        // Barra de desplazamiento metida en la ventana, más gruesa y con los extremos redondos;
        // el contenido recupera los radios de siempre.
        let widgets = page.style().visuals.widgets.clone();
        {
            let style = page.style_mut();
            let scroll = &mut style.spacing.scroll;
            scroll.bar_width = 10.;
            scroll.floating_width = 8.;
            scroll.bar_outer_margin = 7.;
            for visuals in [
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
            ] {
                visuals.corner_radius = 5.into();
            }
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut page, |ui| {
                ui.style_mut().visuals.widgets = widgets.clone();
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: PAD as i8,
                        right: PAD as i8,
                        top: 10,
                        bottom: PAD as i8,
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0., 0.);
                        match self.tab {
                            Tab::General => self.general(ui),
                            Tab::Hotkeys => self.hotkeys(ui),
                            Tab::Advanced => self.advanced(ui),
                        }
                        if let Some(status) = &self.status {
                            ui.add_space(14.);
                            ui.label(RichText::new(status).size(13.).color(g.dim));
                        }
                    });
            });
        frame::finish(ui, &g, false);
        if self.settings.language != before.language {
            i18n::set(self.settings.language);
            ui.ctx().request_repaint();
        }
        if self.settings != before
            && let Err(error) = self.settings.save()
        {
            self.status = Some(tf("Could not save settings: {}", &[&error]));
        }
    }

    // Barra: logo, nombre y versión a la izquierda; secciones centradas en la ventana; los
    // botones de GNOME quedan a la derecha (`frame::toolbar`).
    fn tabs(&mut self, ui: &mut Ui, bar: Rect) {
        let g = self.gnome;
        let painter = ui.painter().clone();
        let y = bar.center().y;
        let logo = Rect::from_min_size(egui::pos2(bar.left() + 4., y - 12.), vec2(24., 24.));
        match crate::icons::logo_texture(ui.ctx()) {
            Some(texture) => {
                painter.image(
                    texture.id(),
                    logo,
                    Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                    Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    logo.center(),
                    Align2::CENTER_CENTER,
                    icon::APERTURE,
                    FontId::proportional(17.),
                    g.text,
                );
            }
        }
        let name = painter.text(
            logo.right_center() + vec2(10., 0.),
            Align2::LEFT_CENTER,
            "ColorShot",
            FontId::proportional(14.),
            g.text,
        );
        painter.text(
            name.right_center() + vec2(8., 1.),
            Align2::LEFT_CENTER,
            concat!("v", env!("CARGO_PKG_VERSION")),
            FontId::proportional(12.),
            g.dim,
        );
        let tabs = [
            (
                Tab::General,
                t("General"),
                "30-pref-general",
                icon::GEAR_SIX,
            ),
            (
                Tab::Hotkeys,
                t("Shortcuts"),
                "30-pref-hotkeys",
                icon::KEYBOARD,
            ),
            (
                Tab::Advanced,
                t("Advanced"),
                "30-pref-advanced",
                icon::FLASK,
            ),
        ];
        let font = FontId::proportional(14.);
        let widths: Vec<f32> = tabs
            .iter()
            .map(|(_, label, _, _)| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(label.to_string(), font.clone(), g.text)
                        .size()
                        .x
                }) + 46.
            })
            .collect();
        let total = widths.iter().sum::<f32>() + 4. * (tabs.len() as f32 - 1.);
        let mut x = ui.ctx().content_rect().center().x - total / 2.;
        for ((tab, label, name, glyph), width) in tabs.into_iter().zip(widths) {
            let item = Rect::from_min_size(egui::pos2(x, y - 15.), vec2(width, 30.));
            x = item.right() + 4.;
            let response = ui
                .interact(item, ui.id().with(label), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() && self.tab != tab {
                self.tab = tab;
                self.status = None;
                self.recording = None;
                self.pending = None;
                if tab == Tab::Hotkeys {
                    self.refresh_hotkeys();
                }
            }
            let selected = self.tab == tab;
            let fill = if selected {
                g.accent
            } else if response.hovered() {
                g.field
            } else {
                Color32::TRANSPARENT
            };
            let color = if selected { g.on_accent } else { g.text };
            painter.rect_filled(item, 8, fill);
            // Icono propio (`assets/icons/settings`) o, si falta, su glifo de Phosphor.
            match crate::icons::texture(ui.ctx(), name) {
                Some(texture) => {
                    painter.image(
                        texture.id(),
                        Rect::from_center_size(item.left_center() + vec2(20.5, 0.), vec2(18., 18.)),
                        Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                        color,
                    );
                }
                None => {
                    painter.text(
                        item.left_center() + vec2(12., 0.),
                        Align2::LEFT_CENTER,
                        glyph,
                        FontId::proportional(17.),
                        color,
                    );
                }
            }
            painter.text(
                item.left_center() + vec2(36., 0.),
                Align2::LEFT_CENTER,
                label,
                font.clone(),
                color,
            );
        }
    }

    fn general(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        let Self {
            settings: s,
            status,
            binary,
            monitors,
            ..
        } = self;
        group(ui, &g, t("Language"), |ui| {
            row(ui, &g, t("Interface language"), None, false, |ui| {
                let options = Language::ALL.map(|l| (l, l.name().to_string()));
                select(ui, &g, 190., &mut s.language, &options);
            });
        });
        let blur = self.blur_supported;
        group(ui, &g, t("Appearance"), |ui| {
            let on = s.frosted_glass && blur;
            row(
                ui,
                &g,
                t("Frosted glass"),
                Some(if blur {
                    t("Blur what is behind the window")
                } else {
                    t("Background blur requires GNOME 51 or later")
                }),
                on,
                |ui| {
                    if !blur {
                        ui.disable();
                    }
                    switch(ui, &g, &mut s.frosted_glass);
                },
            );
            if on {
                row(
                    ui,
                    &g,
                    t("Background tint"),
                    Some(t("Theme color over the blurred background")),
                    true,
                    |ui| {
                        slider(ui, &g, &mut s.glass_tint, settings::GLASS_TINT);
                    },
                );
                row(
                    ui,
                    &g,
                    t("Tint darkness"),
                    Some(t("Darken the background tint")),
                    false,
                    |ui| {
                        slider(ui, &g, &mut s.glass_shade, (0, 100));
                    },
                );
            }
        });
        group(ui, &g, t("Saving"), |ui| {
            row(ui, &g, t("Screenshots folder"), None, true, |ui| {
                let shown = s
                    .save_dir()
                    .map(|p| middle_ellipsis(&p.display().to_string(), 34))
                    .unwrap_or_default();
                let response = pill(ui, &g, &format!("{}  {shown}", icon::FOLDER))
                    .on_hover_text(t("Click to choose a folder · Right-click to reset"));
                if response.clicked()
                    && let Some(dir) = pick_folder()
                {
                    s.save_dir = Some(dir.into());
                }
                if response.secondary_clicked() {
                    s.save_dir = None;
                }
            });
            row(
                ui,
                &g,
                t("Format"),
                Some(t("Auto uses JPEG for photo-like captures")),
                false,
                |ui| {
                    segmented(
                        ui,
                        &g,
                        &mut s.format,
                        &[(Format::Png, "PNG"), (Format::Auto, t("Auto"))],
                    );
                },
            );
        });
        // Las capturas sin guardar viven en la carpeta de la sesión (en memoria).
        group(ui, &g, t("Temporary captures"), |ui| {
            let keep = s.keep_temp;
            let subtitle = if keep {
                t("Unsaved captures stay in memory until you restart")
            } else {
                t("Unsaved captures are deleted when the thumbnail closes")
            };
            row(
                ui,
                &g,
                t("Keep unsaved captures"),
                Some(subtitle),
                keep,
                |ui| {
                    switch(ui, &g, &mut s.keep_temp);
                },
            );
            if !keep {
                return;
            }
            let (count, bytes) = save::temp_usage();
            let usage = match count {
                0 => t("None yet").to_string(),
                1 => tf("1 image · {}", &[&megabytes(bytes)]),
                n => tf("{} images · {}", &[&n, &megabytes(bytes)]),
            };
            row(ui, &g, t("Temporary captures"), Some(&usage), false, |ui| {
                if count > 0
                    && icon_button(ui, &g, icon::TRASH)
                        .on_hover_text(t("Delete all temporary captures"))
                        .clicked()
                {
                    save::purge_temp(std::time::Duration::ZERO);
                }
                if pill(ui, &g, &format!("{}  {}", icon::FOLDER_OPEN, t("Show"))).clicked() {
                    let dir = save::temp_dir();
                    *status = std::fs::create_dir_all(&dir)
                        .and_then(|()| std::process::Command::new("xdg-open").arg(&dir).spawn())
                        .err()
                        .map(|error| tf("Could not open the folder: {}", &[&error]));
                }
            });
        });
        group(ui, &g, t("Capture Screen"), |ui| {
            let subtitle = match (monitors.len() > 1, s.screen_capture) {
                (false, _) => t("Captures the whole screen"),
                (true, ScreenCapture::Cursor) => t("The monitor under the pointer"),
                (true, ScreenCapture::Pick) => t("Click a monitor; press A for all of them"),
                (true, ScreenCapture::All) => t("All screens in one image"),
            };
            row(
                ui,
                &g,
                t("Screens to capture"),
                Some(subtitle),
                false,
                |ui| {
                    segmented(
                        ui,
                        &g,
                        &mut s.screen_capture,
                        &[
                            (ScreenCapture::Cursor, t("Pointer")),
                            (ScreenCapture::Pick, t("Choose")),
                            (ScreenCapture::All, t("All")),
                        ],
                    );
                },
            );
        });
        group(ui, &g, t("After capture"), |ui| {
            row(ui, &g, t("Copy to clipboard"), None, true, |ui| {
                switch(ui, &g, &mut s.copy_after_capture);
            });
            row(ui, &g, t("Save to folder"), None, true, |ui| {
                switch(ui, &g, &mut s.save_after_capture);
            });
            row(ui, &g, t("After area capture, open"), None, false, |ui| {
                segmented(
                    ui,
                    &g,
                    &mut s.area_shows,
                    &[
                        (AreaShows::Thumbnail, t("Thumbnail")),
                        (AreaShows::Editor, t("Editor")),
                    ],
                );
            });
        });
        // Apagada, la tarjeta se queda en el interruptor; encendida, muestra dónde y cuánto.
        group(ui, &g, t("Thumbnail"), |ui| {
            let shown = s.show_after_capture;
            row(ui, &g, t("Show thumbnail"), None, shown, |ui| {
                switch(ui, &g, &mut s.show_after_capture);
            });
            if !shown {
                return;
            }
            row(ui, &g, t("Hide thumbnail"), None, true, |ui| {
                let options = HIDE_AFTER.map(|seconds| (seconds, hide_label(seconds)));
                select(ui, &g, 190., &mut s.thumbnail_seconds, &options);
            });
            let subtitle = match &s.thumbnail_monitor {
                ThumbnailMonitor::Named(name) => tf("Always on {}", &[name]),
                _ => t("Choose a screen below for the thumbnail").into(),
            };
            row(ui, &g, t("Monitor"), Some(&subtitle), true, |ui| {
                let mut choice = s.thumbnail_monitor.clone();
                segmented(
                    ui,
                    &g,
                    &mut choice,
                    &[
                        (ThumbnailMonitor::Cursor, t("Follow cursor")),
                        (ThumbnailMonitor::Primary, t("Primary")),
                    ],
                );
                s.thumbnail_monitor = choice;
            });
            layout_picker(ui, &g, monitors, s);
        });
        group(ui, &g, t("System"), |ui| {
            row(
                ui,
                &g,
                t("Launch at startup"),
                Some(t("Keeps the tray icon and shortcuts ready")),
                false,
                |ui| {
                    let mut enabled = autostart_path().is_some_and(|p| p.is_file());
                    if switch(ui, &g, &mut enabled).clicked() {
                        *status = set_autostart(enabled, binary).err();
                    }
                },
            );
        });
    }

    fn hotkeys(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        let mut add: Vec<(&'static str, String)> = Vec::new();
        let mut remove = None;
        if let Some(i) = self.recording {
            match captured(ui, &mut self.pending) {
                Some(Capture::Cancel) => self.recording = None,
                Some(Capture::Invalid) => {
                    self.status =
                        Some(t("Use Ctrl, Alt or Shift with the key (Esc cancels)").into());
                }
                Some(Capture::Binding(binding)) => {
                    self.recording = None;
                    let taken = hotkeys::HOTKEYS
                        .iter()
                        .enumerate()
                        .find(|(j, _)| *j != i && self.current(*j) == binding);
                    match taken {
                        Some((_, (name, ..))) => {
                            self.status = Some(tf(
                                "{} is already used by {}",
                                &[&pretty_binding(&binding), &t(name)],
                            ));
                        }
                        None => add.push((hotkeys::HOTKEYS[i].1, binding)),
                    }
                }
                None => {}
            }
        }
        let missing = (0..hotkeys::HOTKEYS.len())
            .filter(|i| self.installed.get(*i).cloned().flatten().is_none())
            .count();
        group(ui, &g, t("Global shortcuts"), |ui| {
            row(
                ui,
                &g,
                t("Add to GNOME"),
                Some(t("Click a shortcut to change it · Esc cancels")),
                true,
                |ui| {
                    if missing == 0 {
                        ui.label(
                            RichText::new(format!("{}  {}", icon::CHECK_CIRCLE, t("All added")))
                                .size(14.)
                                .color(OK),
                        );
                    } else if suggested(ui, &g, &format!("{}  {}", icon::PLUS, t("Add all")))
                        .clicked()
                    {
                        add = (0..hotkeys::HOTKEYS.len())
                            .filter(|i| self.installed.get(*i).cloned().flatten().is_none())
                            .map(|i| (hotkeys::HOTKEYS[i].1, self.current(i)))
                            .collect();
                    }
                },
            );
            let count = hotkeys::HOTKEYS.len();
            for (i, (name, command, default)) in hotkeys::HOTKEYS.iter().enumerate() {
                let now = self.installed.get(i).cloned().flatten();
                let subtitle = match &now {
                    None => t("Not added").to_string(),
                    Some(b) if b == default => t("Added").to_string(),
                    Some(_) => tf("Custom · default {}", &[&pretty_binding(default)]),
                };
                let recording = self.recording == Some(i);
                row(ui, &g, t(name), Some(&subtitle), i + 1 < count, |ui| {
                    match &now {
                        None => {
                            if pill(ui, &g, &format!("{}  {}", icon::PLUS, t("Add"))).clicked() {
                                add.push((command, default.to_string()));
                            }
                        }
                        Some(b) => {
                            if icon_button(ui, &g, icon::TRASH)
                                .on_hover_text(t("Remove from GNOME"))
                                .clicked()
                            {
                                remove = Some(*command);
                            }
                            if b != default
                                && icon_button(ui, &g, icon::ARROW_COUNTER_CLOCKWISE)
                                    .on_hover_text(t("Reset shortcut"))
                                    .clicked()
                            {
                                add.push((command, default.to_string()));
                            }
                        }
                    }
                    ui.add_space(6.);
                    let binding = now.clone().unwrap_or_else(|| default.to_string());
                    let response = if recording {
                        recorder(ui, &g, self.pending)
                    } else {
                        keycaps(ui, &g, &binding)
                            .on_hover_text(tf("Click to change · colorshot {}", &[command]))
                    };
                    if response.clicked() {
                        self.recording = if recording { None } else { Some(i) };
                        self.pending = None;
                        self.status = None;
                    }
                });
            }
        });
        if !add.is_empty() {
            self.status = Some(match hotkeys::install(&self.binary, &add) {
                Ok(()) if add.len() > 1 => t("Shortcuts added to GNOME").into(),
                Ok(()) => t("Shortcut saved in GNOME").into(),
                Err(error) => tf("Could not add shortcuts: {}", &[&error]),
            });
            self.refresh_hotkeys();
        }
        if let Some(command) = remove {
            self.status = Some(match hotkeys::remove(command) {
                Ok(()) => t("Shortcut removed from GNOME").into(),
                Err(error) => tf("Could not remove the shortcut: {}", &[&error]),
            });
            self.refresh_hotkeys();
        }
    }

    /// Combinación que tiene (o tendría al añadirse) el atajo `i`.
    fn current(&self, i: usize) -> String {
        self.installed
            .get(i)
            .cloned()
            .flatten()
            .unwrap_or_else(|| hotkeys::HOTKEYS[i].2.to_string())
    }

    fn advanced(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        let s = &mut self.settings;
        group(ui, &g, t("Editor"), |ui| {
            row(ui, &g, t("When hiding with Esc"), None, true, |ui| {
                segmented(
                    ui,
                    &g,
                    &mut s.esc_action,
                    &[
                        (EscAction::Nothing, t("Do nothing")),
                        (EscAction::Copy, t("Copy")),
                        (EscAction::Save, t("Save")),
                    ],
                );
            });
            row(ui, &g, t("Color Format"), None, false, |ui| {
                let options: Vec<_> = ColorFormat::ALL
                    .iter()
                    .map(|f| (*f, f.name().to_owned()))
                    .collect();
                let before = self.color_format;
                select(ui, &g, 200., &mut self.color_format, &options);
                if self.color_format != before {
                    let mut prefs = EditorPrefs::load();
                    prefs.color_format = self.color_format;
                    prefs.save();
                }
            });
        });
        group(ui, &g, t("Color Palettes"), |ui| {
            row(
                ui,
                &g,
                t("Color Format"),
                Some(t("Main value of each color")),
                true,
                |ui| {
                    let options: Vec<_> = View::ALL
                        .iter()
                        .map(|&v| (v, view_label(v).to_owned()))
                        .collect();
                    let before = self.palette_view;
                    select(ui, &g, 200., &mut self.palette_view, &options);
                    if self.palette_view != before {
                        self.status = edit_palettes(|p| p.view = self.palette_view).err();
                    }
                },
            );
            row(
                ui,
                &g,
                t("Secondary Info"),
                Some(t("Shown under the main value")),
                true,
                |ui| {
                    let options: Vec<_> = Secondary::ALL
                        .iter()
                        .map(|&k| (k, crate::palettes::secondary_name(k).to_owned()))
                        .collect();
                    let before = self.palette_secondary;
                    select(ui, &g, 200., &mut self.palette_secondary, &options);
                    if self.palette_secondary != before {
                        let kind = self.palette_secondary;
                        self.status = edit_palettes(|p| p.secondary = kind).err();
                    }
                },
            );
            row(
                ui,
                &g,
                t("Palettes File"),
                Some(t("Adobe Swatch Exchange (.ase); imports .gpl too")),
                false,
                |ui| {
                    // De derecha a izquierda: «Export» queda a la derecha.
                    if pill(ui, &g, &format!("{}  {}", icon::EXPORT, t("Export…"))).clicked() {
                        self.status = export_palettes();
                    }
                    if pill(
                        ui,
                        &g,
                        &format!("{}  {}", icon::DOWNLOAD_SIMPLE, t("Import…")),
                    )
                    .clicked()
                    {
                        self.status = import_palettes();
                    }
                },
            );
        });
        let (tesseract, zbarimg) = self.engines;
        group(ui, &g, t("Text recognition"), |ui| {
            for (name, installed, what, hint) in [
                (
                    "tesseract",
                    tesseract,
                    t("Reads text"),
                    colorshot_core::recognize::install_hint(),
                ),
                ("zbarimg", zbarimg, t("Reads QR and bar codes"), "zbar"),
            ] {
                let subtitle = if installed {
                    t(what).to_string()
                } else {
                    tf("Not installed · {}", &[&hint])
                };
                row(ui, &g, name, Some(&subtitle), name == "tesseract", |ui| {
                    if installed {
                        ui.label(RichText::new(icon::CHECK_CIRCLE).size(18.).color(OK));
                    } else if pill(ui, &g, &format!("{}  {}", icon::COPY, t("Copy command")))
                        .clicked()
                    {
                        ui.ctx().copy_text(hint.into());
                    }
                });
            }
        });
    }
}

/// Tarjeta de tablet-center: título en versalitas dentro y las filas debajo.
fn group(ui: &mut Ui, g: &Gnome, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(18.);
    let frame = egui::Frame::new()
        .fill(g.card)
        .corner_radius(RADIUS)
        .stroke(Stroke::new(1., g.line))
        .inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 16,
            bottom: 2,
        });
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add_space(INSET);
            ui.label(
                RichText::new(title.to_uppercase())
                    .size(12.5)
                    .strong()
                    .extra_letter_spacing(0.7)
                    .color(g.dim),
            );
        });
        ui.add_space(4.);
        add(ui);
    });
}

/// Fila de la tarjeta: nombre (y explicación) a la izquierda, control a la derecha.
fn row(
    ui: &mut Ui,
    g: &Gnome,
    title: &str,
    subtitle: Option<&str>,
    divider: bool,
    add: impl FnOnce(&mut Ui),
) {
    let height = if subtitle.is_some() { ROW + 8. } else { ROW };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    let left = rect.left_center() + vec2(INSET, 0.);
    match subtitle {
        Some(subtitle) => {
            painter.text(
                left - vec2(0., 9.),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(15.),
                g.text,
            );
            painter.text(
                left + vec2(0., 11.),
                Align2::LEFT_CENTER,
                subtitle,
                FontId::proportional(12.5),
                g.dim,
            );
        }
        None => {
            painter.text(
                left,
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(15.),
                g.text,
            );
        }
    }
    if divider {
        painter.hline(
            rect.left() + INSET..=rect.right() - INSET,
            rect.bottom() - 0.5,
            Stroke::new(1., g.line),
        );
    }
    let mut controls = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(INSET, 0.)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    controls.spacing_mut().item_spacing.x = 8.;
    add(&mut controls);
}

/// Fondo común de los controles: relleno del campo, más claro al pasar, y borde fino.
fn control(ui: &Ui, g: &Gnome, rect: Rect, response: &egui::Response) {
    let fill = if response.hovered() || response.is_pointer_button_down_on() {
        g.field_hover
    } else {
        g.field
    };
    ui.painter().rect_filled(rect, CONTROL_RADIUS, fill);
    ui.painter().rect_stroke(
        rect,
        CONTROL_RADIUS,
        Stroke::new(1., g.line),
        egui::StrokeKind::Inside,
    );
}

/// Botón cuadrado con un glifo.
fn icon_button(ui: &mut Ui, g: &Gnome, glyph: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(CONTROL, CONTROL), Sense::click());
    control(ui, g, rect, &response);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(16.),
        g.text,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Interruptor de GNOME.
fn switch(ui: &mut Ui, g: &Gnome, value: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(46., 26.), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *value, 0.12);
    let off = if response.hovered() {
        g.field_hover
    } else {
        g.field
    };
    let track = lerp_color(off, g.accent, t);
    let painter = ui.painter();
    painter.rect_filled(rect, 13., track);
    painter.rect_stroke(
        rect,
        13.,
        Stroke::new(1., g.line.gamma_multiply(1. - t)),
        egui::StrokeKind::Inside,
    );
    let knob = egui::pos2(
        egui::lerp(rect.left() + 13.0..=rect.right() - 13., t),
        rect.center().y,
    );
    painter.circle_filled(knob + vec2(0., 1.), 10., Color32::from_black_alpha(40));
    // Encendido, la bola toma el color contrario al del carril.
    painter.circle_filled(knob, 10., lerp_color(Color32::WHITE, g.on_accent, t));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Barra deslizante de GNOME: carril fino lleno con el acento hasta la bola.
fn slider(ui: &mut Ui, g: &Gnome, value: &mut u8, (min, max): (u8, u8)) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(200., 26.), Sense::click_and_drag());
    let track = rect.shrink2(vec2(10., 0.));
    if let Some(pointer) = response.interact_pointer_pos() {
        let t = ((pointer.x - track.left()) / track.width()).clamp(0., 1.);
        let new = egui::lerp(f32::from(min)..=f32::from(max), t).round() as u8;
        if new != *value {
            *value = new;
            response.mark_changed();
        }
    }
    let t = f32::from((*value).clamp(min, max) - min) / f32::from(max - min);
    let x = egui::lerp(track.left()..=track.right(), t);
    let line = Rect::from_min_max(
        egui::pos2(track.left(), rect.center().y - 2.),
        egui::pos2(track.right(), rect.center().y + 2.),
    );
    let painter = ui.painter();
    painter.rect_filled(line, 2., g.field_hover);
    painter.rect_filled(line.with_max_x(x), 2., g.accent);
    let knob = egui::pos2(x, rect.center().y);
    let radius = if response.hovered() || response.dragged() {
        10.
    } else {
        9.
    };
    painter.circle_filled(knob + vec2(0., 1.), radius, Color32::from_black_alpha(50));
    painter.circle_filled(knob, radius, Color32::from_gray(235));
    painter.circle_stroke(knob, radius, Stroke::new(1., g.line));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Botones unidos en un campo; el elegido resalta como una tarjeta.
fn segmented<T: Clone + PartialEq>(ui: &mut Ui, g: &Gnome, value: &mut T, options: &[(T, &str)]) {
    let font = FontId::proportional(13.5);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap(label.to_string(), font.clone(), g.text)
                .size()
                .x
                + 26.
        })
        .collect();
    let (rect, response) = ui.allocate_exact_size(
        vec2(widths.iter().sum::<f32>() + 6., CONTROL),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, CONTROL_RADIUS, g.field);
    ui.painter().rect_stroke(
        rect,
        CONTROL_RADIUS,
        Stroke::new(1., g.line),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 3.;
    for ((option, label), width) in options.iter().zip(widths) {
        let button = Rect::from_min_size(egui::pos2(x, rect.top() + 3.), vec2(width, CONTROL - 6.));
        x += width;
        let response = ui
            .interact(button, response.id.with(label), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            *value = option.clone();
        }
        let selected = *value == *option;
        let painter = ui.painter();
        if selected {
            painter.rect_filled(
                button.translate(vec2(0., 1.)),
                6.,
                Color32::from_black_alpha(25),
            );
            painter.rect_filled(button, 6., g.view);
        } else if response.hovered() {
            painter.rect_filled(button, 6., g.field_hover);
        }
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            g.text,
        );
    }
}

/// Lista desplegable con el aspecto de los demás controles.
fn select<T: Clone + PartialEq>(
    ui: &mut Ui,
    g: &Gnome,
    width: f32,
    value: &mut T,
    options: &[(T, String)],
) {
    let shown = options
        .iter()
        .find(|(o, _)| o == value)
        .map_or("", |(_, label)| label.as_str());
    let (rect, response) = ui.allocate_exact_size(vec2(width, CONTROL), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    control(ui, g, rect, &response);
    let painter = ui.painter();
    painter.text(
        rect.left_center() + vec2(12., 0.),
        Align2::LEFT_CENTER,
        shown,
        FontId::proportional(13.5),
        g.text,
    );
    painter.text(
        rect.right_center() - vec2(12., 0.),
        Align2::RIGHT_CENTER,
        icon::CARET_DOWN,
        FontId::proportional(14.),
        g.dim,
    );
    egui::Popup::menu(&response)
        .width(width)
        .gap(4.)
        .frame(
            egui::Frame::new()
                .fill(g.popup)
                .corner_radius(CONTROL_RADIUS + 2)
                .stroke(Stroke::new(1., g.line))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 16,
                    spread: 0,
                    color: Color32::from_black_alpha(60),
                })
                .inner_margin(4),
        )
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            for (option, label) in options {
                let (item, hit) = ui.allocate_exact_size(vec2(width - 8., 30.), Sense::click());
                let selected = option == value;
                if hit.clicked() {
                    *value = option.clone();
                }
                let painter = ui.painter();
                if hit.hovered() {
                    painter.rect_filled(item, 6., g.field);
                }
                let color = g.text;
                painter.text(
                    item.left_center() + vec2(10., 0.),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(13.5),
                    color,
                );
                if selected {
                    painter.text(
                        item.right_center() - vec2(10., 0.),
                        Align2::RIGHT_CENTER,
                        icon::CHECK,
                        FontId::proportional(14.),
                        color,
                    );
                }
            }
        });
}

/// Botón con texto.
fn pill(ui: &mut Ui, g: &Gnome, text: &str) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(13.5), g.text);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 28., CONTROL), Sense::click());
    control(ui, g, rect, &response);
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, g.text);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Botón de acción sugerida (acento).
fn suggested(ui: &mut Ui, g: &Gnome, text: &str) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), FontId::proportional(13.5), g.on_accent);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 28., CONTROL), Sense::click());
    let fill = if response.hovered() {
        g.accent.gamma_multiply(0.85)
    } else {
        g.accent
    };
    ui.painter().rect_filled(rect, CONTROL_RADIUS, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, g.on_accent);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `<Alt><Shift>f` como teclas: «Alt» «Shift» «F».
fn keycaps(ui: &mut Ui, g: &Gnome, binding: &str) -> egui::Response {
    let keys: Vec<String> = pretty_binding(binding)
        .split(" + ")
        .map(str::to_string)
        .collect();
    let font = FontId::proportional(12.5);
    let galleys: Vec<_> = keys
        .iter()
        .map(|k| ui.painter().layout_no_wrap(k.clone(), font.clone(), g.text))
        .collect();
    let width: f32 =
        galleys.iter().map(|k| k.size().x + 16.).sum::<f32>() + 4. * (keys.len() as f32 - 1.);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut x = rect.left();
    for galley in galleys {
        let cap = Rect::from_min_size(egui::pos2(x, rect.top()), vec2(galley.size().x + 16., 26.));
        x = cap.right() + 4.;
        let painter = ui.painter();
        painter.rect_filled(cap.translate(vec2(0., 2.)), 6., g.line);
        let fill = if response.hovered() {
            g.field_hover
        } else {
            g.field
        };
        painter.rect_filled(cap, 6., fill);
        painter.galley(cap.center() - galley.size() / 2., galley, g.text);
    }
    response
}

/// Hueco de las teclas mientras se graba: contorno del acento y «Press keys…», o la
/// combinación que se está formando.
fn recorder(
    ui: &mut Ui,
    g: &Gnome,
    pending: Option<(egui::Key, egui::Modifiers)>,
) -> egui::Response {
    let held = ui.input(|i| i.modifiers);
    let (rect, response) = ui.allocate_exact_size(vec2(150., 26.), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 6., g.view);
    painter.rect_stroke(
        rect,
        6.,
        Stroke::new(1.5, g.accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        recording_label(pending, held),
        FontId::proportional(12.5),
        g.dim,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

enum Capture {
    Cancel,
    Invalid,
    Binding(String),
}

/// Va formando la combinación en `pending` y la devuelve cuando se sueltan todos los
/// modificadores: así Alt+A puede seguir hasta Alt+Shift+A sin asignarse a medias.
fn captured(ui: &Ui, pending: &mut Option<(egui::Key, egui::Modifiers)>) -> Option<Capture> {
    ui.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                if *key == egui::Key::Escape && modifiers.is_none() {
                    *pending = None;
                    return Some(Capture::Cancel);
                }
                *pending = Some((base_key(*key, *physical_key, *modifiers), *modifiers));
            }
        }
        let (key, modifiers) = pending.as_mut()?;
        // Un modificador añadido con la tecla ya pulsada también cuenta.
        modifiers.alt |= i.modifiers.alt;
        modifiers.ctrl |= i.modifiers.ctrl;
        modifiers.shift |= i.modifiers.shift;
        if !(i.modifiers.alt || i.modifiers.ctrl || i.modifiers.shift) {
            let capture = match accelerator(*key, *modifiers) {
                Some(binding) => Capture::Binding(binding),
                None => Capture::Invalid,
            };
            *pending = None;
            return Some(capture);
        }
        None
    })
}

/// Lo que muestra el hueco al grabar: «Alt + Shift + A», «Ctrl + Alt + …» o «Press keys…».
fn recording_label(pending: Option<(egui::Key, egui::Modifiers)>, held: egui::Modifiers) -> String {
    let (key, modifiers) = match pending {
        Some((key, modifiers)) => (Some(key), modifiers),
        None => (None, held),
    };
    let mut parts: Vec<String> = [
        (modifiers.ctrl, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| name.to_string())
    .collect();
    if parts.is_empty() && key.is_none() {
        return t("Press keys…").to_string();
    }
    parts.push(key.map_or("…".to_string(), |k| k.name().to_uppercase()));
    parts.join(" + ")
}

/// Con Shift, la tecla lógica es el símbolo (Shift+1 = «!», que GNOME no entiende como
/// `<Shift>!`); los atajos de GNOME nombran la tecla sin Shift, así que vale la física.
fn base_key(key: egui::Key, physical: Option<egui::Key>, modifiers: egui::Modifiers) -> egui::Key {
    match physical {
        Some(physical) if modifiers.shift => physical,
        _ => key,
    }
}

/// Tecla y modificadores → `<Control><Alt>s`. Sin Ctrl, Alt ni Shift solo valen las F.
fn accelerator(key: egui::Key, modifiers: egui::Modifiers) -> Option<String> {
    use egui::Key as K;
    let function = matches!(
        key,
        K::F1
            | K::F2
            | K::F3
            | K::F4
            | K::F5
            | K::F6
            | K::F7
            | K::F8
            | K::F9
            | K::F10
            | K::F11
            | K::F12
    );
    if !function && !(modifiers.ctrl || modifiers.alt || modifiers.shift) {
        return None;
    }
    let name = match key {
        K::Space => "space".to_string(),
        K::Enter => "Return".into(),
        K::Backspace => "BackSpace".into(),
        K::PageUp => "Page_Up".into(),
        K::PageDown => "Page_Down".into(),
        K::Minus => "minus".into(),
        K::Equals => "equal".into(),
        K::Comma => "comma".into(),
        K::Period => "period".into(),
        K::Slash => "slash".into(),
        K::Escape => return None,
        k => {
            let name = k.name();
            if name.chars().count() == 1 {
                name.to_lowercase()
            } else {
                name.to_string()
            }
        }
    };
    let mut binding = String::new();
    for (on, part) in [
        (modifiers.ctrl, "<Control>"),
        (modifiers.alt, "<Alt>"),
        (modifiers.shift, "<Shift>"),
    ] {
        if on {
            binding.push_str(part);
        }
    }
    binding.push_str(&name);
    Some(binding)
}

/// Monitores a escala (según Mutter) con la miniatura en su sitio. Clic en un monitor: fijarlo;
/// clic en uno de los nueve puntos: la esquina, el lado o el centro.
fn layout_picker(ui: &mut Ui, g: &Gnome, monitors: &[Monitor], s: &mut Settings) {
    let fallback = [Monitor {
        connector: "Screen".into(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        primary: true,
    }];
    let monitors = if monitors.is_empty() {
        &fallback[..]
    } else {
        monitors
    };
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 200.), Sense::hover());
    let area = area.shrink2(vec2(20., 18.));
    let min_x = monitors.iter().map(|m| m.x).min().unwrap_or(0) as f32;
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0) as f32;
    let max_x = monitors
        .iter()
        .map(|m| m.x + m.width as i32)
        .max()
        .unwrap_or(1) as f32;
    let max_y = monitors
        .iter()
        .map(|m| m.y + m.height as i32)
        .max()
        .unwrap_or(1) as f32;
    let scale = (area.width() / (max_x - min_x)).min(area.height() / (max_y - min_y));
    let offset = area.center() - vec2(max_x - min_x, max_y - min_y) * scale / 2.;
    let primary = monitors.iter().position(|m| m.primary).unwrap_or(0);
    for (i, m) in monitors.iter().enumerate() {
        let screen = Rect::from_min_size(
            offset + vec2(m.x as f32 - min_x, m.y as f32 - min_y) * scale,
            vec2(m.width as f32, m.height as f32) * scale,
        )
        .shrink(3.);
        let active = match &s.thumbnail_monitor {
            ThumbnailMonitor::Cursor => true,
            ThumbnailMonitor::Primary => i == primary,
            ThumbnailMonitor::Named(name) => *name == m.connector,
        };
        let pinned =
            matches!(&s.thumbnail_monitor, ThumbnailMonitor::Named(n) if *n == m.connector);
        let response = ui.interact(screen, ui.id().with(("monitor", i)), Sense::click());
        if response.clicked() {
            s.thumbnail_monitor = ThumbnailMonitor::Named(m.connector.clone());
        }
        let painter = ui.painter();
        let bezel = if pinned || response.hovered() {
            g.accent
        } else {
            g.line
        };
        painter.rect_filled(screen.expand(3.), 8., bezel);
        let wallpaper = if active {
            if g.dark {
                Color32::from_rgb(24, 24, 28)
            } else {
                Color32::from_rgb(70, 74, 84)
            }
        } else if g.dark {
            Color32::from_rgb(40, 40, 44)
        } else {
            Color32::from_rgb(150, 152, 160)
        };
        painter.rect_filled(screen, 5., wallpaper);
        if m.primary {
            painter.rect_filled(
                Rect::from_min_size(screen.min, vec2(screen.width(), 5.)),
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                Color32::from_black_alpha(120),
            );
        }
        painter.text(
            screen.center_bottom() - vec2(0., 10.),
            Align2::CENTER_CENTER,
            &m.connector,
            FontId::proportional(11.),
            Color32::from_white_alpha(110),
        );
        if !active {
            continue;
        }
        let inner = screen.shrink(8.);
        let thumb = vec2(inner.width() * 0.26, inner.height() * 0.22);
        for placement in Placement::ALL {
            let spot = crate::transition::place(inner, thumb, placement.align(), 0.);
            let hit = ui.interact(
                spot,
                ui.id().with(("spot", i, placement as u8)),
                Sense::click(),
            );
            let chosen = s.thumbnail_placement == placement;
            if hit.clicked() {
                s.thumbnail_placement = placement;
            }
            let painter = ui.painter();
            if chosen {
                painter.rect_filled(spot, 3., g.accent);
                painter.rect_stroke(
                    spot,
                    3.,
                    Stroke::new(1., Color32::from_white_alpha(120)),
                    egui::StrokeKind::Inside,
                );
            } else {
                let color = if hit.hovered() {
                    g.accent.gamma_multiply(0.6)
                } else {
                    Color32::from_white_alpha(60)
                };
                painter.circle_filled(spot.center(), if hit.hovered() { 4. } else { 2.5 }, color);
            }
            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.)
}

fn hide_label(seconds: u32) -> String {
    match seconds {
        0 => t("When closed manually").into(),
        s => tf("After {} seconds", &[&s]),
    }
}

/// `<Alt><Shift>f` → `Alt + Shift + F`.
fn pretty_binding(binding: &str) -> String {
    let key = binding.rsplit('>').next().unwrap_or(binding);
    let mut parts: Vec<String> = binding
        .split('>')
        .filter_map(|m| m.strip_prefix('<'))
        .map(|m| match m {
            "Control" | "Primary" => "Ctrl".to_string(),
            m => m.to_string(),
        })
        .collect();
    let mut chars = key.chars();
    if let Some(first) = chars.next() {
        parts.push(first.to_uppercase().chain(chars).collect());
    }
    parts.join(" + ")
}

/// Recorta por el centro.
fn middle_ellipsis(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let keep = (max - 1) / 2;
    let head: String = chars[..keep].iter().collect();
    let tail: String = chars[chars.len() - (max - 1 - keep)..].iter().collect();
    format!("{head}…{tail}")
}

fn autostart_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("autostart")
            .join("colorshot.desktop"),
    )
}

/// Crea o borra el lanzador de inicio automático; devuelve el error para la línea de estado.
fn set_autostart(enabled: bool, binary: &str) -> Result<(), String> {
    let path = autostart_path().ok_or(t("No config folder"))?;
    let result = if enabled {
        std::fs::create_dir_all(path.parent().unwrap_or(&path)).and_then(|()| {
            std::fs::write(
                &path,
                format!(
                    "[Desktop Entry]\nType=Application\nName=colorshot\nExec={binary}\n\
                     Icon=colorshot\nX-GNOME-Autostart-enabled=true\n"
                ),
            )
        })
    } else {
        std::fs::remove_file(&path)
    };
    result.map_err(|error| tf("Could not change autostart: {}", &[&error]))
}

fn pick_folder() -> Option<String> {
    zenity(&["--directory", "--title=Screenshots folder"])
}

/// Selector de archivos de zenity; `None` si se cancela o no está instalado.
fn zenity(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("zenity")
        .arg("--file-selection")
        .args(args)
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !path.is_empty()).then_some(path)
}

/// Nombre de cada formato de la ventana de paletas; la vista del cuadro muestra el HEX.
fn view_label(view: View) -> &'static str {
    match view {
        View::Picker => "HEX",
        view => crate::palettes::view_name(view),
    }
}

/// Cambia `palettes.ron` partiendo de lo que hay en disco: la ventana de paletas puede tenerlo
/// abierto y lo relee al ver que cambió.
fn edit_palettes(change: impl FnOnce(&mut Palettes)) -> Result<(), String> {
    let mut palettes = Palettes::load();
    change(&mut palettes);
    palettes
        .save()
        .map_err(|error| tf("Could not save palettes: {}", &[&error]))
}

/// Guarda las paletas en un ASE; devuelve el mensaje para la barra de estado (`None` si se
/// cancela).
fn export_palettes() -> Option<String> {
    let saved = Palettes::load().saved;
    if saved.is_empty() {
        return Some(t("There are no saved palettes to export").into());
    }
    let title = format!("--title={}", t("Export Palettes"));
    let mut path = zenity(&[
        "--save",
        "--confirm-overwrite",
        "--filename=Palettes.ase",
        &title,
    ])?;
    if !path.to_lowercase().ends_with(".ase") {
        path.push_str(".ase");
    }
    Some(
        match std::fs::write(&path, colorshot_core::swatches::to_ase(&saved)) {
            Ok(()) => tf("Exported {} palettes", &[&saved.len()]),
            Err(error) => tf("Could not export palettes: {}", &[&error]),
        },
    )
}

/// Añade a las guardadas las paletas de un ASE o GPL; los nombres repetidos llevan un número.
fn import_palettes() -> Option<String> {
    let title = format!("--title={}", t("Import Palettes"));
    let filter = format!("--file-filter={} | *.ase *.ASE *.gpl *.GPL", t("Palettes"));
    let path = zenity(&[&title, &filter, "--file-filter=* | *"])?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => return Some(tf("Could not import palettes: {}", &[&error])),
    };
    let list = match colorshot_core::swatches::import(&bytes, t("Imported")) {
        Ok(list) => list,
        Err(error) => return Some(tf("Could not import palettes: {}", &[&t(&error)])),
    };
    let count = list.len();
    let result = edit_palettes(|palettes| {
        for mut palette in list {
            let base = palette.name.clone();
            let mut n = 2;
            while palettes.saved.iter().any(|p| p.name == palette.name) {
                palette.name = format!("{base} {n}");
                n += 1;
            }
            palettes.saved.push(palette);
        }
    });
    Some(match result {
        Ok(()) => tf("Imported {} palettes", &[&count]),
        Err(error) => error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bindings_and_paths_read_well() {
        assert_eq!(pretty_binding("<Shift><Super>1"), "Shift + Super + 1");
        assert_eq!(pretty_binding("<Alt><Shift>f"), "Alt + Shift + F");
        assert_eq!(pretty_binding("<Control>space"), "Ctrl + Space");
        let ctrl_alt = egui::Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            accelerator(egui::Key::S, ctrl_alt).as_deref(),
            Some("<Control><Alt>s")
        );
        assert_eq!(accelerator(egui::Key::S, egui::Modifiers::NONE), None);
        // Shift+Alt+1 llega como «!»; se registra con la tecla física.
        let alt_shift = egui::Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        let key = base_key(egui::Key::Exclamationmark, Some(egui::Key::Num1), alt_shift);
        assert_eq!(
            accelerator(key, alt_shift).as_deref(),
            Some("<Alt><Shift>1")
        );
        assert_eq!(
            accelerator(egui::Key::F5, egui::Modifiers::NONE).as_deref(),
            Some("F5")
        );
        assert_eq!(middle_ellipsis("/short", 30), "/short");
        let long = middle_ellipsis("/home/someone/Pictures/Screenshots/Very/Deep", 20);
        assert_eq!(long.chars().count(), 20);
        assert!(long.starts_with("/home/som") && long.ends_with("ry/Deep"));
    }
}
