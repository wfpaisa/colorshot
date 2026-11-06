//! Bloques de información de la barra (color, tamaño y zoom) y sus paneles.
use super::toolbar::{ACTIVE, Face, PANEL, PANEL_LINE, info, segmented};
use super::*;
use crate::theme::Tokens;
use egui::{Align2, FontId, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use colorshot_core::i18n::t;

/// Azul de los botones principales y del selector, el de macOS en el video.
const BLUE: Color32 = Color32::from_rgb(10, 110, 230);
const FIELD: Color32 = Color32::from_gray(56);

/// Marco de los paneles: gris 40, borde fino y esquinas de 12.
fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1., PANEL_LINE))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(16))
}

fn separator(ui: &mut Ui) {
    ui.add_space(12.);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1., PANEL_LINE));
    ui.add_space(12.);
}

impl Editor {
    /// Color bajo el puntero; al pulsarlo se elige el formato con el que Tab lo copia.
    pub(super) fn color_info(&mut self, ui: &mut Ui) {
        // Sin color bajo el puntero se muestra el negro: «#000000».
        let value = self
            .color_text()
            .unwrap_or_else(|| self.color_format.text(Color32::BLACK));
        // El ancho sigue al texto para que «UIColor(…)» quepa entero.
        let width = (model::text_width(&value, 14.) + 2.).max(76.);
        let color = info(ui, &value, t("Tab to copy"), width)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        // Un clic dentro, como el que abre el selector, no lo cierra; con la lista del selector
        // abierta (un área aparte), tampoco los clics en ella.
        let close = if self.format_open {
            egui::PopupCloseBehavior::IgnoreClicks
        } else {
            egui::PopupCloseBehavior::CloseOnClickOutside
        };
        // egui guarda un solo popup abierto por ventana: si este fuera el suyo, abrir la lista
        // del selector lo cerraría. Su estado va aparte.
        let mut open = self.color_open ^ color.clicked();
        egui::Popup::menu(&color)
            .open_bool(&mut open)
            .close_behavior(close)
            .frame(panel())
            .width(380.)
            .show(|ui| {
                ui.spacing_mut().item_spacing = vec2(8., 4.);
                let sample = self.pixel_color().unwrap_or(Color32::from_rgb(255, 194, 0));
                // Fila de la altura del selector, con el título centrado a su lado.
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 32.),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new(t("Color Format")).size(15.));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.format_select(ui, sample);
                        });
                    },
                );
                separator(ui);
                for (art, title, text) in [
                    (
                        Art::Pixel,
                        t("Copy Pixel Color"),
                        t("Point to a pixel and press Tab"),
                    ),
                    (
                        Art::Text,
                        t("Copy Text Color"),
                        t("Shift+Tab copies the darkest color within 20 px"),
                    ),
                    (
                        Art::Average,
                        t("Copy Average Color"),
                        t("Select an area · C copies its average color"),
                    ),
                ] {
                    help_row(ui, art, title, text);
                }
            });
        self.color_open = open;
    }

    /// Selector del formato: el elegido con la muestra y el botón azul de flechas.
    fn format_select(&mut self, ui: &mut Ui, sample: Color32) {
        let mut chosen = self.color_format;
        let field = |ui: &mut Ui| {
            let visuals = &mut ui.visuals_mut().widgets;
            for state in [
                &mut visuals.inactive,
                &mut visuals.hovered,
                &mut visuals.open,
            ] {
                state.weak_bg_fill = FIELD;
                state.bg_stroke = Stroke::new(1., PANEL_LINE);
                state.corner_radius = 7.into();
                state.fg_stroke.color = Tokens::TEXT;
            }
            visuals.hovered.weak_bg_fill = Color32::from_gray(64);
            ui.spacing_mut().button_padding = vec2(8., 7.);
        };
        let open = ui.scope(|ui| {
            field(ui);
            let combo = egui::ComboBox::from_id_salt("color-format")
                .width(210.)
                .height(400.)
                .selected_text(RichText::new(chosen.label(sample)).size(14.))
                .icon(|ui, rect, _, _| {
                    // Recuadro azul con ⌃⌄, pegado al borde derecho.
                    let square = Rect::from_center_size(
                        pos2(rect.right() - 2., rect.center().y),
                        vec2(20., 20.),
                    );
                    let painter = ui.painter();
                    painter.rect_filled(square, 5, BLUE);
                    let c = square.center();
                    let stroke = Stroke::new(1.5, Color32::WHITE);
                    painter.line(
                        vec![c + vec2(-3., -2.), c + vec2(0., -5.), c + vec2(3., -2.)],
                        stroke,
                    );
                    painter.line(
                        vec![c + vec2(-3., 2.), c + vec2(0., 5.), c + vec2(3., 2.)],
                        stroke,
                    );
                })
                .show_ui(ui, |ui| {
                    ui.spacing_mut().button_padding = vec2(8., 5.);
                    for format in color::Format::ALL {
                        let check = if format == chosen {
                            egui_phosphor::regular::CHECK
                        } else {
                            "    "
                        };
                        let text =
                            RichText::new(format!("{check}  {}", format.label(sample))).size(14.);
                        if ui.selectable_label(format == chosen, text).clicked() {
                            chosen = format;
                        }
                    }
                });
            egui::ComboBox::is_open(ui.ctx(), combo.response.id)
        });
        self.format_open = open.inner;
        self.color_format = chosen;
    }

    /// Tamaño de la imagen o de la zona; con P, en puntos lógicos.
    pub(super) fn size_info(&mut self, ui: &mut Ui) {
        let (caption, w, h) = match self.crop {
            Some(zone) => (t("Selection size"), zone.width(), zone.height()),
            None => (
                t("Image size"),
                self.doc.image.width() as f32,
                self.doc.image.height() as f32,
            ),
        };
        let value = if self.logical {
            let ppp = ui.ctx().pixels_per_point();
            format!("{:.0}×{:.0}pt", w / ppp, h / ppp)
        } else {
            format!("{:.0}×{:.0}px", w.round(), h.round())
        };
        let width = (model::text_width(&value, 14.) + 2.).max(84.);
        let size = info(ui, &value, caption, width).on_hover_cursor(egui::CursorIcon::PointingHand);
        let (iw, ih) = self.doc.image.dimensions();
        // Los campos vuelven al tamaño real cuando cambia la imagen.
        if self
            .resize_fields
            .is_none_or(|(revision, ..)| revision != self.doc.revision)
        {
            self.resize_fields = Some((self.doc.revision, iw, ih));
        }
        egui::Popup::menu(&size)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(panel())
            .width(340.)
            .show(|ui| {
                ui.spacing_mut().item_spacing = vec2(8., 6.);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t("Size in display points"))
                            .size(15.)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        toggle(ui, &mut self.logical);
                    });
                });
                ui.label(
                    RichText::new(t(
                        "Display points account for screen scaling. Press P to switch units.",
                    ))
                    .size(13.)
                    .color(Tokens::DIM),
                );
                separator(ui);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t("Resize Image")).size(15.).strong());
                    ui.label(
                        RichText::new(t("Flattens annotations"))
                            .size(13.)
                            .color(Tokens::DIM),
                    );
                });
                ui.add_space(4.);
                let Some((revision, mut w, mut h)) = self.resize_fields else {
                    return;
                };
                // Los atajos solo rellenan los campos; nada cambia hasta pulsar «Resize».
                let scaled = |factor: f32| {
                    (
                        ((iw as f32 * factor).round() as u32).max(1),
                        ((ih as f32 * factor).round() as u32).max(1),
                    )
                };
                const FACTORS: [f32; 6] = [0.25, 1. / 3., 0.5, 1., 2., 4.];
                let mut factor = FACTORS
                    .into_iter()
                    .find(|f| scaled(*f) == (w, h))
                    .unwrap_or(0.);
                let before = factor;
                segmented(
                    ui,
                    &mut factor,
                    &[
                        (0.25, Face::Word("25%"), "25%"),
                        (1. / 3., Face::Word("33%"), "33%"),
                        (0.5, Face::Word("50%"), "50%"),
                        (1., Face::Word("1×"), "1×"),
                        (2., Face::Word("2×"), "2×"),
                        (4., Face::Word("4×"), "4×"),
                    ],
                );
                if factor != before {
                    (w, h) = scaled(factor);
                }
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    // Al cambiar un lado, el otro sigue la proporción de la imagen.
                    let ratio = ih as f32 / iw.max(1) as f32;
                    if number(ui, &mut w).changed() {
                        h = ((w as f32 * ratio).round() as u32).max(1);
                    }
                    ui.label(RichText::new("×").size(15.));
                    if number(ui, &mut h).changed() {
                        w = ((h as f32 / ratio).round() as u32).max(1);
                    }
                    ui.label(RichText::new("px").size(15.));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let changed = (w.max(1), h.max(1)) != (iw, ih);
                        let resize = egui::Button::new(
                            RichText::new(t("Resize")).size(14.).color(if changed {
                                Color32::WHITE
                            } else {
                                Tokens::DIM
                            }),
                        )
                        .fill(if changed {
                            BLUE
                        } else {
                            Color32::from_gray(45)
                        })
                        .corner_radius(7)
                        .min_size(vec2(76., 28.));
                        if ui.add_enabled(changed, resize).clicked() {
                            self.commit_text();
                            self.doc.resize(w, h);
                            ui.close();
                        }
                    });
                });
                self.resize_fields = Some((revision, w, h));
            });
    }

    pub(super) fn zoom_info(&mut self, ui: &mut Ui) {
        let zoom = info(ui, &format!("{:.0}%", self.zoom * 100.), t("Zoom"), 44.)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        egui::Popup::menu(&zoom)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(panel())
            .width(240.)
            .show(|ui| {
                ui.spacing_mut().item_spacing = vec2(8., 8.);
                ui.horizontal(|ui| {
                    let mut factor = 1.;
                    if step_button(ui, egui_phosphor::regular::MINUS).clicked() {
                        factor = 1. / 1.25;
                    }
                    ui.add_sized(
                        vec2(ui.available_width() - 40., 34.),
                        egui::Label::new(
                            RichText::new(format!("{:.0}%", self.zoom * 100.)).size(24.),
                        ),
                    );
                    if step_button(ui, egui_phosphor::regular::PLUS).clicked() {
                        factor = 1.25;
                    }
                    if factor != 1. {
                        // Acercar sobre el centro del lienzo.
                        self.zoom = (self.zoom * factor).clamp(0.0001, 64.);
                        self.pan *= factor;
                    }
                });
                separator(ui);
                for (label, keys) in [
                    (t("Zoom In"), ["Ctrl", "+"]),
                    (t("Zoom Out"), ["Ctrl", "−"]),
                    (t("Zoom to Selection"), ["Ctrl", "2"]),
                    (t("Actual Size"), ["Ctrl", "0"]),
                    (t("Fit to Window"), ["Ctrl", "9"]),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).size(14.).color(Tokens::DIM));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.;
                            for key in keys.iter().rev() {
                                keycap(ui, key);
                            }
                        });
                    });
                }
                separator(ui);
                mouse_help(ui);
            });
    }
}

