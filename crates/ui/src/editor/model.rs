//! Documento, historial y rasterización en píxeles originales.
use egui::{Color32, Pos2, Rect};
use image::{Rgba, RgbaImage};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Style {
    pub color: Color32,
    pub width: f32,
    /// Punta de la flecha.
    pub head: Head,
    /// Relleno del rectángulo.
    pub fill: Fill,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(240, 80, 60),
            width: 4.,
            head: Head::Solid,
            fill: Fill::Outline,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Head {
    /// Triángulo relleno.
    Solid,
    /// Dos trazos abiertos.
    Open,
    /// Triángulo en los dos extremos.
    Double,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Fill {
    Outline,
    /// Color al 35 % dentro del borde.
    Translucent,
    Solid,
}
/// Opacidad del relleno translúcido.
const TRANSLUCENT: f32 = 0.35;
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Arrow {
        from: Pos2,
        to: Pos2,
        style: Style,
    },
    Line {
        from: Pos2,
        to: Pos2,
        style: Style,
    },
    Rect {
        rect: Rect,
        style: Style,
    },
    Oval {
        rect: Rect,
        style: Style,
    },
    Freehand {
        points: Vec<Pos2>,
        style: Style,
    },
    /// Trazo libre ancho y translúcido, como un rotulador fosforito.
    Highlighter {
        points: Vec<Pos2>,
        style: Style,
    },
    Counter {
        at: Pos2,
        n: u32,
        style: Style,
    },
    // `at` es la esquina superior izquierda del texto.
    Text {
        at: Pos2,
        text: String,
        style: Style,
    },
    /// Pixelado, o con `erase` relleno del color del borde de la zona.
    Blur {
        rect: Rect,
        erase: bool,
    },
    Spotlight {
        rect: Rect,
        oval: bool,
    },
}
/// Tamaño de letra del texto según el grosor elegido.
pub fn text_size(style: Style) -> f32 {
    12. + style.width * 3.
}
/// Ancho del trazo del resaltador según el grosor elegido.
pub fn marker_width(style: Style) -> f32 {
    style.width * 4.
}
/// Opacidad del resaltador.
pub const MARKER_ALPHA: f32 = 0.4;
/// Lado de cada bloque del pixelado.
pub const BLUR_BLOCK: u32 = 12;
/// Un paso de la historia: solo las figuras, o también la imagen (recorte, tamaño).
pub enum Step {
    Shapes(Vec<Shape>),
    Image(RgbaImage, Vec<Shape>),
}
pub struct Document {
    pub image: RgbaImage,
    pub shapes: Vec<Shape>,
    pub undo: Vec<Step>,
    pub redo: Vec<Step>,
    /// Cambia cada vez que cambia `image`, para rehacer texturas y cachés.
    pub revision: u64,
}
impl Document {
    pub fn new(image: RgbaImage) -> Self {
        Self {
            image,
            shapes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        }
    }
    /// Guarda en la historia las figuras de antes de editarlas a mano.
    pub fn record(&mut self, before: Vec<Shape>) {
        self.undo.push(Step::Shapes(before));
        self.redo.clear();
    }
    pub fn push(&mut self, shape: Shape) {
        self.undo.push(Step::Shapes(self.shapes.clone()));
        self.redo.clear();
        self.shapes.push(shape);
    }
    // Aplica `step` y devuelve el paso que lo revierte.
    fn apply(&mut self, step: Step) -> Step {
        match step {
            Step::Shapes(shapes) => Step::Shapes(std::mem::replace(&mut self.shapes, shapes)),
            Step::Image(image, shapes) => {
                self.revision += 1;
                Step::Image(
                    std::mem::replace(&mut self.image, image),
                    std::mem::replace(&mut self.shapes, shapes),
                )
            }
        }
    }
    pub fn undo(&mut self) {
        if let Some(step) = self.undo.pop() {
            let back = self.apply(step);
            self.redo.push(back);
        }
    }
    pub fn redo(&mut self) {
        if let Some(step) = self.redo.pop() {
            let back = self.apply(step);
            self.undo.push(back);
        }
    }
    /// Funde las figuras en la imagen y la reemplaza por `edit(imagen fundida)`.
    fn flatten_into(&mut self, edit: impl FnOnce(&RgbaImage) -> RgbaImage) {
        let image = edit(&render(self));
        let back = self.apply(Step::Image(image, Vec::new()));
        self.undo.push(back);
        self.redo.clear();
    }
    /// Recorta a `rect` (píxeles de imagen); ignora zonas de menos de 2 px.
    pub fn crop(&mut self, rect: Rect) {
        let full = Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(self.image.width() as f32, self.image.height() as f32),
        );
        let rect = rect.intersect(full);
        let rect = Rect::from_min_max(rect.min.round(), rect.max.round());
        if rect.width() < 2. || rect.height() < 2. {
            return;
        }
        self.flatten_into(|image| {
            image::imageops::crop_imm(
                image,
                rect.min.x as u32,
                rect.min.y as u32,
                rect.width() as u32,
                rect.height() as u32,
            )
            .to_image()
        });
    }
    /// Cambia el tamaño por `factor` (0,25 … 4).
    pub fn scale(&mut self, factor: f32) {
        let w = ((self.image.width() as f32 * factor).round() as u32).max(1);
        let h = ((self.image.height() as f32 * factor).round() as u32).max(1);
        self.resize(w, h);
    }
    /// Redimensiona a `w`×`h` px con las figuras ya fundidas en la imagen.
    pub fn resize(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) == self.image.dimensions() {
            return;
        }
        let shrink = w < self.image.width();
        self.flatten_into(|image| {
            let filter = if shrink {
                image::imageops::FilterType::Lanczos3
            } else {
                // Al ampliar se conservan los píxeles nítidos.
                image::imageops::FilterType::Nearest
            };
            image::imageops::resize(image, w, h, filter)
        });
    }
    pub fn next_counter(&self) -> u32 {
        self.shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Counter { n, .. } => Some(*n),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }
}

