//! Barra de herramientas, panel de ajustes e información de la imagen.
use super::*;
use crate::icons;
use crate::theme::Tokens;
use egui::{Align2, FontId, Rect, Response, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use rapidshot_core::i18n::{n, t, tf};

const ICON: f32 = 18.;
/// Fondo de la herramienta activa: una píldora gris.
pub(super) const ACTIVE: Color32 = Color32::from_gray(72);
pub(super) const PANEL: Color32 = Color32::from_gray(40);
pub(super) const PANEL_LINE: Color32 = Color32::from_gray(62);
/// Paleta de colores, en filas de tres; el noveno círculo es el color propio.
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(240, 60, 40),
    Color32::from_rgb(113, 88, 245),
    Color32::from_rgb(255, 200, 0),
    Color32::from_rgb(255, 51, 102),
    Color32::from_rgb(242, 238, 228),
    Color32::from_rgb(15, 158, 134),
    Color32::from_rgb(30, 160, 240),
    Color32::from_rgb(31, 42, 60),
];
/// Herramientas de la barra principal.
const MAIN: [(Tool, &str, &str, &str); 8] = [
    (Tool::Select, "20-arrow-sf", icon::CURSOR, n("Select (V)")),
    (
        Tool::Arrow,
        "16-arrow",
        icon::ARROW_UP_RIGHT,
        n("Arrow (A)"),
    ),
    (Tool::Text, "20-text-sf", icon::TEXT_T, n("Text (T)")),
    (
        Tool::Rectangle,
        "20-rectangle-sf",
        icon::RECTANGLE,
        n("Rectangle (R)"),
    ),
    (
        Tool::Freehand,
        "20-edit-pen",
        icon::PENCIL_SIMPLE,
        n("Draw (D)"),
    ),
    (
        Tool::Blur,
        "20-blur-lite-sf",
        icon::DROP_HALF,
        n("Blur/Erase (B)"),
    ),
    (
        Tool::Highlighter,
        "20-highlighter",
        icon::HIGHLIGHTER,
        n("Highlighter (H)"),
    ),
    (
        Tool::Spotlight,
        "20-spotlight",
        icon::FLASHLIGHT,
        n("Spotlight (S)"),
    ),
];
/// Las que van dentro de «More tools».
const MORE: [(Tool, &str, &str, &str, &str); 4] = [
    (
        Tool::Counter,
        "20-counter-lite-sf",
        icon::NUMBER_CIRCLE_ONE,
        n("Counter"),
        "C",
    ),
    (Tool::Oval, "20-oval-sf", icon::CIRCLE, n("Oval"), "O"),
    (Tool::Line, "20-line-sf", icon::LINE_SEGMENT, n("Line"), "L"),
    (
        Tool::Crop,
        "20-crop-lite-sf_copy",
        icon::CROP,
        n("Crop"),
        "K",
    ),
];