/// Interruptor redondo, como el de macOS.
fn toggle(ui: &mut Ui, on: &mut bool) {
    let (rect, response) = ui.allocate_exact_size(vec2(44., 24.), Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let painter = ui.painter();
    let track = Color32::from_gray(70).lerp_to_gamma(BLUE, t);
    painter.rect_filled(rect, 12, track);
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12., t);
    painter.circle_filled(pos2(x, rect.center().y), 10., Color32::from_gray(225));
}

/// Campo numérico de los lados, alineado a la derecha.
fn number(ui: &mut Ui, value: &mut u32) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            state.bg_fill = FIELD;
            state.weak_bg_fill = FIELD;
            state.corner_radius = 7.into();
        }
        ui.add_sized(
            vec2(72., 28.),
            egui::DragValue::new(value)
                .range(1..=32_768)
                .speed(1.)
                .custom_formatter(|v, _| format!("{v:.0}")),
        )
    })
    .inner
}

fn step_button(ui: &mut Ui, glyph: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32., 32.), Sense::click());
    let fill = if response.hovered() {
        ACTIVE
    } else {
        Color32::from_gray(46)
    };
    ui.painter().rect_filled(rect, 6, fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(16.),
        Tokens::DIM,
    );
    response
}

/// Tecla dibujada como tecla: recuadro con borde y sombra abajo.
fn keycap(ui: &mut Ui, key: &str) {
    let width = (model::text_width(key, 13.) + 16.).max(30.);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 30.), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect.translate(vec2(0., 1.5)), 6, Color32::from_gray(20));
    painter.rect(
        rect,
        6,
        Color32::from_gray(44),
        Stroke::new(1., Color32::from_gray(96)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        key,
        FontId::proportional(13.),
        Tokens::TEXT,
    );
}

