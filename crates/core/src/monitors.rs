//! Geometría de los monitores y su ubicación dentro de la captura completa.
use crate::capture::Rect;
use ashpd::zvariant::OwnedValue;
use std::collections::HashMap;

/// Monitor en coordenadas lógicas del escritorio.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub connector: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

type Props = HashMap<String, OwnedValue>;
type Spec = (String, String, String, String);
type Mode = (String, i32, i32, f64, f64, Vec<f64>, Props);
type Physical = (Spec, Vec<Mode>, Props);
type Logical = (i32, i32, f64, u32, bool, Vec<Spec>, Props);
type State = (u32, Vec<Physical>, Vec<Logical>, Props);

/// Lee la disposición actual desde Mutter (GNOME); otros escritorios devuelven error.
pub async fn gnome_monitors() -> Result<Vec<Monitor>, ashpd::zbus::Error> {
    let connection = ashpd::zbus::Connection::session().await?;
    let reply = connection
        .call_method(
            Some("org.gnome.Mutter.DisplayConfig"),
            "/org/gnome/Mutter/DisplayConfig",
            Some("org.gnome.Mutter.DisplayConfig"),
            "GetCurrentState",
            &(),
        )
        .await?;
    let (_, physical, logical, props): State = reply.body().deserialize()?;
    // layout-mode 1 = lógico (tamaño = modo / escala); 2 = físico.
    let logical_layout = props
        .get("layout-mode")
        .and_then(|v| u32::try_from(v).ok())
        .is_none_or(|m| m == 1);
    let mut monitors = Vec::new();
    for (x, y, scale, transform, primary, specs, _) in logical {
        let Some(spec) = specs.first() else { continue };
        let Some(mode) = physical
            .iter()
            .find(|(s, _, _)| s.0 == spec.0)
            .and_then(|(_, modes, _)| modes.iter().find(|m| m.6.contains_key("is-current")))
        else {
            continue;
        };
        let divisor = if logical_layout { scale.max(0.1) } else { 1.0 };
        let (mut w, mut h) = (mode.1 as f64 / divisor, mode.2 as f64 / divisor);
        // Rotaciones de 90°/270° (y sus espejos) intercambian ancho y alto.
        if transform % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        monitors.push(Monitor {
            connector: spec.0.clone(),
            x,
            y,
            width: w.round() as u32,
            height: h.round() as u32,
            primary,
        });
    }
    Ok(monitors)
}

/// Marco de cada ventana visible (x, y, ancho, alto en coordenadas lógicas), de la de arriba a
/// la de abajo. Lo da la extensión de GNOME `rapidshot@wfelipe.com`.
pub async fn gnome_windows() -> Result<Vec<(i32, i32, i32, i32)>, ashpd::zbus::Error> {
    let connection = ashpd::zbus::Connection::session().await?;
    let reply = connection
        .call_method(
            Some("com.wfelipe.RapidShot"),
            "/com/wfelipe/RapidShot",
            Some("com.wfelipe.RapidShot"),
            "Windows",
            &(),
        )
        .await?;
    reply.body().deserialize()
}

/// Ubica cada monitor en píxeles de la imagen: el portal entrega el escritorio lógico
/// escalado por un único factor (p. ej. ×1,5 con escala fraccionaria).
pub fn layout(monitors: &[Monitor], image: (u32, u32)) -> Vec<Rect> {
    let full = vec![Rect {
        x: 0,
        y: 0,
        w: image.0,
        h: image.1,
    }];
    let (Some(min_x), Some(max_x)) = (
        monitors.iter().map(|m| m.x).min(),
        monitors.iter().map(|m| m.x + m.width as i32).max(),
    ) else {
        return full;
    };
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0);
    if max_x <= min_x {
        return full;
    }
    let s = image.0 as f64 / (max_x - min_x) as f64;
    let px = |v: f64, limit: u32| ((v * s).round().max(0.0) as u32).min(limit);
    monitors
        .iter()
        .map(|m| {
            let x = px((m.x - min_x) as f64, image.0);
            let y = px((m.y - min_y) as f64, image.1);
            Rect {
                x,
                y,
                w: px(m.width as f64, image.0 - x),
                h: px(m.height as f64, image.1 - y),
            }
        })
        .collect()
}

