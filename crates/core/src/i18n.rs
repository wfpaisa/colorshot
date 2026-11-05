//! Textos de la interfaz en ocho idiomas. La clave es el texto en inglés; `strings.tsv` trae
//! una línea por texto con las columnas de `Language::ALL` (sin `System`).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// El del escritorio (`LC_ALL`, `LC_MESSAGES` o `LANG`); inglés si no está.
    #[default]
    System,
    English,
    Spanish,
    Portuguese,
    French,
    German,
    Russian,
    Chinese,
    Japanese,
}

impl Language {
    pub const ALL: [Self; 9] = [
        Self::System,
        Self::English,
        Self::Spanish,
        Self::Portuguese,
        Self::French,
        Self::German,
        Self::Russian,
        Self::Chinese,
        Self::Japanese,
    ];
    /// Nombre en su propio idioma, como se ve en el selector.
    pub fn name(self) -> &'static str {
        match self {
            Self::System => t("System"),
            Self::English => "English",
            Self::Spanish => "Español",
            Self::Portuguese => "Português",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Russian => "Русский",
            Self::Chinese => "简体中文",
            Self::Japanese => "日本語",
        }
    }
    /// Código ISO 639-1.
    pub fn code(self) -> &'static str {
        match self.resolve() {
            Self::Spanish => "es",
            Self::Portuguese => "pt",
            Self::French => "fr",
            Self::German => "de",
            Self::Russian => "ru",
            Self::Chinese => "zh",
            Self::Japanese => "ja",
            _ => "en",
        }
    }
    /// Necesita una fuente CJK del sistema (egui no la trae).
    pub fn cjk(self) -> bool {
        matches!(self.resolve(), Self::Chinese | Self::Japanese)
    }
    fn resolve(self) -> Self {
        if self != Self::System {
            return self;
        }
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        from_locale(&locale)
    }
    // Columna en `strings.tsv`; 0 = inglés.
    fn column(self) -> usize {
        Self::ALL
            .iter()
            .position(|l| *l == self.resolve())
            .map_or(0, |i| i.saturating_sub(1))
    }
}

fn from_locale(locale: &str) -> Language {
    match locale.get(..2).unwrap_or("") {
        "es" => Language::Spanish,
        "pt" => Language::Portuguese,
        "fr" => Language::French,
        "de" => Language::German,
        "ru" => Language::Russian,
        "zh" => Language::Chinese,
        "ja" => Language::Japanese,
        _ => Language::English,
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Idioma de los textos de este proceso.
pub fn set(language: Language) {
    CURRENT.store(language.column() as u8, Ordering::Relaxed);
}

/// Idioma en uso, ya resuelto (nunca `System`).
pub fn current() -> Language {
    Language::ALL[CURRENT.load(Ordering::Relaxed) as usize + 1]
}

type Table = HashMap<&'static str, Vec<String>>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| parse(include_str!("strings.tsv")))
}

// `\n` escrito en el archivo es un salto de línea en el texto.
fn parse(text: &'static str) -> Table {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let key = columns.next()?;
            let key: &'static str = if key.contains("\\n") {
                Box::leak(key.replace("\\n", "\n").into_boxed_str())
            } else {
                key
            };
            Some((key, columns.map(|c| c.replace("\\n", "\n")).collect()))
        })
        .collect()
}

/// Marca un texto para traducirlo más tarde con `t` (en tablas `const`).
pub const fn n(en: &'static str) -> &'static str {
    en
}

/// El texto en el idioma actual; si no está traducido, el original.
pub fn t(en: &str) -> &str {
    let column = CURRENT.load(Ordering::Relaxed) as usize;
    if column == 0 {
        return en;
    }
    table()
        .get(en)
        .and_then(|row| row.get(column - 1))
        .filter(|s| !s.is_empty())
        .map_or(en, String::as_str)
}

/// `t` con cada `{}` reemplazado, en orden, por `args`.
pub fn tf(en: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut parts = t(en).split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for part in parts {
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        out.push_str(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_all_languages_and_placeholders() {
        let raw = include_str!("strings.tsv");
        for line in raw.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let columns: Vec<&str> = line.split('\t').collect();
            assert_eq!(columns.len(), 8, "{line}");
            let holes = columns[0].matches("{}").count();
            for c in &columns {
                assert!(!c.trim().is_empty(), "{line}");
                assert_eq!(c.matches("{}").count(), holes, "{line}");
            }
        }
    }

    // Cada texto pasado a `t`, `tf` o `n` en el código tiene su línea en la tabla.
    #[test]
    fn every_used_text_is_translated() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut missing = Vec::new();
        let mut stack = vec![root.join("crates"), root.join("apps")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    // Las pruebas no muestran textos.
                    let source = source.split("#[cfg(test)]").next().unwrap_or_default();
                    for start in ["t(", "tf(", "n("] {
                        for (i, _) in source.match_indices(start) {
                            let before = source[..i].chars().last();
                            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                                continue;
                            }
                            let Some(rest) =
                                source[i + start.len()..].trim_start().strip_prefix('"')
                            else {
                                continue;
                            };
                            let Some(end) = rest.find('"') else { continue };
                            let key = rest[..end].replace("\\n", "\n");
                            if !table().contains_key(key.as_str()) {
                                missing.push(key);
                            }
                        }
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "missing translations: {missing:#?}");
    }

    #[test]
    fn locale_and_placeholders() {
        assert_eq!(from_locale("es_CO.UTF-8"), Language::Spanish);
        assert_eq!(from_locale("C"), Language::English);
        assert_eq!(Language::German.column(), 4);
        set(Language::Spanish);
        assert_eq!(t("Copy"), "Copiar");
        assert_eq!(tf("Copied {}", &[&"#FFF"]), "Copiado #FFF");
        let unknown = String::from("not in the table");
        assert_eq!(t(&unknown), unknown);
        set(Language::English);
        assert_eq!(t("Copy"), "Copy");
    }
}
