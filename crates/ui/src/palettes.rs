//! Ventana de paletas de colores: columnas a lo ancho con su HEX y un dato secundario y, a la
//! derecha, la biblioteca (los últimos colores del cuentagotas y las paletas guardadas).
//!
//! Al pasar el ratón por una columna salen sus acciones (quitar, tonos, arrastrar, copiar); por
//! los costados, un «+» añade un tono más oscuro a la izquierda o más claro a la derecha. Un clic
//! en el color abre el selector. Todo se guarda al cambiar: no hay botón «Save».
use crate::editor::color::Format;
use crate::frame;
use crate::theme;
use crate::theme::Gnome;
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Id, Layout, Pos2, Rect, Sense, Stroke, Ui, Vec2,
    pos2, vec2,
};
use egui_phosphor::regular as icon;
use colorshot_core::i18n::{t, tf};
use colorshot_core::palettes::{self, COLORS_MAX, Palette, Palettes, Rgb, Secondary, View};
use colorshot_core::settings::{Glass, Settings};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

/// Tamaño inicial, cercano al del editor sin imagen.
pub const SIZE: Vec2 = vec2(1000., 620.);
pub const MIN_SIZE: Vec2 = vec2(640., 420.);
/// Alto de la cabecera de la barra lateral, el de la barra del editor y Preferencias.
const HEADER: f32 = 52.;
const SIDEBAR: f32 = 264.;
/// Rojo de GNOME para confirmar un borrado.
const RED: Color32 = Color32::from_rgb(0xe0, 0x1b, 0x24);
/// Ancho mínimo de una columna; si no caben, se desplazan en horizontal.
const MIN_COLUMN: f32 = 140.;
/// Franja de cada costado donde sale el «+».
const EDGE: f32 = 44.;
const BUTTON: f32 = 36.;
/// Ancho del contenido del selector de color.
const PICKER: f32 = 240.;

/// Lo que llega del cuentagotas: los guardados con clic derecho y, al cerrarse, el elegido.
enum Pick {
    Keep(Rgb),
    Done(Option<Rgb>),
}

/// Icono de `assets/icons/palettes` (instalado con `just icons`) y su glifo de Phosphor
/// por si falta.
type Icon = (&'static str, &'static str);
const ADD: Icon = ("20-palette-add", icon::PLUS);
const EYEDROPPER: Icon = ("20-palette-eyedropper", icon::EYEDROPPER);
const SAVE: Icon = ("20-palette-save", icon::FLOPPY_DISK);
const DELETE: Icon = ("20-palette-delete", icon::TRASH);
const DUPLICATE: Icon = ("20-palette-duplicate", icon::COPY);
const REMOVE: Icon = ("20-palette-remove", icon::X);
const SHADES: Icon = ("20-palette-shades", icon::STACK);
const DRAG: Icon = ("20-palette-drag", icon::ARROWS_LEFT_RIGHT);
const COPY: Icon = ("20-palette-copy", icon::COPY);
const DUPLICATE_COLOR: Icon = ("20-palette-duplicate-color", icon::PLUS_SQUARE);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    Picked,
    Saved(usize),
}

/// Selector de color abierto sobre una columna.
struct Picker {
    column: usize,
    /// Tono (grados), saturación y brillo: el tono sobrevive aunque el color sea gris.
    hsb: [f32; 3],
    /// HEX del campo, tal como se escribe.
    text: String,
    /// Canales de los deslizadores y la vista a la que pertenecen. Se rehacen del color si
    /// cambia la vista o el color llega de otro lado; mientras se arrastra mandan ellos.
    channels: Option<(View, [f32; 4])>,
    /// LAB u OKLCH fuera de sRGB: el color es el más cercano bajando el croma.
    clipped: bool,
    sv: Option<egui::TextureHandle>,
    sv_hue: f32,
    /// Degradado de cada deslizador.
    tracks: Vec<egui::TextureHandle>,
    /// Columna y área del cuadro anterior, para cerrarlo al pulsar fuera.
    anchor: Rect,
    area: Rect,
    opened: bool,
}

impl Picker {
    fn new(column: usize, c: Rgb, anchor: Rect, at: Pos2) -> Self {
        Self {
            column,
            hsb: palettes::to_hsb(c),
            text: format!("#{}", palettes::hex(c)),
            channels: None,
            clipped: false,
            sv: None,
            sv_hue: -1.,
            tracks: Vec::new(),
            anchor,
            area: Rect::from_min_size(at, Vec2::ZERO),
            opened: true,
        }
    }

    /// El color cambió desde fuera de los deslizadores.
    fn set(&mut self, c: Rgb) {
        let [h, s, v] = palettes::to_hsb(c);
        // En los grises el tono no cuenta: se queda el de antes.
        let h = if s < 0.001 || v < 0.001 {
            self.hsb[0]
        } else {
            h
        };
        self.hsb = [h, s, v];
        // Lo que se está escribiendo en el campo y ya da este color se deja como está.
        if palettes::parse_hex(&self.text) != Some(c) {
            self.text = format!("#{}", palettes::hex(c));
        }
        self.channels = None;
        self.clipped = false;
    }
}

/// Deslizadores de cada vista: nombre, mínimo, máximo y decimales.
fn sliders(view: View) -> &'static [(&'static str, f32, f32, usize)] {
    match view {
        View::Picker => &[],
        View::Hsb => &[
            ("Hue", 0., 360., 0),
            ("Saturation", 0., 100., 0),
            ("Brightness", 0., 100., 0),
        ],
        View::Hsl => &[
            ("Hue", 0., 360., 0),
            ("Saturation", 0., 100., 0),
            ("Luminance", 0., 100., 0),
        ],
        View::Rgb => &[
            ("Red", 0., 255., 0),
            ("Green", 0., 255., 0),
            ("Blue", 0., 255., 0),
        ],
        View::Cmyk => &[
            ("Cyan", 0., 100., 0),
            ("Magenta", 0., 100., 0),
            ("Yellow", 0., 100., 0),
            ("Key", 0., 100., 0),
        ],
        View::Lab => &[
            ("Luminance", 0., 100., 0),
            ("Green-Red", -128., 127., 0),
            ("Blue-Yellow", -128., 127., 0),
        ],
        View::Oklch => &[
            ("Lightness", 0., 1., 3),
            ("Chroma", 0., 0.37, 3),
            ("Hue", 0., 360., 1),
        ],
    }
}

pub(crate) fn view_name(view: View) -> &'static str {
    match view {
        View::Picker => t("Picker"),
        View::Hsb => "HSB",
        View::Hsl => "HSL",
        View::Rgb => "RGB",
        View::Cmyk => "CMYK",
        View::Lab => "LAB",
        View::Oklch => "OKLCH",
    }
}

/// Lo que copia el selector: el color en el formato de su vista.
fn view_text(c: Rgb, view: View) -> String {
    match view {
        View::Picker => format!("#{}", palettes::hex(c)),
        View::Hsb => secondary(c, Secondary::Hsb),
        View::Hsl => secondary(c, Secondary::Hsl),
        View::Rgb => secondary(c, Secondary::Rgb),
        View::Cmyk => secondary(c, Secondary::Cmyk),
        View::Lab => secondary(c, Secondary::Lab),
        View::Oklch => secondary(c, Secondary::Oklch),
    }
}

