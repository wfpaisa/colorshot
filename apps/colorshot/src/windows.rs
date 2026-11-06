//! Ventanas eframe: selector por monitor, miniatura y editor.
use anyhow::Context as _;
use eframe::egui::{self, ViewportBuilder, ViewportId};
use colorshot_core::i18n::{t, tf};
use colorshot_core::{
    capture::{Capture, Rect},
    clipboard, monitors, recognize, save,
    settings::{EscAction, Format, Placement, Settings, ThumbnailMonitor},
};
use colorshot_ui::{
    area_selector::{AreaSelector, SelectorResult},
    blur::Blur,
    editor::{Editor, EditorAction, model},
    frame,
    palettes::PaletteWindow,
    pin::{Pin, PinAction},
    preferences::Preferences,
    preview::{self, Preview, PreviewAction},
    tablet::Tablet,
    theme, transition,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Imagen capturada y de dónde sale: zona dentro de la captura completa y tamaño de esta.
pub type Shot = (image::RgbaImage, (Rect, (u32, u32)));

/// Captura lo elegido en un monitor; corre en otro hilo con el selector abierto.
pub type Grab = Arc<dyn Fn(usize, SelectorResult) -> anyhow::Result<Shot> + Send + Sync>;

/// Lo elegido en el selector: monitor, resultado y, si se pidió, la captura tomada al soltar.
pub type Selected = (usize, SelectorResult, Option<Shot>);

/// Corre un selector por monitor. Con `grab`, al confirmar una zona captura la pantalla con
/// las ventanas aún abiertas pero vacías: cerrarlas y esperar a que GNOME las retire tarda más.
pub fn run_selector(
    selectors: Vec<AreaSelector>,
    screens: &[monitors::Monitor],
    grab: Option<Grab>,
) -> anyhow::Result<Option<Selected>> {
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    let sizes: Vec<_> = screens
        .iter()
        .map(|m| egui::vec2(m.width as f32, m.height as f32))
        .collect();
    let multi = sizes.len() > 1;
    let transparent = selectors.first().is_some_and(AreaSelector::transparent);
    // En Wayland, Mutter pinta de negro detrás de una ventana a pantalla completa aunque sea
    // transparente; con XWayland se ve el escritorio.
    let x11 = transparent && std::env::var_os("DISPLAY").is_some();
    let places = if x11 {
        places(screens, &monitors::x11_monitors())
    } else {
        Vec::new()
    };
    let mut options = native(overlay(0, multi, transparent, places.first()));
    if x11 {
        use_x11(&mut options);
    }
    eframe::run_native(
        "colorshot",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(SelectorApp {
                tablet: Tablet::new(cc, &cc.egui_ctx),
                selectors,
                sizes,
                places,
                focused: false,
                transparent,
                output,
                grab,
                grabbing: None,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let selected = result.lock().map_err(|e| anyhow::anyhow!("{e}"))?.take();
    selected
        .map(|(monitor, result, capture)| Ok((monitor, result, capture.transpose()?)))
        .transpose()
}

/// `Selected` mientras la captura puede haber fallado.
type Pending = (usize, SelectorResult, Option<anyhow::Result<Shot>>);

/// Captura en curso tras soltar.
struct Grabbing {
    since: Instant,
    started: bool,
    done: Arc<Mutex<Option<anyhow::Result<Shot>>>>,
}

// Lo que se deja pasar para que el compositor muestre las ventanas ya vacías.
const BLANK: Duration = Duration::from_millis(40);

/// Opciones comunes: OpenGL (glow) sin vsync en todas las ventanas, sin recordar geometría.
///
/// Con wgpu (Vulkan, NVIDIA) la ventana transparente no llega a dibujarse en Wayland, y una
/// ventana glow abierta después de otra wgpu en el mismo proceso se congela. Con vsync, una
/// ventana tapada bloquea el hilo al presentar. En los tres casos GNOME la marca «Not
/// Responding». eframe guarda el tamaño y la pantalla completa por nombre de aplicación: el
/// editor heredaba la pantalla completa del selector.
fn native(viewport: ViewportBuilder) -> eframe::NativeOptions {
    // Un solo app_id para todas las ventanas: GNOME lo asocia a colorshot.desktop y su icono.
    let mut viewport = viewport.with_app_id("colorshot");
    if let Some(logo) = colorshot_ui::icons::logo() {
        viewport = viewport.with_icon(logo);
    }
    eframe::NativeOptions {
        viewport,
        persist_window: false,
        renderer: eframe::Renderer::Glow,
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Zona de la captura en la pantalla, `x,y,w,h,ancho,alto` en píxeles de la captura completa;
/// la pasa `flows::deliver` a la miniatura para elegir el monitor del puntero.
pub const FROM: &str = "COLORSHOT_FROM";
/// Sitio del editor en puntos X11 (`x,y,ancho,alto`), centrado en el monitor de la miniatura.
pub const AT: &str = "COLORSHOT_AT";
/// `1` si el proceso que capturó ya copió la imagen (ver `flows::deliver`).
pub const COPIED: &str = "COLORSHOT_COPIED";

/// Tope del editor sin monitor conocido; con monitor, el 90 % de este.
const EDITOR_MAX: egui::Vec2 = egui::vec2(1600., 1000.);
// Sin vsync, las animaciones se limitan a ~140 cuadros por segundo.
const FRAME: Duration = Duration::from_millis(7);
// Lo que se espera a que un monitor reciba el puntero (≈200 ms en GNOME) antes de usar el
// principal.
const FIND_CURSOR: Duration = Duration::from_millis(350);

/// Lee `FROM`: zona y tamaño de la captura completa.
pub fn parse_from(text: &str) -> Option<(Rect, (u32, u32))> {
    let v: Vec<u32> = text
        .split(',')
        .map(|n| n.parse().ok())
        .collect::<Option<_>>()?;
    let [x, y, w, h, fw, fh] = v[..] else {
        return None;
    };
    Some((Rect { x, y, w, h }, (fw, fh)))
}

/// Lo que la miniatura necesita de la captura.
struct Delivery {
    image: image::RgbaImage,
    from: Option<(Rect, (u32, u32))>,
    copied: bool,
    // PNG que dejó el proceso que capturó; por stdin no hay.
    temp: Option<PathBuf>,
}

/// Envía la captura a una miniatura lanzada con `preview -`: una línea `ancho alto copiada
/// zona` (zona como `FROM`, o `-`) y después los píxeles RGBA en crudo.
pub fn write_delivery(
    output: &mut impl std::io::Write,
    image: &image::RgbaImage,
    from: Option<(Rect, (u32, u32))>,
    copied: bool,
) -> std::io::Result<()> {
    let from = from.map_or("-".to_string(), |(r, full)| {
        format!("{},{},{},{},{},{}", r.x, r.y, r.w, r.h, full.0, full.1)
    });
    writeln!(
        output,
        "{} {} {} {from}",
        image.width(),
        image.height(),
        u8::from(copied)
    )?;
    output.write_all(image.as_raw())?;
    output.flush()
}

fn read_delivery(input: &mut impl std::io::BufRead) -> anyhow::Result<Delivery> {
    let mut header = String::new();
    input.read_line(&mut header)?;
    let mut fields = header.split_whitespace();
    let mut number =
        || -> anyhow::Result<u32> { Ok(fields.next().context("short preview header")?.parse()?) };
    let (width, height, copied) = (number()?, number()?, number()? == 1);
    let from = fields.next().and_then(parse_from);
    let mut pixels = vec![0; width as usize * height as usize * 4];
    input.read_exact(&mut pixels)?;
    let image = image::RgbaImage::from_raw(width, height, pixels).context("bad preview size")?;
    Ok(Delivery {
        image,
        from,
        copied,
        temp: None,
    })
}

fn open_delivery(temp: &Path) -> anyhow::Result<Delivery> {
    Ok(Delivery {
        image: image::open(temp)?.into_rgba8(),
        from: std::env::var(FROM).ok().and_then(|v| parse_from(&v)),
        copied: std::env::var(COPIED).is_ok_and(|v| v == "1"),
        temp: Some(temp.to_path_buf()),
    })
}

/// Miniatura en su propio proceso. Con XWayland va como ventana X11: en Wayland una ventana
/// no puede elegir su posición, y así se coloca en el monitor y la esquina elegidos.
/// Guardar corre mientras tanto.
///
/// Con `-` como ruta la captura llega por stdin (ver `flows::Warm`): el proceso arranca
/// mientras se elige la zona, crea la ventana oculta y la muestra en cuanto la recibe.
pub fn run_preview(runtime: &tokio::runtime::Runtime, temp: &Path) -> anyhow::Result<()> {
    let started = Instant::now();
    let (sender, incoming) = std::sync::mpsc::channel();
    // La ventana puede no existir aún cuando llega la captura: entonces la recoge su primer
    // cuadro.
    let waker = Arc::new(std::sync::OnceLock::<egui::Context>::new());
    if temp == Path::new("-") {
        let waker = waker.clone();
        std::thread::spawn(move || {
            let _ = sender.send(read_delivery(&mut std::io::stdin().lock()));
            if let Some(ctx) = waker.get() {
                ctx.request_repaint();
            }
        });
    } else {
        let _ = sender.send(open_delivery(temp));
    }
    let settings = Settings::load();
    if !settings.keep_temp {
        // Restos de cierres forzados; las de esta captura son recientes.
        save::purge_temp(Duration::from_secs(3600));
    }
    let x11 = if std::env::var_os("DISPLAY").is_some() {
        monitors::x11_monitors()
    } else {
        Vec::new()
    };
    let mutter = if x11.is_empty() {
        Vec::new()
    } else {
        runtime
            .block_on(monitors::gnome_monitors())
            .unwrap_or_default()
    };
    log::info!("preview: monitors in {:?}", started.elapsed());
    let mut options = native(
        ViewportBuilder::default()
            .with_title("colorshot preview")
            .with_inner_size(preview::MIN_WINDOW)
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top()
            .with_transparent(true)
            .with_active(false)
            // Mientras espera, transparente y sin recibir clics.
            .with_mouse_passthrough(true)
            .with_window_type(egui::X11WindowType::Utility),
    );
    if !x11.is_empty() {
        use_x11(&mut options);
    }
    eframe::run_native(
        "colorshot preview",
        options,
        Box::new(move |cc| {
            log::info!("preview: window in {:?}", started.elapsed());
            theme::apply(&cc.egui_ctx);
            let _ = waker.set(cc.egui_ctx.clone());
            Ok(Box::new(PreviewHost {
                incoming,
                setup: Some((settings, mutter, x11)),
                app: None,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

// No se oculta con `Visible(false)`: eframe pinta las ventanas ocultas fuera del bucle de
// eventos y las ventanas inmediatas que buscan el puntero hacen que entre en pánico.
/// Ventana vacía a la espera de la captura; después, la miniatura.
struct PreviewHost {
    incoming: std::sync::mpsc::Receiver<anyhow::Result<Delivery>>,
    setup: Option<(Settings, Vec<monitors::Monitor>, Vec<monitors::XMonitor>)>,
    app: Option<PreviewApp>,
}

impl eframe::App for PreviewHost {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.app.is_none() {
            match self.incoming.try_recv() {
                Ok(Ok(delivery)) => {
                    let Some((settings, mutter, x11)) = self.setup.take() else {
                        return;
                    };
                    self.app = Some(PreviewApp::new(&ctx, delivery, settings, &mutter, &x11));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
                    // Mutter no siempre respeta «encima» pedido al crearla, antes de mostrarse.
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        egui::WindowLevel::AlwaysOnTop,
                    ));
                }
                Ok(Err(error)) => {
                    log::warn!("preview: no capture: {error:#}");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close)
                }
            }
        }
        if let Some(app) = &mut self.app {
            app.ui(ui, frame);
        }
    }
}

fn contains(r: Rect, p: (u32, u32)) -> bool {
    p.0 >= r.x && p.1 >= r.y && p.0 < r.x + r.w && p.1 < r.y + r.h
}

/// Ventanas X11 (XWayland) en este proceso.
fn use_x11(options: &mut eframe::NativeOptions) {
    options.event_loop_builder = Some(Box::new(|builder| {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }));
}

// Guarda con la compresión definitiva; la copia ya la hizo el proceso que capturó.
fn save_only(
    image: &image::RgbaImage,
    format: Format,
    dir: Option<Result<PathBuf, save::SaveError>>,
    copied: bool,
) -> (preview::Status, Option<PathBuf>) {
    let saved = match dir {
        Some(dir) => match dir.and_then(|dir| save::save_with(image, dir, format)) {
            Ok(path) => Some(path),
            Err(error) => {
                return (
                    preview::Status::Failed(tf("Not saved: {}", &[&error])),
                    None,
                );
            }
        },
        None => None,
    };
    (
        preview::Status::Done {
            saved: saved.is_some(),
            copied,
        },
        saved,
    )
}

enum Phase {
    /// Buscando el monitor del puntero.
    Waiting,
    Shown,
}

struct PreviewApp {
    preview: Preview,
    image: image::RgbaImage,
    // PNG rápido que dejó el proceso que capturó; se borra al salir si la captura quedó
    // guardada y ningún editor o pin lo recibió.
    temp: Option<PathBuf>,
    temp_shared: bool,
    keep_temp: bool,
    // Tamaño pedido; XWayland ignora el inicial y hay que reclamarlo.
    size: egui::Vec2,
    results: std::sync::mpsc::Receiver<(preview::Status, Option<PathBuf>)>,
    screens: Vec<monitors::Screen>,
    target: Option<usize>,
    placement: Placement,
    born: Instant,
    phase: Phase,
    // Rectángulo de la miniatura en puntos X11, una vez colocada.
    window: Option<egui::Rect>,
}

impl PreviewApp {
    fn new(
        ctx: &egui::Context,
        delivery: Delivery,
        settings: Settings,
        mutter: &[monitors::Monitor],
        x11: &[monitors::XMonitor],
    ) -> Self {
        let Delivery {
            image,
            from,
            copied,
            temp,
        } = delivery;
        let full = from.map_or(image.dimensions(), |f| f.1);
        let screens = if x11.is_empty() {
            Vec::new()
        } else {
            monitors::screens(mutter, full, x11)
        };
        let target = match &settings.thumbnail_monitor {
            ThumbnailMonitor::Named(name) => screens.iter().position(|s| &s.connector == name),
            ThumbnailMonitor::Primary => None,
            // El puntero estaba en la zona al confirmarla; en capturas completas se busca.
            ThumbnailMonitor::Cursor => from
                .filter(|(r, _)| r.w < full.0 || r.h < full.1)
                .and_then(|(r, _)| {
                    let center = (r.x + r.w / 2, r.y + r.h / 2);
                    screens.iter().position(|s| contains(s.image, center))
                }),
        };
        let find_cursor =
            target.is_none() && settings.thumbnail_monitor == ThumbnailMonitor::Cursor;
        let target = target.or_else(|| {
            (!find_cursor).then(|| screens.iter().position(|s| s.primary).unwrap_or(0))
        });
        let (sender, results) = std::sync::mpsc::channel();
        let job = image.clone();
        let format = settings.format;
        let dir = settings.save_after_capture.then(|| settings.save_dir());
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let t = Instant::now();
            let result = save_only(&job, format, dir, copied);
            log::info!("preview: saved in {:?}: {result:?}", t.elapsed());
            let _ = sender.send(result);
            repaint.request_repaint();
        });
        let size = egui::vec2(image.width() as f32, image.height() as f32);
        let animate = !screens.is_empty();
        Self {
            preview: Preview::new(ctx, &image, temp.clone().unwrap_or_default())
                .with_lifetime(settings.thumbnail_seconds),
            image,
            temp,
            temp_shared: false,
            keep_temp: settings.keep_temp,
            size: preview::window_size(size / ctx.pixels_per_point()),
            results,
            screens,
            target,
            placement: settings.thumbnail_placement,
            born: Instant::now(),
            phase: if animate {
                Phase::Waiting
            } else {
                Phase::Shown
            },
            window: None,
        }
    }

    /// Monitor en puntos X11 (XWayland usa un único factor para todos).
    fn monitor(&self, index: usize, ppp: f32) -> egui::Rect {
        let m = &self.screens[index].x11;
        egui::Rect::from_min_size(
            egui::pos2(m.x as f32, m.y as f32) / ppp,
            egui::vec2(m.width as f32, m.height as f32) / ppp,
        )
    }

    /// Tamaño de la captura en puntos.
    fn points(&self, ppp: f32) -> egui::Vec2 {
        egui::vec2(self.image.width() as f32, self.image.height() as f32) / ppp
    }

    // Una ventana transparente encima de cada monitor que participa; devuelve el monitor en
    // el que está el puntero, si alguno lo recibió.
    fn overlays(&self, ctx: &egui::Context, ppp: f32) -> Option<usize> {
        // Ya colocada no queda ninguna: taparían el monitor y se comerían los clics.
        if let Phase::Shown = self.phase {
            return None;
        }
        let mut found = None;
        for index in 0..self.screens.len() {
            let monitor = self.monitor(index, ppp);
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of(("flight", index)),
                ViewportBuilder::default()
                    .with_title("colorshot flight")
                    .with_position(monitor.min)
                    .with_inner_size(monitor.size())
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_transparent(true)
                    .with_always_on_top()
                    .with_active(false)
                    .with_window_type(egui::X11WindowType::Notification),
                |ctx, _| {
                    if ctx.input(|i| i.pointer.hover_pos()).is_some() {
                        found = Some(index);
                    }
                },
            );
        }
        found
    }

    // El editor, centrado en el monitor de la miniatura.
    fn editor_rect(&self, ppp: f32, window: egui::Rect) -> egui::Rect {
        let index = (0..self.screens.len())
            .find(|i| self.monitor(*i, ppp).contains(window.center()))
            .or(self.target)
            .unwrap_or(0);
        let monitor = self.monitor(index, ppp);
        let image = egui::vec2(self.image.width() as f32, self.image.height() as f32);
        let size = colorshot_ui::editor::window_size(image, monitor.size() * 0.9);
        transition::place(monitor, size, (0.5, 0.5), 0.)
    }
}

impl Drop for PreviewApp {
    // Si la recibió un editor o un pin, la borra ese proceso al cargarla.
    fn drop(&mut self) {
        if !self.temp_shared
            && !self.keep_temp
            && let Some(temp) = &self.temp
        {
            let _ = std::fs::remove_file(temp);
        }
    }
}

impl PreviewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let ppp = ctx.pixels_per_point();
        // La captura en píxeles de pantalla: se muestra a su tamaño real.
        self.size = preview::window_size(self.points(ppp));
        if (ctx.content_rect().size() - self.size).abs().max_elem() > 1. {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(self.size));
        }
        while let Ok((status, saved)) = self.results.try_recv() {
            self.preview.set_status(status);
            if let Some(path) = saved {
                self.preview.path = path;
            }
        }
        let found = if self.screens.is_empty() {
            None
        } else {
            self.overlays(&ctx, ppp)
        };
        if let Phase::Waiting = self.phase {
            if self.target.is_none() {
                self.target = found.or_else(|| {
                    (now - self.born > FIND_CURSOR)
                        .then(|| self.screens.iter().position(|s| s.primary).unwrap_or(0))
                });
            }
            if let Some(target) = self.target {
                log::info!(
                    "preview: monitor {target} (pointer {found:?}) after {:?}",
                    now - self.born
                );
                let mut area = self.monitor(target, ppp);
                // La barra superior de GNOME está en el monitor principal.
                if self.screens[target].primary {
                    area.min.y += 32.;
                }
                let window = transition::place(area, self.size, self.placement.align(), 20.);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(window.min));
                self.window = Some(window);
                // Aparece en su sitio con un fundido corto.
                self.phase = Phase::Shown;
                self.preview.restart();
            }
            ctx.request_repaint_after(FRAME);
            return;
        }
        ui.set_opacity(transition::appear(&ctx));
        match self.preview.ui(ui) {
            Some(PreviewAction::Close) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(PreviewAction::Edit) => {
                let window = ctx
                    .input(|i| i.viewport().outer_rect)
                    .or(self.window)
                    .filter(|_| !self.screens.is_empty());
                let at = window.map(|window| self.editor_rect(ppp, window));
                self.temp_shared |= self.temp.as_ref() == Some(&self.preview.path);
                if let Err(error) = spawn_at("edit", &self.image, Some(&self.preview.path), at) {
                    log::warn!("Could not open the editor: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(PreviewAction::Pin) => {
                self.temp_shared |= self.temp.as_ref() == Some(&self.preview.path);
                if let Err(error) = spawn("pin", &self.image, Some(&self.preview.path)) {
                    log::warn!("Could not pin: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }
}

/// Lectura OCR sobre la zona elegida: una ventana X11 sin bordes justo encima de ella con la
/// misma imagen, el barrido mientras tesseract trabaja y la medalla del resultado.
pub fn run_scan(runtime: &tokio::runtime::Runtime, temp: &Path) -> anyhow::Result<()> {
    let image = image::open(temp)?.into_rgba8();
    let _ = std::fs::remove_file(temp);
    let from = std::env::var(FROM).ok().and_then(|v| parse_from(&v));
    let mutter = runtime
        .block_on(monitors::gnome_monitors())
        .unwrap_or_default();
    let full = from.map_or(image.dimensions(), |f| f.1);
    let screens = monitors::screens(&mutter, full, &monitors::x11_monitors());
    // Zona en píxeles X: esquinas pasadas por el monitor que contiene su centro.
    let area = from.and_then(|(r, _)| {
        let screen = screens
            .iter()
            .find(|s| contains(s.image, (r.x + r.w / 2, r.y + r.h / 2)))?;
        let a = screen.to_x11((r.x as f32, r.y as f32));
        let b = screen.to_x11(((r.x + r.w) as f32, (r.y + r.h) as f32));
        Some(egui::Rect::from_min_max(
            egui::pos2(a.0, a.1),
            egui::pos2(b.0, b.1),
        ))
    });
    let (sender, result) = std::sync::mpsc::channel();
    let job = image.clone();
    let mut options = native(
        ViewportBuilder::default()
            .with_title("colorshot scan")
            .with_inner_size(egui::vec2(
                image.width().clamp(1, 600) as f32,
                image.height().clamp(1, 400) as f32,
            ))
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_active(false)
            .with_window_type(egui::X11WindowType::Notification),
    );
    use_x11(&mut options);
    eframe::run_native(
        "colorshot scan",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let outcome = recognize::recognize(&job)
                    .map_err(anyhow::Error::from)
                    .and_then(|found| crate::flows::notify_result(&found));
                let _ = sender.send(match outcome {
                    Ok(title) => (true, title.to_string()),
                    Err(error) => {
                        crate::notify_error(&error);
                        (false, t(&error.to_string()).to_string())
                    }
                });
                ctx.request_repaint();
            });
            Ok(Box::new(ScanApp {
                texture: scan_texture(&cc.egui_ctx, &image),
                scan: colorshot_ui::scan::Scan::new(),
                area,
                placed: false,
                result,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

// La zona entera si cabe en la GPU; si no, reducida.
fn scan_texture(ctx: &egui::Context, image: &image::RgbaImage) -> egui::TextureHandle {
    let max = 4096.;
    let f = (max / image.width().max(image.height()).max(1) as f32).min(1.);
    let shown = if f < 1. {
        std::borrow::Cow::Owned(image::imageops::thumbnail(
            image,
            ((image.width() as f32 * f) as u32).max(1),
            ((image.height() as f32 * f) as u32).max(1),
        ))
    } else {
        std::borrow::Cow::Borrowed(image)
    };
    ctx.load_texture(
        "scan",
        egui::ColorImage::from_rgba_unmultiplied(
            [shown.width() as usize, shown.height() as usize],
            shown.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

struct ScanApp {
    texture: egui::TextureHandle,
    scan: colorshot_ui::scan::Scan,
    // Zona en píxeles X; `None` si no se sabe dónde estaba (queda donde la ponga GNOME).
    area: Option<egui::Rect>,
    placed: bool,
    result: std::sync::mpsc::Receiver<(bool, String)>,
}

impl eframe::App for ScanApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // El factor de XWayland solo se conoce con la ventana creada: se coloca en el primer
        // cuadro, sin dibujar nada todavía.
        if !self.placed {
            self.placed = true;
            if let Some(area) = self.area {
                let ppp = ctx.pixels_per_point();
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(area.min / ppp));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(area.size() / ppp));
            }
            ctx.request_repaint_after(FRAME);
            return;
        }
        if let Ok((ok, message)) = self.result.try_recv() {
            self.scan.finish(
                ok,
                if ok {
                    tf("Done · {}", &[&message])
                } else {
                    message
                },
            );
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let rect = ctx.content_rect();
        let painter = ui.painter();
        painter.image(
            self.texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
        if self
            .scan
            .paint(painter, rect, theme::Tokens::SELECTION, Instant::now())
        {
            ctx.request_repaint_after(FRAME);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

// Ventana sin bordes que cubre un monitor; con varios monitores se fija el índice de winit.
// Transparente, deja ver el escritorio vivo debajo. Con `place` no es pantalla completa.
pub fn overlay(
    index: usize,
    multi: bool,
    transparent: bool,
    place: Option<&Place>,
) -> ViewportBuilder {
    let builder = ViewportBuilder::default()
        .with_title("colorshot")
        .with_decorations(false)
        .with_always_on_top()
        .with_transparent(transparent);
    match place {
        Some(place) => {
            let window = place.window(place.guess);
            builder
                .with_position(window.min)
                .with_inner_size(window.size())
                .with_clamp_size_to_monitor_size(false)
                // Mutter encaja las ventanas normales en el área de trabajo (bajo la barra).
                .with_window_type(egui::X11WindowType::Dock)
        }
        None if multi => builder.with_fullscreen(true).with_monitor(index),
        None => builder.with_fullscreen(true),
    }
}

/// Ventana del selector sobre un monitor, en píxeles X. Sobra un punto por un borde sin
/// vecinos: si mide justo lo que el monitor, Mutter la trata como pantalla completa y GNOME
/// reacciona (oculta la barra, Caffeine se activa…).
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    window: egui::Rect,
    monitor: egui::Rect,
    // Píxeles X por punto según Mutter, hasta que la ventana sepa el suyo.
    guess: f32,
}

impl Place {
    fn window(&self, ppp: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            (self.window.min.to_vec2() / ppp).to_pos2(),
            self.window.size() / ppp,
        )
    }
    /// El monitor dentro de la ventana, en puntos.
    fn screen(&self, ppp: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            ((self.monitor.min - self.window.min) / ppp).to_pos2(),
            self.monitor.size() / ppp,
        )
    }
}

/// Un `Place` por monitor de Mutter; vacío si alguno no se ubica (se usa pantalla completa).
fn places(screens: &[monitors::Monitor], x11: &[monitors::XMonitor]) -> Vec<Place> {
    let rect = |m: &monitors::XMonitor| {
        egui::Rect::from_min_size(
            egui::pos2(m.x as f32, m.y as f32),
            egui::vec2(m.width as f32, m.height as f32),
        )
    };
    let all: Vec<_> = x11.iter().map(rect).collect();
    let place = |m: &monitors::Monitor| {
        let monitor = rect(x11.iter().find(|x| x.connector == m.connector)?);
        let guess = (monitor.width() / m.width.max(1) as f32).round().max(1.);
        let grow = [
            egui::vec2(0., guess),
            egui::vec2(guess, 0.),
            egui::vec2(0., -guess),
            egui::vec2(-guess, 0.),
        ];
        // Abajo, derecha, arriba, izquierda: el primero cuya franja no pisa otro monitor.
        let window = grow.into_iter().find_map(|g| {
            let window = egui::Rect::from_min_max(
                monitor.min + g.min(egui::Vec2::ZERO),
                monitor.max + g.max(egui::Vec2::ZERO),
            );
            let outside =
                |other: &egui::Rect| *other == monitor || !window.intersect(*other).is_positive();
            all.iter().all(outside).then_some(window)
        })?;
        Some(Place {
            window,
            monitor,
            guess,
        })
    };
    screens
        .iter()
        .map(place)
        .collect::<Option<_>>()
        .unwrap_or_default()
}

pub struct SelectorApp {
    selectors: Vec<AreaSelector>,
    // Tamaño lógico de cada monitor según Mutter, para reconocer en cuál quedó cada ventana.
    sizes: Vec<egui::Vec2>,
    // Con XWayland, dónde va cada ventana (en el orden de Mutter); vacío, pantalla completa.
    places: Vec<Place>,
    focused: bool,
    transparent: bool,
    output: Arc<Mutex<Option<Pending>>>,
    grab: Option<Grab>,
    grabbing: Option<Grabbing>,
    // Lápiz de tableta en Wayland (solo en la ventana del primer monitor): con la imitación
    // de ratón de GNOME el cursor desaparece a los pocos segundos.
    tablet: Option<Tablet>,
}

impl SelectorApp {
    // El orden de winit no tiene por qué coincidir con el de Mutter: se elige por tamaño
    // y, si hay empate, por índice.
    fn monitor_for(&self, index: usize, size: egui::Vec2) -> usize {
        // Colocadas a mano, cada ventana ya está en su monitor.
        if !self.places.is_empty() {
            return index;
        }
        if self.sizes.len() <= 1 {
            return 0;
        }
        let distance = |i: usize| (self.sizes[i] - size).length() + (i != index) as u8 as f32;
        (0..self.sizes.len())
            .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
            .unwrap_or(0)
    }

    fn frame(&mut self, ctx: &egui::Context, index: usize) {
        if self.grabbing.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            return;
        }
        let monitor = self.monitor_for(index, ctx.content_rect().size());
        // Empezar una zona en un monitor borra la de los demás.
        if ctx.input(|i| i.pointer.primary_pressed()) {
            for (i, selector) in self.selectors.iter_mut().enumerate() {
                if i != monitor {
                    selector.clear();
                }
            }
        }
        let Some(selector) = self.selectors.get_mut(monitor) else {
            return;
        };
        if let Some(place) = self.places.get(index) {
            let ppp = ctx.pixels_per_point();
            let window = place.window(ppp);
            let outer = ctx.input(|i| i.viewport().outer_rect);
            if outer.is_none_or(|o| (o.min - window.min).abs().max_elem() > 0.5)
                || (ctx.content_rect().size() - window.size()).abs().max_elem() > 0.5
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(window.min));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(window.size()));
            }
            selector.set_screen(place.screen(ppp));
            // Un dock no recibe el teclado al abrirse; se pide hasta tenerlo para que Esc funcione.
            if index == 0 && !self.focused {
                self.focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
                if !self.focused {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
        if let Some(result) = selector.ui(ctx) {
            let grabs = matches!(result, SelectorResult::Area { .. } | SelectorResult::All)
                && self.grab.is_some();
            if let Ok(mut output) = self.output.lock() {
                *output = Some((monitor, result, None));
            }
            if grabs {
                self.grabbing = Some(Grabbing {
                    since: Instant::now(),
                    started: false,
                    done: Arc::default(),
                });
                ctx.request_repaint();
            } else {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Close);
            }
        }
    }

    // Vacías las ventanas, captura en otro hilo y cierra al tenerla.
    fn grab(&mut self, ctx: &egui::Context) {
        let (Some(grabbing), Some(grab)) = (&mut self.grabbing, &self.grab) else {
            return;
        };
        let chosen = self
            .output
            .lock()
            .ok()
            .and_then(|o| o.as_ref().map(|(monitor, result, _)| (*monitor, *result)));
        if !grabbing.started
            && grabbing.since.elapsed() >= BLANK
            && let Some((monitor, result)) = chosen
        {
            grabbing.started = true;
            let (done, grab, ctx) = (grabbing.done.clone(), grab.clone(), ctx.clone());
            std::thread::spawn(move || {
                let capture = grab(monitor, result);
                if let Ok(mut done) = done.lock() {
                    *done = Some(capture);
                }
                ctx.request_repaint();
            });
        }
        let capture = grabbing.done.lock().ok().and_then(|mut d| d.take());
        match capture {
            Some(capture) => {
                if let Ok(mut output) = self.output.lock()
                    && let Some(output) = output.as_mut()
                {
                    output.2 = Some(capture);
                }
                ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Close);
            }
            None => ctx.request_repaint_after(FRAME),
        }
    }
}

impl eframe::App for SelectorApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        if self.transparent {
            [0.; 4]
        } else {
            visuals.panel_fill.to_normalized_gamma_f32()
        }
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(tablet) = &mut self.tablet {
            tablet.feed(ctx, raw);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for index in 1..self.sizes.len() {
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of(("monitor", index)),
                overlay(index, true, self.transparent, self.places.get(index)),
                |ctx, _| self.frame(ctx, index),
            );
        }
        self.frame(&ctx, 0);
        self.grab(&ctx);
        if let Some(tablet) = &self.tablet {
            tablet.cursor(&ctx);
        }
    }
}

// Ventana decorada y acciones del editor sobre los píxeles originales.
pub fn run_editor(capture: Capture) -> anyhow::Result<()> {
    let settings = Settings::load();
    let image = egui::vec2(capture.image.width() as f32, capture.image.height() as f32);
    let mut viewport = ViewportBuilder::default()
        .with_title("colorshot")
        .with_inner_size(colorshot_ui::editor::window_size(image, EDITOR_MAX))
        .with_min_inner_size(colorshot_ui::editor::MIN_WINDOW)
        .with_decorations(false)
        .with_transparent(true);
    // Desde la miniatura: X11 y en el monitor de la miniatura.
    let at = std::env::var(AT).ok().and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
        let [x, y, w, h] = n[..] else { return None };
        Some(egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(w, h),
        ))
    });
    if let Some(at) = at {
        viewport = viewport.with_position(at.min).with_inner_size(at.size());
    }
    let mut options = native(viewport);
    // GNOME solo desenfoca ventanas Wayland: con frosted glass el editor no va por X11, aunque
    // GNOME lo coloque a su manera.
    if at.is_some() && std::env::var_os("DISPLAY").is_some() && !settings.frosted_glass {
        use_x11(&mut options);
    }
    eframe::run_native(
        "colorshot",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let (results, finished) = std::sync::mpsc::channel();
            let mut blur = Blur::new(cc).filter(|_| settings.frosted_glass);
            let glass = settings
                .glass()
                .filter(|_| blur.as_mut().is_some_and(Blur::supported));
            let mut editor = Editor::new(capture.image);
            editor.glass = glass.is_some();
            Ok(Box::new(EditorApp {
                editor,
                gnome: theme::Gnome::new(true).frosted(glass),
                blur: blur.filter(|_| glass.is_some()),
                tablet: Tablet::new(cc, &cc.egui_ctx),
                settings,
                results,
                finished,
                running: 0,
                focus: if at.is_some() { 3 } else { 0 },
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
struct EditorApp {
    editor: Editor,
    // Colores del marco: el editor siempre es oscuro.
    gnome: theme::Gnome,
    // Con frosted glass, el desenfoque del fondo detrás de la ventana.
    blur: Option<Blur>,
    // Lápiz de tableta en Wayland, que winit no recibe.
    tablet: Option<Tablet>,
    settings: Settings,
    // Copiar, guardar y reconocer tardan segundos con capturas grandes: corren en otro hilo
    // y devuelven el aviso para la barra. En el hilo de la ventana GNOME la daba por colgada.
    // Con `Some(ok)`, el aviso es el resultado de una lectura OCR.
    results: std::sync::mpsc::Sender<(String, Option<bool>)>,
    finished: std::sync::mpsc::Receiver<(String, Option<bool>)>,
    running: usize,
    // Abierto desde la miniatura (otro proceso, sin token de activación): GNOME lo dejaría
    // detrás con «colorshot is ready»; en X11 Mutter acepta pedir el foco sin marca de tiempo.
    // Se pide al tercer cuadro: winit lo ignora mientras eframe no muestra la ventana.
    focus: u8,
}
impl EditorApp {
    fn background(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> (String, Option<bool>) + Send + 'static,
    ) {
        let results = self.results.clone();
        let ctx = ctx.clone();
        self.running += 1;
        std::thread::spawn(move || {
            let _ = results.send(job());
            ctx.request_repaint();
        });
    }
}
impl eframe::App for EditorApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        frame::release_hook(ctx, raw);
        if let Some(tablet) = &mut self.tablet {
            tablet.feed(ctx, raw);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.set_opacity(transition::appear(&ctx));
        // Las herramientas van en la barra de título.
        let (bar, body) = frame::toolbar(ui, &self.gnome, colorshot_ui::editor::BAR, true);
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body));
        self.editor_ui(&mut inner, &ctx, bar.shrink2(egui::vec2(12., 0.)));
        frame::finish(ui, &self.gnome, true);
        if let Some(blur) = &mut self.blur {
            blur.cover(&ctx, true);
        }
        if let Some(tablet) = &self.tablet {
            tablet.cursor(&ctx);
        }
    }
}

impl EditorApp {
    fn editor_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, bar: egui::Rect) {
        let ctx = ctx.clone();
        if self.focus > 0 {
            self.focus -= 1;
            if self.focus == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            ctx.request_repaint();
        }
        while let Ok((message, scan)) = self.finished.try_recv() {
            self.running -= 1;
            match scan {
                Some(ok) => self.editor.finish_scan(ok, message),
                None => self.editor.notify(message),
            }
        }
        match self.editor.ui_in(ui, bar) {
            Some(EditorAction::Close) => {
                // Esc copia o guarda según las preferencias antes de ocultar.
                let image = model::render(&self.editor.doc);
                let result = match self.settings.esc_action {
                    EscAction::Nothing => Ok(()),
                    EscAction::Copy => clipboard::copy_image(&image).map_err(anyhow::Error::from),
                    EscAction::Save => self
                        .settings
                        .save_dir()
                        .and_then(|dir| save::save_with(&image, dir, self.settings.format))
                        .map(|_| ())
                        .map_err(anyhow::Error::from),
                };
                if let Err(error) = result {
                    log::warn!("Esc action failed: {error}");
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            // Copiar, guardar y fijar terminan la edición: la ventana se cierra si salen bien.
            Some(EditorAction::Pin(image)) => match spawn("pin", &image, None) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => self.editor.notify(tf("Pin failed: {}", &[&error])),
            },
            Some(EditorAction::Recognize(image)) => {
                self.editor.start_scan();
                self.background(&ctx, move || {
                    match recognize::recognize(&image)
                        .map_err(anyhow::Error::from)
                        .and_then(|found| crate::flows::notify_result(&found))
                    {
                        Ok(title) => (tf("Done · {}", &[&title]), Some(true)),
                        Err(error) => (t(&error.to_string()).to_string(), Some(false)),
                    }
                });
            }
            Some(EditorAction::Copy(image)) => match clipboard::copy_image(&image) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => self.editor.notify(tf("Copy failed: {}", &[&error])),
            },
            Some(EditorAction::Save(image)) => {
                let saved = self
                    .settings
                    .save_dir()
                    .and_then(|dir| save::save_with(&image, dir, self.settings.format));
                match saved {
                    Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(error) => self.editor.notify(tf("Save failed: {}", &[&error])),
                }
            }
            None => {}
        }
        if self.running > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// Lanza `colorshot pin|edit <png>` en un proceso propio: cada ventana nueva necesita un
/// bucle de eventos fresco (ver `main::spawn_child`). Si la imagen no está en disco (o fue
/// editada), se escribe en el directorio de ejecución de la sesión.
pub fn spawn(
    verb: &str,
    image: &image::RgbaImage,
    saved: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    spawn_at(verb, image, saved, None)
}

/// Como `spawn`, con el sitio de la ventana en puntos X11 (ver `AT`).
pub fn spawn_at(
    verb: &str,
    image: &image::RgbaImage,
    saved: Option<&std::path::Path>,
    at: Option<egui::Rect>,
) -> anyhow::Result<()> {
    let path = match saved.filter(|p| p.is_file()) {
        Some(path) => path.to_path_buf(),
        None => {
            let dir = save::temp_dir();
            std::fs::create_dir_all(&dir)?;
            save::save_png(image, dir)?
        }
    };
    let mut command = std::process::Command::new(crate::exe()?);
    command
        .arg(verb)
        .arg(path)
        .env(crate::CHILD, "1")
        .env_remove(FROM);
    match at {
        Some(at) => command.env(
            AT,
            format!("{},{},{},{}", at.min.x, at.min.y, at.width(), at.height()),
        ),
        None => command.env_remove(AT),
    };
    command.spawn()?;
    Ok(())
}

/// Sin «Keep temporary images», la captura temporal se borra en cuanto otra ventana la ha
/// cargado: la imagen ya está en memoria y nadie más la espera.
pub fn forget_temp(path: &Path) {
    if save::is_temp(path) && !Settings::load().keep_temp {
        let _ = std::fs::remove_file(path);
    }
}

pub fn run_pin(path: &std::path::Path) -> anyhow::Result<()> {
    let path = path.to_path_buf();
    let image = image::open(&path)?.into_rgba8();
    forget_temp(&path);
    let size = colorshot_ui::pin::scaled(
        egui::vec2(image.width() as f32, image.height() as f32),
        1.,
        1200.,
    ) + egui::vec2(2., 2.);
    eframe::run_native(
        "colorshot pin",
        native(
            ViewportBuilder::default()
                .with_title("Pin")
                .with_inner_size(size)
                .with_min_inner_size(egui::vec2(120., 80.))
                .with_decorations(false)
                .with_always_on_top()
                .with_transparent(true),
        ),
        Box::new(move |cc| {
            Ok(Box::new(PinApp {
                pin: Pin::new(&cc.egui_ctx, &image),
                tablet: Tablet::new(cc, &cc.egui_ctx),
                image,
                path,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
struct PinApp {
    pin: Pin,
    // Lápiz de tableta en Wayland, que winit no recibe.
    tablet: Option<Tablet>,
    image: image::RgbaImage,
    path: PathBuf,
}
impl eframe::App for PinApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        frame::release_hook(ctx, raw);
        if let Some(tablet) = &mut self.tablet {
            tablet.feed(ctx, raw);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.set_opacity(transition::appear(&ctx));
        match self.pin.ui(ui) {
            Some(PinAction::Close) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(PinAction::Resize(size)) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size + egui::vec2(2., 2.)))
            }
            Some(PinAction::Copy) => {
                if let Err(error) = clipboard::copy_image(&self.image) {
                    log::warn!("Could not copy pin: {error}");
                }
            }
            Some(PinAction::Edit) => {
                if let Err(error) = spawn("edit", &self.image, Some(&self.path)) {
                    log::warn!("Could not open the editor: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
        if let Some(tablet) = &self.tablet {
            tablet.cursor(&ctx);
        }
    }
}

pub fn run_preferences(runtime: &tokio::runtime::Runtime) -> anyhow::Result<()> {
    let binary = crate::exe()?.display().to_string();
    let monitors = runtime
        .block_on(monitors::gnome_monitors())
        .unwrap_or_default();
    let gnome = theme::Gnome::current();
    eframe::run_native(
        "colorshot preferences",
        native(
            ViewportBuilder::default()
                .with_title("Preferences")
                .with_inner_size(colorshot_ui::preferences::SIZE)
                .with_resizable(false)
                .with_decorations(false)
                .with_transparent(true),
        ),
        Box::new(move |cc| {
            theme::apply_gnome(&cc.egui_ctx, &gnome);
            Ok(Box::new(PreferencesApp {
                preferences: Preferences::new(binary, monitors, gnome).with_blur(Blur::new(cc)),
                tablet: Tablet::new(cc, &cc.egui_ctx),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
/// Asistente de bienvenida, del tamaño de Preferencias.
pub fn run_welcome() -> anyhow::Result<()> {
    let binary = crate::exe()?.display().to_string();
    let gnome = theme::Gnome::current();
    eframe::run_native(
        "colorshot welcome",
        native(
            ViewportBuilder::default()
                .with_title("Welcome to ColorShot")
                .with_inner_size(colorshot_ui::welcome::SIZE)
                .with_resizable(false)
                .with_decorations(false)
                .with_transparent(true),
        ),
        Box::new(move |cc| {
            theme::apply_gnome(&cc.egui_ctx, &gnome);
            Ok(Box::new(WelcomeApp {
                welcome: colorshot_ui::welcome::Welcome::new(binary, gnome),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
struct WelcomeApp {
    welcome: colorshot_ui::welcome::Welcome,
}
impl eframe::App for WelcomeApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        frame::release_hook(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.set_opacity(transition::appear(ui.ctx()));
        self.welcome.ui(ui);
    }
}

/// Ventana de paletas: con el esquema de GNOME, redimensionable y con el marco propio.
pub fn run_palettes() -> anyhow::Result<()> {
    let binary = crate::exe()?.display().to_string();
    let gnome = theme::Gnome::current();
    eframe::run_native(
        "colorshot palettes",
        native(
            ViewportBuilder::default()
                .with_title("Color Palettes")
                .with_inner_size(colorshot_ui::palettes::SIZE)
                .with_min_inner_size(colorshot_ui::palettes::MIN_SIZE)
                .with_decorations(false)
                .with_transparent(true),
        ),
        Box::new(move |cc| {
            theme::apply_gnome(&cc.egui_ctx, &gnome);
            theme::apply_system_font(&cc.egui_ctx);
            Ok(Box::new(PalettesApp {
                palettes: PaletteWindow::new(binary, gnome).with_blur(Blur::new(cc)),
                tablet: Tablet::new(cc, &cc.egui_ctx),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
struct PalettesApp {
    palettes: PaletteWindow,
    // Lápiz de tableta en Wayland, que winit no recibe.
    tablet: Option<Tablet>,
}
impl eframe::App for PalettesApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        frame::release_hook(ctx, raw);
        if let Some(tablet) = &mut self.tablet {
            tablet.feed(ctx, raw);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.set_opacity(transition::appear(ui.ctx()));
        self.palettes.ui(ui);
        if let Some(tablet) = &self.tablet {
            tablet.cursor(ui.ctx());
        }
    }
}

struct PreferencesApp {
    preferences: Preferences,
    // Lápiz de tableta en Wayland, que winit no recibe.
    tablet: Option<Tablet>,
}
impl eframe::App for PreferencesApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        frame::release_hook(ctx, raw);
        if let Some(tablet) = &mut self.tablet {
            tablet.feed(ctx, raw);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.set_opacity(transition::appear(ui.ctx()));
        self.preferences.ui(ui);
        if let Some(tablet) = &self.tablet {
            tablet.cursor(ui.ctx());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selector_grows_past_a_free_edge() {
        let mutter = |connector: &str, x, width, height| monitors::Monitor {
            connector: connector.into(),
            x,
            y: 0,
            width,
            height,
            primary: false,
        };
        let x11 = |connector: &str, x, width, height| monitors::XMonitor {
            connector: connector.into(),
            x,
            y: 0,
            width,
            height,
        };
        let screens = [
            mutter("DP-4", 0, 2560, 1440),
            mutter("HDMI-2", 2560, 1920, 1200),
        ];
        let placed = places(
            &screens,
            &[x11("DP-4", 0, 5120, 2880), x11("HDMI-2", 5120, 3840, 2400)],
        );
        // Un punto (2 píxeles X) más por abajo, que no pisa al vecino.
        assert_eq!(
            placed[0].window(2.),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2560., 1441.))
        );
        assert_eq!(
            placed[0].screen(2.),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2560., 1440.))
        );
        assert_eq!(placed[1].window(2.).min, egui::pos2(2560., 0.));
        // Rodeado por todos lados no hay borde libre: se vuelve a la pantalla completa.
        let grid = (0..9)
            .map(|i| monitors::XMonitor {
                connector: i.to_string(),
                x: i % 3 * 100,
                y: i / 3 * 100,
                width: 100,
                height: 100,
            })
            .collect::<Vec<_>>();
        let center = monitors::Monitor {
            connector: "4".into(),
            ..mutter("", 100, 100, 100)
        };
        assert!(places(&[center], &grid).is_empty());
        assert!(places(&screens, &[]).is_empty());
    }
    #[test]
    fn delivery_round_trip() {
        let image = image::RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let from = Some((
            Rect {
                x: 1,
                y: 2,
                w: 3,
                h: 2,
            },
            (10, 20),
        ));
        let mut pipe = Vec::new();
        write_delivery(&mut pipe, &image, from, true).unwrap();
        let read = read_delivery(&mut pipe.as_slice()).unwrap();
        assert_eq!(read.image, image);
        assert_eq!(read.from, from);
        assert!(read.copied);
        let mut pipe = Vec::new();
        write_delivery(&mut pipe, &image, None, false).unwrap();
        let read = read_delivery(&mut pipe.as_slice()).unwrap();
        assert_eq!((read.from, read.copied), (None, false));
    }
    #[test]
    fn window_matches_monitor_by_size() {
        let app = SelectorApp {
            selectors: Vec::new(),
            sizes: vec![egui::vec2(2560., 1440.), egui::vec2(1920., 1200.)],
            places: Vec::new(),
            focused: false,
            transparent: true,
            output: Arc::default(),
            grab: None,
            grabbing: None,
            tablet: None,
        };
        // winit puede listar primero el monitor pequeño.
        assert_eq!(app.monitor_for(0, egui::vec2(1920., 1200.)), 1);
        assert_eq!(app.monitor_for(1, egui::vec2(2560., 1440.)), 0);
        // Mismo tamaño: decide el índice.
        let twins = SelectorApp {
            sizes: vec![egui::vec2(1920., 1080.); 2],
            places: Vec::new(),
            focused: false,
            ..app
        };
        assert_eq!(twins.monitor_for(1, egui::vec2(1920., 1080.)), 1);
    }
}
