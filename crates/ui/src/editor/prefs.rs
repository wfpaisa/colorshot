//! Lo último elegido en cada herramienta, guardado en `~/.config/colorshot/editor.ron`.
use super::{Tool, color, model::Style};
use egui::Color32;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub styles: BTreeMap<Tool, Style>,
    pub erase: bool,
    pub oval_spotlight: bool,
    pub color_format: color::Format,
    pub options_open: bool,
    pub logical: bool,
    /// Herramienta con la que abre el editor: la última que se eligió.
    pub tool: Tool,
    /// Color propio de la paleta, el último círculo.
    pub custom: Option<Color32>,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            styles: BTreeMap::new(),
            erase: false,
            oval_spotlight: false,
            color_format: color::Format::default(),
            options_open: true,
            logical: false,
            tool: Tool::Arrow,
            custom: None,
        }
    }
}

/// Estilo de partida de una herramienta que aún no se ha tocado.
pub fn default_style(tool: Tool) -> Style {
    match tool {
        Tool::Highlighter => Style {
            color: Color32::from_rgb(255, 214, 10),
            ..Style::default()
        },
        _ => Style::default(),
    }
}

fn path() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("colorshot").join("editor.ron"))
}

impl Prefs {
    /// Si falta o no se puede leer, valores por defecto.
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        let Some(path) = path() else {
            return;
        };
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                ron::ser::to_string_pretty(self, Default::default()).map_err(|e| e.to_string())
            })
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
        if let Err(error) = result {
            log::warn!("Could not save editor preferences: {error}");
        }
    }
    pub fn style(&self, tool: Tool) -> Style {
        self.styles
            .get(&tool)
            .copied()
            .unwrap_or_else(|| default_style(tool))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_through_ron() {
        let mut prefs = Prefs::default();
        prefs.styles.insert(
            Tool::Arrow,
            Style {
                width: 9.,
                ..Style::default()
            },
        );
        prefs.color_format = color::Format::Css;
        prefs.tool = Tool::Text;
        let text = ron::ser::to_string_pretty(&prefs, Default::default()).unwrap();
        assert_eq!(ron::from_str::<Prefs>(&text).unwrap(), prefs);
        assert_eq!(prefs.style(Tool::Arrow).width, 9.);
        assert_eq!(
            prefs.style(Tool::Highlighter),
            default_style(Tool::Highlighter)
        );
    }
}