/// El color de unos canales; fuera de sRGB (LAB y OKLCH), el más cercano bajando el croma,
/// como el «fallback» de oklch.com. Devuelve también si hubo que ajustarlo.
fn fit(view: View, channels: [f32; 4]) -> (Rgb, bool) {
    if let Some(c) = palettes::from_channels(view, channels) {
        return (c, false);
    }
    let [x, y, z, w] = channels;
    let scaled = |f: f32| match view {
        View::Oklch => [x, y * f, z, w],
        _ => [x, y * f, z * f, w],
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..20 {
        let middle = (low + high) / 2.;
        if palettes::from_channels(view, scaled(middle)).is_some() {
            low = middle;
        } else {
            high = middle;
        }
    }
    let gray = (x / if view == View::Oklch { 1. } else { 100. } * 255.).clamp(0., 255.) as u8;
    (
        palettes::from_channels(view, scaled(low)).unwrap_or([gray; 3]),
        true,
    )
}

/// Degradado del deslizador `k`: el color moviendo solo ese canal. Lo que cae fuera de sRGB
/// queda transparente. El tono de HSB y HSL va con el color puro.
fn track_pixels(view: View, channels: [f32; 4], k: usize) -> Vec<u8> {
    const STEPS: usize = 96;
    let (_, min, max, _) = sliders(view)[k];
    (0..STEPS)
        .flat_map(|i| {
            let mut ch = channels;
            ch[k] = min + (max - min) * i as f32 / (STEPS - 1) as f32;
            if k == 0 && view == View::Hsb {
                ch = [ch[0], 100., 100., 0.];
            } else if k == 0 && view == View::Hsl {
                ch = [ch[0], 100., 50., 0.];
            }
            match palettes::from_channels(view, ch) {
                Some([r, g, b]) => [r, g, b, 255],
                None => [0; 4],
            }
        })
        .collect()
}

pub struct PaletteWindow {
    store: Palettes,
    // Fecha del archivo leído o escrito: el cuentagotas lo cambia desde otro proceso.
    stamp: Option<SystemTime>,
    checked: Instant,
    dirty: bool,
    shown: Shown,
    gnome: Gnome,
    // Ruta del binario para lanzar el cuentagotas.
    binary: String,
    picker: Option<Picker>,
    shades: Option<usize>,
    dragging: Option<usize>,
    // Cuentagotas en curso: columna que recibe el color (`None` = solo a «Picked colors»).
    eyedropper: Option<(Option<usize>, Receiver<Pick>)>,
    confirm_delete: bool,
    // Paleta guardada cuyo nombre se edita y si su campo aún debe tomar el foco.
    renaming: Option<usize>,
    focus_name: bool,
    notice: Option<(String, Instant)>,
    hue: Option<egui::TextureHandle>,
    /// Desplazamiento horizontal de las columnas.
    scroll: f32,
    /// Zona de las columnas y radio de sus esquinas izquierdas en este cuadro.
    view: Rect,
    radius: u8,
    /// Desenfoque de lo que hay detrás de la barra lateral, como el de Preferencias.
    blur: Option<crate::blur::Blur>,
    // «Frosted glass» de los ajustes; se relee con el archivo de paletas.
    glass: Option<Glass>,
    side_fill: Color32,
}

fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Texto legible encima del color.
fn ink(c: Rgb) -> Color32 {
    if palettes::luminance(c) > 0.55 {
        Color32::from_gray(20)
    } else {
        Color32::WHITE
    }
}

fn mtime() -> Option<SystemTime> {
    std::fs::metadata(palettes::path()?).ok()?.modified().ok()
}

/// Nombre corto de cada dato secundario, para su menú.
pub(crate) fn secondary_name(kind: Secondary) -> &'static str {
    match kind {
        Secondary::Name => t("Name"),
        Secondary::Rgb => "RGB",
        Secondary::Hsl => "HSL",
        Secondary::Hsb => "HSB",
        Secondary::Cmyk => "CMYK",
        Secondary::Oklch => "OKLCH",
        Secondary::Lab => "LAB",
        Secondary::Lch => "LCH",
    }
}

/// El dato secundario de `c`, bajo su HEX.
pub fn secondary(c: Rgb, kind: Secondary) -> String {
    let pct = |v: f32| (v * 100.).round();
    match kind {
        Secondary::Name => palettes::name(c).to_owned(),
        Secondary::Rgb => format!("{}, {}, {}", c[0], c[1], c[2]),
        Secondary::Hsl => {
            let [h, s, l] = palettes::to_hsl(c);
            format!("{}°, {}%, {}%", h.round(), pct(s), pct(l))
        }
        Secondary::Hsb => {
            let [h, s, b] = palettes::to_hsb(c);
            format!("{}°, {}%, {}%", h.round(), pct(s), pct(b))
        }
        Secondary::Cmyk => {
            let [r, g, b] = c.map(|v| v as f32 / 255.);
            let k = 1. - r.max(g).max(b);
            let part = |v: f32| if k >= 1. { 0. } else { (1. - v - k) / (1. - k) };
            format!(
                "{}, {}, {}, {}",
                pct(part(r)),
                pct(part(g)),
                pct(part(b)),
                pct(k)
            )
        }
        Secondary::Oklch => Format::Oklch.text(color(c)),
        Secondary::Lab => Format::Lab.text(color(c)),
        Secondary::Lch => Format::Lch.text(color(c)),
    }
}

impl PaletteWindow {
    pub fn new(binary: String, gnome: Gnome) -> Self {
        let store = Palettes::load();
        // Abre la primera paleta guardada; sin ninguna, los colores del cuentagotas.
        let shown = if store.saved.is_empty() {
            Shown::Picked
        } else {
            Shown::Saved(0)
        };
        Self {
            store,
            stamp: mtime(),
            checked: Instant::now(),
            dirty: false,
            shown,
            gnome,
            binary,
            picker: None,
            shades: None,
            dragging: None,
            eyedropper: None,
            confirm_delete: false,
            renaming: None,
            focus_name: false,
            notice: None,
            hue: None,
            scroll: 0.,
            view: Rect::NOTHING,
            radius: 0,
            blur: None,
            glass: Settings::load().glass(),
            side_fill: gnome.header,
        }
    }

    pub fn with_blur(mut self, blur: Option<crate::blur::Blur>) -> Self {
        self.blur = blur;
        self
    }

    fn colors(&self) -> &[Rgb] {
        match self.shown {
            Shown::Picked => &self.store.picked,
            Shown::Saved(i) => self.store.saved.get(i).map_or(&[], |p| &p.colors),
        }
    }

    fn colors_mut(&mut self) -> Option<&mut Vec<Rgb>> {
        self.dirty = true;
        match self.shown {
            Shown::Picked => Some(&mut self.store.picked),
            Shown::Saved(i) => self.store.saved.get_mut(i).map(|p| &mut p.colors),
        }
    }

    fn show(&mut self, shown: Shown) {
        self.shown = shown;
        self.picker = None;
        self.shades = None;
        self.dragging = None;
        self.confirm_delete = false;
        if self.renaming.is_some_and(|i| shown != Shown::Saved(i)) {
            self.renaming = None;
        }
    }

    fn notify(&mut self, text: String) {
        self.notice = Some((text, Instant::now()));
    }

    fn copy(&mut self, ctx: &egui::Context, c: Rgb) {
        self.copy_text(ctx, format!("#{}", palettes::hex(c)));
    }

    fn copy_text(&mut self, ctx: &egui::Context, text: String) {
        ctx.copy_text(text.clone());
        self.notify(tf("Copied {}", &[&text]));
    }

    /// Pinta `rect` recortado a las columnas; lo que toca su borde izquierdo sigue la curva
    /// de la ventana (las columnas quedan dentro del brillo del borde).
    fn fill(&self, painter: &egui::Painter, rect: Rect, fill: Color32) {
        let area = self.view;
        let r = rect.intersect(area);
        if !r.is_positive() {
            return;
        }
        let left = r.left() <= area.left() + 0.5;
        let corner = |touch: bool| if left && touch { self.radius } else { 0 };
        let corners = egui::CornerRadius {
            nw: corner(r.top() <= area.top() + 0.5),
            sw: corner(r.bottom() >= area.bottom() - 0.5),
            ne: 0,
            se: 0,
        };
        painter.rect_filled(r, corners, fill);
    }

