//! Cada orden de captura de principio a fin: capturar, elegir, entregar.
use crate::windows;
use anyhow::Context;
use eframe::egui;
use colorshot_core::i18n::t;
use colorshot_core::{
    capture::{self, Capture, Rect},
    clipboard,
    command::Command,
    monitors, palettes,
    recognize::{self, Recognized},
    save,
    settings::{AreaShows, ScreenCapture, Settings},
};
use colorshot_ui::area_selector::{AreaSelector, SelectorResult};
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};

pub fn run(command: &Command, runtime: &Runtime) -> anyhow::Result<()> {
    match command {
        Command::Area => area(runtime),
        Command::Full => full(runtime, Settings::load().screen_capture),
        // Tras la cuenta no se elige nada: se toma el monitor del puntero.
        Command::Delayed => {
            std::thread::sleep(std::time::Duration::from_secs(3));
            full(runtime, ScreenCapture::Cursor)
        }
        Command::Repeat => repeat(runtime),
        Command::Window => window(runtime),
        Command::Ocr => ocr(runtime),
        Command::Color => pick_color(runtime),
        Command::Edit(path) => {
            let image = image::open(path)
                .with_context(|| colorshot_core::i18n::tf("Could not open {}", &[&path.display()]))?
                .into_rgba8();
            windows::forget_temp(path);
            windows::run_editor(Capture { image })
        }
        Command::OpenFile => match pick_file()? {
            Some(path) => run(&Command::Edit(path), runtime),
            None => Ok(()),
        },
        Command::Clipboard => windows::run_editor(Capture {
            image: clipboard_image()?,
        }),
        Command::Preferences => windows::run_preferences(runtime),
        Command::Palettes => windows::run_palettes(),
        Command::Pin(path) => windows::run_pin(path),
        Command::Preview(path) => windows::run_preview(runtime, path),
        Command::Scan(path) => windows::run_scan(runtime, path),
        Command::Daemon | Command::Quit => Ok(()),
    }
}

fn area(runtime: &Runtime) -> anyhow::Result<()> {
    let editor = Settings::load().area_shows == AreaShows::Editor;
    let warm = if editor { Warm::none() } else { Warm::start() };
    if let Some((image, from)) = select(runtime)? {
        if editor {
            return windows::spawn("edit", &image, None);
        }
        deliver(image, Some(from), warm)?;
    }
    Ok(())
}

/// Miniatura lanzada antes de capturar: crea su ventana mientras se elige la zona y recibe
/// la captura por stdin (ver `windows::run_preview`). Si no llega, se cierra al soltarla.
pub struct Warm(Option<std::process::Child>);

impl Warm {
    fn none() -> Self {
        Self(None)
    }

    fn start() -> Self {
        if !Settings::load().show_after_capture {
            return Self::none();
        }
        let child = crate::exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(["preview", "-"])
                .env(crate::CHILD, "1")
                .stdin(std::process::Stdio::piped())
                .spawn()
        });
        match child {
            Ok(child) => Self(Some(child)),
            Err(error) => {
                log::warn!("Could not start the preview: {error}");
                Self::none()
            }
        }
    }

    fn send(
        mut self,
        image: &image::RgbaImage,
        from: Option<From>,
        copied: bool,
    ) -> anyhow::Result<()> {
        let mut child = match self.0.take() {
            Some(child) => child,
            None => Self::start()
                .0
                .take()
                .context("Could not start the preview")?,
        };
        let mut stdin = child.stdin.take().context("preview without stdin")?;
        windows::write_delivery(&mut stdin, image, from, copied)?;
        Ok(())
    }
}