/// Monitor tal como lo ve XWayland, en píxeles X (con escalado propio de Xwayland, p. ej. ×2).
#[derive(Clone, Debug, PartialEq)]
pub struct XMonitor {
    pub connector: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Lee `xrandr --listmonitors`; vacío si no hay XWayland.
pub fn x11_monitors() -> Vec<XMonitor> {
    std::process::Command::new("xrandr")
        .arg("--listmonitors")
        .output()
        .map(|o| parse_xrandr(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

// ` 0: +*DP-4 5120/600x2880/340+0+0  DP-4` → conector, tamaño y origen.
fn parse_xrandr(text: &str) -> Vec<XMonitor> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace().skip(2);
            let geometry = words.next()?;
            let connector = words.next()?.to_string();
            let (size, origin) = geometry.split_once('+')?;
            let (x, y) = origin.split_once('+')?;
            let (w, h) = size.split_once('x')?;
            let number = |v: &str| v.split('/').next()?.parse::<u32>().ok();
            Some(XMonitor {
                connector,
                x: x.parse().ok()?,
                y: y.parse().ok()?,
                width: number(w)?,
                height: number(h)?,
            })
        })
        .collect()
}

/// Un monitor con su zona en la captura completa y en coordenadas X.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub connector: String,
    pub primary: bool,
    pub image: Rect,
    pub x11: XMonitor,
}

impl Screen {
    /// Punto de la captura (píxeles de imagen) → coordenadas X.
    pub fn to_x11(&self, p: (f32, f32)) -> (f32, f32) {
        let sx = self.x11.width as f32 / self.image.w.max(1) as f32;
        let sy = self.x11.height as f32 / self.image.h.max(1) as f32;
        (
            self.x11.x as f32 + (p.0 - self.image.x as f32) * sx,
            self.x11.y as f32 + (p.1 - self.image.y as f32) * sy,
        )
    }
}

/// Junta Mutter, la captura y XWayland por conector. Sin Mutter, un único monitor X ocupa
/// toda la imagen.
pub fn screens(monitors: &[Monitor], image: (u32, u32), x11: &[XMonitor]) -> Vec<Screen> {
    if monitors.is_empty() {
        return match x11 {
            [only] => vec![Screen {
                connector: only.connector.clone(),
                primary: true,
                image: layout(&[], image)[0],
                x11: only.clone(),
            }],
            _ => Vec::new(),
        };
    }
    monitors
        .iter()
        .zip(layout(monitors, image))
        .filter_map(|(m, rect)| {
            Some(Screen {
                connector: m.connector.clone(),
                primary: m.primary,
                image: rect,
                x11: x11.iter().find(|x| x.connector == m.connector)?.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn monitor(x: i32, width: u32, height: u32) -> Monitor {
        Monitor {
            connector: String::new(),
            x,
            y: 0,
            width,
            height,
            primary: false,
        }
    }
    #[test]
    fn xrandr_and_capture_line_up_by_connector() {
        let x11 = parse_xrandr(
            "Monitors: 2\n 0: +*DP-4 5120/600x2880/340+0+0  DP-4\n 1: +HDMI-2 3840/520x2400/320+5120+0  HDMI-2\n",
        );
        assert_eq!(x11.len(), 2);
        assert_eq!((x11[1].x, x11[1].width, x11[1].height), (5120, 3840, 2400));
        let mutter = [
            Monitor {
                connector: "DP-4".into(),
                primary: true,
                ..monitor(0, 2560, 1440)
            },
            Monitor {
                connector: "HDMI-2".into(),
                ..monitor(2560, 1920, 1200)
            },
        ];
        let screens = screens(&mutter, (6720, 2160), &x11);
        assert_eq!(
            screens[1].to_x11((3840. + 1440., 900.)),
            (5120. + 3840. / 2., 1200.)
        );
        assert_eq!(screens[0].to_x11((3840., 2160.)), (5120., 2880.));
    }
    #[test]
    fn fractional_scale_layout() {
        // 4K a 1,5 (2560×1440 lógico) + 1920×1200 a su derecha → imagen 6720×2160.
        let rects = layout(
            &[monitor(0, 2560, 1440), monitor(2560, 1920, 1200)],
            (6720, 2160),
        );
        assert_eq!(
            rects,
            vec![
                Rect {
                    x: 0,
                    y: 0,
                    w: 3840,
                    h: 2160
                },
                Rect {
                    x: 3840,
                    y: 0,
                    w: 2880,
                    h: 1800
                }
            ]
        );
    }
    #[test]
    fn no_monitors_covers_whole_image() {
        assert_eq!(
            layout(&[], (10, 20)),
            vec![Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 20
            }]
        );
    }
}