impl Editor {
    pub(super) fn toolbar(&mut self, ui: &mut Ui) -> Option<EditorAction> {
        ui.spacing_mut().item_spacing.x = 6.;
        ui.spacing_mut().button_padding = vec2(5., 4.);
        // Al pasar o pulsar, solo el fondo gris, sin borde.
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [&mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            state.bg_stroke = Stroke::NONE;
        }
        let mut action = None;
        if icon_button(ui, "20-copy-lite-sf", icon::COPY, false)
            .tip(t("Copy (Ctrl+C)"))
            .clicked()
        {
            action = Some(EditorAction::Copy(model::render(&self.doc)));
        }
        divider(ui);
        if icon_button(ui, "20-save-lite-sf", icon::FLOPPY_DISK, false)
            .tip(t("Save (Ctrl+S)"))
            .clicked()
        {
            action = Some(EditorAction::Save(model::render(&self.doc)));
        }
        divider(ui);
        if icon_button(ui, "20-pin-lite-sf", icon::PUSH_PIN, false)
            .tip(t("Pin to Screen (Ctrl+P)"))
            .clicked()
        {
            action = Some(EditorAction::Pin(model::render(&self.doc)));
        }
        ui.add_space(14.);
        match self.crop {
            Some(zone) => self.crop_tools(ui, zone, &mut action),
            None => self.tools(ui),
        }
        let info = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.;
            ui.add_space(4.);
            self.zoom_info(ui);
            divider(ui);
            self.size_info(ui);
            divider(ui);
            self.color_info(ui);
            let (rect, _) = ui.allocate_exact_size(vec2(22., 22.), Sense::hover());
            ui.painter().circle_filled(
                rect.center(),
                10.,
                self.pixel_color().unwrap_or(Color32::BLACK),
            );
            ui.add_space(6.);
            // Revertir, en naranja; tenue en reposo como los demás iconos.
            let hovered = ui
                .ctx()
                .read_response(ui.next_auto_id())
                .is_some_and(|r| r.hovered());
            let accent = if hovered {
                Tokens::ACCENT
            } else {
                Tokens::ACCENT.gamma_multiply(0.8)
            };
            let revert = match icons::texture(ui.ctx(), "ico-20-revert") {
                Some(texture) => egui::Button::image(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(vec2(ICON, ICON))
                        .tint(accent),
                ),
                None => egui::Button::new(
                    RichText::new(icon::ARROWS_CLOCKWISE)
                        .size(ICON)
                        .color(accent),
                ),
            };
            let revert =
                ui.add_enabled(!self.doc.undo.is_empty(), revert.frame_when_inactive(false));
            if revert.tip(t("Revert all changes")).clicked() {
                self.commit_text();
                self.selected = None;
                while !self.doc.undo.is_empty() {
                    self.doc.undo();
                }
            }
            ui.min_rect().width()
        });
        self.info_width = info.inner;
        action
    }

    fn pick(&mut self, tool: Tool) {
        self.set_tool(tool);
        self.crop = None;
        self.commit_text();
        self.drawing = None;
        self.anchor = None;
    }

    /// Todas las herramientas en fila mientras quepan; las que no, de la última hacia atrás,
    /// pasan al menú «…», que solo aparece entonces.
    fn tools(&mut self, ui: &mut Ui) {
        let tools: Vec<(Tool, &str, &str, &str, &str)> = MAIN
            .iter()
            .map(|&(tool, name, glyph, label)| (tool, name, glyph, label, ""))
            .chain(MORE)
            .collect();
        // Ancho de cada botón con su separación; lo de la derecha es del cuadro anterior.
        let step = ICON + 2. * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
        let room = ui.available_width() - self.info_width - 14.;
        let shown = fitting(tools.len(), step, room);
        let (inline, rest) = tools.split_at(shown);
        for &(tool, name, glyph, label, key) in inline {
            let tip = match key {
                "" => t(label).to_string(),
                key => format!("{} ({key})", t(label)),
            };
            if icon_button(ui, name, glyph, self.tool == tool)
                .tip(tip)
                .clicked()
            {
                self.pick(tool);
            }
        }
        if rest.is_empty() {
            return;
        }
        let active = rest.iter().any(|m| m.0 == self.tool);
        let items: Vec<_> = rest
            .iter()
            .map(|&(tool, name, glyph, label, key)| (name, glyph, label, key, self.tool == tool))
            .collect();
        if let Some(i) = more_menu(ui, &items, active) {
            self.pick(rest[i].0);
        }
    }

    /// Con una zona marcada, la barra cambia a las acciones sobre la selección; las que no
    /// caben pasan al menú «…», como las herramientas.
    fn crop_tools(&mut self, ui: &mut Ui, zone: Rect, action: &mut Option<EditorAction>) {
        let crop = ui.add(
            egui::Button::new(RichText::new(t("Crop")).size(13.))
                .fill(ACTIVE)
                .corner_radius(6),
        );
        if crop.tip(t("Crop (Enter)")).clicked() {
            self.crop = None;
            self.doc.crop(zone);
        }
        const ACTIONS: [(&str, &str, &str, &str); 6] = [
            ("20-ocr-sf", icon::SCAN, n("Read Text / QR"), "Ctrl+O"),
            (
                "20-adjust-lite-sf",
                icon::FRAME_CORNERS,
                n("Autoadjust"),
                "A",
            ),
            ("20-resize", icon::ARROWS_OUT, n("Resize"), "⇧+]"),
            (
                "20-selectionsize-lite-sf",
                icon::RULER,
                n("Add size label"),
                "",
            ),
            ("20-erase-lite-sf", icon::ERASER, n("Erase"), "Del"),
            (
                "20-reset-lite-sf",
                icon::X_CIRCLE,
                n("Clear Selection"),
                "Esc",
            ),
        ];
        let step = ICON + 2. * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
        // El divisor antes de «Reset Selection» ocupa lo suyo.
        let room = ui.available_width() - self.info_width - 14. - 1. - ui.spacing().item_spacing.x;
        let shown = fitting(ACTIONS.len(), step, room);
        let mut chosen = None;
        for (i, &(name, glyph, label, key)) in ACTIONS[..shown].iter().enumerate() {
            if i == ACTIONS.len() - 1 {
                divider(ui);
            }
            let tip = match key {
                "" => t(label).to_string(),
                key => format!("{} ({key})", t(label)),
            };
            if icon_button(ui, name, glyph, false).tip(tip).clicked() {
                chosen = Some(i);
            }
        }
        let rest: Vec<_> = ACTIONS[shown..]
            .iter()
            .map(|&(name, glyph, label, key)| (name, glyph, label, key, false))
            .collect();
        if !rest.is_empty()
            && let Some(i) = more_menu(ui, &rest, false)
        {
            chosen = Some(shown + i);
        }
        match chosen {
            Some(0) => *action = Some(EditorAction::Recognize(self.selection_image(zone))),
            Some(1) => self.autoadjust(),
            Some(2) => self.grow_zone(),
            Some(3) => self.imprint_size(),
            Some(4) => self.erase_zone(),
            Some(5) => self.crop = None,
            _ => {}
        }
    }

    /// Píxeles originales de la zona `zone` (en píxeles de imagen).
    pub(super) fn selection_image(&self, zone: Rect) -> image::RgbaImage {
        let (w, h) = self.doc.image.dimensions();
        let x = (zone.min.x.max(0.) as u32).min(w.saturating_sub(1));
        let y = (zone.min.y.max(0.) as u32).min(h.saturating_sub(1));
        let cw = (zone.width().round() as u32).clamp(1, w - x);
        let ch = (zone.height().round() as u32).clamp(1, h - y);
        image::imageops::crop_imm(&self.doc.image, x, y, cw, ch).to_image()
    }

    /// Aviso («Copied», …) en la esquina inferior izquierda del lienzo, sobre un fondo propio
    /// para leerse encima de la imagen; se desvanece al final.
    pub(super) fn notice_toast(&self, ctx: &egui::Context, left_bottom: egui::Pos2) {
        let Some((text, since)) = &self.notice else {
            return;
        };
        let left = 2. - since.elapsed().as_secs_f32();
        let alpha = (left / 0.3).clamp(0., 1.);
        egui::Area::new(egui::Id::new("editor-notice"))
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(left_bottom)
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(Stroke::new(1., PANEL_LINE))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).size(14.).color(Tokens::TEXT));
                    });
            });
    }

    /// Panel flotante bajo la barra, a la derecha: color y grosor de la herramienta o de la
    /// figura seleccionada.
    pub(super) fn options(&mut self, ctx: &egui::Context, right_top: egui::Pos2) {
        // La figura seleccionada manda sobre la herramienta: su panel es el de su tipo.
        let kind = match self.selected.and_then(|i| self.doc.shapes.get(i)) {
            Some(Shape::Arrow { .. }) => Tool::Arrow,
            Some(Shape::Rect { .. }) => Tool::Rectangle,
            Some(Shape::Blur { .. }) => Tool::Blur,
            Some(Shape::Spotlight { .. }) => Tool::Spotlight,
            Some(_) => Tool::Line,
            None => self.tool,
        };
        let colored = !matches!(
            kind,
            Tool::Select | Tool::Crop | Tool::Blur | Tool::Spotlight
        );
        if kind == Tool::Select || kind == Tool::Crop || self.crop.is_some() {
            return;
        }
        egui::Area::new(egui::Id::new("tool-options"))
            .order(egui::Order::Foreground)
            .pivot(Align2::RIGHT_TOP)
            .fixed_pos(right_top)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(Stroke::new(1., PANEL_LINE))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.;
                            // La flecha se dibuja con dos trazos, centrada en la fila.
                            let (rect, chevron) =
                                ui.allocate_exact_size(vec2(18., 24.), Sense::click());
                            if chevron.hovered() {
                                ui.painter().rect_filled(rect, 5, ACTIVE);
                            }
                            let c = rect.center();
                            let dx = if self.options_open { 2. } else { -2. };
                            ui.painter().line(
                                vec![c + vec2(dx, -4.), c + vec2(-dx, 0.), c + vec2(dx, 4.)],
                                Stroke::new(1.6, Tokens::TEXT),
                            );
                            let tip = if self.options_open {
                                t("Collapse")
                            } else {
                                t("Expand")
                            };
                            if chevron.tip(tip).clicked() {
                                self.options_open = !self.options_open;
                            }
                            if colored {
                                self.swatch(ui);
                            }
                            if self.options_open {
                                self.variants(ui, kind);
                                if colored {
                                    width_slider(ui, &mut self.style.width);
                                }
                            }
                        });
                    });
            });
    }

    /// Estilos propios de la herramienta, en un segmentado.
    fn variants(&mut self, ui: &mut Ui, kind: Tool) {
        match kind {
            Tool::Arrow => segmented(
                ui,
                &mut self.style.head,
                &[
                    (
                        model::Head::Solid,
                        Face::Glyph(icon::ARROW_FAT_RIGHT),
                        t("Solid"),
                    ),
                    (model::Head::Open, Face::Glyph(icon::ARROW_RIGHT), t("Line")),
                    (
                        model::Head::Double,
                        Face::Glyph(icon::ARROWS_HORIZONTAL),
                        t("Double"),
                    ),
                ],
            ),
            Tool::Rectangle => segmented(
                ui,
                &mut self.style.fill,
                &[
                    (model::Fill::Outline, Face::Square(0.), t("Outline")),
                    (
                        model::Fill::Translucent,
                        Face::Square(0.35),
                        t("Translucent"),
                    ),
                    (model::Fill::Solid, Face::Square(1.), t("Filled")),
                ],
            ),
            Tool::Blur => segmented(
                ui,
                &mut self.erase,
                &[
                    (false, Face::Word(t("Blur")), t("Pixelate (B)")),
                    (
                        true,
                        Face::Word(t("Erase")),
                        t("Fill with the border color"),
                    ),
                ],
            ),
            Tool::Spotlight => segmented(
                ui,
                &mut self.oval_spotlight,
                &[
                    (false, Face::Glyph(icon::RECTANGLE), t("Rectangle")),
                    (true, Face::Glyph(icon::CIRCLE), t("Oval")),
                ],
            ),
            _ => {}
        }
    }

    fn swatch(&mut self, ui: &mut Ui) {
        let (rect, swatch) = ui.allocate_exact_size(vec2(22., 22.), Sense::click());
        ui.painter()
            .circle_filled(rect.center(), 9., self.style.color);
        if swatch.hovered() {
            ui.painter()
                .circle_stroke(rect.center(), 11., Stroke::new(1., Tokens::DIM));
        }
        // Fuera de la memoria de popups de egui, que solo admite uno abierto: así el selector
        // del color propio no la cierra al arrastrar.
        let mut open = self.swatch_open ^ swatch.clicked();
        egui::Popup::menu(&swatch.tip(t("Color")))
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let mut close = false;
                egui::Grid::new("palette")
                    .spacing(vec2(6., 6.))
                    .show(ui, |ui| {
                        for (i, color) in PALETTE.into_iter().enumerate() {
                            let response = circle(ui, color == self.style.color);
                            ui.painter()
                                .circle_filled(response.rect.center(), 10., color);
                            if response.clicked() {
                                self.style.color = color;
                                close = true;
                            }
                            if i % 3 == 2 {
                                ui.end_row();
                            }
                        }
                        // El último círculo es el color propio, con un anillo de colores.
                        let custom = self.prefs.custom;
                        let response = circle(ui, custom == Some(self.style.color));
                        let center = response.rect.center();
                        rainbow(ui.painter(), center, 10.);
                        match custom {
                            Some(color) => {
                                ui.painter().circle_filled(center, 7., color);
                            }
                            None => {
                                ui.painter().text(
                                    center,
                                    Align2::CENTER_CENTER,
                                    icon::PLUS,
                                    FontId::proportional(12.),
                                    Tokens::TEXT,
                                );
                            }
                        }
                        if response.tip(t("Custom Color")).clicked() {
                            self.custom_open = !self.custom_open;
                            let color = *self.prefs.custom.get_or_insert(self.style.color);
                            self.style.color = color;
                        }
                    });
                if self.custom_open {
                    ui.add_space(4.);
                    ui.spacing_mut().slider_width = 150.;
                    let color = self.prefs.custom.get_or_insert(self.style.color);
                    if egui::color_picker::color_picker_color32(
                        ui,
                        color,
                        egui::color_picker::Alpha::Opaque,
                    ) {
                        self.style.color = *color;
                    }
                }
                if close {
                    ui.close();
                }
            });
        if !open {
            self.custom_open = false;
        }
        self.swatch_open = open;
    }
}

