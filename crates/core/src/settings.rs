//! Preferencias persistentes en ~/.config/colorshot/settings.ron.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Png,
    /// PNG salvo que pese mucho (fotos): entonces JPEG.
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AreaShows {
    #[default]
    Thumbnail,
    Editor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscAction {
    Nothing,
    #[default]
    Copy,
    Save,
}

/// Qué toma «Capture Screen» con varios monitores.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenCapture {
    /// El monitor con el puntero.
    #[default]
    Cursor,
    /// Se elige con un clic; A toma todos.
    Pick,
    All,
}

/// Monitor donde aparece la miniatura.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThumbnailMonitor {
    /// El del puntero (o el de la zona capturada).
    #[default]
    Cursor,
    Primary,
    /// Por conector de Mutter («DP-4»); si no está conectado, el principal.
    Named(String),
}

/// Lugar de la miniatura dentro del monitor: esquinas, centros de los lados o el centro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    #[default]
    BottomRight,
}

impl Placement {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];
    /// Alineación por eje: 0 = inicio, 0,5 = centro, 1 = final.
    pub fn align(self) -> (f32, f32) {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(8);
        ((i % 3) as f32 / 2., (i / 3) as f32 / 2.)
    }
}

/// Límites de `glass_tint`: por debajo no se lee el texto; por encima no se ve el fondo.
pub const GLASS_TINT: (u8, u8) = (20, 95);

/// Frosted glass listo para pintar: `tint` es cuánto color va sobre el fondo y `shade` cuánto
/// se oscurece ese color hacia el negro, ambos de 0 a 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub tint: f32,
    pub shade: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` = ~/Pictures/Screenshots.
    pub save_dir: Option<PathBuf>,
    pub format: Format,
    pub copy_after_capture: bool,
    pub save_after_capture: bool,
    pub show_after_capture: bool,
    pub area_shows: AreaShows,
    pub screen_capture: ScreenCapture,
    /// Segundos hasta ocultar la miniatura; 0 = manualmente.
    pub thumbnail_seconds: u32,
    pub thumbnail_monitor: ThumbnailMonitor,
    pub thumbnail_placement: Placement,
    pub esc_action: EscAction,
    pub language: crate::i18n::Language,
    /// Conservar las capturas temporales (en memoria, hasta reiniciar) en vez de borrarlas
    /// al cerrar la miniatura.
    pub keep_temp: bool,
    /// Fondo de las ventanas desenfocado por el compositor (`ext_background_effect_v1`).
    pub frosted_glass: bool,
    /// Con frosted glass, opacidad del color del tema sobre el fondo, en %.
    pub glass_tint: u8,
    /// Con frosted glass, del gris del tema (0) al negro (100), en %.
    pub glass_shade: u8,
    /// Ya se mostró el asistente de bienvenida (se abre al primer arranque de la instancia).
    pub welcome_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_dir: None,
            format: Format::Png,
            copy_after_capture: true,
            save_after_capture: true,
            show_after_capture: true,
            area_shows: AreaShows::Thumbnail,
            screen_capture: ScreenCapture::Cursor,
            thumbnail_seconds: 7,
            thumbnail_monitor: ThumbnailMonitor::Cursor,
            thumbnail_placement: Placement::BottomRight,
            esc_action: EscAction::Copy,
            language: crate::i18n::Language::System,
            keep_temp: false,
            frosted_glass: false,
            glass_tint: 60,
            glass_shade: 0,
            welcome_done: false,
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("colorshot").join("settings.ron"))
}

impl Settings {
    /// Si falta o no se puede leer, valores por defecto.
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), crate::save::SaveError> {
        let path = path().ok_or(crate::save::SaveError::NoConfig)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text)?;
        Ok(())
    }
    /// Tinte y oscuridad del frosted glass, si está activado.
    pub fn glass(&self) -> Option<Glass> {
        self.frosted_glass.then(|| Glass {
            tint: f32::from(self.glass_tint.clamp(GLASS_TINT.0, GLASS_TINT.1)) / 100.,
            shade: f32::from(self.glass_shade.min(100)) / 100.,
        })
    }
    pub fn save_dir(&self) -> Result<PathBuf, crate::save::SaveError> {
        match &self.save_dir {
            Some(dir) => Ok(dir.clone()),
            None => crate::save::default_dir(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_fields_take_defaults() {
        let parsed: Settings = ron::from_str("(format: Auto)").unwrap();
        assert_eq!(parsed.format, Format::Auto);
        assert_eq!(parsed.thumbnail_seconds, 7);
        assert_eq!(Placement::TopLeft.align(), (0., 0.));
        assert_eq!(Placement::Right.align(), (1., 0.5));
        assert_eq!(Placement::Bottom.align(), (0.5, 1.));
        let text = ron::to_string(&Settings::default()).unwrap();
        assert_eq!(
            ron::from_str::<Settings>(&text).unwrap(),
            Settings::default()
        );
    }
}
