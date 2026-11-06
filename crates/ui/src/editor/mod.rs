//! Estado del editor y despacho de acciones y atajos.
mod canvas;
pub(crate) mod color;
pub mod edit;
mod info;
pub mod model;
pub(crate) mod prefs;
mod toolbar;
mod zone;
use egui::{Color32, Key, TextureHandle, Vec2};
use image::RgbaImage;
use model::{Document, Shape, Style};

/// El color en el formato elegido en «Color Format» del editor (lo usa el cuentagotas).
pub fn color_text(rgb: [u8; 3]) -> String {
    prefs::Prefs::load()
        .color_format
        .text(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
}
use std::time::{Duration, Instant};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Tool {
    Select,
    Arrow,
    Line,
    Rectangle,
    Oval,
    Freehand,
    Highlighter,
    Counter,
    Text,
    Blur,
    Spotlight,
    Crop,
}
/// Lo que se arrastra: una figura o la zona de recorte pendiente.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Shape(usize),
    Crop,
}
/// Arrastre en curso: tirador (`None` = mover entero), último punto y figuras de antes.
struct Grab {
    target: Target,
    handle: Option<usize>,
    last: egui::Pos2,
    before: Vec<Shape>,
}
pub enum EditorAction {
    Copy(RgbaImage),
    Save(RgbaImage),
    Pin(RgbaImage),
    /// Reconocer texto o QR en la imagen.
    Recognize(RgbaImage),
    Close,
}
/// Alto de la barra de herramientas, que en la ventana del editor es también la de título.
/// Separación del panel de la herramienta respecto al borde derecho y a la barra.
const OPTIONS_MARGIN: f32 = 12.;
pub const BAR: f32 = 52.;
/// Tamaño mínimo de la ventana del editor.
pub const MIN_WINDOW: egui::Vec2 = egui::vec2(800., 500.);