/// Celda redonda de la paleta, con aro claro si es el color elegido o está bajo el ratón.
fn circle(ui: &mut Ui, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(26., 26.), Sense::click());
    if selected || response.hovered() {
        ui.painter()
            .circle_stroke(rect.center(), 12.5, Stroke::new(1.5, Tokens::TEXT));
    }
    response
}

/// Anillo con la rueda de tonos, para el círculo del color propio.
fn rainbow(painter: &egui::Painter, center: egui::Pos2, radius: f32) {
    const STEPS: usize = 36;
    let point = |i: usize| {
        let angle = i as f32 / STEPS as f32 * std::f32::consts::TAU;
        center + radius * 0.85 * vec2(angle.cos(), angle.sin())
    };
    for i in 0..STEPS {
        let hue = egui::ecolor::Hsva::new(i as f32 / STEPS as f32, 0.85, 1., 1.);
        painter.line_segment(
            [point(i), point(i + 1)],
            Stroke::new(3., Color32::from(hue)),
        );
    }
}

/// Botón de icono con la píldora gris detrás cuando está activo. En reposo el icono va un
/// 20 % más tenue; activo o con el ratón encima, en su color normal.
fn icon_button(ui: &mut Ui, name: &str, glyph: &str, active: bool) -> Response {
    // El hover se sabe del cuadro anterior: el botón aún no existe en este.
    let hovered = ui
        .ctx()
        .read_response(ui.next_auto_id())
        .is_some_and(|r| r.hovered());
    let color = if active || hovered {
        Tokens::TEXT
    } else {
        Tokens::TEXT.gamma_multiply(0.8)
    };
    // Un botón sin marco ignora `fill`: el fondo se pinta detrás antes de saber su rectángulo.
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(icons::button(ui, name, glyph, ICON, color));
    if response.hovered() != hovered {
        ui.ctx().request_repaint();
    }
    if active {
        ui.painter().set(
            background,
            egui::Shape::rect_filled(response.rect, 6., ACTIVE),
        );
    }
    response
}

