//! Agent-authored catalogs. Only English is native-reviewed; other locales are explicitly unreviewed.
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock};

pub const LOCALES: &[&str] = &[
    "en", "de", "es", "fr", "pt", "it", "zh-Hans", "ja", "hi", "ar", "ru", "tr",
];
static CATALOGS: LazyLock<BTreeMap<&str, Value>> = LazyLock::new(|| {
    macro_rules! catalogs { ($($l:literal),*) => { BTreeMap::from([$(($l,
        serde_json::from_str(include_str!(concat!("messages/",$l,".json"))).expect("valid search catalog"))),*]) }; }
    catalogs!(
        "en", "de", "es", "fr", "pt", "it", "zh-Hans", "ja", "hi", "ar", "ru", "tr"
    )
});

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    pub key: String,
    pub params: Value,
}
impl Message {
    pub fn new(key: &str, params: Value) -> Self {
        Self {
            key: format!("search.{key}"),
            params,
        }
    }
    pub fn text(&self, lang: &str) -> String {
        let key = self.key.strip_prefix("search.").unwrap_or(&self.key);
        let catalog = CATALOGS
            .get(lang)
            .or_else(|| CATALOGS.get(lang.split('-').next().unwrap_or("en")))
            .unwrap_or(&CATALOGS["en"]);
        let mut text = catalog["messages"][key]
            .as_str()
            .or_else(|| CATALOGS["en"]["messages"][key].as_str())
            .unwrap_or(key)
            .to_owned();
        if let Some(params) = self.params.as_object() {
            for (k, v) in params {
                text = text.replace(
                    &format!("{{{k}}}"),
                    &v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()),
                );
            }
        }
        text
    }
    pub fn rendered(&self, lang: &str) -> Value {
        json!({"key":self.key,"params":self.params,"text":self.text(lang),"fallback":self.text("en")})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogs_have_identical_keys_and_parameters_and_review_flags() {
        let en = CATALOGS["en"]["messages"].as_object().unwrap();
        fn placeholders(s: &str) -> Vec<String> {
            let mut p: Vec<_> = s
                .split('{')
                .skip(1)
                .map(|s| s.split('}').next().unwrap().to_owned())
                .collect();
            p.sort();
            p
        }
        for &locale in LOCALES {
            let catalog = &CATALOGS[locale];
            assert_eq!(catalog["native_reviewed"], locale == "en");
            let values = catalog["messages"].as_object().unwrap();
            assert_eq!(values.keys().collect::<Vec<_>>(), en.keys().collect::<Vec<_>>());
            for (key, v) in en {
                assert!(
                    !values[key].as_str().unwrap().contains('?'),
                    "damaged characters: {locale}/{key}"
                );
                assert_eq!(
                    placeholders(v.as_str().unwrap()),
                    placeholders(values[key].as_str().unwrap()),
                    "{locale}/{key}"
                );
            }
        }
        assert_eq!(
            Message::new("phenotype_overlap", json!({"n":2,"total":3})).text("de"),
            "2 Ihrer 3 Symptome stimmen überein"
        );
    }
}
