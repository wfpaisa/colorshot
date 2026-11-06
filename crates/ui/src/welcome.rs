//! Asistente de bienvenida: se abre la primera vez que arranca la instancia (y desde la
//! bandeja). Explica la app, instala la extensión de GNOME, enseña el menú, añade los atajos
//! y el inicio automático, con las tarjetas y controles de Preferencias.
use crate::frame;
use crate::preferences::{
    BAR, OK, PAD, after_capture, appearance, autostart_path, group, keycaps, language, pill, row,
    saving, set_autostart, suggested, switch,
};
use crate::theme::Gnome;
use colorshot_core::extensions::{self, State};
use colorshot_core::hotkeys;
use colorshot_core::i18n::{self, n, t, tf};
use colorshot_core::settings::Settings;
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;

pub const SIZE: egui::Vec2 = crate::preferences::SIZE;
/// Alto de la franja de abajo con «Atrás» y «Siguiente».
const FOOTER: f32 = 68.;
/// Cada cuánto se vuelve a mirar el estado de las extensiones mientras se ve su paso.
const RECHECK: f64 = 3.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Intro,
    Extension,
    Menu,
    Shortcuts,
    Finish,
}

impl Step {
    const ALL: [Self; 5] = [
        Self::Intro,
        Self::Extension,
        Self::Menu,
        Self::Shortcuts,
        Self::Finish,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Intro => t("Welcome"),
            Self::Extension => t("Extension"),
            Self::Menu => t("Menu"),
            Self::Shortcuts => t("Shortcuts"),
            Self::Finish => t("Finish"),
        }
    }
    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

pub struct Welcome {
    step: Step,
    // Colores de GNOME; `gnome` es esta paleta con o sin vidrio esmerilado.
    base: Gnome,
    gnome: Gnome,
    // Desenfoque del fondo; `None` fuera de Wayland o si el compositor no lo ofrece.
    blur: Option<crate::blur::Blur>,
    blur_supported: bool,
    // Ruta del binario para los atajos, el inicio automático y abrir Preferencias.
    binary: String,
    status: Option<String>,
    // Estado de la extensión propia y de AppIndicator, y cuándo se miró.
    colorshot: State,
    appindicator: State,
    checked: f64,
    installed: Vec<Option<String>>,
    // Lo que muestran los ajustes rápidos; se relee al cambiar de paso.
    settings: Settings,
    // Al abrir o cambiar de paso, el contenido vuelve arriba (eframe recuerda el desplazamiento).
    top: bool,
}