/// Tooltip: cápsula gris bajo el control, con el atajo tenue entre paréntesis.
pub(super) trait Tip {
    fn tip(self, text: impl Into<String>) -> Self;
}
impl Tip for Response {
    fn tip(self, text: impl Into<String>) -> Self {
        let ctx = self.ctx.clone();
        // Como egui: tras medio segundo quieto encima, y nunca con un menú abierto.
        let still = ctx.input(|i| i.pointer.time_since_last_movement());
        if !self.hovered() || self.is_pointer_button_down_on() || egui::Popup::is_any_open(&ctx) {
            return self;
        }
        if still < 0.5 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(0.5 - still));
            return self;
        }
        let text = text.into();
        let (label, keys) = match text.rfind(" (") {
            Some(at) if text.ends_with(')') => (&text[..at], &text[at..]),
            _ => (text.as_str(), ""),
        };
        let font = FontId::proportional(12.);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            self.id.with("tip"),
        ));
        let label = painter.layout_no_wrap(label.into(), font.clone(), Tokens::TEXT);
        let keys = painter.layout_no_wrap(keys.into(), font, Tokens::DIM);
        let size = vec2(label.size().x + keys.size().x, label.size().y) + vec2(20., 8.);
        let mut pill =
            Rect::from_center_size(self.rect.center_bottom() + vec2(0., 8. + size.y / 2.), size);
        // Que no se salga por los lados de la ventana.
        let screen = ctx.content_rect();
        pill = pill.translate(vec2(
            (screen.left() + 4. - pill.left()).max(0.)
                + (screen.right() - 4. - pill.right()).min(0.),
            0.,
        ));
        painter.rect(
            pill,
            12,
            PANEL,
            Stroke::new(1., PANEL_LINE),
            egui::StrokeKind::Inside,
        );
        let at = pill.left_center() + vec2(10., -label.size().y / 2.);
        let width = label.size().x;
        painter.galley(at, label, Tokens::TEXT);
        painter.galley(at + vec2(width, 0.), keys, Tokens::DIM);
        self
    }
}

