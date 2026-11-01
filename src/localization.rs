//! Complete built-in interface catalogs. User data and technical details stay untouched.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "it")]
    Italian,
    #[default]
    #[serde(rename = "en", other)]
    English,
}
impl Language {
    pub const ALL: [Self; 6] = [
        Self::English,
        Self::Russian,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Italian,
    ];
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Italian => "Italiano",
        }
    }
    pub fn text(self, english: &'static str) -> &'static str {
        let index = match self {
            Self::English => return english,
            Self::Russian => 0,
            Self::French => 1,
            Self::German => 2,
            Self::Spanish => 3,
            Self::Italian => 4,
        };
        catalog()
            .get(english)
            .map_or(english, |row| row[index].as_str())
    }
}
fn catalog() -> &'static BTreeMap<String, [String; 5]> {
    static CATALOG: OnceLock<BTreeMap<String, [String; 5]>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("locales.json"))
            .expect("Validated built-in localization catalog")
    })
}

/// Store message identity and values so a language change also updates existing status.
#[derive(Clone, Debug)]
pub enum Message {
    Text(&'static str),
    Values(&'static str, Vec<(&'static str, String)>),
    /// Diagnostic details returned by the OS, media codecs or network are preserved.
    Details(String),
}
impl Message {
    pub fn with<const N: usize>(key: &'static str, values: [(&'static str, String); N]) -> Self {
        Self::Values(key, values.into())
    }
    pub fn render(&self, language: Language) -> String {
        match self {
            Self::Text(key) => language.text(key).to_owned(),
            Self::Values(key, values) => {
                let mut template = language.text(key);
                let mut text = String::with_capacity(template.len());
                while let Some(start) = template.find('{') {
                    text.push_str(&template[..start]);
                    let Some(end) = template[start..].find('}').map(|end| start + end) else {
                        text.push_str(&template[start..]);
                        return text;
                    };
                    let name = &template[start + 1..end];
                    if let Some((_, value)) = values.iter().find(|(key, _)| *key == name) {
                        text.push_str(value);
                    } else {
                        text.push_str(&template[start..=end]);
                    }
                    template = &template[end + 1..];
                }
                text.push_str(template);
                text
            }
            Self::Details(detail) => detail.clone(),
        }
    }
}
impl From<&'static str> for Message {
    fn from(value: &'static str) -> Self {
        Self::Text(value)
    }
}
impl From<String> for Message {
    fn from(value: String) -> Self {
        Self::Details(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn placeholders(text: &str) -> Vec<&str> {
        let mut values: Vec<_> = text
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .collect();
        values.sort_unstable();
        values
    }
    #[test]
    fn every_translation_is_complete_and_preserves_parameters() {
        assert!(catalog().len() >= 140);
        for (english, translations) in catalog() {
            for translation in translations {
                assert!(!translation.trim().is_empty(), "Missing: {english}");
                assert_eq!(
                    placeholders(english),
                    placeholders(translation),
                    "Parameters changed: {english}"
                );
            }
        }
    }
    #[test]
    fn language_switch_reformats_status_without_changing_user_values() {
        let message = Message::with(
            "Recording saved: {path}",
            [("path", "C:/Recordings/Settings.srec".into())],
        );
        assert_eq!(
            message.render(Language::English),
            "Recording saved: C:/Recordings/Settings.srec"
        );
        assert_eq!(
            message.render(Language::Russian),
            "Запись сохранена: C:/Recordings/Settings.srec"
        );
        assert_eq!(
            Language::French.text("Unknown future message"),
            "Unknown future message"
        );
        let nested = Message::with(
            "{detail}\nAny complete frames can be opened from {path}",
            [
                ("detail", "OS error mentions {path}".into()),
                ("path", "C:/Recordings/Keep.srec".into()),
            ],
        );
        assert!(
            nested
                .render(Language::German)
                .starts_with("OS error mentions {path}\n")
        );
    }
}