impl Drop for Warm {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Zona capturada y tamaño de la captura completa, para la transición de la miniatura.
type From = (Rect, (u32, u32));

/// Monitores según Mutter; sin ellos, una sola pantalla.
fn screens(runtime: &Runtime) -> Vec<monitors::Monitor> {
    runtime
        .block_on(monitors::gnome_monitors())
        .unwrap_or_else(|e| {
            log::warn!("Monitor layout unavailable ({e}); using one screen");
            Vec::new()
        })
}

/// Se marca la zona sobre el escritorio vivo y se captura al soltar; `None` si se cancela.
fn select(runtime: &Runtime) -> anyhow::Result<Option<windows::Shot>> {
    let screens = screens(runtime);
    let selectors = (0..screens.len().max(1))
        .map(|_| AreaSelector::area())
        .collect();
    let started = std::time::Instant::now();
    let Some((_, SelectorResult::Area { .. }, Some((image, from)))) =
        windows::run_selector(selectors, &screens, Some(grabber(runtime, &screens)))?
    else {
        return Ok(None);
    };
    log::info!("area: selected and captured in {:?}", started.elapsed());
    save::save_last_area(from.0)?;
    Ok(Some((image, from)))
}

/// Captura al soltar: con la extensión, solo la zona; si no, el escritorio por el portal.
fn grabber(runtime: &Runtime, screens: &[monitors::Monitor]) -> windows::Grab {
    let (runtime, screens) = (runtime.handle().clone(), screens.to_vec());
    Arc::new(move |monitor, result| shoot(&runtime, &screens, monitor, result))
}

fn shoot(
    runtime: &Handle,
    screens: &[monitors::Monitor],
    monitor: usize,
    result: SelectorResult,
) -> anyhow::Result<windows::Shot> {
    if let SelectorResult::Area { rect, size } = result
        && screens.get(monitor).is_some()
    {
        match zone(runtime, screens, monitor, rect, size) {
            Ok(shot) => return Ok(shot),
            Err(error) => log::info!("Area capture unavailable ({error:#}); using the portal"),
        }
    }
    let capture = runtime.block_on(capture::capture_full())?;
    let full = capture.image.dimensions();
    let mut rect = Rect {
        x: 0,
        y: 0,
        w: full.0,
        h: full.1,
    };
    if let SelectorResult::Area { rect: zone, size } = result {
        let layout = monitors::layout(screens, full);
        if let Some(&screen) = layout.get(monitor).or(layout.first()) {
            rect = to_capture(zone, size, screen);
        }
    }
    Ok((capture.crop(rect), (rect, full)))
}

/// Captura con la extensión la zona en coordenadas lógicas (redondeada hacia fuera) y la
/// recorta al píxel físico. Lo entregado coincide con lo que daría el portal.
fn zone(
    runtime: &Handle,
    screens: &[monitors::Monitor],
    monitor: usize,
    rect: Rect,
    size: (u32, u32),
) -> anyhow::Result<windows::Shot> {
    let (x, y, w, h) = logical(rect, size, &screens[monitor]);
    let capture = runtime.block_on(capture::capture_area(x, y, w, h))?;
    let scale = capture.image.width() as f64 / w as f64;
    let (min_x, min_y, max_x, max_y) = bounds(screens);
    let px = |v: i32| (v as f64 * scale).round().max(0.0) as u32;
    let full = (px(max_x - min_x), px(max_y - min_y));
    let layout = monitors::layout(screens, full);
    let screen = *layout.get(monitor).context("monitor outside the layout")?;
    let rect = to_capture(rect, size, screen);
    let (ox, oy) = (px(x - min_x), px(y - min_y));
    let image = capture.crop(Rect {
        x: rect.x.saturating_sub(ox),
        y: rect.y.saturating_sub(oy),
        w: rect.w,
        h: rect.h,
    });
    Ok((image, (rect, full)))
}

/// La zona de píxeles de la ventana (`size`) en coordenadas lógicas del escritorio, sin
/// perder píxeles: el inicio se redondea hacia abajo y el final hacia arriba.
fn logical(r: Rect, size: (u32, u32), m: &monitors::Monitor) -> (i32, i32, u32, u32) {
    let along = |v: u32, from: u32, to: u32| v as f64 * to as f64 / from.max(1) as f64;
    let x = along(r.x, size.0, m.width).floor() as i32;
    let y = along(r.y, size.1, m.height).floor() as i32;
    let right = along(r.x + r.w, size.0, m.width).ceil() as i32;
    let bottom = along(r.y + r.h, size.1, m.height).ceil() as i32;
    (
        m.x + x,
        m.y + y,
        (right - x).max(1) as u32,
        (bottom - y).max(1) as u32,
    )
}

/// Rectángulo que abarca todos los monitores: (x mín., y mín., x máx., y máx.).
fn bounds(screens: &[monitors::Monitor]) -> (i32, i32, i32, i32) {
    screens.iter().fold(
        (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
        |(x0, y0, x1, y1), m| {
            (
                x0.min(m.x),
                y0.min(m.y),
                x1.max(m.x + m.width as i32),
                y1.max(m.y + m.height as i32),
            )
        },
    )
}

/// Pasa la zona de píxeles de la ventana (`size`) a los del monitor dentro de la captura.
fn to_capture(r: Rect, size: (u32, u32), screen: Rect) -> Rect {
    let along = |v: u32, from: u32, to: u32| (v as u64 * to as u64 / from.max(1) as u64) as u32;
    let x = along(r.x, size.0, screen.w);
    let y = along(r.y, size.1, screen.h);
    Rect {
        x: screen.x + x,
        y: screen.y + y,
        w: along(r.x + r.w, size.0, screen.w) - x,
        h: along(r.y + r.h, size.1, screen.h) - y,
    }
}

/// Cuentagotas sobre una captura congelada: copia el color en el formato del editor.
fn pick_color(runtime: &Runtime) -> anyhow::Result<()> {
    let capture = runtime.block_on(capture::capture_full())?;
    let screens = screens(runtime);
    let selectors = monitors::layout(&screens, capture.image.dimensions())
        .iter()
        .map(|r| AreaSelector::color(capture.crop(*r)).keeping(std::sync::Arc::new(keep_color)))
        .collect();
    if let Some((_, SelectorResult::Color(rgb), _)) =
        windows::run_selector(selectors, &screens, None)?
    {
        let text = colorshot_ui::editor::color_text(rgb);
        clipboard::copy_text(&text)?;
        // A «Picked colors» de la ventana de paletas; por la salida, para quien lo lanzó.
        save_picked(rgb);
        println!("#{}", palettes::hex(rgb));
        let _ = std::process::Command::new("notify-send")
            .args(["--app-name=colorshot", t("Color copied"), &text])
            .spawn()
            .and_then(|mut child| child.wait());
    }
    Ok(())
}

fn save_picked(rgb: palettes::Rgb) {
    let mut store = palettes::Palettes::load();
    store.add_picked(rgb);
    if let Err(error) = store.save() {
        log::warn!("Could not save the picked color: {error}");
    }
}

/// Clic derecho en el cuentagotas: el color va a «Picked colors» y, con `+` delante, a la
/// salida (la ventana de paletas lo añade a la paleta abierta). El cuentagotas sigue.
fn keep_color(rgb: palettes::Rgb) {
    use std::io::Write as _;
    save_picked(rgb);
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "+#{}", palettes::hex(rgb));
    let _ = out.flush();
}

/// Se pulsa una ventana sobre el escritorio vivo (sus marcos los da la extensión de GNOME);
/// sin la extensión, la captura interactiva del portal.
fn window(runtime: &Runtime) -> anyhow::Result<()> {
    let frames = match runtime.block_on(monitors::gnome_windows()) {
        Ok(frames) => frames,
        Err(error) => {
            log::warn!("Window list unavailable ({error}); using the portal");
            let capture = runtime.block_on(capture::capture_interactive()).map_err(|error| {
                log::warn!("Interactive portal failed: {error}");
                anyhow::anyhow!(
                    t("Capture Window needs the ColorShot GNOME extension. Run just install and log in again.")
                        .to_string()
                )
            })?;
            return deliver(capture.image, None, Warm::none());
        }
    };
    let warm = Warm::start();
    let screens = screens(runtime);
    let origins: Vec<(i32, i32)> = if screens.is_empty() {
        vec![(0, 0)]
    } else {
        screens.iter().map(|m| (m.x, m.y)).collect()
    };
    let selectors = origins
        .iter()
        .map(|(ox, oy)| {
            AreaSelector::window(
                frames
                    .iter()
                    .map(|&(x, y, w, h)| {
                        egui::Rect::from_min_size(
                            egui::pos2((x - ox) as f32, (y - oy) as f32),
                            egui::vec2(w as f32, h as f32),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    let Some((_, SelectorResult::Area { .. }, Some((image, from)))) =
        windows::run_selector(selectors, &screens, Some(grabber(runtime, &screens)))?
    else {
        return Ok(());
    };
    deliver(image, Some(from), warm)
}

fn ocr(runtime: &Runtime) -> anyhow::Result<()> {
    let Some((image, from)) = select(runtime)? else {
        return Ok(());
    };
    // Con XWayland, la lectura se ve sobre la zona (ver `windows::run_scan`).
    if std::env::var_os("DISPLAY").is_some() {
        let temp = temp_png(&image)?;
        return spawn_with_from("scan", &temp, Some(from), &[]);
    }
    notify_result(&recognize::recognize(&image)?).map(|_| ())
}

/// Copia lo reconocido y lo anuncia con una notificación; devuelve el título del aviso.
pub fn notify_result(found: &Recognized) -> anyhow::Result<&'static str> {
    let (title, text) = match found {
        Recognized::Code(text) => (t("QR code copied"), text),
        Recognized::Text(text) => (t("Text copied"), text),
        Recognized::Nothing => anyhow::bail!(t("No text or codes found").to_string()),
    };
    clipboard::copy_text(text)?;
    let preview: String = text.chars().take(200).collect();
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=colorshot", title, &preview])
        .spawn()
        .and_then(|mut child| child.wait());
    Ok(title)
}

/// Con un monitor o «All screens», todo el escritorio; si no, el monitor del puntero o el
/// que se pulse, capturado con las ventanas del selector ya vacías.
fn full(runtime: &Runtime, mode: ScreenCapture) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let warm = Warm::start();
    let screens = screens(runtime);
    if screens.len() > 1 && mode != ScreenCapture::All {
        let selectors = screens
            .iter()
            .map(|m| AreaSelector::screen(mode == ScreenCapture::Pick, m.primary))
            .collect();
        let grab = Some(grabber(runtime, &screens));
        return match windows::run_selector(selectors, &screens, grab)? {
            Some((monitor, SelectorResult::Area { .. } | SelectorResult::All, Some(shot))) => {
                log::info!("full: monitor {monitor} in {:?}", started.elapsed());
                let (image, from) = shot;
                deliver(image, Some(from), warm)
            }
            _ => Ok(()),
        };
    }
    let capture = runtime.block_on(capture::capture_full())?;
    log::info!("full: captured in {:?}", started.elapsed());
    let (w, h) = capture.image.dimensions();
    deliver(
        capture.image,
        Some((Rect { x: 0, y: 0, w, h }, (w, h))),
        warm,
    )
}

fn repeat(runtime: &Runtime) -> anyhow::Result<()> {
    let rect: Rect = save::load_last_area().context(t("Capture an area first"))?;
    let warm = Warm::start();
    let capture = runtime.block_on(capture::capture_full())?;
    let image = capture.crop(rect);
    anyhow::ensure!(
        image.width() > 0 && image.height() > 0,
        "{}",
        t("The selected area is empty")
    );
    deliver(image, Some((rect, capture.image.dimensions())), warm)
}

/// Guarda, copia y muestra la miniatura según las preferencias; desde ella se puede editar.
///
/// Con miniatura, todo pasa a su proceso (`warm`, ya arrancado si se pudo): recibe los píxeles
/// en crudo y guarda mientras se muestra; aquí solo se copia, con un PNG rápido.
fn deliver(image: image::RgbaImage, from: Option<From>, warm: Warm) -> anyhow::Result<()> {
    let settings = Settings::load();
    if !settings.show_after_capture {
        if settings.save_after_capture {
            let path = save::save_with(&image, settings.save_dir()?, settings.format)?;
            println!("{}", path.display());
        }
        if settings.copy_after_capture {
            clipboard::copy_image(&image)?;
        }
        return Ok(());
    }
    let started = std::time::Instant::now();
    // Se copia aquí, antes de que aparezca la miniatura: en GNOME wl-copy necesita el foco y
    // una ventana nueva se lo quita (queda esperando con «wl-clipboard is ready»).
    let copied = settings.copy_after_capture
        && match save::encode_fast(&image)
            .map_err(anyhow::Error::from)
            .and_then(|png| Ok(clipboard::copy_png(&png)?))
        {
            Ok(()) => true,
            Err(error) => {
                log::warn!("Could not copy capture: {error}");
                false
            }
        };
    log::info!("deliver: copied in {:?}", started.elapsed());
    warm.send(&image, from, copied)?;
    log::info!("deliver: sent in {:?}", started.elapsed());
    Ok(())
}

/// PNG rápido en el directorio de la sesión, para pasarle la imagen a otro proceso.
fn temp_png(image: &image::RgbaImage) -> anyhow::Result<std::path::PathBuf> {
    save_temp(&save::encode_fast(image)?)
}

fn save_temp(png: &[u8]) -> anyhow::Result<std::path::PathBuf> {
    let dir = save::temp_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(save::save_bytes(png, "png", dir)?)
}

/// `colorshot <verb> <png>` con la zona de origen en `windows::FROM`.
fn spawn_with_from(
    verb: &str,
    path: &std::path::Path,
    from: Option<From>,
    env: &[(&str, &str)],
) -> anyhow::Result<()> {
    let mut command = std::process::Command::new(crate::exe()?);
    command
        .args([verb.as_ref(), path.as_os_str()])
        .env(crate::CHILD, "1")
        .envs(env.iter().copied());
    if let Some((r, full)) = from {
        command.env(
            windows::FROM,
            format!("{},{},{},{},{},{}", r.x, r.y, r.w, r.h, full.0, full.1),
        );
    }
    command.spawn()?;
    Ok(())
}

// Diálogo de archivo con zenity (GNOME); sin él no hay selector disponible.
fn pick_file() -> anyhow::Result<Option<std::path::PathBuf>> {
    let output = std::process::Command::new("zenity")
        .args([
            "--file-selection",
            &format!("--title={}", t("Open File")),
            "--file-filter=Images | *.png *.jpg *.jpeg",
        ])
        .output()
        .context(t("Install zenity to open images"))?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((output.status.success() && !path.is_empty()).then(|| path.into()))
}

// En Wayland, wl-paste; si falla, arboard (X11/XWayland).
fn clipboard_image() -> anyhow::Result<image::RgbaImage> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "image/png"])
        .output()
        && output.status.success()
        && let Ok(image) = image::load_from_memory(&output.stdout)
    {
        return Ok(image.into_rgba8());
    }
    let data = arboard::Clipboard::new()
        .and_then(|mut c| c.get_image())
        .context(t("The clipboard has no image"))?;
    image::RgbaImage::from_raw(
        data.width as u32,
        data.height as u32,
        data.bytes.into_owned(),
    )
    .context(t("The clipboard has no image"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_zone_maps_into_the_capture() {
        // Ventana de 1920×1080 físicos sobre un monitor que ocupa 2880×1620 en la captura.
        let screen = Rect {
            x: 2560,
            y: 0,
            w: 2880,
            h: 1620,
        };
        let zone = Rect {
            x: 100,
            y: 200,
            w: 640,
            h: 360,
        };
        assert_eq!(
            to_capture(zone, (1920, 1080), screen),
            Rect {
                x: 2710,
                y: 300,
                w: 960,
                h: 540
            }
        );
    }

    #[test]
    fn zone_rounds_out_to_logical_pixels() {
        // Segundo monitor a escala 2: la ventana mide 3840×2400 y Mutter 1920×1200.
        let m = monitors::Monitor {
            connector: "HDMI-2".into(),
            x: 2560,
            y: 0,
            width: 1920,
            height: 1200,
            primary: false,
        };
        let zone = Rect {
            x: 101,
            y: 50,
            w: 200,
            h: 101,
        };
        // 50,5 lógicos empieza en 50; 150,5 termina en 151.
        assert_eq!(logical(zone, (3840, 2400), &m), (2610, 25, 101, 51));
        let primary = monitors::Monitor {
            connector: "DP-4".into(),
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
            primary: true,
        };
        assert_eq!(bounds(&[primary, m]), (0, 0, 4480, 1440));
    }
}