/// Cara de un botón del segmentado.
pub(super) enum Face {
    Glyph(&'static str),
    Word(&'static str),
    /// Cuadro con el relleno a esa opacidad (0 = solo borde).
    Square(f32),
}

/// Botones pegados con el elegido resaltado; escribe en `value` el que se pulse.
pub(super) fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, Face, &str)],
) {
    let height = 24.;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, face, _)| match face {
            Face::Word(word) => model::text_width(word, 12.) + 16.,
            _ => 28.,
        })
        .collect();
    let (rect, _) = ui.allocate_exact_size(vec2(widths.iter().sum(), height), Sense::hover());
    ui.painter().rect_filled(rect, 6, Color32::from_gray(30));
    let mut x = rect.left();
    for ((option, face, tip), width) in options.iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(width, height));
        x += width;
        let response = ui
            .interact(cell, ui.id().with(("segment", tip)), Sense::click())
            .tip(*tip);
        let chosen = *option == *value;
        if chosen {
            ui.painter().rect_filled(cell.shrink(2.), 5, ACTIVE);
        }
        let color = if chosen || response.hovered() {
            Tokens::TEXT
        } else {
            Tokens::DIM
        };
        let painter = ui.painter();
        match face {
            Face::Glyph(glyph) => {
                painter.text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    glyph,
                    FontId::proportional(15.),
                    color,
                );
            }
            Face::Word(word) => {
                painter.text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    word,
                    FontId::proportional(12.),
                    color,
                );
            }
            Face::Square(alpha) => {
                let square = Rect::from_center_size(cell.center(), vec2(13., 11.));
                if *alpha > 0. {
                    painter.rect_filled(square, 2, color.gamma_multiply(*alpha));
                }
                painter.rect_stroke(square, 2, Stroke::new(1.5, color), egui::StrokeKind::Inside);
            }
        }
        if response.clicked() {
            *value = *option;
        }
    }
}