/// Pie del panel de zoom: el ratón con la rueda y el botón derecho señalados.
fn mouse_help(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 88.), Sense::hover());
    let painter = ui.painter_at(rect.expand(16.));
    let font = FontId::proportional(13.);
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        t("Zoom with\nthe scroll\nwheel"),
        font.clone(),
        Tokens::DIM,
    );
    painter.text(
        rect.right_top(),
        Align2::RIGHT_TOP,
        t("Pan with\nthe right\nbutton"),
        font,
        Tokens::DIM,
    );
    // Ratón: arco arriba y lados que se salen por abajo del panel.
    let c = pos2(rect.center().x, rect.bottom() - 26.);
    let r = 26.;
    let line = Stroke::new(1., Color32::from_gray(150));
    let mut outline: Vec<Pos2> = vec![c + vec2(-r, 40.)];
    outline.extend((0..=24).map(|i| {
        let a = std::f32::consts::PI * (1. + i as f32 / 24.);
        c + vec2(a.cos() * r, a.sin() * r)
    }));
    outline.push(c + vec2(r, 40.));
    painter.line(outline, line);
    painter.line_segment([c + vec2(0., -r), c + vec2(0., 12.)], line);
    let gold = Color32::from_rgb(214, 184, 90);
    let wheel = Rect::from_center_size(c + vec2(0., -8.), vec2(8., 16.));
    painter.rect(
        wheel,
        4,
        Color32::from_rgb(120, 104, 50),
        Stroke::new(1.2, gold),
        egui::StrokeKind::Inside,
    );
    let curve = |from: Pos2, to: Pos2| {
        egui::epaint::CubicBezierShape::from_points_stroke(
            [
                from,
                pos2((from.x + to.x) / 2., from.y),
                pos2((from.x + to.x) / 2., to.y),
                to,
            ],
            false,
            Color32::TRANSPARENT,
            Stroke::new(1.2, gold),
        )
    };
    painter.add(curve(rect.left_top() + vec2(48., 45.), wheel.left_center()));
    let dot = c + vec2(14., -6.);
    painter.add(curve(rect.right_top() + vec2(-58., 45.), dot));
    painter.circle_filled(dot, 2.5, gold);
}