    /// Guarda lo cambiado cuando no hay un arrastre a medias.
    fn save_if_dirty(&mut self, ctx: &egui::Context) {
        if !self.dirty || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.dirty = false;
        // «Picked colors» lo escribe el cuentagotas, otro proceso: si cambió desde la última
        // lectura, manda el del archivo.
        if mtime() != self.stamp {
            self.store.picked = Palettes::load().picked;
        }
        if let Err(error) = self.store.save() {
            log::warn!("Could not save palettes: {error}");
        }
        self.stamp = mtime();
    }

    /// Relee el archivo si otro proceso (el cuentagotas) lo cambió.
    fn reload_if_changed(&mut self) {
        if self.checked.elapsed() < Duration::from_millis(500) || self.dirty {
            return;
        }
        self.checked = Instant::now();
        self.glass = Settings::load().glass();
        let stamp = mtime();
        if stamp == self.stamp || self.dragging.is_some() {
            return;
        }
        self.stamp = stamp;
        self.store = Palettes::load();
        if let Shown::Saved(i) = self.shown
            && i >= self.store.saved.len()
        {
            self.show(Shown::Picked);
        }
        let len = self.colors().len();
        if self.picker.as_ref().is_some_and(|p| p.column >= len) {
            self.picker = None;
        }
        if self.shades.is_some_and(|i| i >= len) {
            self.shades = None;
        }
    }

    /// Lanza el cuentagotas en otro proceso; el color vuelve por su salida estándar.
    fn eyedropper(&mut self, ctx: &egui::Context, target: Option<usize>) {
        if self.eyedropper.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let (binary, ctx) = (self.binary.clone(), ctx.clone());
        std::thread::spawn(move || {
            use std::io::BufRead as _;
            // Como hijo de la instancia (`CHILD` en la app): corre aquí y no por el socket.
            let child = std::process::Command::new(binary)
                .arg("color")
                .env("COLORSHOT_CHILD", "1")
                .stdout(std::process::Stdio::piped())
                .spawn();
            let mut chosen = None;
            if let Ok(mut child) = child {
                // Una línea por color: `+#RRGGBB` con clic derecho, `#RRGGBB` al elegir.
                if let Some(stdout) = child.stdout.take() {
                    for line in std::io::BufReader::new(stdout)
                        .lines()
                        .map_while(Result::ok)
                    {
                        match line.strip_prefix('+') {
                            Some(kept) => {
                                if let Some(rgb) = palettes::parse_hex(kept) {
                                    let _ = sender.send(Pick::Keep(rgb));
                                    ctx.request_repaint();
                                }
                            }
                            None => chosen = palettes::parse_hex(&line).or(chosen),
                        }
                    }
                }
                let _ = child.wait();
            }
            let _ = sender.send(Pick::Done(chosen));
            ctx.request_repaint();
        });
        self.eyedropper = Some((target, receiver));
    }

    fn poll_eyedropper(&mut self) {
        let Some((target, receiver)) = &self.eyedropper else {
            return;
        };
        let target = *target;
        let mut done = None;
        let mut kept = Vec::new();
        while let Ok(pick) = receiver.try_recv() {
            match pick {
                Pick::Keep(rgb) => kept.push(rgb),
                Pick::Done(rgb) => done = Some(rgb),
            }
        }
        // Los del clic derecho se suman a la paleta abierta; en «Picked colors» ya están en el
        // archivo y llegan al releerlo.
        if !kept.is_empty() {
            if self.shown == Shown::Picked {
                self.checked = Instant::now() - Duration::from_secs(1);
                self.reload_if_changed();
            } else if let Some(colors) = self.colors_mut() {
                for rgb in kept {
                    if colors.len() < COLORS_MAX {
                        colors.push(rgb);
                    }
                }
                self.scroll = f32::MAX;
            }
        }
        let Some(rgb) = done else {
            return;
        };
        self.eyedropper = None;
        // El cuentagotas ya lo añadió a «Picked colors» en el archivo.
        self.checked = Instant::now() - Duration::from_secs(1);
        self.reload_if_changed();
        let (Some(rgb), Some(i)) = (rgb, target) else {
            return;
        };
        if let Some(c) = self.colors_mut().and_then(|colors| colors.get_mut(i)) {
            *c = rgb;
        }
        if let Some(picker) = self.picker.as_mut().filter(|p| p.column == i) {
            picker.set(rgb);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.reload_if_changed();
        self.poll_eyedropper();
        let g = self.gnome;
        // Sin barra de título: los colores llegan arriba y `round_window` redondea las esquinas.
        let r = ctx.content_rect();
        let side = Rect::from_min_max(pos2(r.right() - SIDEBAR, r.top()), r.max);
        ui.painter()
            .rect_filled(r.with_max_x(side.left()), 0., g.window);
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        self.frost(&ctx, side, maximized);
        // Las columnas no pisan el brillo del borde (2 px): sin esto asoman por fuera de él.
        let inset = if maximized { 0. } else { 2. };
        let area = Rect::from_min_max(
            r.min + Vec2::splat(inset),
            pos2(side.left(), r.bottom() - inset),
        );
        self.view = area;
        self.radius = if maximized { 0 } else { frame::RADIUS - 2 };
        self.columns(ui, area);
        self.library(ui, side);
        self.picker_ui(&ctx);
        self.notice_toast(&ctx, area.left_bottom() + vec2(16., -16.));
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.picker = None;
            self.shades = None;
            self.renaming = None;
        }
        self.save_if_dirty(&ctx);
        frame::finish(ui, &g, true);
        ctx.request_repaint_after(Duration::from_millis(600));
    }

    /// Con «Frosted glass» y un compositor que desenfoca, la barra lateral se vuelve
    /// translúcida y lo de detrás se desenfoca; si no, su color de siempre.
    fn frost(&mut self, ctx: &egui::Context, side: Rect, maximized: bool) {
        let supported = self.blur.as_mut().is_some_and(|b| b.supported());
        let glass = self.glass.filter(|_| supported);
        self.side_fill = match glass {
            Some(_) => self.gnome.frosted(glass).window,
            None => self.gnome.header,
        };
        let Some(blur) = &mut self.blur else {
            return;
        };
        // Se alarga bajo las columnas, que son opacas, para que su curva izquierda no se vea.
        let zoom = ctx.zoom_factor();
        let rect = side.with_min_x(side.left() - frame::RADIUS as f32);
        let rect = Rect::from_min_max(
            (rect.min.to_vec2() * zoom).to_pos2(),
            (rect.max.to_vec2() * zoom).to_pos2(),
        );
        let radius = if maximized {
            0.
        } else {
            frame::RADIUS as f32 * zoom
        };
        blur.set(glass.map(|_| (rect, radius)));
    }

