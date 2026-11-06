//! Tokens oscuros (los grises de tablet-center), fuentes y paleta en blanco y negro.
use egui::{Color32, Context, FontId, Stroke};
pub struct Tokens;
impl Tokens {
    pub const CANVAS: Color32 = Color32::from_gray(30);
    pub const CHECKER: Color32 = Color32::from_gray(36);
    pub const BAR: Color32 = Color32::from_gray(46);
    pub const TEXT: Color32 = Color32::WHITE;
    pub const DIM: Color32 = Color32::from_gray(168);
    pub const ACCENT: Color32 = Color32::from_rgb(240, 80, 60);
    pub const SELECTION: Color32 = Color32::from_rgb(53, 132, 228);
}

/// Colores de libadwaita según el esquema de GNOME; el acento es blanco en oscuro y negro en
/// claro, sin el color de GNOME.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gnome {
    pub dark: bool,
    pub window: Color32,
    pub header: Color32,
    pub card: Color32,
    /// Fondo de menús y desplegables: opaco también con frosted glass, que se leen encima
    /// de otros controles.
    pub popup: Color32,
    pub line: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub field: Color32,
    pub field_hover: Color32,
    /// Brillo de 1 px dentro del borde de la ventana.
    pub highlight: Color32,
    /// Hundido: fondo de los selectores segmentados.
    pub view: Color32,
    pub accent: Color32,
    /// Texto y figuras sobre el acento.
    pub on_accent: Color32,
}

impl Gnome {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                window: Color32::from_gray(36),
                header: Color32::from_gray(48),
                card: Color32::from_gray(46),
                popup: Color32::from_gray(46),
                line: Color32::from_gray(58),
                text: Color32::WHITE,
                dim: Color32::from_gray(168),
                field: Color32::from_gray(56),
                field_hover: Color32::from_gray(70),
                highlight: Color32::from_gray(78),
                view: Color32::from_gray(30),
                accent: Color32::WHITE,
                on_accent: Color32::from_gray(30),
            }
        } else {
            Self {
                dark,
                window: Color32::from_rgb(250, 250, 251),
                header: Color32::from_rgb(235, 235, 237),
                card: Color32::from_rgb(255, 255, 255),
                popup: Color32::from_rgb(255, 255, 255),
                line: Color32::from_rgb(222, 222, 226),
                text: Color32::from_rgb(30, 30, 34),
                dim: Color32::from_rgb(110, 110, 116),
                field: Color32::from_rgb(228, 228, 231),
                field_hover: Color32::from_rgb(216, 216, 220),
                highlight: Color32::from_rgb(255, 255, 255),
                view: Color32::from_rgb(232, 232, 235),
                accent: Color32::from_rgb(30, 30, 34),
                on_accent: Color32::WHITE,
            }
        }
    }

    /// Frosted glass: el fondo de la ventana deja pasar lo que hay detrás (desenfocado por el
    /// compositor) con `tint` de color encima, oscurecido hacia el negro según `shade`. Barra y
    /// tarjetas van a medias sobre ese fondo, para que sigan destacando.
    pub fn frosted(self, glass: Option<colorshot_core::settings::Glass>) -> Self {
        let Some(glass) = glass else {
            return self;
        };
        let dark = |c: Color32| c.lerp_to_gamma(Color32::BLACK, glass.shade);
        Self {
            window: dark(self.window).gamma_multiply(glass.tint),
            header: dark(self.header).gamma_multiply(0.5),
            card: dark(self.card).gamma_multiply(0.45),
            view: dark(self.view).gamma_multiply(0.5),
            // Opacos, los bordes resaltan demasiado sobre el fondo translúcido.
            line: self.line.gamma_multiply(0.4),
            highlight: self.highlight.gamma_multiply(0.35),
            ..self
        }
    }

    /// Lee `color-scheme` de GNOME; sin gsettings, oscuro.
    pub fn current() -> Self {
        let dark = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .trim_matches('\'')
                    .to_string()
            })
            .is_none_or(|s| s != "default" && s != "prefer-light");
        Self::new(dark)
    }
}

/// Estilo de egui con la paleta de GNOME (para los desplegables y menús de Preferencias).
pub fn apply_gnome(ctx: &Context, gnome: &Gnome) {
    // El selector de idioma muestra cada nombre en su escritura.
    apply_fonts(ctx, true);
    dark_style(ctx);
    let theme = if gnome.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let mut visuals = if gnome.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = gnome.window;
    visuals.window_fill = gnome.popup;
    visuals.override_text_color = Some(gnome.text);
    visuals.selection.bg_fill = gnome.accent.gamma_multiply(0.3);
    visuals.selection.stroke = Stroke::new(1., gnome.text);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
        widget.bg_stroke = Stroke::NONE;
    }
    visuals.widgets.inactive.weak_bg_fill = gnome.field;
    visuals.widgets.inactive.bg_fill = gnome.field;
    visuals.widgets.hovered.weak_bg_fill = gnome.field_hover;
    visuals.widgets.hovered.bg_fill = gnome.field_hover;
    visuals.widgets.active.weak_bg_fill = gnome.field_hover;
    visuals.widgets.open.weak_bg_fill = gnome.field_hover;
    visuals.window_stroke = Stroke::new(1., gnome.line);
    visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals = visuals;
    ctx.set_style_of(theme, style);
}
pub fn apply(ctx: &Context) {
    apply_fonts(ctx, colorshot_core::i18n::current().cjk());
    dark_style(ctx);
}
fn dark_style(ctx: &Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Tokens::CANVAS;
    style.visuals.window_fill = Tokens::BAR;
    style.visuals.override_text_color = Some(Tokens::TEXT);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1., Tokens::DIM);
    style.visuals.selection.bg_fill = Tokens::SELECTION;
    style.visuals.hyperlink_color = Tokens::ACCENT;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.));
    ctx.set_style_of(egui::Theme::Dark, style);
}
/// Fuentes de egui y Phosphor; con `cjk`, además una fuente china o japonesa del sistema.
pub fn apply_fonts(ctx: &Context, cjk: bool) {
    ctx.set_fonts(font_definitions(cjk));
}

