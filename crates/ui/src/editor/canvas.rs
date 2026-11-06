//! Transformación del lienzo, gestos y vista previa vectorial.
use super::*;
use crate::theme::Tokens;
use egui::{PointerButton, Pos2, Rect, Stroke, TextureOptions};
use model::{Primitive, primitives};
use colorshot_core::i18n::t;
impl Editor {
    fn fit(&mut self, rect: Rect) {
        self.zoom = ((rect.size() - Vec2::splat(32.)) / self.image_size())
            .min_elem()
            .clamp(0.0001, 1.);
        self.pan = Vec2::ZERO;
        self.fitted = true;
    }
    fn image_size(&self) -> Vec2 {
        egui::vec2(
            self.doc.image.width().max(1) as f32,
            self.doc.image.height().max(1) as f32,
        )
    }
    fn zoom_at(&mut self, rect: Rect, at: Pos2, zoom: f32) {
        let next = zoom.clamp(0.0001, 64.);
        self.pan = at - rect.center() - (at - rect.center() - self.pan) * (next / self.zoom);
        self.zoom = next;
    }
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(rect);
        // Con frosted glass, el fondo translúcido de la ventana hace de base y encima solo van
        // los cuadros claros: otra capa entera lo volvería casi opaco.
        if self.glass {
            crate::theme::paint_checker(&painter, rect, Color32::from_white_alpha(10));
        } else {
            crate::theme::paint_canvas(&painter, rect);
        }
        if self.revision != self.doc.revision {
            // Recorte, tamaño o deshacer: la imagen es otra.
            self.revision = self.doc.revision;
            self.texture = None;
            self.blur_cache.clear();
            self.fitted = false;
        }
        if !self.fitted {
            self.fit(rect);
        }
        let ctx = ui.ctx().clone();
        let panning = response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle);
        ctx.input(|i| {
            let at = i.pointer.hover_pos().filter(|p| rect.contains(*p));
            if let Some(at) = at {
                let scroll = i.smooth_scroll_delta.y;
                if scroll != 0. {
                    self.zoom_at(rect, at, self.zoom * (scroll * 0.002).exp());
                }
            }
            if i.modifiers.ctrl || i.modifiers.command {
                if i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals) {
                    self.zoom_at(rect, rect.center(), self.zoom * 1.25);
                }
                if i.key_pressed(Key::Minus) {
                    self.zoom_at(rect, rect.center(), self.zoom / 1.25);
                }
                if i.key_pressed(Key::Num0) {
                    self.zoom_at(rect, rect.center(), 1.);
                }
                if i.key_pressed(Key::Num9) {
                    self.fit(rect);
                }
                // Encuadra la figura seleccionada o la zona.
                let target = self
                    .selected
                    .and_then(|i| self.doc.shapes.get(i))
                    .map(edit::bounds)
                    .or(self.crop);
                if i.key_pressed(Key::Num2)
                    && let Some(area) = target
                {
                    self.zoom = ((rect.size() - Vec2::splat(64.))
                        / area.size().max(Vec2::splat(1.)))
                    .min_elem()
                    .clamp(0.0001, 64.);
                    self.pan = (self.image_size() / 2. - area.center().to_vec2()) * self.zoom;
                }
            }
            if panning {
                self.pan += i.pointer.delta();
            }
        });
        let image_rect =
            Rect::from_center_size(rect.center() + self.pan, self.image_size() * self.zoom);
        let options = if self.zoom > 1. {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };
        if self.texture_nearest != (self.zoom > 1.) {
            self.texture = None;
        }
        self.texture_nearest = self.zoom > 1.;
        if self.texture.is_none() {
            let max = ctx.input(|i| i.max_texture_side).max(1) as u32;
            let shown = image::imageops::thumbnail(
                &self.doc.image,
                max.min(self.doc.image.width()).max(1),
                max.min(self.doc.image.height()).max(1),
            );
            self.texture = Some(ctx.load_texture(
                "editor-image",
                egui::ColorImage::from_rgba_unmultiplied(
                    [shown.width() as usize, shown.height() as usize],
                    shown.as_raw(),
                ),
                options,
            ));
        }
        if let Some(texture) = &mut self.texture {
            painter.image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        let hover = ctx.pointer_hover_pos();
        let pixel = hover
            .filter(|p| rect.contains(*p) && image_rect.contains(*p))
            .map(|p| Pos2::ZERO + (p - image_rect.min) / self.zoom);
        // Sobre un menú o panel que tapa la imagen, el cuentagotas se queda con el último color.
        let covered = hover
            .and_then(|p| ctx.layer_id_at(p))
            .is_some_and(|layer| layer != painter.layer_id());
        if !covered {
            self.hover_pixel = pixel
                .map(|p| (p.x.floor() as u32, p.y.floor() as u32))
                .filter(|(x, y)| *x < self.doc.image.width() && *y < self.doc.image.height());
        }
        if self.selected.is_some_and(|i| i >= self.doc.shapes.len()) {
            self.selected = None;
        }
        if !self.crops() {
            self.crop = None;
        }
        // Posición del puntero en píxeles de imagen, también fuera de la imagen.
        let zoom = self.zoom;
        let to_image = |p: Pos2| Pos2::ZERO + (p - image_rect.min) / zoom;
        let at = ctx.pointer_hover_pos().map(to_image);
        let tolerance = 6. / zoom;
        if response.hovered() {
            let over = at.and_then(|at| self.grab_at(at, tolerance));
            ctx.set_cursor_icon(match over {
                Some(Grab {
                    handle: Some(_), ..
                }) => egui::CursorIcon::Crosshair,
                Some(_) => egui::CursorIcon::Move,
                None if self.tool == Tool::Select => egui::CursorIcon::Default,
                None => egui::CursorIcon::Crosshair,
            });
        }
        let pressed = ctx.input(|i| i.pointer.primary_pressed());
        let grabbed = if pressed && response.contains_pointer() {
            at.and_then(|at| self.grab_at(at, tolerance))
        } else {
            None
        };
        if let Some(grab) = grabbed {
            if let Target::Shape(index) = grab.target {
                self.commit_text();
                self.selected = Some(index);
                // Al seleccionar, la paleta muestra el estilo de la figura.
                match self.doc.shapes.get_mut(index) {
                    Some(Shape::Blur { erase, .. }) => self.erase = *erase,
                    Some(Shape::Spotlight { oval, .. }) => self.oval_spotlight = *oval,
                    Some(shape) => {
                        if let Some(style) = edit::style_mut(shape) {
                            self.style = *style;
                        }
                    }
                    None => {}
                }
            }
            self.grab = Some(grab);
        } else if pressed
            && response.contains_pointer()
            && let Some(p) = pixel
        {
            self.selected = None;
            if self.tool == Tool::Text {
                self.commit_text();
                self.text_edit = Some((p, String::new()));
            } else if self.tool == Tool::Counter {
                self.doc.push(Shape::Counter {
                    at: p,
                    n: self.doc.next_counter(),
                    style: self.style,
                });
                self.selected = Some(self.doc.shapes.len() - 1);
            } else {
                self.crop = None;
                self.anchor = Some(p);
                self.drawing = Some(self.make_shape(p, p));
            }
        }
        if let Some(start) = self.anchor {
            if let Some(pointer) = ctx.pointer_hover_pos() {
                let mut p = Pos2::ZERO + (pointer - image_rect.min) / self.zoom;
                p.x = p.x.clamp(0., self.image_size().x);
                p.y = p.y.clamp(0., self.image_size().y);
                if ctx.input(|i| i.modifiers.shift) {
                    p = constrain(start, p, matches!(self.tool, Tool::Line | Tool::Arrow));
                }
                if let Some(Shape::Freehand { points, .. } | Shape::Highlighter { points, .. }) =
                    &mut self.drawing
                {
                    if points.last().is_none_or(|last| last.distance(p) >= 0.5) {
                        points.push(p);
                    }
                } else {
                    self.drawing = Some(self.make_shape(start, p));
                }
            }
            if ctx.input(|i| i.pointer.primary_released()) {
                match self.drawing.take() {
                    // El recorte se ve como un spotlight y queda ajustable hasta Enter.
                    Some(Shape::Spotlight { rect: zone, .. }) if self.crops() => {
                        if zone.width() >= 2. && zone.height() >= 2. {
                            self.crop = Some(zone);
                        }
                    }
                    // Un clic sin arrastrar no deja figuras vacías.
                    Some(shape) if edit::bounds(&shape).size().max_elem() < 1. => {}
                    Some(shape) => {
                        self.doc.push(shape);
                        self.selected = Some(self.doc.shapes.len() - 1);
                    }
                    None => {}
                }
                self.anchor = None;
            }
        }
        if let Some(mut grab) = self.grab.take() {
            if let Some(at) = ctx.pointer_latest_pos().map(to_image) {
                let d = at - grab.last;
                grab.last = at;
                let apply = |shape: &mut Shape| match grab.handle {
                    Some(handle) => edit::set_handle(shape, handle, at),
                    None => edit::translate(shape, d),
                };
                match grab.target {
                    Target::Shape(index) => {
                        if let Some(shape) = self.doc.shapes.get_mut(index) {
                            apply(shape);
                        }
                    }
                    Target::Crop => {
                        if let Some(zone) = &mut self.crop {
                            let mut shape = Shape::Spotlight {
                                rect: *zone,
                                oval: false,
                            };
                            apply(&mut shape);
                            *zone = edit::bounds(&shape);
                        }
                    }
                }
            }
            if ctx.input(|i| i.pointer.primary_released()) {
                if self.doc.shapes != grab.before {
                    self.doc.record(grab.before);
                }
            } else {
                self.grab = Some(grab);
            }
        }
        // Doble clic en un texto lo vuelve a abrir para escribir.
        if response.double_clicked()
            && let Some(index) = self.selected
            && let Some(Shape::Text { at, text, style }) = self.doc.shapes.get(index).cloned()
        {
            let before = self.doc.shapes.clone();
            self.doc.shapes.remove(index);
            self.doc.record(before);
            self.selected = None;
            self.style = style;
            self.text_edit = Some((at, text));
        }
        let outer = painter.clone();
        let painter = painter.with_clip_rect(rect.intersect(image_rect));
        let shapes: Vec<Shape> = self
            .doc
            .shapes
            .iter()
            .chain(self.drawing.iter())
            .cloned()
            .collect();
        for shape in &shapes {
            if let Shape::Blur {
                rect: zone,
                erase: false,
            } = shape
            {
                self.paint_blur(&painter, *zone, image_rect.min);
            }
            paint_shape(&painter, shape, image_rect, self.zoom);
            if let Shape::Blur {
                rect: zone,
                erase: true,
            } = shape
            {
                // Como al exportar: el color medio del borde de la zona.
                let color = model::border_color(&self.doc.image, *zone);
                painter.rect_filled(
                    Rect::from_min_max(
                        image_rect.min + zone.min.to_vec2() * self.zoom,
                        image_rect.min + zone.max.to_vec2() * self.zoom,
                    ),
                    0.,
                    color,
                );
            }
        }
        let screen = |p: Pos2| image_rect.min + p.to_vec2() * zoom;
        // Selección: velo claro, borde fino y tamaño con la pista debajo.
        let zone = self.crop.or(match &self.drawing {
            Some(Shape::Spotlight { rect, .. }) if self.crops() => Some(*rect),
            _ => None,
        });
        // Contraste entre los dos colores de la zona, en una píldora encima, como «C: 2.7».
        if let Some(zone) = self.crop
            && let Some(ratio) = self.zone_contrast(zone)
        {
            let top = egui::pos2(
                (screen(zone.min).x + screen(zone.max).x) / 2.,
                screen(zone.min).y,
            );
            let galley = outer.layout_no_wrap(
                format!("C: {ratio:.1}"),
                egui::FontId::proportional(11.),
                Tokens::TEXT,
            );
            let pill = Rect::from_center_size(
                top - egui::vec2(0., 8. + galley.size().y / 2.),
                galley.size() + egui::vec2(10., 4.),
            );
            outer.rect_filled(pill, 4., Color32::from_black_alpha(170));
            outer.galley(pill.center() - galley.size() / 2., galley, Tokens::TEXT);
        }
        if let Some(zone) = zone {
            let on_screen = Rect::from_min_max(screen(zone.min), screen(zone.max));
            outer.rect_filled(on_screen, 0., Color32::from_white_alpha(18));
            outer.rect_stroke(
                on_screen,
                0.,
                Stroke::new(1., Color32::from_white_alpha(200)),
                egui::StrokeKind::Outside,
            );
            let size = format!("{}×{}px", zone.width().round(), zone.height().round());
            let mut y = on_screen.bottom() + 8.;
            let hint = self.crop.is_some().then_some((t("Enter to crop"), false));
            for (text, pill) in std::iter::once((size.as_str(), true)).chain(hint) {
                let galley = outer.layout_no_wrap(
                    text.into(),
                    egui::FontId::proportional(11.),
                    Tokens::TEXT,
                );
                let at = Rect::from_center_size(
                    egui::pos2(on_screen.center().x, y + galley.size().y / 2.),
                    galley.size() + egui::vec2(10., 4.),
                );
                if pill {
                    outer.rect_filled(at, 4., Color32::from_black_alpha(170));
                }
                outer.galley(at.center() - galley.size() / 2., galley, Tokens::TEXT);
                y = at.bottom() + 2.;
            }
        }
        if let Some(shape) = self.selected.and_then(|i| self.doc.shapes.get(i)) {
            let handles = edit::handles(shape);
            if handles.is_empty() || matches!(shape, Shape::Highlighter { .. } | Shape::Blur { .. })
            {
                let bounds = edit::bounds(shape);
                outer.rect_stroke(
                    Rect::from_min_max(screen(bounds.min), screen(bounds.max)).expand(4.),
                    3.,
                    Stroke::new(1., Tokens::SELECTION),
                    egui::StrokeKind::Outside,
                );
            }
            paint_handles(&outer, &handles, screen);
        }
        self.text_box(ui, image_rect);
        if let Some(scan) = &self.scan {
            let now = std::time::Instant::now();
            let clip = outer.with_clip_rect(rect.intersect(image_rect));
            if scan.paint(&clip, image_rect, Tokens::SELECTION, now) {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(8));
            } else {
                self.scan = None;
            }
        }
    }
    /// Qué se agarraría en `at`: tiradores del recorte o de la figura seleccionada, el
    /// recorte entero o la figura más alta bajo el puntero.
    fn grab_at(&self, at: Pos2, tolerance: f32) -> Option<Grab> {
        let grab = |target, handle| {
            Some(Grab {
                target,
                handle,
                last: at,
                before: self.doc.shapes.clone(),
            })
        };
        let near = |handles: Vec<Pos2>| {
            handles
                .iter()
                .position(|h| h.distance(at) <= tolerance * 1.5)
        };
        if let Some(zone) = self.crop {
            if let Some(handle) = near(edit::handles(&Shape::Spotlight {
                rect: zone,
                oval: false,
            })) {
                return grab(Target::Crop, Some(handle));
            }
            if zone.contains(at) {
                return grab(Target::Crop, None);
            }
        }
        if let Some(index) = self.selected
            && let Some(shape) = self.doc.shapes.get(index)
        {
            if let Some(handle) = near(edit::handles(shape)) {
                return grab(Target::Shape(index), Some(handle));
            }
            if edit::hit(shape, at, tolerance) {
                return grab(Target::Shape(index), None);
            }
        }
        // Con el recorte se dibuja siempre una zona nueva.
        if self.tool == Tool::Crop {
            return None;
        }
        let index = self
            .doc
            .shapes
            .iter()
            .rposition(|shape| edit::hit(shape, at, tolerance))?;
        grab(Target::Shape(index), None)
    }
    /// Arrastrar en vacío marca una zona de recorte: Crop y Select.
    pub(super) fn crops(&self) -> bool {
        matches!(self.tool, Tool::Crop | Tool::Select)
    }
    fn paint_blur(&mut self, painter: &egui::Painter, zone: Rect, origin: Pos2) {
        let key = [zone.min.x, zone.min.y, zone.max.x, zone.max.y].map(f32::to_bits);
        let image = &self.doc.image;
        let blocks = self
            .blur_cache
            .entry(key)
            .or_insert_with(|| model::block_colors(image, zone));
        for (block, color) in blocks.iter() {
            painter.rect_filled(
                Rect::from_min_max(
                    origin + block.min.to_vec2() * self.zoom,
                    origin + block.max.to_vec2() * self.zoom,
                ),
                0.,
                *color,
            );
        }
    }
    // Cuadro de texto en el sitio: Enter confirma, Shift+Enter salta de línea, Esc cancela.
    fn text_box(&mut self, ui: &mut egui::Ui, image_rect: Rect) {
        let Some((at, mut text)) = self.text_edit.take() else {
            return;
        };
        let size = model::text_size(self.style) * self.zoom;
        let pos = image_rect.min + at.to_vec2() * self.zoom;
        let id = egui::Id::new("editor-text");
        let width = (model::text_width(&text, model::text_size(self.style)) * self.zoom)
            .max(size * 4.)
            + size;
        let mut keep = true;
        egui::Area::new(id.with("area"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .id(id)
                        .font(egui::FontId::proportional(size))
                        .text_color(self.style.color)
                        .frame(egui::Frame::NONE)
                        .desired_rows(1)
                        .desired_width(width)
                        .hint_text(t("Text")),
                );
                response.request_focus();
                ui.painter().rect_stroke(
                    response.rect.expand(2.),
                    2.,
                    Stroke::new(1., Tokens::SELECTION),
                    egui::StrokeKind::Outside,
                );
                let (enter, shift, escape) = ui.input(|i| {
                    (
                        i.key_pressed(Key::Enter),
                        i.modifiers.shift,
                        i.key_pressed(Key::Escape),
                    )
                });
                if escape {
                    keep = false;
                    text.clear();
                } else if enter && !shift {
                    // El salto que insertó el Enter no forma parte del texto.
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    keep = false;
                }
            });
        self.text_edit = Some((at, text));
        if !keep {
            self.commit_text();
        }
    }
    pub(super) fn commit_text(&mut self) {
        if let Some((at, text)) = self.text_edit.take()
            && !text.trim().is_empty()
        {
            self.doc.push(Shape::Text {
                at,
                text,
                style: self.style,
            });
        }
    }
    fn make_shape(&self, from: Pos2, to: Pos2) -> Shape {
        let style = self.style;
        let rect = Rect::from_two_pos(from, to);
        match self.tool {
            Tool::Arrow => Shape::Arrow { from, to, style },
            Tool::Line => Shape::Line { from, to, style },
            Tool::Rectangle => Shape::Rect { rect, style },
            Tool::Oval => Shape::Oval { rect, style },
            Tool::Highlighter => Shape::Highlighter {
                points: vec![from, to],
                style,
            },
            Tool::Blur => Shape::Blur {
                rect,
                erase: self.erase,
            },
            Tool::Spotlight => Shape::Spotlight {
                rect,
                oval: self.oval_spotlight,
            },
            Tool::Crop | Tool::Select => Shape::Spotlight { rect, oval: false },
            _ => Shape::Freehand {
                points: vec![from, to],
                style,
            },
        }
    }
}
fn paint_handles(painter: &egui::Painter, handles: &[Pos2], screen: impl Fn(Pos2) -> Pos2) {
    for handle in handles {
        painter.circle(
            screen(*handle),
            5.,
            Color32::WHITE,
            Stroke::new(1.5, Tokens::SELECTION),
        );
    }
}
fn constrain(start: Pos2, end: Pos2, angle: bool) -> Pos2 {
    let d = end - start;
    if angle {
        let a = (d.angle() / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
        start + Vec2::angled(a) * d.length()
    } else {
        let side = d.abs().max_elem();
        start + egui::vec2(d.x.signum() * side, d.y.signum() * side)
    }
}
/// Trazo del resaltador como una sola malla: tira a lo largo de los puntos con uniones en
/// inglete limitado y remates redondos que comparten sus vértices, todo con el borde suavizado
/// de 1 px. Al ser una pieza, el color no se acumula en las uniones ni en los extremos.
fn marker_mesh(points: &[Pos2], radius: f32, color: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let n = points.len();
    let segment = |i: usize| (points[i + 1] - points[i]).normalized().rot90();
    let (inside, outside) = ((radius - 0.5).max(0.), radius + 0.5);
    // Por punto: borde exterior izquierdo, interior izquierdo, interior derecho, exterior
    // derecho.
    for (i, &p) in points.iter().enumerate() {
        let normal = match i {
            0 => segment(0),
            _ if i == n - 1 => segment(n - 2),
            _ => {
                let (a, b) = (segment(i - 1), segment(i));
                let mid = (a + b).normalized();
                // Inglete con tope: en un giro cerrado no se dispara.
                mid / mid.dot(a).max(0.5)
            }
        };
        mesh.colored_vertex(p + normal * outside, Color32::TRANSPARENT);
        mesh.colored_vertex(p + normal * inside, color);
        mesh.colored_vertex(p - normal * inside, color);
        mesh.colored_vertex(p - normal * outside, Color32::TRANSPARENT);
    }
    for i in 0..n as u32 - 1 {
        let (a, b) = (4 * i, 4 * (i + 1));
        for k in 0..3 {
            mesh.add_triangle(a + k, a + k + 1, b + k + 1);
            mesh.add_triangle(a + k, b + k + 1, b + k);
        }
    }
    // Remates: medio disco del lado izquierdo al derecho de cada extremo, girando hacia fuera
    // (hacia atrás en el inicio, hacia delante en el final).
    let steps = ((radius * 0.8).ceil() as u32).clamp(6, 48);
    for (at, first, normal, turn) in [
        (points[0], 0u32, segment(0), -1.),
        (points[n - 1], 4 * (n as u32 - 1), segment(n - 2), 1.),
    ] {
        let center = mesh.vertices.len() as u32;
        mesh.colored_vertex(at, color);
        let (mut inner, mut outer) = (first + 1, first);
        let base = normal.angle();
        for i in 1..=steps {
            let a = base + turn * std::f32::consts::PI * i as f32 / steps as f32;
            let dir = Vec2::angled(a);
            let (next_inner, next_outer) = if i == steps {
                (first + 2, first + 3)
            } else {
                let next = mesh.vertices.len() as u32;
                mesh.colored_vertex(at + dir * inside, color);
                mesh.colored_vertex(at + dir * outside, Color32::TRANSPARENT);
                (next, next + 1)
            };
            mesh.add_triangle(center, inner, next_inner);
            mesh.add_triangle(inner, outer, next_outer);
            mesh.add_triangle(inner, next_outer, next_inner);
            (inner, outer) = (next_inner, next_outer);
        }
    }
    mesh
}
fn paint_shape(painter: &egui::Painter, shape: &Shape, image_rect: Rect, zoom: f32) {
    let origin = image_rect.min;
    let screen = |p: Pos2| origin + p.to_vec2() * zoom;
    for primitive in primitives(shape) {
        match primitive {
            Primitive::Path(points, style) => {
                let radius = style.width * zoom / 2.;
                for p in &points {
                    painter.circle_filled(screen(*p), radius, style.color);
                }
                if points.len() > 1 {
                    painter.add(egui::Shape::line(
                        points.into_iter().map(screen).collect(),
                        Stroke::new(style.width * zoom, style.color),
                    ));
                }
            }
            Primitive::Dot(at, radius, color) => {
                painter.circle_filled(screen(at), radius * zoom, color);
            }
            Primitive::Polygon(points, color) => {
                painter.add(egui::Shape::convex_polygon(
                    points.into_iter().map(screen).collect(),
                    color,
                    Stroke::NONE,
                ));
            }
            // Una sola malla: si se pintaran círculos en los puntos, el color se acumularía
            // donde se solapan.
            Primitive::Marker(points, width, color) => {
                let mut points: Vec<Pos2> = points.into_iter().map(screen).collect();
                points.dedup();
                let radius = width * zoom / 2.;
                if points.len() == 1 {
                    painter.circle_filled(points[0], radius, color);
                } else {
                    painter.add(marker_mesh(&points, radius, color));
                }
            }
            Primitive::Text(at, text, size, color) => {
                painter.text(
                    screen(at),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(size * zoom),
                    color,
                );
            }
            Primitive::Fill(rect, color) => {
                painter.rect_filled(
                    Rect::from_min_max(screen(rect.min), screen(rect.max)),
                    0.,
                    color,
                );
            }
            // El pixelado se pinta antes, desde la caché del editor, y Erase después.
            Primitive::Blur(_) | Primitive::Erase(_) => {}
            Primitive::Spotlight(zone, true) => {
                let zone = Rect::from_min_max(screen(zone.min), screen(zone.max));
                painter.add(oval_shade(image_rect, zone));
            }
            Primitive::Spotlight(zone, false) => {
                let zone = Rect::from_min_max(screen(zone.min), screen(zone.max));
                let shade = Color32::from_black_alpha(153);
                for outside in [
                    Rect::from_min_max(image_rect.min, egui::pos2(image_rect.right(), zone.top())),
                    Rect::from_min_max(
                        egui::pos2(image_rect.left(), zone.bottom()),
                        image_rect.max,
                    ),
                    Rect::from_min_max(
                        egui::pos2(image_rect.left(), zone.top()),
                        zone.left_bottom(),
                    ),
                    Rect::from_min_max(
                        zone.right_top(),
                        egui::pos2(image_rect.right(), zone.bottom()),
                    ),
                ] {
                    painter.rect_filled(outside, 0., shade);
                }
            }
            Primitive::Counter(at, n, style) => {
                let radius = (style.width * 3.).max(12.) * zoom;
                painter.circle_filled(screen(at), radius, style.color);
                painter.text(
                    screen(at),
                    egui::Align2::CENTER_CENTER,
                    n.to_string(),
                    egui::FontId::proportional(radius * 1.2),
                    Color32::WHITE,
                );
            }
        }
    }
}
/// Sombra fuera de la elipse inscrita en `zone`: una franja de triángulos entre la elipse y
/// el borde de la imagen, por rayos desde el centro (las esquinas van como rayos propios).
fn oval_shade(image_rect: Rect, zone: Rect) -> egui::Shape {
    let shade = Color32::from_black_alpha(153);
    let c = zone.center();
    let r = (zone.size() / 2.).max(Vec2::splat(0.5));
    let mut angles: Vec<f32> = (0..128)
        .map(|i| i as f32 / 128. * std::f32::consts::TAU)
        .chain(
            [
                image_rect.left_top(),
                image_rect.right_top(),
                image_rect.right_bottom(),
                image_rect.left_bottom(),
            ]
            .map(|corner| (corner - c).angle().rem_euclid(std::f32::consts::TAU)),
        )
        .collect();
    angles.sort_by(f32::total_cmp);
    let mut mesh = egui::Mesh::default();
    for a in &angles {
        let dir = Vec2::angled(*a);
        let inner = c + dir / ((dir.x / r.x).powi(2) + (dir.y / r.y).powi(2)).sqrt();
        // Distancia por el rayo hasta el lado más cercano de la imagen (o más allá si el
        // centro está fuera: entonces basta con un rayo largo).
        let reach = |d: f32, lo: f32, hi: f32, p: f32| {
            if d > 0. {
                (hi - p) / d
            } else if d < 0. {
                (lo - p) / d
            } else {
                f32::INFINITY
            }
        };
        let t = reach(dir.x, image_rect.left(), image_rect.right(), c.x)
            .min(reach(dir.y, image_rect.top(), image_rect.bottom(), c.y))
            .max(0.);
        let outer = c + dir * t;
        // Si la elipse sobresale de la imagen, el borde manda.
        let inner = if (inner - c).length() > t {
            outer
        } else {
            inner
        };
        mesh.colored_vertex(inner, shade);
        mesh.colored_vertex(outer, shade);
    }
    let n = angles.len() as u32;
    for i in 0..n {
        let (a, b) = (2 * i, 2 * ((i + 1) % n));
        mesh.add_triangle(a, a + 1, b + 1);
        mesh.add_triangle(a, b + 1, b);
    }
    egui::Shape::mesh(mesh)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square_and_angle_constraints() {
        assert_eq!(
            constrain(Pos2::ZERO, egui::pos2(-10., 4.), false),
            egui::pos2(-10., 10.)
        );
        let p = constrain(Pos2::ZERO, egui::pos2(10., 1.), true);
        assert!(p.y.abs() < 0.001);
    }
    #[test]
    fn cursor_stays_on_same_image_pixel_when_zooming() {
        let mut editor = Editor::new(RgbaImage::new(400, 200));
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
        let at = egui::pos2(100., 120.);
        let before = (at - rect.center() - editor.pan) / editor.zoom;
        editor.zoom_at(rect, at, 2.);
        assert_eq!((at - rect.center() - editor.pan) / editor.zoom, before);
    }
    #[test]
    fn marker_caps_round_outwards() {
        let points = [
            Pos2::new(10., 10.),
            Pos2::new(20., 10.),
            Pos2::new(30., 10.),
        ];
        let mesh = marker_mesh(&points, 5., Color32::RED);
        let xs = || mesh.vertices.iter().map(|v| v.pos.x);
        // Los remates pasan de los extremos hasta el radio (más el borde suave), sin doblarse.
        assert!((xs().fold(f32::MAX, f32::min) - 4.5).abs() < 0.01);
        assert!((xs().fold(f32::MIN, f32::max) - 35.5).abs() < 0.01);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| (v.pos.y - 10.).abs() <= 5.5 + 0.01)
        );
        assert!(
            mesh.indices
                .iter()
                .all(|i| (*i as usize) < mesh.vertices.len())
        );
    }
}