/// Botón «…» con las entradas que no cupieron (icono, glifo, nombre, atajo, elegida);
/// devuelve la pulsada.
fn more_menu(ui: &mut Ui, items: &[(&str, &str, &str, &str, bool)], active: bool) -> Option<usize> {
    let more = icon_button(ui, "20-more", icon::DOTS_THREE, active).tip(t("More tools"));
    let mut clicked = None;
    egui::Popup::menu(&more).show(|ui| {
        ui.set_min_width(170.);
        for (i, &(name, glyph, label, key, selected)) in items.iter().enumerate() {
            let text = t(label).to_string();
            let item = match icons::texture(ui.ctx(), name) {
                Some(texture) => egui::Button::image_and_text(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(vec2(16., 16.))
                        .tint(Tokens::TEXT),
                    text,
                ),
                None => egui::Button::new(format!("{glyph}  {text}")),
            };
            if ui.add(item.shortcut_text(key).selected(selected)).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// Cuántos de `count` botones de `step` caben en `room`; si no caben todos, deja sitio
/// para el botón «…».
fn fitting(count: usize, step: f32, room: f32) -> usize {
    let fit = |room: f32| (room / step).floor().max(0.) as usize;
    if fit(room) >= count {
        count
    } else {
        fit(room - step).min(count)
    }
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(1., 26.), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1., PANEL_LINE));
}

/// Grosor en una cuña que se ensancha hacia la derecha.
fn width_slider(ui: &mut Ui, width: &mut f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(84., 22.), Sense::click_and_drag());
    let (min, max) = (1., 24.);
    if let Some(p) = response.interact_pointer_pos() {
        let t = ((p.x - rect.left() - 4.) / (rect.width() - 8.)).clamp(0., 1.);
        *width = (min + t * (max - min)).round();
    }
    let painter = ui.painter();
    let y = rect.center().y;
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(rect.left(), y - 0.5),
            pos2(rect.right(), y - 4.),
            pos2(rect.right(), y + 4.),
            pos2(rect.left(), y + 0.5),
        ],
        Color32::from_gray(90),
        Stroke::NONE,
    ));
    let t = (*width - min) / (max - min);
    let knob = Rect::from_center_size(
        pos2(rect.left() + 4. + t * (rect.width() - 8.), y),
        vec2(8., 18.),
    );
    painter.rect_filled(knob, 3, Color32::from_gray(200));
    response.tip(tf("Width {}", &[&format!("{width:.0}")]));
}