#[derive(Clone, Copy)]
enum Art {
    Pixel,
    Text,
    Average,
}

/// Fila de ayuda: ilustración a la izquierda, título y explicación a la derecha.
fn help_row(ui: &mut Ui, art: Art, title: &str, text: &str) {
    ui.add_space(6.);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 18.;
        let (rect, _) = ui.allocate_exact_size(vec2(120., 84.), Sense::hover());
        paint_art(&ui.painter_at(rect), rect, art);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.;
            ui.add_space(6.);
            ui.label(RichText::new(title).size(15.).strong());
            ui.label(RichText::new(text).size(14.).color(Tokens::DIM));
        });
    });
    ui.add_space(6.);
}

/// Ilustración de la fila (`assets/illustrations/color-panel`, a 3× de su hueco), cargada una
/// vez por ventana.
fn paint_art(painter: &Painter, rect: Rect, art: Art) {
    let (name, bytes): (&str, &[u8]) = match art {
        Art::Pixel => (
            "color-art-pixel",
            include_bytes!("../../../../assets/illustrations/color-panel/pixel@3.png"),
        ),
        Art::Text => (
            "color-art-text",
            include_bytes!("../../../../assets/illustrations/color-panel/text@3.png"),
        ),
        Art::Average => (
            "color-art-average",
            include_bytes!("../../../../assets/illustrations/color-panel/average@3.png"),
        ),
    };
    let ctx = painter.ctx();
    let id = egui::Id::new(name);
    let texture = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id));
    let texture = texture.unwrap_or_else(|| {
        let texture = image::load_from_memory(bytes).ok().map(|image| {
            let image = image.into_rgba8();
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
        texture
    });
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
            Color32::WHITE,
        );
    }
}