    /// Barra lateral: arriba los botones de la ventana, el cuentagotas y «+»; debajo,
    /// «Picked colors» y las paletas guardadas, cada una con su tira de colores.
    fn library(&mut self, ui: &mut Ui, side: Rect) {
        let g = self.gnome;
        ui.painter().rect_filled(side, 0., self.side_fill);
        ui.painter().line_segment(
            [side.left_top(), side.left_bottom()],
            Stroke::new(1., g.line),
        );
        // La cabecera hace de barra de título: arrastra la ventana.
        let header = Rect::from_min_size(side.min, vec2(side.width(), HEADER));
        frame::drag_area(ui, header, true);
        let buttons = frame::window_buttons(ui, &g, side.right() - 12., header.center().y, true);
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(side.left() + 16., header.top()),
                    pos2(buttons - 10., header.bottom()),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.label(egui::RichText::new(t("Library")).size(15.).color(g.text));
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            if bar_button(ui, &g, ADD)
                .on_hover_text(t("New palette"))
                .clicked()
            {
                let palette = Palette {
                    name: self.store.next_name(t("Palette")),
                    colors: starter(),
                };
                self.store.saved.push(palette);
                self.dirty = true;
                self.show(Shown::Saved(self.store.saved.len() - 1));
            }
            if bar_button(ui, &g, EYEDROPPER)
                .on_hover_text(t("Pick Color"))
                .clicked()
            {
                self.eyedropper(ui.ctx(), None);
            }
        });
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(side.left(), header.bottom()),
                    side.max,
                ))
                .layout(Layout::top_down(Align::Min)),
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 4,
                        bottom: 16,
                    })
                    .show(ui, |ui| {
                        self.entry(ui, Shown::Picked);
                        // «Picked colors» va aparte de las paletas guardadas.
                        ui.add_space(10.);
                        let (line, _) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 1.), Sense::hover());
                        ui.painter().rect_filled(line, 0., g.line);
                        ui.add_space(16.);
                        for i in 0..self.store.saved.len() {
                            self.entry(ui, Shown::Saved(i));
                            ui.add_space(14.);
                        }
                    });
            });
    }

    /// Entrada de la biblioteca: la tira de colores y, debajo, el nombre (un clic lo edita) y,
    /// en la elegida, sus acciones.
    fn entry(&mut self, ui: &mut Ui, shown: Shown) {
        let g = self.gnome;
        let selected = shown == self.shown;
        let (name, colors) = match shown {
            Shown::Picked => (t("Picked colors").to_owned(), self.store.picked.clone()),
            Shown::Saved(i) => match self.store.saved.get(i) {
                Some(p) => (p.name.clone(), p.colors.clone()),
                None => return,
            },
        };
        if strip(ui, &g, &colors, selected).clicked() && !selected {
            self.show(shown);
        }
        ui.add_space(6.);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 24.),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 2.;
                // Las acciones, a la derecha, solo en la paleta elegida.
                if selected {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.entry_actions(ui, shown);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            self.entry_name(ui, shown, &name, selected);
                        });
                    });
                } else {
                    self.entry_name(ui, shown, &name, selected);
                }
            },
        );
    }

    /// Nombre de la entrada: en las guardadas, un clic lo vuelve un campo para renombrarla.
    fn entry_name(&mut self, ui: &mut Ui, shown: Shown, name: &str, selected: bool) {
        let g = self.gnome;
        let ink = if selected { g.text } else { g.dim };
        let Shown::Saved(i) = shown else {
            ui.label(egui::RichText::new(name).size(13.).color(ink));
            return;
        };
        if self.renaming == Some(i)
            && let Some(palette) = self.store.saved.get_mut(i)
        {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut palette.name)
                    .font(FontId::proportional(13.))
                    .margin(egui::Margin::symmetric(4, 2))
                    .desired_width(ui.available_width()),
            );
            if std::mem::take(&mut self.focus_name) {
                edit.request_focus();
            }
            if edit.changed() {
                self.dirty = true;
            }
            if edit.lost_focus() {
                self.renaming = None;
                if palette.name.trim().is_empty() {
                    palette.name.clear();
                    let name = self.store.next_name(t("Palette"));
                    if let Some(palette) = self.store.saved.get_mut(i) {
                        palette.name = name;
                    }
                    self.dirty = true;
                }
            }
            return;
        }
        let label = ui
            .add(
                egui::Label::new(egui::RichText::new(name).size(13.).color(ink))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::Text)
            .on_hover_text(t("Click to rename"));
        if label.clicked() {
            if !selected {
                self.show(shown);
            }
            self.renaming = Some(i);
            self.focus_name = true;
        }
    }

    /// Acciones de la paleta elegida: guardar «Picked colors» como paleta, o duplicar y borrar.
    fn entry_actions(&mut self, ui: &mut Ui, shown: Shown) {
        let g = self.gnome;
        match shown {
            Shown::Picked => {
                if !self.store.picked.is_empty()
                    && small_button(ui, &g, SAVE, None)
                        .on_hover_text(t("Save as palette"))
                        .clicked()
                {
                    let palette = Palette {
                        name: self.store.next_name(t("Palette")),
                        colors: self.store.picked.clone(),
                    };
                    self.store.saved.push(palette);
                    self.dirty = true;
                    self.show(Shown::Saved(self.store.saved.len() - 1));
                }
            }
            Shown::Saved(i) => {
                // Borrar pide un segundo clic: el icono se pone rojo.
                let (warn, tip) = if self.confirm_delete {
                    (Some(RED), t("Delete palette?"))
                } else {
                    (None, t("Delete palette"))
                };
                if small_button(ui, &g, DELETE, warn)
                    .on_hover_text(tip)
                    .clicked()
                {
                    if self.confirm_delete {
                        self.store.saved.remove(i);
                        self.dirty = true;
                        let next = if self.store.saved.is_empty() {
                            Shown::Picked
                        } else {
                            Shown::Saved(i.min(self.store.saved.len() - 1))
                        };
                        self.show(next);
                        return;
                    }
                    self.confirm_delete = true;
                }
                if small_button(ui, &g, DUPLICATE, None)
                    .on_hover_text(t("Duplicate"))
                    .clicked()
                    && let Some(palette) = self.store.saved.get(i)
                {
                    let copy = Palette {
                        name: self.store.next_name(&palette.name),
                        colors: palette.colors.clone(),
                    };
                    self.store.saved.insert(i + 1, copy);
                    self.dirty = true;
                    self.show(Shown::Saved(i + 1));
                }
            }
        }
    }

    fn columns(&mut self, ui: &mut Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let colors = self.colors().to_vec();
        if colors.is_empty() {
            self.empty(ui, area);
            return;
        }
        let n = colors.len();
        // Todo lo de las columnas se recorta a su zona, también lo desplazado fuera.
        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(area));
        ui.set_clip_rect(area);
        let width = (area.width() / n as f32).max(MIN_COLUMN);
        let max_scroll = (width * n as f32 - area.width()).max(0.);
        let pointer = ctx.pointer_hover_pos().filter(|p| area.contains(*p));
        if pointer.is_some() && self.picker.is_none() {
            // La rueda vertical también desplaza: no hay nada que mover en vertical.
            let delta = ctx.input(|i| i.smooth_scroll_delta);
            self.scroll -= delta.x + delta.y;
        }
        self.scroll = self.scroll.clamp(0., max_scroll);
        let scroll = self.scroll;
        let column = |i: usize| {
            let x = |i: usize| (area.left() + i as f32 * width - scroll).round();
            Rect::from_min_max(pos2(x(i), area.top()), pos2(x(i + 1), area.bottom()))
        };
        // Arrastre con «Drag»: la columna sigue al puntero y se reordena al cruzar otra; cerca
        // de un costado, las columnas se desplazan.
        if let Some(from) = self.dragging {
            if ctx.input(|i| i.pointer.primary_down()) {
                if let Some(p) = ctx.pointer_latest_pos() {
                    if p.x < area.left() + EDGE {
                        self.scroll = (self.scroll - 10.).max(0.);
                    } else if p.x > area.right() - EDGE {
                        self.scroll = (self.scroll + 10.).min(max_scroll);
                    }
                    let to = ((p.x - area.left() + self.scroll) / width)
                        .floor()
                        .clamp(0., n as f32 - 1.) as usize;
                    if to != from
                        && let Some(colors) = self.colors_mut()
                    {
                        let c = colors.remove(from);
                        colors.insert(to, c);
                        self.dragging = Some(to);
                    }
                }
                ctx.set_cursor_icon(CursorIcon::Grabbing);
            } else {
                self.dragging = None;
            }
        }
        let colors = self.colors().to_vec();
        // El «+» de un costado manda sobre las acciones de la columna que está debajo.
        let can_add = n < COLORS_MAX && self.dragging.is_none() && self.shades.is_none();
        let side = pointer.filter(|_| can_add).and_then(|p| {
            if p.x < area.left() + EDGE {
                Some(false)
            } else if p.x > area.right() - EDGE {
                Some(true)
            } else {
                None
            }
        });
        let painter = ui.painter().clone();
        for (i, &c) in colors.iter().enumerate() {
            let rect = column(i);
            if rect.right() < area.left() || rect.left() > area.right() {
                continue;
            }
            self.fill(&painter, rect, color(c));
            if self.shades == Some(i) {
                self.shades_ui(ui, i, rect, c);
                continue;
            }
            let ink = ink(c);
            let background = ui
                .interact(rect, Id::new(("palette-column", i)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            // El dato grande va en el formato elegido en el selector (HEX en la vista «Picker»).
            let main = match self.store.view {
                View::Picker => palettes::hex(c),
                view => view_text(c, view),
            };
            // Clic derecho en cualquier parte del color: copia ese dato.
            if background.secondary_clicked() {
                self.copy_text(&ctx, view_text(c, self.store.view));
            }
            // Dato grande y, debajo, el secundario con su menú; ambos se achican si no caben.
            let hex_center = pos2(rect.center().x, rect.bottom() - 74.);
            let open = self.picker.as_ref().is_some_and(|p| p.column == i);
            let room = rect.width() - 32.;
            let hex = pill(
                ui,
                Id::new(("palette-hex", i)),
                hex_center,
                &main,
                shrink(
                    ui,
                    &main,
                    FontId::new(
                        (rect.width() * 0.15).clamp(16., 30.),
                        egui::FontFamily::Name(theme::BOLD.into()),
                    ),
                    room,
                ),
                ink,
                open,
            )
            .on_hover_text(t("Right-click to copy"));
            if hex.secondary_clicked() {
                self.copy_text(&ctx, view_text(c, self.store.view));
            }
            let detail = secondary(c, self.store.secondary);
            let small = pill(
                ui,
                Id::new(("palette-secondary", i)),
                pos2(rect.center().x, rect.bottom() - 36.),
                &detail,
                shrink(ui, &detail, FontId::proportional(12.5), room),
                ink.gamma_multiply(0.75),
                false,
            )
            .on_hover_text(t("Right-click to copy"));
            if small.secondary_clicked() {
                self.copy_text(&ctx, secondary(c, self.store.secondary));
            }
            self.secondary_menu(&small, c);
            if let Some(picker) = self.picker.as_mut().filter(|p| p.column == i) {
                picker.anchor = rect;
                picker.area.min = hex.rect.center_top() - vec2(0., 12.);
            }
            let hovered = self.dragging == Some(i)
                || (self.dragging.is_none()
                    && side.is_none()
                    && pointer.is_some_and(|p| rect.contains(p)));
            if hovered {
                self.actions(ui, i, rect, c, ink);
            }
            // El HEX y el fondo de la columna abren (o cierran) el selector.
            if background.clicked() || hex.clicked() {
                self.picker = (!open).then(|| Picker::new(i, c, rect, hex.rect.center_top()));
                self.shades = None;
            }
        }
        if let Some(right) = side {
            let center = if right {
                pos2(area.right() - EDGE / 2., area.center().y)
            } else {
                pos2(area.left() + EDGE / 2., area.center().y)
            };
            let response = ui
                .interact(
                    Rect::from_center_size(center, Vec2::splat(BUTTON)),
                    Id::new(("palette-add", right)),
                    Sense::click(),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(if right {
                    t("Add lighter color")
                } else {
                    t("Add darker color")
                });
            let grow = if response.hovered() { 2. } else { 0. };
            painter.circle_filled(
                center + vec2(0., 1.5),
                16. + grow,
                Color32::from_black_alpha(50),
            );
            painter.circle_filled(center, 16. + grow, Color32::WHITE);
            paint_icon(ui, center, ADD, 18., Color32::from_gray(30));
            if response.clicked() {
                // Se ve el color nuevo: el desplazamiento va a ese extremo (se acota luego).
                self.scroll = if right { f32::MAX } else { 0. };
            }
            if response.clicked()
                && let Some(colors) = self.colors_mut()
            {
                if right {
                    let last = colors.last().copied().unwrap_or([128; 3]);
                    colors.push(palettes::lighter(last));
                } else {
                    let first = colors.first().copied().unwrap_or([128; 3]);
                    colors.insert(0, palettes::darker(first));
                    // El selector sigue sobre el mismo color, que se corrió a la derecha.
                    if let Some(picker) = &mut self.picker {
                        picker.column += 1;
                    }
                }
            }
        }
        if max_scroll > 0. {
            self.scrollbar(ui, area, max_scroll);
        }
    }

    /// Barra fina abajo cuando las columnas no caben; se arrastra.
    fn scrollbar(&mut self, ui: &mut Ui, area: Rect, max_scroll: f32) {
        let total = area.width() + max_scroll;
        let track = Rect::from_min_max(
            pos2(area.left() + 16., area.bottom() - 10.),
            pos2(area.right() - 16., area.bottom() - 4.),
        );
        let thumb_width = (track.width() * area.width() / total).max(32.);
        let x = track.left() + (track.width() - thumb_width) * self.scroll / max_scroll;
        let thumb = Rect::from_min_size(pos2(x, track.top()), vec2(thumb_width, track.height()));
        let response = ui.interact(
            thumb.expand2(vec2(0., 6.)),
            Id::new("palette-scrollbar"),
            Sense::drag(),
        );
        if response.dragged() {
            let per_point = max_scroll / (track.width() - thumb_width).max(1.);
            self.scroll = (self.scroll + response.drag_delta().x * per_point).clamp(0., max_scroll);
        }
        let strong = response.hovered() || response.dragged();
        let painter = ui.painter();
        painter.rect_filled(track, 3., Color32::from_black_alpha(40));
        painter.rect_filled(
            thumb,
            3.,
            Color32::from_black_alpha(if strong { 170 } else { 110 }),
        );
    }

    /// Paleta vacía: un botón para añadir un color o, en «Picked colors», el cuentagotas.
    fn empty(&mut self, ui: &mut Ui, area: Rect) {
        let g = self.gnome;
        self.fill(ui.painter(), area, g.view);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(
                    area.center(),
                    vec2(area.width(), 120.),
                ))
                .layout(Layout::top_down(Align::Center)),
        );
        if self.shown == Shown::Picked {
            child.label(
                egui::RichText::new(t("Colors you pick with the eyedropper appear here"))
                    .color(g.dim),
            );
            child.add_space(12.);
            if text_button(&mut child, &g, icon::EYEDROPPER, t("Pick Color"), true).clicked() {
                self.eyedropper(ui.ctx(), None);
            }
        } else if text_button(&mut child, &g, icon::PLUS, t("Add color"), true).clicked()
            && let Some(colors) = self.colors_mut()
        {
            colors.extend(starter());
        }
    }

    /// Acciones de la columna, apiladas en el centro.
    fn actions(&mut self, ui: &mut Ui, i: usize, rect: Rect, c: Rgb, ink: Color32) {
        let ctx = ui.ctx().clone();
        let items = [
            (REMOVE, t("Remove color")),
            (SHADES, t("View shades")),
            (DRAG, t("Drag")),
            (DUPLICATE_COLOR, t("Duplicate color")),
            (COPY, t("Copy HEX")),
        ];
        let top = rect.center().y - items.len() as f32 * (BUTTON + 6.) / 2.;
        for (k, (glyph, tip)) in items.into_iter().enumerate() {
            let center = pos2(
                rect.center().x,
                top + k as f32 * (BUTTON + 6.) + BUTTON / 2.,
            );
            let drag = k == 2;
            let sense = if drag { Sense::drag() } else { Sense::click() };
            let button = Rect::from_center_size(center, Vec2::splat(BUTTON));
            let response = ui.interact(button, Id::new(("palette-action", i, k)), sense);
            let response = if drag {
                response.on_hover_cursor(CursorIcon::Grab)
            } else {
                response.on_hover_cursor(CursorIcon::PointingHand)
            };
            let active = response.hovered() || (drag && self.dragging == Some(i));
            if active {
                ui.painter()
                    .rect_filled(button, 9., ink.gamma_multiply(0.14));
            }
            paint_icon(ui, center, glyph, 20., ink);
            let response = response.on_hover_text(tip);
            match k {
                0 if response.clicked() => {
                    if let Some(colors) = self.colors_mut() {
                        colors.remove(i);
                    }
                    self.picker = None;
                }
                1 if response.clicked() => {
                    self.shades = Some(i);
                    self.picker = None;
                }
                2 if response.drag_started() => self.dragging = Some(i),
                3 if response.clicked() => self.duplicate(i, c),
                4 if response.clicked() => self.copy(&ctx, c),
                _ => {}
            }
        }
    }

    /// Copia del color justo a su derecha, si cabe.
    fn duplicate(&mut self, i: usize, c: Rgb) {
        if self.colors().len() >= COLORS_MAX {
            return;
        }
        if let Some(colors) = self.colors_mut() {
            colors.insert(i + 1, c);
        }
        // El selector sigue sobre su color aunque se corra.
        if let Some(picker) = self.picker.as_mut().filter(|p| p.column > i) {
            picker.column += 1;
        }
    }

    /// «View shades»: la gama del color en franjas; un clic elige una.
    fn shades_ui(&mut self, ui: &mut Ui, i: usize, rect: Rect, c: Rgb) {
        let shades = palettes::shades(c);
        let step = rect.height() / shades.len() as f32;
        // La franja más parecida lleva el punto.
        let closest = shades
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| {
                s.iter()
                    .zip(c)
                    .map(|(a, b)| (*a as i32 - b as i32).pow(2))
                    .sum::<i32>()
            })
            .map_or(0, |(k, _)| k);
        let painter = ui.painter().clone();
        let mut chosen = None;
        for (k, &shade) in shades.iter().enumerate() {
            let band = Rect::from_min_size(
                pos2(rect.left(), rect.top() + k as f32 * step),
                vec2(rect.width(), step + 0.5),
            );
            self.fill(&painter, band, color(shade));
            let response = ui
                .interact(band, Id::new(("palette-shade", i, k)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            if k == closest {
                painter.circle_filled(band.center(), 3.5, ink(shade));
            }
            if response.hovered() && step >= 14. {
                painter.text(
                    band.left_center() + vec2(10., 0.),
                    Align2::LEFT_CENTER,
                    palettes::hex(shade),
                    FontId::proportional(11.),
                    ink(shade),
                );
            }
            if response.clicked() {
                chosen = Some(shade);
            }
        }
        let outside = ui.input(|input| {
            input.pointer.primary_clicked()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|p| !rect.contains(p))
        });
        if let Some(shade) = chosen {
            if let Some(c) = self.colors_mut().and_then(|colors| colors.get_mut(i)) {
                *c = shade;
            }
            self.shades = None;
        } else if outside {
            self.shades = None;
        }
    }

    /// Menú del dato secundario: copiarlo y, debajo, el dato de todas las columnas.
    fn secondary_menu(&mut self, response: &egui::Response, c: Rgb) {
        let g = self.gnome;
        let value = secondary(c, self.store.secondary);
        let mut copy = false;
        egui::Popup::menu(response)
            .gap(6.)
            .frame(popup_frame(&g))
            .show(|ui| {
                ui.set_min_width(140.);
                if ui
                    .button(mixed(
                        &[(icon::COPY, true), ("  ", false), (t("Copy"), false)],
                        14.,
                    ))
                    .on_hover_text(&value)
                    .clicked()
                {
                    copy = true;
                    ui.close();
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(t("Secondary info"))
                        .size(12.)
                        .color(g.dim),
                );
                for kind in Secondary::ALL {
                    if ui
                        .selectable_label(self.store.secondary == kind, secondary_name(kind))
                        .clicked()
                    {
                        self.store.secondary = kind;
                        self.dirty = true;
                        ui.close();
                    }
                }
            });
        if copy {
            self.copy_text(&response.ctx, value);
        }
    }

    /// Selector de color sobre su columna: el cuadro de saturación y brillo con el tono y el
    /// HEX, o los deslizadores de la vista elegida abajo a la izquierda; a la derecha, el
    /// cuentagotas y copiar.
    fn picker_ui(&mut self, ctx: &egui::Context) {
        let g = self.gnome;
        let view = self.store.view;
        let hue = self
            .hue
            .get_or_insert_with(|| {
                let pixels: Vec<u8> = (0..360)
                    .flat_map(|h| {
                        let [r, g, b] = palettes::from_hsb([h as f32, 1., 1.]);
                        [r, g, b, 255]
                    })
                    .collect();
                ctx.load_texture(
                    "palette-hue",
                    egui::ColorImage::from_rgba_unmultiplied([360, 1], &pixels),
                    egui::TextureOptions::LINEAR,
                )
            })
            .clone();
        let column = match &self.picker {
            Some(picker) => picker.column,
            None => return,
        };
        let Some(current) = self.colors().get(column).copied() else {
            self.picker = None;
            return;
        };
        let Some(picker) = &mut self.picker else {
            return;
        };
        if view == View::Picker {
            if palettes::from_hsb(picker.hsb) != current {
                picker.set(current);
            }
            picker.square(ctx);
        } else {
            picker.tracks(ctx, view, current);
        }
        // Lo que cambió en este cuadro: el color nuevo y, si vino de los deslizadores, sus
        // canales (que mandan sobre el color).
        let mut changed: Option<Rgb> = None;
        let mut slid = false;
        let mut new_view = None;
        let mut copy = false;
        let mut pick = false;
        let shown = egui::Area::new(Id::new("palette-picker"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(picker.area.min)
            .constrain(true)
            .show(ctx, |ui| {
                popup_frame(&g)
                    .inner_margin(14)
                    .corner_radius(16)
                    .show(ui, |ui| {
                        ui.set_width(PICKER);
                        ui.spacing_mut().item_spacing.y = 12.;
                        if view == View::Picker {
                            changed = picker_square(ui, &g, picker, &hue);
                        } else if let Some(c) = picker_sliders(ui, &g, picker, view) {
                            changed = Some(c);
                            slid = true;
                        }
                        let (line, _) =
                            ui.allocate_exact_size(vec2(PICKER + 28., 1.), Sense::hover());
                        ui.painter()
                            .rect_filled(line.translate(vec2(-14., 0.)), 0., g.line);
                        ui.horizontal(|ui| {
                            // La vista, como el «Picker ⌄» de abajo a la izquierda.
                            let button = ui.add(
                                egui::Button::new(mixed(
                                    &[
                                        (view_name(view), false),
                                        ("  ", false),
                                        (icon::CARET_DOWN, true),
                                    ],
                                    14.,
                                ))
                                .frame(false),
                            );
                            egui::Popup::menu(&button)
                                .gap(6.)
                                .frame(popup_frame(&g))
                                .show(|ui| {
                                    ui.set_min_width(110.);
                                    for kind in View::ALL {
                                        if ui
                                            .selectable_label(view == kind, view_name(kind))
                                            .clicked()
                                        {
                                            new_view = Some(kind);
                                            ui.close();
                                        }
                                    }
                                });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                copy = bar_button(ui, &g, COPY)
                                    .on_hover_text(view_text(current, view))
                                    .clicked();
                                pick = bar_button(ui, &g, EYEDROPPER)
                                    .on_hover_text(t("Pick Color"))
                                    .clicked();
                            });
                        });
                    });
            });
        let area = shown.response.rect;
        if let Some(rgb) = changed {
            if slid {
                // Los canales siguen como se dejaron; el resto se pone al día.
                let channels = picker.channels;
                let clipped = picker.clipped;
                picker.set(rgb);
                picker.channels = channels;
                picker.clipped = clipped;
            }
            if let Some(c) = self.colors_mut().and_then(|colors| colors.get_mut(column)) {
                *c = rgb;
            }
        }
        if let Some(kind) = new_view {
            self.store.view = kind;
            self.dirty = true;
        }
        let Some(picker) = &mut self.picker else {
            return;
        };
        // Cierra al pulsar fuera del selector y de su columna (la columna lo cierra con su clic).
        let opened = std::mem::replace(&mut picker.opened, false);
        let anchor = picker.anchor;
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !area.contains(p) && !anchor.contains(p))
        });
        let rgb = changed.unwrap_or(current);
        if copy {
            self.copy_text(ctx, view_text(rgb, self.store.view));
        }
        if pick {
            self.eyedropper(ctx, Some(column));
        }
        if outside && !opened {
            self.picker = None;
        }
    }

    /// Aviso («Copied #…») abajo a la izquierda; se desvanece al final.
    fn notice_toast(&mut self, ctx: &egui::Context, left_bottom: Pos2) {
        let Some((text, since)) = &self.notice else {
            return;
        };
        let left = 1.8 - since.elapsed().as_secs_f32();
        if left <= 0. {
            self.notice = None;
            return;
        }
        let g = self.gnome;
        egui::Area::new(Id::new("palette-notice"))
            .order(egui::Order::Tooltip)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(left_bottom)
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity((left / 0.3).clamp(0., 1.));
                popup_frame(&g)
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| ui.label(egui::RichText::new(text).size(13.)));
            });
        ctx.request_repaint();
    }
}