impl Welcome {
    pub fn new(binary: String, gnome: Gnome) -> Self {
        // Se da por visto al abrirlo: se puede volver a abrir desde la bandeja.
        let mut settings = Settings::load();
        if !settings.welcome_done {
            settings.welcome_done = true;
            let _ = settings.save();
        }
        let mut welcome = Self {
            // `COLORSHOT_STEP=2` abre otro paso (para revisarlo sin ratón).
            step: std::env::var("COLORSHOT_STEP")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|i| Step::ALL.get(i.wrapping_sub(1)).copied())
                .unwrap_or(Step::Intro),
            base: gnome,
            gnome,
            blur: None,
            blur_supported: false,
            binary,
            status: None,
            colorshot: State::Missing,
            appindicator: State::Missing,
            checked: 0.,
            installed: Vec::new(),
            settings: Settings::default(),
            top: true,
        };
        welcome.refresh();
        welcome
    }

    /// Desenfoque de la ventana, creado con su `CreationContext`.
    pub fn with_blur(mut self, blur: Option<crate::blur::Blur>) -> Self {
        self.blur = blur;
        self
    }

    fn refresh(&mut self) {
        self.colorshot = extensions::state(extensions::UUID);
        self.appindicator = extensions::state(extensions::APPINDICATOR);
        self.installed =
            hotkeys::installed().unwrap_or_else(|_| vec![None; hotkeys::HOTKEYS.len()]);
        self.settings = Settings::load();
    }

    fn go(&mut self, step: Step) {
        if step != self.step {
            self.step = step;
            self.status = None;
            self.top = true;
            self.refresh();
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.blur_supported = self.blur.as_mut().is_some_and(|b| b.supported());
        let glass = self.settings.glass().filter(|_| self.blur_supported);
        self.gnome = self.base.frosted(glass);
        if let Some(blur) = &mut self.blur {
            blur.cover(ui.ctx(), glass.is_some());
        }
        let g = self.gnome;
        let now = ui.input(|i| i.time);
        if self.step == Step::Extension && now - self.checked > RECHECK {
            self.checked = now;
            self.refresh();
        }
        if self.step == Step::Extension {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(RECHECK));
        }
        let (bar, body) = frame::toolbar(ui, &g, BAR, false);
        self.header(ui, bar);
        let content = body.with_max_y(body.bottom() - FOOTER);
        let footer = body.with_min_y(content.bottom());
        let mut page = ui.new_child(egui::UiBuilder::new().max_rect(content));
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if std::mem::take(&mut self.top) {
            scroll = scroll.vertical_scroll_offset(0.);
        }
        scroll.show(&mut page, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: PAD as i8,
                    right: PAD as i8,
                    top: 10,
                    bottom: 18,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0., 0.);
                    match self.step {
                        Step::Intro => self.intro(ui),
                        Step::Extension => self.extension(ui),
                        Step::Menu => self.menu(ui),
                        Step::Shortcuts => self.shortcuts(ui),
                        Step::Finish => self.finish(ui),
                    }
                    if let Some(status) = &self.status {
                        ui.add_space(14.);
                        ui.label(RichText::new(status).size(13.).color(g.dim));
                    }
                });
        });
        self.footer(ui, footer);
        frame::finish(ui, &g, false);
    }

    // Barra: logo y nombre a la izquierda; los pasos centrados, cada uno con su número.
    fn header(&mut self, ui: &mut Ui, bar: Rect) {
        let g = self.gnome;
        let painter = ui.painter().clone();
        let y = bar.center().y;
        let logo = Rect::from_min_size(egui::pos2(bar.left() + 4., y - 12.), vec2(24., 24.));
        if let Some(texture) = crate::icons::logo_texture(ui.ctx()) {
            painter.image(
                texture.id(),
                logo,
                Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        painter.text(
            logo.right_center() + vec2(10., 0.),
            Align2::LEFT_CENTER,
            "ColorShot",
            FontId::proportional(14.),
            g.text,
        );
        let font = FontId::proportional(13.5);
        let widths: Vec<f32> = Step::ALL
            .iter()
            .map(|s| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(s.label().to_string(), font.clone(), g.text)
                        .size()
                        .x
                }) + 44.
            })
            .collect();
        let total = widths.iter().sum::<f32>() + 4. * (Step::ALL.len() as f32 - 1.);
        let mut x = ui.ctx().content_rect().center().x - total / 2.;
        for (i, (step, width)) in Step::ALL.into_iter().zip(widths).enumerate() {
            let item = Rect::from_min_size(egui::pos2(x, y - 15.), vec2(width, 30.));
            x = item.right() + 4.;
            let response = ui
                .interact(item, ui.id().with(("step", i)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                self.go(step);
            }
            let selected = self.step == step;
            let fill = if selected {
                g.accent
            } else if response.hovered() {
                g.field
            } else {
                Color32::TRANSPARENT
            };
            let color = if selected { g.on_accent } else { g.text };
            painter.rect_filled(item, 8, fill);
            let number = item.left_center() + vec2(17., 0.);
            let done = step.index() < self.step.index();
            painter.circle_stroke(number, 9., egui::Stroke::new(1.2, color));
            painter.text(
                number,
                Align2::CENTER_CENTER,
                if done {
                    icon::CHECK.to_string()
                } else {
                    (i + 1).to_string()
                },
                FontId::proportional(11.),
                color,
            );
            painter.text(
                item.left_center() + vec2(33., 0.),
                Align2::LEFT_CENTER,
                step.label(),
                font.clone(),
                color,
            );
        }
    }

    fn footer(&mut self, ui: &mut Ui, rect: Rect) {
        let g = self.gnome;
        ui.painter().hline(
            rect.left() + PAD..=rect.right() - PAD,
            rect.top() + 0.5,
            egui::Stroke::new(1., g.line),
        );
        let inner = rect.shrink2(vec2(PAD, 0.));
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 8.;
        let i = self.step.index();
        let last = i + 1 == Step::ALL.len();
        let next = if last {
            format!("{}  {}", t("Done"), icon::CHECK)
        } else {
            format!("{}  {}", t("Next"), icon::ARROW_RIGHT)
        };
        if suggested(&mut right, &g, &next).clicked() {
            if last {
                right.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.go(Step::ALL[i + 1]);
            }
        }
        if i > 0
            && pill(
                &mut right,
                &g,
                &format!("{}  {}", icon::ARROW_LEFT, t("Back")),
            )
            .clicked()
        {
            self.go(Step::ALL[i - 1]);
        }
        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        left.label(
            RichText::new(tf("Step {} of {}", &[&(i + 1), &Step::ALL.len()]))
                .size(13.)
                .color(g.dim),
        );
    }

    fn intro(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        ui.add_space(14.);
        ui.vertical_centered(|ui| {
            if let Some(texture) = crate::icons::logo_texture(ui.ctx()) {
                ui.add(egui::Image::new(&texture).fit_to_exact_size(vec2(72., 72.)));
                ui.add_space(12.);
            }
            ui.label(
                RichText::new(t("Welcome to ColorShot"))
                    .size(24.)
                    .strong()
                    .color(g.text),
            );
            ui.add_space(6.);
            ui.label(
                RichText::new(t(
                    "Capture, annotate, pin, read text and pick colors. This guide sets up the GNOME extension, your shortcuts and startup in a minute.",
                ))
                .size(14.)
                .color(g.dim),
            );
        });
        group(ui, &g, t("How it works"), |ui| {
            let rows = [
                (
                    n("Tray icon"),
                    n("Click it to capture an area; its menu has every action"),
                ),
                (
                    n("After a capture"),
                    n("A thumbnail appears: drag it to an app, click it to edit"),
                ),
                (
                    n("Editor"),
                    n("Arrows, text, blur and more, then copy or save"),
                ),
                (
                    n("Read Text / QR"),
                    n("Select an area and its text or QR code is copied"),
                ),
                (n("Pick Color"), n("Click any pixel to copy its color")),
                (
                    n("Launcher menu"),
                    n("Right-click ColorShot in the dock or app grid for quick actions"),
                ),
            ];
            let count = rows.len();
            for (i, (title, subtitle)) in rows.into_iter().enumerate() {
                row(ui, &g, t(title), Some(t(subtitle)), i + 1 < count, |_| {});
            }
        });
    }

    fn extension(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        ui.add_space(14.);
        ui.label(
            RichText::new(t("Activate the extension?"))
                .size(20.)
                .strong()
                .color(g.text),
        );
        paragraph(
            ui,
            &g,
            t(
                "GNOME on Wayland keeps apps from seeing other windows. The ColorShot extension lets ColorShot take screenshots over the top bar, capture a single window and capture only the area you choose. AppIndicator shows its tray icon.",
            ),
        );
        let mut action = None;
        group(ui, &g, t("ColorShot extension"), |ui| {
            let subtitle = match self.colorshot {
                State::Active => t("Active"),
                State::NeedsRelogin => t("Log out and back in to load it"),
                State::Disabled => t("Installed but turned off"),
                State::Missing => t("Not installed"),
            };
            row(
                ui,
                &g,
                t("ColorShot for GNOME Shell"),
                Some(subtitle),
                true,
                |ui| match self.colorshot {
                    State::Active | State::NeedsRelogin => ready(ui, self.colorshot),
                    State::Disabled => {
                        if suggested(ui, &g, t("Enable")).clicked() {
                            action = Some(Action::Enable(extensions::UUID));
                        }
                    }
                    State::Missing => {
                        if suggested(
                            ui,
                            &g,
                            &format!("{}  {}", icon::DOWNLOAD_SIMPLE, t("Install")),
                        )
                        .clicked()
                        {
                            action = Some(Action::Install);
                        }
                    }
                },
            );
            let uses = [
                (n("Capture Window"), n("Pick any window with a click")),
                (
                    n("Over the top bar"),
                    n("The area selector also covers the top bar"),
                ),
                (
                    n("Only the chosen area"),
                    n("Captures just the area, without a full-screen shot"),
                ),
            ];
            let count = uses.len();
            for (i, (title, subtitle)) in uses.into_iter().enumerate() {
                row(ui, &g, t(title), Some(t(subtitle)), i + 1 < count, |_| {});
            }
        });
        group(ui, &g, t("Tray icon"), |ui| {
            let subtitle = match self.appindicator {
                State::Active => t("The ColorShot icon is in the top bar"),
                State::NeedsRelogin => t("Log out and back in to load it"),
                State::Disabled => t("Installed but turned off"),
                State::Missing => t("Needed to show the tray icon in GNOME"),
            };
            row(
                ui,
                &g,
                t("AppIndicator support"),
                Some(subtitle),
                false,
                |ui| match self.appindicator {
                    State::Active | State::NeedsRelogin => ready(ui, self.appindicator),
                    State::Disabled => {
                        if suggested(ui, &g, t("Enable")).clicked() {
                            action = Some(Action::Enable(extensions::APPINDICATOR));
                        }
                    }
                    State::Missing => {
                        if pill(
                            ui,
                            &g,
                            &format!("{}  {}", icon::ARROW_SQUARE_OUT, t("Get it")),
                        )
                        .clicked()
                        {
                            action = Some(Action::Open(extensions::APPINDICATOR_URL));
                        }
                    }
                },
            );
        });
        if [self.colorshot, self.appindicator].contains(&State::NeedsRelogin) {
            paragraph(
                ui,
                &g,
                t(
                    "GNOME loads new extensions the next time you log in. Everything else already works.",
                ),
            );
        }
        match action {
            Some(Action::Install) => {
                self.status = extensions::install()
                    .err()
                    .map(|e| tf("Could not install the extension: {}", &[&e]));
            }
            Some(Action::Enable(uuid)) => {
                self.status = extensions::enable(uuid)
                    .err()
                    .map(|e| tf("Could not enable the extension: {}", &[&e]));
            }
            Some(Action::Open(url)) => {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
            None => {}
        }
        if action.is_some() {
            self.refresh();
        }
    }

    // El menú de la bandeja con números sobre la imagen y su leyenda al lado; debajo, los
    // ajustes que más se cambian.
    fn menu(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        paragraph(
            ui,
            &g,
            t(
                "Everything starts from the ColorShot icon in the top bar. Open its menu to capture or reach any tool.",
            ),
        );
        ui.add_space(14.);
        let legend = [
            (
                n("ColorShot icon"),
                n("In the top bar, next to the system icons"),
            ),
            (
                n("Captures and tools"),
                n("Screen, area, window, text / QR, color and palettes"),
            ),
            (
                n("More"),
                n("Repeat, delayed capture, default folder, edit last and this guide"),
            ),
            (
                n("Preferences…"),
                n("Turn options on or off and change the appearance"),
            ),
        ];
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(20., 0.);
            if let Some(texture) = menu_texture(ui.ctx()) {
                let size = texture.size_vec2() * (300. / texture.size_vec2().x);
                let rect = ui
                    .add(
                        egui::Image::new(&texture)
                            .fit_to_exact_size(size)
                            .corner_radius(12),
                    )
                    .rect;
                markers(ui, &g, rect, texture.size_vec2());
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(0., 0.);
                for (i, (title, subtitle)) in legend.into_iter().enumerate() {
                    ui.add_space(if i == 0 { 6. } else { 16. });
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.;
                        let (rect, _) = ui.allocate_exact_size(vec2(22., 22.), Sense::hover());
                        badge(ui.painter(), &g, rect.center(), i + 1);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(t(title)).size(14.5).color(g.text));
                            ui.add_space(2.);
                            ui.label(RichText::new(t(subtitle)).size(13.).color(g.dim));
                        });
                    });
                }
            });
        });
        // Los mismos grupos que Preferencias; cada cambio se guarda en el momento porque la
        // instancia y las demás ventanas leen el archivo.
        let mut settings = self.settings.clone();
        saving(ui, &g, &mut settings, &mut self.status);
        after_capture(ui, &g, &mut settings);
        appearance(ui, &g, &mut settings, self.blur_supported);
        language(ui, &g, &mut settings);
        if settings != self.settings {
            if settings.language != self.settings.language {
                i18n::set(settings.language);
            }
            if let Err(error) = settings.save() {
                self.status = Some(error.to_string());
            }
            self.settings = settings;
        }
    }

    fn shortcuts(&mut self, ui: &mut Ui) {
        let g = self.gnome;
        paragraph(
            ui,
            &g,
            t(
                "Global shortcuts work from any app, even with no ColorShot window open. They are added to GNOME's keyboard settings.",
            ),
        );
        let missing = self.installed.iter().filter(|b| b.is_none()).count();
        let mut add_all = false;
        group(ui, &g, t("Global shortcuts"), |ui| {
            row(
                ui,
                &g,
                t("Add to GNOME"),
                Some(t("You can change any of them later")),
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
                        add_all = true;
                    }
                },
            );
            let count = hotkeys::HOTKEYS.len();
            for (i, (name, _, default)) in hotkeys::HOTKEYS.iter().enumerate() {
                let now = self.installed.get(i).cloned().flatten();
                let subtitle = if now.is_some() {
                    t("Added")
                } else {
                    t("Not added")
                };
                let binding = now.unwrap_or_else(|| default.to_string());
                row(ui, &g, t(name), Some(subtitle), i + 1 < count, |ui| {
                    keycaps(ui, &g, &binding);
                });
            }
        });
        group(ui, &g, t("Customize"), |ui| {
            row(
                ui,
                &g,
                t("Change a shortcut"),
                Some(t("Opens Preferences on the Shortcuts page")),
                false,
                |ui| {
                    if pill(
                        ui,
                        &g,
                        &format!("{}  {}", icon::KEYBOARD, t("Open Shortcuts")),
                    )
                    .clicked()
                    {
                        self.status = open_preferences(&self.binary, "shortcuts").err();
                    }
                },
            );
        });
        if add_all {
            // Las que faltan, con su combinación de fábrica; las ya añadidas no se tocan.
            let add: Vec<_> = hotkeys::HOTKEYS
                .iter()
                .zip(&self.installed)
                .filter(|(_, now)| now.is_none())
                .map(|((_, command, default), _)| (*command, default.to_string()))
                .collect();
            self.status = Some(match hotkeys::install(&self.binary, &add) {
                Ok(()) => t("Shortcuts added to GNOME").into(),
                Err(error) => tf("Could not add shortcuts: {}", &[&error]),
            });
            self.refresh();
        }
    }

    fn finish(&mut self, ui: &mut Ui) {
        let g = self.gnome;
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
                        self.status = set_autostart(enabled, &self.binary).err();
                    }
                },
            );
        });
        group(ui, &g, t("Good to know"), |ui| {
            row(
                ui,
                &g,
                t("Preferences"),
                Some(t("Save folder, format, thumbnail and more")),
                true,
                |ui| {
                    if pill(ui, &g, &format!("{}  {}", icon::GEAR_SIX, t("Open"))).clicked() {
                        self.status = open_preferences(&self.binary, "general").err();
                    }
                },
            );
            row(
                ui,
                &g,
                t("Open this guide again"),
                Some(t("Tray menu → More → Welcome Guide")),
                false,
                |_| {},
            );
        });
    }
}