/// Familia en negrita de la fuente de la interfaz; existe siempre tras `apply_system_font`.
pub const BOLD: &str = "system-bold";

/// Como `apply_fonts` con CJK, pero con la fuente de la interfaz de GNOME delante y su
/// seminegrita en la familia `BOLD`. Sin hinting: a escala fraccionaria el ajuste a la rejilla
/// deja los trazos dentados.
pub fn apply_system_font(ctx: &Context) {
    let mut fonts = font_definitions(true);
    let mut bold = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Some(regular) = system_font() {
        let tweak = egui::FontTweak {
            hinting: Some(false),
            ..Default::default()
        };
        let variable = regular
            .variation_axes()
            .iter()
            .any(|axis| axis.tag == "wght");
        let heavy = if variable {
            Some(regular.clone().tweak(egui::FontTweak {
                coords: egui::epaint::text::VariationCoords::new([("wght", 600.)]),
                ..tweak.clone()
            }))
        } else {
            None
        };
        let regular = regular.tweak(tweak);
        fonts
            .font_data
            .insert("system".into(), std::sync::Arc::new(regular));
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "system".into());
        }
        bold.insert(0, "system".into());
        if let Some(heavy) = heavy {
            fonts
                .font_data
                .insert("system-bold".into(), std::sync::Arc::new(heavy));
            bold.insert(0, "system-bold".into());
        }
    }
    fonts
        .families
        .insert(egui::FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);
}

// La fuente de `org.gnome.desktop.interface font-name` («Adwaita Sans 11») por fontconfig.
fn system_font() -> Option<egui::FontData> {
    let setting = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "font-name"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .trim_matches('\'')
                .to_owned()
        })
        .unwrap_or_default();
    // Quita el tamaño del final.
    let family = setting
        .rsplit_once(' ')
        .filter(|(_, size)| size.parse::<f32>().is_ok())
        .map_or(setting.as_str(), |(family, _)| family);
    let family = if family.is_empty() {
        "sans-serif"
    } else {
        family
    };
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", family])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut lines = text.lines();
    let (file, index) = (lines.next()?, lines.next().unwrap_or("0"));
    let mut data = egui::FontData::from_owned(std::fs::read(file).ok()?);
    data.index = index.parse().unwrap_or(0);
    Some(data)
}

fn font_definitions(cjk: bool) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    if cjk && let Some(data) = cjk_font() {
        fonts
            .font_data
            .insert("cjk".into(), std::sync::Arc::new(data));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push("cjk".into());
        }
    }
    fonts
}

// egui no trae ideogramas: Noto Sans CJK de la variante del idioma o lo que dé fontconfig.
fn cjk_font() -> Option<egui::FontData> {
    let japanese = colorshot_core::i18n::current() == colorshot_core::i18n::Language::Japanese;
    let (family, lang) = if japanese {
        ("Noto Sans CJK JP", "ja")
    } else {
        ("Noto Sans CJK SC", "zh-cn")
    };
    let pattern = format!("sans-serif:lang={lang}");
    let found = [family, pattern.as_str()].into_iter().find_map(|pattern| {
        let output = std::process::Command::new("fc-match")
            .args(["-f", "%{file}\n%{index}\n%{family[0]}", pattern])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut lines = text.lines();
        let (file, index, name) = (lines.next()?, lines.next()?, lines.next()?);
        (pattern != family || name == family)
            .then(|| (file.to_string(), index.parse::<u32>().unwrap_or(0)))
    })?;
    let mut data = egui::FontData::from_owned(std::fs::read(&found.0).ok()?);
    data.index = found.1;
    Some(data)
}

pub fn paint_canvas(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0., Tokens::CANVAS);
    paint_checker(painter, rect, Tokens::CHECKER);
}

/// Solo los cuadros claros de la cuadrícula, para pintarlos sobre otro fondo.
pub fn paint_checker(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    for y in 0..(rect.height() / 12.).ceil() as i32 {
        for x in 0..(rect.width() / 12.).ceil() as i32 {
            if (x + y) % 2 == 0 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(x as f32 * 12., y as f32 * 12.),
                        egui::vec2(12., 12.),
                    )
                    .intersect(rect),
                    0.,
                    color,
                );
            }
        }
    }
}