/// Paleta nueva: cinco tonos de un color al azar, de oscuro a claro.
fn starter() -> Vec<Rgb> {
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let base = palettes::from_hsl([(seed % 360) as f32, 0.45, 0.55]);
    let dark = palettes::darker(base);
    let light = palettes::lighter(base);
    vec![
        palettes::darker(dark),
        dark,
        base,
        light,
        palettes::lighter(light),
    ]
}

impl Picker {
    /// Cuadro de saturación y brillo del tono actual.
    fn square(&mut self, ctx: &egui::Context) {
        if (self.sv_hue - self.hsb[0]).abs() <= 0.01 && self.sv.is_some() {
            return;
        }
        self.sv_hue = self.hsb[0];
        let size = 48;
        let pixels: Vec<u8> = (0..size * size)
            .flat_map(|k| {
                let (x, y) = (k % size, k / size);
                let s = x as f32 / (size - 1) as f32;
                let v = 1. - y as f32 / (size - 1) as f32;
                let [r, g, b] = palettes::from_hsb([self.hsb[0], s, v]);
                [r, g, b, 255]
            })
            .collect();
        let image = egui::ColorImage::from_rgba_unmultiplied([size, size], &pixels);
        match &mut self.sv {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.sv = Some(ctx.load_texture("palette-sv", image, egui::TextureOptions::LINEAR))
            }
        }
    }

    /// Canales de `view` (del color si no los hay) y el degradado de cada deslizador.
    fn tracks(&mut self, ctx: &egui::Context, view: View, c: Rgb) {
        // Valen mientras den el color de la columna: si cambió por otro lado, se rehacen.
        let channels = match self.channels {
            Some((v, channels)) if v == view && fit(v, channels).0 == c => channels,
            _ => {
                let mut channels = palettes::channels(c, view);
                // Un gris no tiene tono: se queda el del cuadro.
                if matches!(view, View::Hsb | View::Hsl) && channels[1] < 0.05 {
                    channels[0] = self.hsb[0];
                }
                self.clipped = false;
                channels
            }
        };
        self.channels = Some((view, channels));
        for k in 0..sliders(view).len() {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([96, 1], &track_pixels(view, channels, k));
            match self.tracks.get_mut(k) {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => self.tracks.push(ctx.load_texture(
                    format!("palette-track-{k}"),
                    image,
                    egui::TextureOptions::LINEAR,
                )),
            }
        }
    }
}