// Valor arriba y leyenda tenue abajo, alineados a la izquierda en un ancho fijo:
// `ui.vertical` dentro de un layout de derecha a izquierda ocupa todo el ancho libre.
/// Valor con su leyenda debajo. `width` es el mínimo: si el valor o la leyenda (traducida)
/// son más largos, el bloque se ensancha y empuja a los vecinos en vez de pisarlos.
pub(super) fn info(ui: &mut Ui, value: &str, caption: &str, width: f32) -> Response {
    let painter = ui.painter();
    let value = painter.layout_no_wrap(value.to_owned(), FontId::proportional(14.), Tokens::TEXT);
    let caption =
        painter.layout_no_wrap(caption.to_owned(), FontId::proportional(12.), Tokens::DIM);
    let width = width.max(value.size().x).max(caption.size().x).ceil();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.expand2(vec2(5., 1.)), 6, ACTIVE);
    }
    let painter = ui.painter();
    painter.galley(rect.left_top() + vec2(0., 2.), value, Tokens::TEXT);
    let below = rect.left_bottom() - vec2(0., 2. + caption.size().y);
    painter.galley(below, caption, Tokens::DIM);
    response
}

#[cfg(test)]
mod tests {
    #[test]
    fn tools_overflow_into_more() {
        // Caben las 12: sin «…».
        assert_eq!(super::fitting(12, 34., 12. * 34.), 12);
        // Falta sitio para una: salen 10 y el «…» ocupa el hueco de la undécima.
        assert_eq!(super::fitting(12, 34., 11. * 34.), 10);
        assert_eq!(super::fitting(12, 34., 10.), 0);
    }
}