/// Ventana a la medida de la imagen al 100 % (con el margen de `fit` y la barra), entre
/// `MIN_WINDOW` y `max`.
pub fn window_size(image: egui::Vec2, max: egui::Vec2) -> egui::Vec2 {
    (image + egui::vec2(32., 32. + BAR))
        .min(max)
        .max(MIN_WINDOW)
        .round()
}
pub struct Editor {
    pub doc: Document,
    pub tool: Tool,
    pub style: Style,
    pub zoom: f32,
    pub pan: Vec2,
    texture: Option<TextureHandle>,
    texture_nearest: bool,
    pub hover_pixel: Option<(u32, u32)>,
    pub drawing: Option<Shape>,
    anchor: Option<egui::Pos2>,
    fitted: bool,
    notice: Option<(String, Instant)>,
    // Texto en edición: esquina en píxeles de imagen y contenido.
    text_edit: Option<(egui::Pos2, String)>,
    // Bloques del pixelado por zona, para no recalcularlos en cada cuadro.
    blur_cache: std::collections::HashMap<[u32; 4], Vec<(egui::Rect, Color32)>>,
    // Revisión de la imagen para la que valen la textura y la caché.
    revision: u64,
    /// Figura seleccionada: se mueve, se redimensiona, cambia de estilo o se borra.
    pub selected: Option<usize>,
    /// Zona de recorte ajustable; Enter la aplica y Esc la descarta.
    pub crop: Option<egui::Rect>,
    grab: Option<Grab>,
    /// Lectura OCR en curso o su resultado, pintada sobre la imagen.
    scan: Option<crate::scan::Scan>,
    /// Ancho de los datos de la derecha de la barra en el cuadro anterior, para saber cuántas
    /// herramientas caben.
    info_width: f32,
    /// Panel de ajustes de la herramienta, plegado con «‹».
    options_open: bool,
    /// Blur rellena con el color del borde en vez de pixelar.
    erase: bool,
    /// Spotlight en óvalo.
    oval_spotlight: bool,
    /// Contraste de la última zona medida, para no recalcularlo en cada cuadro.
    contrast: Option<(egui::Rect, u64, Option<f32>)>,
    /// Formato con el que Tab copia el color.
    color_format: color::Format,
    // La lista de «Color Format» estaba abierta en el cuadro anterior.
    format_open: bool,
    // Panel del color abierto (fuera de la memoria de popups de egui, ver `color_info`).
    color_open: bool,
    // Paleta de colores abierta, y en ella el selector del color propio.
    swatch_open: bool,
    custom_open: bool,
    /// Ajustes de cada herramienta y los que hay en disco.
    prefs: prefs::Prefs,
    saved_prefs: prefs::Prefs,
    /// Tamaños en puntos lógicos en vez de píxeles (P).
    logical: bool,
    /// Campos de «Resize Image»: revisión de la imagen para la que valen, ancho y alto.
    resize_fields: Option<(u64, u32, u32)>,
    /// Con frosted glass, el lienzo deja ver el fondo de la ventana bajo la cuadrícula.
    pub glass: bool,
}
impl Editor {
    pub fn new(image: RgbaImage) -> Self {
        // Las pruebas no leen ni escriben los ajustes del usuario.
        let prefs = if cfg!(test) {
            prefs::Prefs::default()
        } else {
            prefs::Prefs::load()
        };
        Self {
            doc: Document::new(image),
            tool: prefs.tool,
            style: prefs.style(prefs.tool),
            zoom: 1.,
            pan: Vec2::ZERO,
            texture: None,
            texture_nearest: false,
            hover_pixel: None,
            drawing: None,
            anchor: None,
            fitted: false,
            notice: None,
            text_edit: None,
            blur_cache: Default::default(),
            revision: 0,
            selected: None,
            crop: None,
            grab: None,
            scan: None,
            info_width: 0.,
            glass: false,
            options_open: prefs.options_open,
            erase: prefs.erase,
            oval_spotlight: prefs.oval_spotlight,
            contrast: None,
            color_format: prefs.color_format,
            format_open: false,
            color_open: false,
            swatch_open: false,
            custom_open: false,
            logical: prefs.logical,
            resize_fields: None,
            saved_prefs: prefs.clone(),
            prefs,
        }
    }
    /// Cambia de herramienta con el último estilo que se usó en ella.
    /// Cambiar de herramienta suelta la figura: si no, el estilo de la nueva se le aplicaría.
    fn set_tool(&mut self, tool: Tool) {
        self.selected = None;
        self.tool = tool;
        self.style = self.prefs.style(tool);
    }
    /// Recuerda lo elegido y lo guarda en disco cuando se suelta el ratón.
    fn remember(&mut self, ctx: &egui::Context) {
        self.prefs.erase = self.erase;
        self.prefs.oval_spotlight = self.oval_spotlight;
        self.prefs.color_format = self.color_format;
        self.prefs.options_open = self.options_open;
        self.prefs.logical = self.logical;
        self.prefs.tool = self.tool;
        if self.prefs != self.saved_prefs && !ctx.input(|i| i.pointer.any_down()) {
            if !cfg!(test) {
                self.prefs.save();
            }
            self.saved_prefs = self.prefs.clone();
        }
    }
    /// Empieza la animación de lectura sobre la imagen.
    pub fn start_scan(&mut self) {
        self.scan = Some(crate::scan::Scan::new());
    }
    /// Termina la lectura con su medalla («Done» o el aviso).
    pub fn finish_scan(&mut self, ok: bool, message: impl Into<String>) {
        match &mut self.scan {
            Some(scan) => scan.finish(ok, message),
            None => self.notify(message),
        }
    }
    pub fn notify(&mut self, message: impl Into<String>) {
        self.notice = Some((message.into(), Instant::now()));
    }
    fn pixel_color(&self) -> Option<Color32> {
        // Tras recortar o reducir, el píxel del cuadro anterior puede quedar fuera de la
        // imagen nueva: `get_pixel` entraba en pánico y cerraba el editor.
        let (x, y) = self.hover_pixel?;
        let p = self.doc.image.get_pixel_checked(x, y)?;
        Some(Color32::from_rgb(p[0], p[1], p[2]))
    }
    /// Color bajo el puntero en el formato elegido en «Color Format».
    fn color_text(&self) -> Option<String> {
        self.pixel_color().map(|c| self.color_format.text(c))
    }
    fn copy_color(&mut self, ctx: &egui::Context, color: Color32) {
        let text = self.color_format.text(color);
        ctx.copy_text(text.clone());
        self.notify(colorshot_core::i18n::tf("Copied {}", &[&text]));
    }
    pub(super) fn autoadjust(&mut self) {
        if let Some(zone) = &mut self.crop {
            *zone = zone::autoadjust(&self.doc.image, *zone);
        }
    }
    pub(super) fn grow_zone(&mut self) {
        if let Some(zone) = &mut self.crop {
            *zone = zone::grow(&self.doc.image, *zone);
        }
    }
    /// Rellena la zona con el color de su borde y la suelta.
    pub(super) fn erase_zone(&mut self) {
        if let Some(rect) = self.crop.take() {
            self.doc.push(Shape::Blur { rect, erase: true });
        }
    }
    /// Graba en la imagen el marco de la zona y su medida debajo, con el estilo actual.
    pub(super) fn imprint_size(&mut self) {
        let Some(rect) = self.crop.take() else { return };
        let text = format!("{}×{}", rect.width().round(), rect.height().round());
        let width = model::text_width(&text, model::text_size(self.style));
        let before = self.doc.shapes.clone();
        self.doc.shapes.push(Shape::Rect {
            rect,
            style: Style {
                fill: model::Fill::Outline,
                ..self.style
            },
        });
        self.doc.shapes.push(Shape::Text {
            at: egui::pos2(
                rect.center().x - width / 2.,
                rect.bottom() + self.style.width + 4.,
            ),
            text,
            style: self.style,
        });
        self.doc.record(before);
    }
    /// Contraste de la zona, recalculado solo cuando cambia la zona o la imagen.
    pub(super) fn zone_contrast(&mut self, zone: egui::Rect) -> Option<f32> {
        match self.contrast {
            Some((at, revision, ratio)) if at == zone && revision == self.doc.revision => ratio,
            _ => {
                let ratio = zone::contrast(&self.doc.image, zone);
                self.contrast = Some((zone, self.doc.revision, ratio));
                ratio
            }
        }
    }
    /// Editor con su propia barra encima del lienzo.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<EditorAction> {
        let (bar, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), BAR), egui::Sense::hover());
        ui.painter().rect_filled(bar, 0., crate::theme::Tokens::BAR);
        self.ui_in(ui, bar.shrink2(egui::vec2(10., 0.)))
    }
    /// Editor con las herramientas en `bar` (la barra de título de la ventana) y el lienzo
    /// en el resto de `ui`.
    pub fn ui_in(&mut self, ui: &mut egui::Ui, bar: egui::Rect) -> Option<EditorAction> {
        let mut action = None;
        let ctx = ui.ctx().clone();
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(2))
        {
            self.notice = None;
        }
        if self.notice.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        // Tab copia el color bajo el puntero y ⇧Tab el más oscuro de alrededor (el del texto).
        let tab = ctx.input(|i| {
            (i.key_pressed(Key::Tab)
                && !i.modifiers.ctrl
                && !i.modifiers.command
                && !i.modifiers.alt)
                .then_some(i.modifiers.shift)
        });
        let mut copy_color = match tab {
            Some(false) => self.pixel_color(),
            Some(true) => self
                .hover_pixel
                .and_then(|(x, y)| color::darkest(&self.doc.image, x, y)),
            None => None,
        };
        // Mientras se escribe texto, las teclas son del cuadro de texto.
        // También mientras se escribe en un campo de un panel (los lados de «Resize Image»).
        let typing = self.text_edit.is_some() || ctx.text_edit_focused();
        ctx.input(|i| {
            if typing {
                return;
            }
            // Esc descarta primero el recorte, luego la selección y por último cierra.
            if i.key_pressed(Key::Escape)
                && self.crop.take().is_none()
                && self.selected.take().is_none()
            {
                action = Some(EditorAction::Close);
            }
            if i.key_pressed(Key::Enter)
                && let Some(zone) = self.crop.take()
            {
                self.doc.crop(zone);
            }
            // Con una zona marcada, A, ⇧+] y Supr actúan sobre ella y no cambian de herramienta.
            let zone_keys = self.crop.is_some() && !i.modifiers.ctrl && !i.modifiers.command;
            if zone_keys {
                if i.key_pressed(Key::A) {
                    self.autoadjust();
                }
                if i.key_pressed(Key::CloseBracket) && i.modifiers.shift {
                    self.grow_zone();
                }
                if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
                    self.erase_zone();
                }
                // C copia el color medio de la zona.
                if i.key_pressed(Key::C)
                    && let Some(zone) = self.crop
                {
                    copy_color = color::average(&self.doc.image, zone);
                }
            }
            if let Some(index) = self.selected.filter(|i| *i < self.doc.shapes.len()) {
                if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
                    let before = self.doc.shapes.clone();
                    self.doc.shapes.remove(index);
                    self.doc.record(before);
                    self.selected = None;
                } else {
                    // Flechas: 1 px, con Shift 10 px.
                    let step = if i.modifiers.shift { 10. } else { 1. };
                    let mut d = Vec2::ZERO;
                    for (key, dir) in [
                        (Key::ArrowLeft, egui::vec2(-1., 0.)),
                        (Key::ArrowRight, egui::vec2(1., 0.)),
                        (Key::ArrowUp, egui::vec2(0., -1.)),
                        (Key::ArrowDown, egui::vec2(0., 1.)),
                    ] {
                        if i.key_pressed(key) {
                            d += dir * step;
                        }
                    }
                    if d != Vec2::ZERO {
                        let before = self.doc.shapes.clone();
                        edit::translate(&mut self.doc.shapes[index], d);
                        self.doc.record(before);
                    }
                }
            }
            if i.modifiers.ctrl || i.modifiers.command {
                if i.key_pressed(Key::Z) {
                    self.drawing = None;
                    self.anchor = None;
                    self.selected = None;
                    if i.modifiers.shift {
                        self.doc.redo();
                    } else {
                        self.doc.undo();
                    }
                }
                if i.key_pressed(Key::C) || i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                {
                    action = Some(EditorAction::Copy(model::render(&self.doc)));
                }
                if i.key_pressed(Key::S) {
                    action = Some(EditorAction::Save(model::render(&self.doc)));
                }
                if i.key_pressed(Key::P) {
                    action = Some(EditorAction::Pin(model::render(&self.doc)));
                }
                // Reconocer la selección, o la imagen entera si no hay.
                if i.key_pressed(Key::O) {
                    action = Some(EditorAction::Recognize(match self.crop {
                        Some(zone) => self.selection_image(zone),
                        None => self.doc.image.clone(),
                    }));
                }
            } else if !i.modifiers.alt && !zone_keys {
                if i.key_pressed(Key::P) {
                    self.logical = !self.logical;
                }
                for (key, tool) in [
                    (Key::V, Tool::Select),
                    (Key::A, Tool::Arrow),
                    (Key::L, Tool::Line),
                    (Key::R, Tool::Rectangle),
                    (Key::O, Tool::Oval),
                    (Key::D, Tool::Freehand),
                    (Key::H, Tool::Highlighter),
                    (Key::C, Tool::Counter),
                    (Key::T, Tool::Text),
                    (Key::B, Tool::Blur),
                    (Key::S, Tool::Spotlight),
                    (Key::K, Tool::Crop),
                ] {
                    if i.key_pressed(key) {
                        self.set_tool(tool);
                        self.drawing = None;
                        self.anchor = None;
                        self.crop = None;
                    }
                }
            }
        });
        if !typing && let Some(color) = copy_color {
            self.copy_color(&ctx, color);
        }
        let mut toolbar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let before = (self.style, self.erase, self.oval_spotlight);
        if let Some(command) = self.toolbar(&mut toolbar) {
            action = Some(command);
        }
        // En la esquina superior derecha del lienzo, con el mismo margen arriba y a la derecha.
        let corner = egui::pos2(
            ui.max_rect().right() - OPTIONS_MARGIN,
            bar.bottom() + OPTIONS_MARGIN,
        );
        self.options(ui.ctx(), corner);
        self.notice_toast(
            ui.ctx(),
            egui::pos2(
                ui.max_rect().left() + OPTIONS_MARGIN,
                ui.max_rect().bottom() - OPTIONS_MARGIN,
            ),
        );
        // Sin figura seleccionada, lo elegido queda como estilo de la herramienta.
        if self.selected.is_none() && before.0 != self.style {
            self.prefs.styles.insert(self.tool, self.style);
        }
        // Lo elegido en la barra o en el panel con una figura seleccionada se aplica a ella.
        if before != (self.style, self.erase, self.oval_spotlight)
            && let Some(index) = self.selected
        {
            let shapes = self.doc.shapes.clone();
            let changed = match self.doc.shapes.get_mut(index) {
                Some(Shape::Blur { erase, .. }) => {
                    *erase = self.erase;
                    true
                }
                Some(Shape::Spotlight { oval, .. }) => {
                    *oval = self.oval_spotlight;
                    true
                }
                Some(shape) => edit::style_mut(shape).map(|s| *s = self.style).is_some(),
                None => false,
            };
            if changed && self.doc.shapes != shapes {
                self.doc.record(shapes);
            }
        }
        self.canvas(ui);
        self.remember(&ctx);
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        editor: &mut Editor,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                editor.ui(ui);
            },
        );
        output.textures_delta.clear();
        output
    }
    fn button(p: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn drawing_commits_on_release_and_tab_copies_color() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut editor = Editor::new(RgbaImage::from_pixel(
            400,
            300,
            image::Rgba([10, 20, 30, 255]),
        ));
        editor.tool = Tool::Line;
        frame(&ctx, &mut editor, vec![]);
        let start = egui::pos2(550., 400.);
        let end = egui::pos2(650., 450.);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        assert!(editor.doc.shapes.is_empty());
        assert!(editor.drawing.is_some());
        frame(&ctx, &mut editor, vec![button(end, false)]);
        assert_eq!(editor.doc.shapes.len(), 1);
        assert!(editor.drawing.is_none());
        let output = frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(text) if text == "#0A141E"))
        );
    }
    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }
    fn drag(ctx: &egui::Context, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        frame(
            ctx,
            editor,
            vec![egui::Event::PointerMoved(from), button(from, true)],
        );
        frame(ctx, editor, vec![egui::Event::PointerMoved(to)]);
        frame(ctx, editor, vec![button(to, false)]);
    }
    #[test]
    fn shapes_stay_editable_and_crop_waits_for_enter() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut editor = Editor::new(RgbaImage::new(400, 300));
        editor.tool = Tool::Line;
        frame(&ctx, &mut editor, vec![]);
        // La imagen queda a escala 1.
        drag(
            &ctx,
            &mut editor,
            egui::pos2(550., 400.),
            egui::pos2(650., 450.),
        );
        assert_eq!(editor.selected, Some(0));
        // El extremo se arrastra por su tirador.
        drag(
            &ctx,
            &mut editor,
            egui::pos2(650., 450.),
            egui::pos2(700., 500.),
        );
        assert!(matches!(
            editor.doc.shapes[0],
            Shape::Line { from, to, .. } if to - from == egui::vec2(150., 100.)
        ));
        frame(&ctx, &mut editor, vec![key(Key::Delete)]);
        assert!(editor.doc.shapes.is_empty());
        editor.doc.undo();
        assert_eq!(editor.doc.shapes.len(), 1);
        editor.tool = Tool::Crop;
        drag(
            &ctx,
            &mut editor,
            egui::pos2(450., 300.),
            egui::pos2(550., 400.),
        );
        assert!(editor.crop.is_some());
        assert_eq!(editor.doc.image.width(), 400);
        frame(&ctx, &mut editor, vec![key(Key::Enter)]);
        assert!(editor.crop.is_none());
        assert_eq!(editor.doc.image.dimensions(), (100, 100));
    }
    #[test]
    fn changing_tool_releases_the_shape() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut editor = Editor::new(RgbaImage::new(400, 300));
        editor.set_tool(Tool::Line);
        frame(&ctx, &mut editor, vec![]);
        drag(
            &ctx,
            &mut editor,
            egui::pos2(550., 400.),
            egui::pos2(650., 450.),
        );
        assert_eq!(editor.selected, Some(0));
        let drawn = editor.doc.shapes.clone();
        editor.set_tool(Tool::Arrow);
        frame(&ctx, &mut editor, vec![]);
        assert_eq!(editor.selected, None);
        assert_eq!(editor.doc.shapes, drawn);
    }
}