/// Vista «Picker»: cuadro, tono y HEX. Devuelve el color si cambió.
fn picker_square(
    ui: &mut Ui,
    g: &Gnome,
    picker: &mut Picker,
    hue: &egui::TextureHandle,
) -> Option<Rgb> {
    let mut changed = None;
    let [h, s, v] = picker.hsb;
    let (square, response) = ui.allocate_exact_size(vec2(PICKER, 160.), Sense::click_and_drag());
    if let Some(texture) = &picker.sv {
        egui::Image::from_texture((texture.id(), square.size()))
            .corner_radius(8)
            .paint_at(ui, square);
    }
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let s = ((p.x - square.left()) / square.width()).clamp(0., 1.);
        let v = 1. - ((p.y - square.top()) / square.height()).clamp(0., 1.);
        picker.hsb = [h, s, v];
        changed = Some(palettes::from_hsb(picker.hsb));
    }
    let knob = pos2(
        square.left() + s * square.width(),
        square.top() + (1. - v) * square.height(),
    );
    handle(ui, knob, color(palettes::from_hsb([h, s, v])));
    let (track, response) = ui.allocate_exact_size(vec2(PICKER, 12.), Sense::click_and_drag());
    egui::Image::from_texture((hue.id(), track.size()))
        .corner_radius(6)
        .paint_at(ui, track);
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let h = ((p.x - track.left()) / track.width()).clamp(0., 1.) * 359.9;
        picker.hsb = [h, s.max(0.05), v.max(0.05)];
        changed = Some(palettes::from_hsb(picker.hsb));
    }
    handle(
        ui,
        pos2(track.left() + h / 360. * track.width(), track.center().y),
        color(palettes::from_hsb([h, 1., 1.])),
    );
    // HEX con la muestra a la derecha.
    egui::Frame::new()
        .stroke(Stroke::new(1., g.line))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut picker.text)
                        .frame(egui::Frame::NONE)
                        .font(FontId::proportional(15.))
                        .desired_width(PICKER - 64.),
                );
                if edit.changed()
                    && let Some(rgb) = palettes::parse_hex(&picker.text)
                {
                    picker.hsb = palettes::to_hsb(rgb);
                    changed = Some(rgb);
                }
                let (swatch, _) = ui.allocate_exact_size(Vec2::splat(24.), Sense::hover());
                let rgb = palettes::from_hsb(picker.hsb);
                ui.painter().rect_filled(swatch, 5., color(rgb));
                ui.painter().rect_stroke(
                    swatch,
                    5.,
                    Stroke::new(1., g.line),
                    egui::StrokeKind::Inside,
                );
            });
        });
    // Lo escrito en el campo se queda como está; lo demás lo rehace.
    if let Some(rgb) = changed
        && palettes::parse_hex(&picker.text) != Some(rgb)
    {
        picker.text = format!("#{}", palettes::hex(rgb));
    }
    changed
}