// Geometría común al lienzo y a la exportación para conservar las proporciones.
pub(super) enum Primitive {
    Path(Vec<Pos2>, Style),
    /// Polígono convexo relleno.
    Polygon(Vec<Pos2>, Color32),
    /// Trazo translúcido: cada píxel se tiñe una sola vez aunque el trazo se cruce.
    Marker(Vec<Pos2>, f32, Color32),
    Counter(Pos2, u32, Style),
    /// Círculo relleno de radio dado.
    Dot(Pos2, f32, Color32),
    Text(Pos2, String, f32, Color32),
    Fill(Rect, Color32),
    Blur(Rect),
    Erase(Rect),
    Spotlight(Rect, bool),
}
pub(super) fn primitives(shape: &Shape) -> Vec<Primitive> {
    match shape {
        Shape::Line { from, to, style } => vec![Primitive::Path(vec![*from, *to], *style)],
        Shape::Arrow { from, to, style } => arrow(*from, *to, *style),
        Shape::Rect { rect, style } => {
            let outline = Primitive::Path(
                vec![
                    rect.left_top(),
                    rect.right_top(),
                    rect.right_bottom(),
                    rect.left_bottom(),
                    rect.left_top(),
                ],
                *style,
            );
            match style.fill {
                Fill::Outline => vec![outline],
                Fill::Translucent => vec![
                    Primitive::Fill(*rect, style.color.gamma_multiply(TRANSLUCENT)),
                    outline,
                ],
                Fill::Solid => vec![Primitive::Fill(*rect, style.color), outline],
            }
        }
        Shape::Oval { rect, style } => {
            let count = (rect.width().max(rect.height()) * std::f32::consts::PI)
                .ceil()
                .clamp(32., 8192.) as usize;
            let points = (0..=count)
                .map(|i| {
                    let a = i as f32 / count as f32 * std::f32::consts::TAU;
                    rect.center()
                        + egui::vec2(a.cos() * rect.width() / 2., a.sin() * rect.height() / 2.)
                })
                .collect();
            vec![Primitive::Path(points, *style)]
        }
        Shape::Freehand { points, style } => vec![Primitive::Path(points.clone(), *style)],
        Shape::Highlighter { points, style } => vec![Primitive::Marker(
            smooth(points, marker_width(*style) / 4.),
            marker_width(*style),
            style.color.gamma_multiply(MARKER_ALPHA),
        )],
        Shape::Counter { at, n, style } => vec![Primitive::Counter(*at, *n, *style)],
        Shape::Text { at, text, style } => vec![Primitive::Text(
            *at,
            text.clone(),
            text_size(*style),
            style.color,
        )],
        Shape::Blur { rect, erase: false } => vec![Primitive::Blur(*rect)],
        Shape::Blur { rect, erase: true } => vec![Primitive::Erase(*rect)],
        Shape::Spotlight { rect, oval } => vec![Primitive::Spotlight(*rect, *oval)],
    }
}
/// Flecha moderna: el cuerpo se ensancha desde un inicio fino y redondo hasta la punta, que
/// tiene las esquinas redondeadas.
fn arrow(from: Pos2, to: Pos2, style: Style) -> Vec<Primitive> {
    let w = style.width.max(1.);
    let length = from.distance(to);
    let dir = (to - from).normalized();
    let normal = egui::vec2(-dir.y, dir.x);
    // En flechas cortas la punta se encoge para no comerse el cuerpo.
    let head = (w * 4.5).max(14.).min(length * 0.6);
    let half = head * 0.5;
    let base = to - dir * head;
    let shaft = |a: Pos2, b: Pos2, start: f32, end: f32| {
        Primitive::Polygon(
            vec![
                a + normal * start,
                b + normal * end,
                b - normal * end,
                a - normal * start,
            ],
            style.color,
        )
    };
    // Punta de dardo: dos mitades convexas que se cruzan un poco en el eje para que no quede
    // una costura entre ellas; la parte de atrás se hunde hacia la punta.
    let tip = |at: Pos2, back: Pos2| {
        let d = (at - back).normalized();
        let n = egui::vec2(-d.y, d.x);
        let notch = back + d * head * 0.28;
        let overlap = n * (w * 0.2).max(0.8);
        [1., -1.].map(|side| {
            Primitive::Polygon(
                rounded(
                    &[at, back + n * half * side, notch - overlap * side],
                    &[w * 0.3, w * 0.6, 0.],
                ),
                style.color,
            )
        })
    };
    let thin = (w * 0.22).max(0.6);
    match style.head {
        Head::Solid => vec![
            Primitive::Dot(from, thin, style.color),
            shaft(from, to - dir * head * 0.5, thin, w * 0.5),
        ]
        .into_iter()
        .chain(tip(to, base))
        .collect(),
        Head::Open => vec![
            Primitive::Dot(from, thin, style.color),
            shaft(from, to - dir * w * 0.3, thin, w * 0.5),
            Primitive::Path(vec![base + normal * half, to, base - normal * half], style),
        ],
        Head::Double => {
            let tail = from + dir * head;
            let mut parts = vec![shaft(
                from + dir * head * 0.5,
                to - dir * head * 0.5,
                w * 0.5,
                w * 0.5,
            )];
            parts.extend(tip(to, base));
            parts.extend(tip(from, tail));
            parts
        }
    }
}
/// Polígono convexo con cada esquina cortada por una curva de radio aproximado `radius[i]`.
fn rounded(points: &[Pos2], radius: &[f32]) -> Vec<Pos2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n * 7);
    for i in 0..n {
        let (prev, at, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let cut = radius[i]
            .min(at.distance(prev) * 0.45)
            .min(at.distance(next) * 0.45);
        let a = at + (prev - at).normalized() * cut;
        let b = at + (next - at).normalized() * cut;
        for k in 0..=6 {
            let t = k as f32 / 6.;
            // Bézier cuadrática con el vértice como control.
            out.push(a.lerp(at, t).lerp(at.lerp(b, t), t));
        }
    }
    out
}
/// Trazo a mano sin temblores: descarta puntos a menos de `spacing` y redondea las esquinas
/// (Chaikin, dos pasadas) conservando los extremos. Con los puntos crudos del ratón, un trazo
/// grueso dobla sus uniones y queda con bultos.
pub fn smooth(points: &[Pos2], spacing: f32) -> Vec<Pos2> {
    let spacing = spacing.max(1.5);
    let mut kept: Vec<Pos2> = Vec::with_capacity(points.len());
    for &p in points {
        if kept.last().is_none_or(|last| last.distance(p) >= spacing) {
            kept.push(p);
        }
    }
    if let (Some(&last), Some(&end)) = (kept.last(), points.last())
        && last != end
    {
        kept.push(end);
    }
    for _ in 0..2 {
        if kept.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(kept.len() * 2);
        next.push(kept[0]);
        for w in kept.windows(2) {
            next.push(w[0].lerp(w[1], 0.25));
            next.push(w[0].lerp(w[1], 0.75));
        }
        next.push(kept[kept.len() - 1]);
        kept = next;
    }
    kept
}
fn fill(image: &mut RgbaImage, rect: Rect, mut color: impl FnMut(Pos2, &mut Rgba<u8>)) {
    for y in (rect.top().floor().max(0.) as u32)
        ..(rect.bottom().ceil().max(0.) as u32).min(image.height())
    {
        for x in (rect.left().floor().max(0.) as u32)
            ..(rect.right().ceil().max(0.) as u32).min(image.width())
        {
            color(
                egui::pos2(x as f32 + 0.5, y as f32 + 0.5),
                image.get_pixel_mut(x, y),
            );
        }
    }
}
/// Cobertura (0–1) de cada píxel en un recuadro, para pintar con bordes suavizados. Se guarda
/// el máximo de las piezas, así los solapes de un mismo trazo no se oscurecen.
struct Coverage {
    x: i64,
    y: i64,
    w: usize,
    h: usize,
    data: Vec<f32>,
}
impl Coverage {
    fn new(image: &RgbaImage, area: Rect) -> Self {
        let x = (area.left().floor() as i64).max(0);
        let y = (area.top().floor() as i64).max(0);
        let right = (area.right().ceil() as i64).clamp(x, image.width() as i64);
        let bottom = (area.bottom().ceil() as i64).clamp(y, image.height() as i64);
        let (w, h) = ((right - x) as usize, (bottom - y) as usize);
        Self {
            x,
            y,
            w,
            h,
            data: vec![0.; w * h],
        }
    }
    /// Recorre los píxeles de `rect` con su centro; `cover` devuelve la cobertura.
    fn add(&mut self, rect: Rect, cover: impl Fn(Pos2) -> f32) {
        let x0 = (rect.left().floor() as i64 - self.x).max(0) as usize;
        let y0 = (rect.top().floor() as i64 - self.y).max(0) as usize;
        let x1 = ((rect.right().ceil() as i64 - self.x).max(0) as usize).min(self.w);
        let y1 = ((rect.bottom().ceil() as i64 - self.y).max(0) as usize).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = egui::pos2(
                    (self.x + x as i64) as f32 + 0.5,
                    (self.y + y as i64) as f32 + 0.5,
                );
                let c = cover(p);
                let cell = &mut self.data[y * self.w + x];
                *cell = cell.max(c);
            }
        }
    }
    /// Segmento con extremos redondos de radio `r`.
    fn capsule(&mut self, a: Pos2, b: Pos2, r: f32) {
        let ab = b - a;
        self.add(Rect::from_two_pos(a, b).expand(r + 1.), |p| {
            let t = ((p - a).dot(ab) / ab.length_sq().max(f32::EPSILON)).clamp(0., 1.);
            (r + 0.5 - p.distance(a + ab * t)).clamp(0., 1.)
        });
    }
    /// Polígono convexo: la distancia es la del lado más alejado.
    fn polygon(&mut self, points: &[Pos2]) {
        if points.len() < 3 {
            return;
        }
        // Sentido de giro para que la normal de cada lado apunte hacia fuera.
        let area: f32 = (0..points.len())
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        let sign = if area >= 0. { 1. } else { -1. };
        let edges: Vec<(Pos2, egui::Vec2)> = (0..points.len())
            .filter_map(|i| {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                let d = b - a;
                (d.length() > 1e-4).then(|| (a, egui::vec2(d.y, -d.x).normalized() * sign))
            })
            .collect();
        self.add(Rect::from_points(points).expand(1.), |p| {
            let outside = edges
                .iter()
                .map(|(a, n)| (p - *a).dot(*n))
                .fold(f32::MIN, f32::max);
            (0.5 - outside).clamp(0., 1.)
        });
    }
    fn dot(&mut self, at: Pos2, r: f32) {
        self.add(
            Rect::from_center_size(at, egui::Vec2::splat(r * 2. + 2.)),
            |p| (r + 0.5 - p.distance(at)).clamp(0., 1.),
        );
    }
    /// Mezcla `color` (premultiplicado) según la cobertura; con `opaque` también cubre la
    /// transparencia de la imagen (el resaltador la conserva).
    fn paint(&self, image: &mut RgbaImage, color: Color32, opaque: bool) {
        let a = color.a() as f32 / 255.;
        let source = [color.r(), color.g(), color.b()].map(|c| c as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                let c = self.data[y * self.w + x];
                if c <= 0. {
                    continue;
                }
                let pixel =
                    image.get_pixel_mut((self.x as usize + x) as u32, (self.y as usize + y) as u32);
                for i in 0..3 {
                    pixel[i] = (pixel[i] as f32 * (1. - a * c) + source[i] * c).round() as u8;
                }
                if opaque {
                    pixel[3] = (pixel[3] as f32 + (255. - pixel[3] as f32) * a * c).round() as u8;
                }
            }
        }
    }
}
fn disk(image: &mut RgbaImage, at: Pos2, radius: f32, color: Color32) {
    let mut coverage = Coverage::new(
        image,
        Rect::from_center_size(at, egui::Vec2::splat(radius * 2. + 2.)),
    );
    coverage.dot(at, radius);
    coverage.paint(image, color, true);
}
fn path(image: &mut RgbaImage, points: &[Pos2], style: Style, opaque: bool) {
    let Some(first) = points.first() else { return };
    let r = style.width.max(1.) / 2.;
    let mut coverage = Coverage::new(image, Rect::from_points(points).expand(r + 2.));
    coverage.dot(*first, r);
    for pair in points.windows(2) {
        coverage.capsule(pair[0], pair[1], r);
    }
    coverage.paint(image, style.color, opaque);
}
fn polygon(image: &mut RgbaImage, points: &[Pos2], color: Color32) {
    let mut coverage = Coverage::new(image, Rect::from_points(points).expand(2.));
    coverage.polygon(points);
    coverage.paint(image, color, true);
}
fn marker(image: &mut RgbaImage, points: &[Pos2], width: f32, color: Color32) {
    // `path` guarda la cobertura máxima: cada píxel se tiñe una sola vez.
    path(
        image,
        points,
        Style {
            width,
            color,
            ..Style::default()
        },
        false,
    );
}
pub fn render(doc: &Document) -> RgbaImage {
    let mut image = doc.image.clone();
    for shape in &doc.shapes {
        for primitive in primitives(shape) {
            match primitive {
                Primitive::Path(points, style) => path(&mut image, &points, style, true),
                Primitive::Polygon(points, color) => polygon(&mut image, &points, color),
                Primitive::Dot(at, radius, color) => disk(&mut image, at, radius, color),
                Primitive::Marker(points, width, color) => {
                    marker(&mut image, &points, width, color)
                }
                Primitive::Counter(at, n, style) => {
                    let radius = (style.width * 3.).max(12.);
                    disk(&mut image, at, radius, style.color);
                    let size = radius * 1.2;
                    let text = n.to_string();
                    let width = text_width(&text, size);
                    draw_text(
                        &mut image,
                        at - egui::vec2(width / 2., size * 0.62),
                        &text,
                        size,
                        Color32::WHITE,
                    );
                }
                Primitive::Text(at, text, size, color) => {
                    draw_text(&mut image, at, &text, size, color);
                }
                Primitive::Fill(rect, color) => {
                    let a = color.a() as f32 / 255.;
                    // `color` viene premultiplicado.
                    let source = [color.r(), color.g(), color.b()];
                    fill(&mut image, rect, |_, pixel| {
                        for c in 0..3 {
                            pixel[c] =
                                (pixel[c] as f32 * (1. - a) + source[c] as f32).round() as u8;
                        }
                    });
                }
                Primitive::Blur(rect) => pixelate(&mut image, rect),
                Primitive::Erase(rect) => {
                    let color = border_color(&image, rect);
                    fill(&mut image, rect, |_, pixel| {
                        pixel[0] = color.r();
                        pixel[1] = color.g();
                        pixel[2] = color.b();
                    });
                }
                Primitive::Spotlight(rect, oval) => {
                    let full = Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(image.width() as f32, image.height() as f32),
                    );
                    fill(&mut image, full, |p, pixel| {
                        if !in_spotlight(rect, oval, p) {
                            for c in 0..3 {
                                pixel[c] = (pixel[c] as f32 * 0.4).round() as u8;
                            }
                        }
                    });
                }
            }
        }
    }
    image
}
fn font() -> Option<ab_glyph::FontRef<'static>> {
    ab_glyph::FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).ok()
}
/// Ancho del texto en píxeles con la misma fuente que usa egui por defecto.
pub fn text_width(text: &str, size: f32) -> f32 {
    use ab_glyph::{Font, ScaleFont};
    let Some(font) = font() else { return 0. };
    let font = font.as_scaled(size);
    text.chars().map(|c| font.h_advance(font.glyph_id(c))).sum()
}
// Rasteriza texto (multilínea) mezclando con la cobertura de cada glifo.
fn draw_text(image: &mut RgbaImage, at: Pos2, text: &str, size: f32, color: Color32) {
    use ab_glyph::{Font, ScaleFont, point};
    let Some(font) = font() else { return };
    let scaled = font.as_scaled(size);
    for (row, line) in text.lines().enumerate() {
        let mut x = at.x;
        let baseline = at.y + scaled.ascent() + row as f32 * scaled.height();
        for c in line.chars() {
            let id = scaled.glyph_id(c);
            let glyph = id.with_scale_and_position(size, point(x, baseline));
            x += scaled.h_advance(id);
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let (px, py) = (
                    bounds.min.x as i64 + gx as i64,
                    bounds.min.y as i64 + gy as i64,
                );
                if px < 0 || py < 0 || px >= image.width() as i64 || py >= image.height() as i64 {
                    return;
                }
                let pixel = image.get_pixel_mut(px as u32, py as u32);
                let a = coverage.clamp(0., 1.) * color.a() as f32 / 255.;
                for (i, target) in [color.r(), color.g(), color.b()].into_iter().enumerate() {
                    pixel[i] = (pixel[i] as f32 * (1. - a) + target as f32 * a).round() as u8;
                }
                pixel[3] = pixel[3].max((a * 255.).round() as u8);
            });
        }
    }
}
/// Promedio de cada bloque de `BLUR_BLOCK` px alineado a la imagen, recortado a `rect`.
pub fn block_colors(image: &RgbaImage, rect: Rect) -> Vec<(Rect, Color32)> {
    let b = BLUR_BLOCK;
    let x0 = (rect.left().max(0.) as u32 / b) * b;
    let y0 = (rect.top().max(0.) as u32 / b) * b;
    let x1 = (rect.right().max(0.).ceil() as u32).min(image.width());
    let y1 = (rect.bottom().max(0.).ceil() as u32).min(image.height());
    let mut blocks = Vec::new();
    for by in (y0..y1).step_by(b as usize) {
        for bx in (x0..x1).step_by(b as usize) {
            let (w, h) = (
                (bx + b).min(image.width()) - bx,
                (by + b).min(image.height()) - by,
            );
            let mut sum = [0u64; 3];
            for y in by..by + h {
                for x in bx..bx + w {
                    let p = image.get_pixel(x, y);
                    for c in 0..3 {
                        sum[c] += p[c] as u64;
                    }
                }
            }
            let n = (w * h).max(1) as u64;
            let block = Rect::from_min_size(
                egui::pos2(bx as f32, by as f32),
                egui::vec2(w as f32, h as f32),
            )
            .intersect(rect);
            blocks.push((
                block,
                Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8),
            ));
        }
    }
    blocks
}
/// Si `p` cae dentro del spotlight: el rectángulo o la elipse inscrita en él.
pub fn in_spotlight(rect: Rect, oval: bool, p: Pos2) -> bool {
    if !oval {
        return rect.contains(p);
    }
    let r = (rect.size() / 2.).max(egui::Vec2::splat(0.5));
    let d = (p - rect.center()) / r;
    d.length_sq() <= 1.
}
/// Color medio del contorno de `rect`, con el que Erase rellena la zona.
pub fn border_color(image: &RgbaImage, rect: Rect) -> Color32 {
    let (w, h) = (image.width() as i64, image.height() as i64);
    if w == 0 || h == 0 {
        return Color32::BLACK;
    }
    let clamp = |v: f32, max: i64| (v.floor() as i64).clamp(0, max - 1);
    let (x0, x1) = (clamp(rect.left(), w), clamp(rect.right() - 1., w));
    let (y0, y1) = (clamp(rect.top(), h), clamp(rect.bottom() - 1., h));
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    let mut add = |x: i64, y: i64| {
        let p = image.get_pixel(x as u32, y as u32);
        for c in 0..3 {
            sum[c] += p[c] as u64;
        }
        n += 1;
    };
    for x in x0..=x1 {
        add(x, y0);
        add(x, y1);
    }
    for y in y0..=y1 {
        add(x0, y);
        add(x1, y);
    }
    let n = n.max(1);
    Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8)
}
fn pixelate(image: &mut RgbaImage, rect: Rect) {
    for (block, color) in block_colors(image, rect) {
        fill(image, block, |_, pixel| {
            pixel[0] = color.r();
            pixel[1] = color.g();
            pixel[2] = color.b();
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn counter(n: u32) -> Shape {
        Shape::Counter {
            at: Pos2::new(10., 10.),
            n,
            style: Style::default(),
        }
    }
    #[test]
    fn history_and_counter_branch() {
        let mut doc = Document::new(RgbaImage::new(30, 30));
        doc.undo();
        doc.redo();
        assert_eq!(doc.next_counter(), 1);
        doc.push(counter(1));
        doc.push(counter(2));
        doc.undo();
        assert_eq!(doc.next_counter(), 2);
        doc.redo();
        assert_eq!(doc.next_counter(), 3);
        doc.undo();
        doc.push(counter(2));
        doc.redo();
        assert_eq!(doc.shapes, vec![counter(1), counter(2)]);
        doc.undo();
        doc.undo();
        assert!(doc.shapes.is_empty());
    }
    #[test]
    fn crop_and_scale_flatten_and_undo() {
        let mut doc = Document::new(RgbaImage::from_pixel(40, 20, Rgba([255; 4])));
        doc.push(counter(1));
        doc.crop(Rect::from_min_max(Pos2::new(5., 2.), Pos2::new(60., 12.)));
        assert_eq!(doc.image.dimensions(), (35, 10));
        assert!(doc.shapes.is_empty());
        doc.scale(2.);
        assert_eq!(doc.image.dimensions(), (70, 20));
        doc.undo();
        doc.undo();
        assert_eq!(doc.image.dimensions(), (40, 20));
        assert_eq!(doc.shapes, vec![counter(1)]);
        doc.redo();
        assert_eq!(doc.image.dimensions(), (35, 10));
        assert_eq!(doc.revision, 5);
        doc.crop(Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)));
        assert_eq!(doc.image.dimensions(), (35, 10));
    }
    #[test]
    fn line_changes_pixel_without_changing_original() {
        let mut doc = Document::new(RgbaImage::from_pixel(20, 20, Rgba([255; 4])));
        doc.push(Shape::Line {
            from: Pos2::new(2., 10.),
            to: Pos2::new(18., 10.),
            style: Style {
                color: Color32::RED,
                width: 3.,
                ..Style::default()
            },
        });
        assert_eq!(render(&doc).get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(doc.image.get_pixel(10, 10).0, [255; 4]);
    }
    #[test]
    fn smooth_drops_jitter_and_keeps_the_ends() {
        let jitter: Vec<Pos2> = (0..=40)
            .map(|i| Pos2::new(i as f32 * 0.5, if i % 2 == 0 { 0. } else { 0.4 }))
            .collect();
        let smoothed = smooth(&jitter, 4.);
        assert_eq!(smoothed.first(), Some(&Pos2::new(0., 0.)));
        assert_eq!(smoothed.last(), Some(&Pos2::new(20., 0.)));
        assert!(smoothed.len() < jitter.len());
        assert!(smoothed.windows(2).all(|w| w[1].x > w[0].x));
    }
    #[test]
    fn highlighter_tints_once_and_preserves_alpha() {
        let mut doc = Document::new(RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 128])));
        // Ida y vuelta sobre la misma línea: el color no se acumula.
        doc.push(Shape::Highlighter {
            points: vec![Pos2::new(2., 10.), Pos2::new(18., 10.), Pos2::new(2., 10.)],
            style: Style {
                color: Color32::from_rgb(250, 200, 0),
                width: 1.,
                ..Style::default()
            },
        });
        let image = render(&doc);
        assert_eq!(image.get_pixel(10, 10).0, [100, 80, 0, 128]);
        assert_eq!(image.get_pixel(10, 16).0, [0, 0, 0, 128]);
    }
    #[test]
    fn blur_averages_blocks_and_spotlight_dims_outside() {
        let mut image = RgbaImage::from_pixel(24, 24, Rgba([0, 0, 0, 255]));
        image.put_pixel(0, 0, Rgba([144, 144, 144, 255]));
        let mut doc = Document::new(image);
        doc.push(Shape::Blur {
            rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(12., 12.)),
            erase: false,
        });
        // 144 / 144 píxeles del bloque = 1.
        assert_eq!(render(&doc).get_pixel(5, 5).0, [1, 1, 1, 255]);
        let mut doc = Document::new(RgbaImage::from_pixel(10, 10, Rgba([100; 4])));
        doc.push(Shape::Spotlight {
            rect: Rect::from_min_max(Pos2::new(2., 2.), Pos2::new(6., 6.)),
            oval: false,
        });
        let out = render(&doc);
        assert_eq!(out.get_pixel(3, 3).0, [100; 4]);
        assert_eq!(out.get_pixel(8, 8).0, [40, 40, 40, 100]);
    }
    #[test]
    fn rect_fill_erase_and_oval_spotlight() {
        let mut doc = Document::new(RgbaImage::from_pixel(20, 20, Rgba([200, 200, 200, 255])));
        doc.push(Shape::Rect {
            rect: Rect::from_min_max(Pos2::new(2., 2.), Pos2::new(18., 18.)),
            style: Style {
                color: Color32::from_rgb(0, 0, 255),
                width: 1.,
                fill: Fill::Solid,
                ..Style::default()
            },
        });
        assert_eq!(render(&doc).get_pixel(10, 10).0, [0, 0, 255, 255]);
        let mut image = RgbaImage::from_pixel(10, 10, Rgba([50, 50, 50, 255]));
        image.put_pixel(5, 5, Rgba([255, 0, 0, 255]));
        let mut doc = Document::new(image);
        doc.push(Shape::Blur {
            rect: Rect::from_min_max(Pos2::new(3., 3.), Pos2::new(8., 8.)),
            erase: true,
        });
        assert_eq!(render(&doc).get_pixel(5, 5).0, [50, 50, 50, 255]);
        let zone = Rect::from_min_max(Pos2::ZERO, Pos2::new(10., 10.));
        assert!(in_spotlight(zone, true, Pos2::new(5., 5.)));
        assert!(!in_spotlight(zone, true, Pos2::new(0.5, 0.5)));
        assert!(in_spotlight(zone, false, Pos2::new(0.5, 0.5)));
    }
    #[test]
    fn text_paints_glyph_pixels() {
        let mut doc = Document::new(RgbaImage::from_pixel(80, 40, Rgba([255; 4])));
        doc.push(Shape::Text {
            at: Pos2::new(2., 2.),
            text: "HI".into(),
            style: Style {
                color: Color32::BLACK,
                width: 4.,
                ..Style::default()
            },
        });
        let out = render(&doc);
        assert!(out.pixels().any(|p| p.0[0] < 100));
        assert!(text_width("HI", 24.) > 10.);
    }
}