#[derive(Clone, Copy)]
enum Action {
    Install,
    Enable(&'static str),
    Open(&'static str),
}

// Marca verde si funciona; reloj si espera a volver a iniciar sesión.
fn ready(ui: &mut Ui, state: State) {
    let (glyph, text, color) = if state == State::Active {
        (icon::CHECK_CIRCLE, t("Active"), OK)
    } else {
        (
            icon::CLOCK,
            t("After login"),
            Color32::from_rgb(246, 211, 45),
        )
    };
    ui.label(
        RichText::new(format!("{glyph}  {text}"))
            .size(14.)
            .color(color),
    );
}

/// Captura del menú de la bandeja (`docs/screenshots/menu.png`), cargada una vez.
fn menu_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("colorshot-welcome-menu");
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return cached;
    }
    let texture = image::load_from_memory(include_bytes!("../../../docs/screenshots/menu.png"))
        .ok()
        .map(|image| {
            let image = image.into_rgba8();
            ctx.load_texture(
                "colorshot-welcome-menu",
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                crate::icons::SMOOTH,
            )
        });
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// Números sobre la captura del menú; las posiciones van en píxeles de la imagen original.
fn markers(ui: &Ui, g: &Gnome, rect: Rect, source: egui::Vec2) {
    let scale = rect.width() / source.x;
    let at = |x: f32, y: f32| rect.min + vec2(x, y) * scale;
    let painter = ui.painter();
    // El icono de la bandeja, rodeado; el resto, a la izquierda de su entrada.
    painter.circle_stroke(at(260., 22.), 26. * scale, egui::Stroke::new(2., g.accent));
    badge(painter, g, at(260., 22.) + vec2(24., 14.), 1);
    painter.line_segment(
        [at(122., 82.), at(122., 386.)],
        egui::Stroke::new(2., g.accent),
    );
    badge(painter, g, at(122., 234.), 2);
    badge(painter, g, at(122., 428.), 3);
    badge(painter, g, at(122., 512.), 4);
}

fn badge(painter: &egui::Painter, g: &Gnome, center: egui::Pos2, number: usize) {
    painter.circle_filled(center, 11., g.accent);
    painter.text(
        center,
        Align2::CENTER_CENTER,
        number.to_string(),
        FontId::proportional(12.),
        g.on_accent,
    );
}

fn paragraph(ui: &mut Ui, g: &Gnome, text: &str) {
    ui.add_space(14.);
    ui.label(RichText::new(text).size(14.).color(g.dim));
}

/// Preferencias en su propio proceso y en la sección `tab`; va directo, sin pasar por la
/// instancia, para que le llegue la sección.
fn open_preferences(binary: &str, tab: &str) -> Result<(), String> {
    std::process::Command::new(binary)
        .arg("prefs")
        .env("COLORSHOT_CHILD", "1")
        .env("COLORSHOT_TAB", tab)
        .spawn()
        .map(drop)
        .map_err(|error| error.to_string())
}