/// Vistas con deslizadores: nombre y valor (se arrastra o se escribe) y, debajo, el
/// degradado con su tirador. Devuelve el color si cambió.
fn picker_sliders(ui: &mut Ui, g: &Gnome, picker: &mut Picker, view: View) -> Option<Rgb> {
    let (_, mut channels) = picker.channels?;
    let mut moved = false;
    ui.spacing_mut().item_spacing.y = 6.;
    for (k, &(name, min, max, decimals)) in sliders(view).iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t(name)).size(13.5).color(g.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let visuals = &mut ui.visuals_mut().widgets;
                visuals.inactive.weak_bg_fill = Color32::TRANSPARENT;
                visuals.inactive.bg_fill = Color32::TRANSPARENT;
                let value = ui.add(
                    egui::DragValue::new(&mut channels[k])
                        .range(min..=max)
                        .fixed_decimals(decimals)
                        .speed((max - min) / 300.),
                );
                moved |= value.changed();
            });
        });
        let (rect, response) = ui.allocate_exact_size(vec2(PICKER, 22.), Sense::click_and_drag());
        let track = Rect::from_center_size(rect.center(), vec2(PICKER, 10.));
        // Debajo, el fondo de los controles: se ve donde el color cae fuera de sRGB.
        ui.painter().rect_filled(track, 5., g.field);
        if let Some(texture) = picker.tracks.get(k) {
            egui::Image::from_texture((texture.id(), track.size()))
                .corner_radius(5)
                .paint_at(ui, track);
        }
        if (response.dragged() || response.clicked())
            && let Some(p) = response.interact_pointer_pos()
        {
            let f = ((p.x - track.left()) / track.width()).clamp(0., 1.);
            channels[k] = min + (max - min) * f;
            moved = true;
        }
        let f = ((channels[k] - min) / (max - min)).clamp(0., 1.);
        let (knob_color, _) = fit(view, channels);
        handle(
            ui,
            pos2(track.left() + f * track.width(), track.center().y),
            color(knob_color),
        );
        ui.add_space(4.);
    }
    if view == View::Oklch || view == View::Lab {
        let note = if picker.clipped {
            t("Outside sRGB: showing the closest color")
        } else {
            ""
        };
        ui.label(egui::RichText::new(note).size(12.).color(g.dim));
    }
    if !moved {
        return None;
    }
    let (rgb, clipped) = fit(view, channels);
    picker.channels = Some((view, channels));
    picker.clipped = clipped;
    Some(rgb)
}

fn popup_frame(g: &Gnome) -> egui::Frame {
    egui::Frame::new()
        .fill(g.popup)
        .corner_radius(10)
        .stroke(Stroke::new(1., g.line))
        .inner_margin(6)
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(60),
        })
}

/// Tirador redondo del selector: borde blanco con el color dentro.
fn handle(ui: &Ui, center: Pos2, fill: Color32) {
    let painter = ui.painter();
    painter.circle_filled(center + vec2(0., 1.), 10., Color32::from_black_alpha(50));
    painter.circle_filled(center, 9.5, Color32::WHITE);
    painter.circle_filled(center, 6.5, fill);
}

/// Texto centrado en `center` que se resalta al pasar el ratón y responde al clic.
/// Con `active`, el resalte queda fijo (el selector está abierto sobre él).
fn pill(
    ui: &mut Ui,
    id: Id,
    center: Pos2,
    text: &str,
    font: FontId,
    ink: Color32,
    active: bool,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let rect = Rect::from_center_size(center, galley.size() + vec2(20., 10.));
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if active || response.hovered() {
        ui.painter()
            .rect_filled(rect, 10., ink.gamma_multiply(0.14));
    }
    let at = (rect.center() - galley.size() / 2.).round();
    ui.painter().galley(at, galley, ink);
    response
}

/// `font` reducida lo justo para que `text` quepa en `width` (sin bajar de 9 px).
fn shrink(ui: &Ui, text: &str, font: FontId, width: f32) -> FontId {
    let measured = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    if measured <= width || measured <= 0. {
        return font;
    }
    // El ancho crece casi en proporción al tamaño; se redondea hacia abajo para no pasarse.
    let size = (font.size * width / measured).floor().max(9.);
    FontId::new(size, font.family)
}

/// Fuente de los iconos. La de la interfaz (Adwaita Sans) trae glifos propios en el área
/// privada de Unicode, donde están los de Phosphor: con `proportional` taparía algunos.
fn glyph_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("phosphor".into()))
}

/// Texto con iconos: cada parte con `true` va en la fuente de los iconos.
fn mixed(parts: &[(&str, bool)], size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (text, glyph) in parts {
        let font = if *glyph {
            glyph_font(size)
        } else {
            FontId::proportional(size)
        };
        // El color lo pone quien pinta el texto.
        job.append(
            text,
            0.,
            egui::TextFormat::simple(font, Color32::PLACEHOLDER),
        );
    }
    job
}

/// Icono de `size` puntos centrado en `center`, teñido de `ink`.
fn paint_icon(ui: &Ui, center: Pos2, (name, glyph): Icon, size: f32, ink: Color32) {
    match crate::icons::texture(ui.ctx(), name) {
        Some(texture) => {
            let rect = Rect::from_center_size(center, Vec2::splat(size));
            let uv = Rect::from_min_max(Pos2::ZERO, pos2(1., 1.));
            ui.painter().image(texture.id(), rect, uv, ink);
        }
        None => {
            ui.painter()
                .text(center, Align2::CENTER_CENTER, glyph, glyph_font(size), ink);
        }
    }
}

/// Botón cuadrado con un glifo, como los de la barra de Preferencias.
fn bar_button(ui: &mut Ui, g: &Gnome, glyph: Icon) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 8., g.field_hover);
    }
    paint_icon(ui, rect.center(), glyph, 17., g.text);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Botón con glifo y texto sobre el fondo de los controles.
fn text_button(ui: &mut Ui, g: &Gnome, glyph: &str, text: &str, enabled: bool) -> egui::Response {
    let galley = ui
        .painter()
        .layout_job(mixed(&[(glyph, true), ("  ", false), (text, false)], 13.));
    let size = galley.size() + vec2(24., 0.);
    let (rect, response) = ui.allocate_exact_size(vec2(size.x, 30.), Sense::click());
    let fill = if enabled && response.hovered() {
        g.field_hover
    } else {
        g.field
    };
    ui.painter().rect_filled(rect, 8., fill);
    let ink = if enabled { g.text } else { g.dim };
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, ink);
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        // Deshabilitado: no responde al clic.
        ui.interact(rect, response.id.with("off"), Sense::hover())
    }
}

/// Tira de colores de una entrada de la biblioteca.
fn strip(ui: &mut Ui, g: &Gnome, colors: &[Rgb], selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (bar, response) = ui.allocate_exact_size(vec2(width, 34.), Sense::click());
    let painter = ui.painter();
    if colors.is_empty() {
        painter.rect_filled(bar, 7., g.field);
    } else {
        let step = width / colors.len() as f32;
        let last = colors.len() - 1;
        for (k, &c) in colors.iter().enumerate() {
            let x0 = (bar.left() + k as f32 * step).round();
            let x1 = (bar.left() + (k + 1) as f32 * step).round();
            let corner = egui::CornerRadius {
                nw: if k == 0 { 7 } else { 0 },
                sw: if k == 0 { 7 } else { 0 },
                ne: if k == last { 7 } else { 0 },
                se: if k == last { 7 } else { 0 },
            };
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, bar.top()), pos2(x1, bar.bottom())),
                corner,
                color(c),
            );
        }
    }
    let outline = if selected {
        Stroke::new(2., g.accent)
    } else if response.hovered() {
        Stroke::new(1., g.dim)
    } else {
        Stroke::new(1., g.line)
    };
    painter.rect_stroke(bar.expand(1.), 8., outline, egui::StrokeKind::Outside);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Botón pequeño con un glifo para las acciones de una entrada; `warn` lo tiñe.
fn small_button(ui: &mut Ui, g: &Gnome, glyph: Icon, warn: Option<Color32>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 6., g.field_hover);
    }
    let ink = warn.unwrap_or(if response.hovered() { g.text } else { g.dim });
    paint_icon(ui, rect.center(), glyph, 15., ink);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_formats() {
        let c = [0x27, 0x27, 0x42];
        assert_eq!(secondary(c, Secondary::Rgb), "39, 39, 66");
        assert_eq!(secondary([255, 0, 0], Secondary::Hsl), "0°, 100%, 50%");
        assert_eq!(secondary([255, 0, 0], Secondary::Cmyk), "0, 100, 100, 0");
        assert_eq!(secondary([0, 0, 0], Secondary::Cmyk), "0, 0, 0, 100");
        assert_eq!(secondary([255, 255, 255], Secondary::Name), "White");
        assert_eq!(starter().len(), 5);
    }
}
