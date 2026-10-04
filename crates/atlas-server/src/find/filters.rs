//! The single filter definition shared by search, resolution and graph masks.
use super::{Item, msg};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const KINDS: &[&str] = &[
    "condition",
    "gene",
    "group",
    "study",
    "model_sample",
    "therapy_programme",
    "funding",
    "paper",
];

#[derive(Clone, Debug, Default, Serialize)]
pub struct Filters {
    pub kind: Vec<String>,
    pub country: Vec<String>,
    pub language: Vec<String>,
    pub recruiting: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Params {
    #[serde(default, alias = "text")]
    pub q: String,
    pub focus: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub include_retired: bool,
    pub kind: Option<String>,
    pub country: Option<String>,
    pub language: Option<String>,
    pub recruiting: Option<String>,
    pub lang: Option<String>,
    pub llm: Option<u8>,
    pub suggest: Option<u8>,
}

// ISO alpha-2 codes; EU is the contract's extra value for EU-wide organisations.
const COUNTRIES: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW EU";
const LANGUAGES: &str = "aa ab ae af ak am an ar as av ay az ba be bg bh bi bm bn bo br bs ca ce ch co cr cs cu cv cy da de dv dz ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv ha he hi ho hr ht hu hy hz ia id ie ig ii ik io is it iu ja jv ka kg ki kj kk kl km kn ko kr ks ku kv kw ky la lb lg li ln lo lt lu lv mg mh mi mk ml mn mr ms mt my na nb nd ne ng nl nn no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ro ru rw sa sc sd se sg si sk sl sm sn so sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt tw ty ug uk ur uz ve vi vo wa wo xh yi yo za zh zu";

impl Params {
    pub fn filters(&self) -> Result<Filters, &'static str> {
        fn list(input: &Option<String>, upper: bool) -> Vec<String> {
            let mut v: Vec<_> = input
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if upper {
                        s.to_ascii_uppercase()
                    } else {
                        s.to_ascii_lowercase()
                    }
                })
                .collect();
            v.sort();
            v.dedup();
            v
        }
        let f = Filters {
            kind: list(&self.kind, false),
            country: list(&self.country, true),
            language: list(&self.language, false),
            recruiting: match self
                .recruiting
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                None | Some("") => None,
                Some("true") => Some(true),
                Some("false") => Some(false),
                _ => return Err("recruiting"),
            },
        };
        if f.kind.iter().any(|k| !KINDS.contains(&k.as_str())) {
            return Err("kind");
        }
        if f.country.iter().any(|c| !COUNTRIES.split_whitespace().any(|v| v == c)) {
            return Err("country");
        }
        if f.language.iter().any(|c| !LANGUAGES.split_whitespace().any(|v| v == c)) {
            return Err("language");
        }
        Ok(f)
    }
}

pub fn country(raw: &str) -> Option<String> {
    let up = raw.trim().to_ascii_uppercase();
    if COUNTRIES.split_whitespace().any(|v| v == up) {
        return Some(up);
    }
    // Country English names from Windows .NET RegionInfo, plus registry spelling aliases below.
    // This software table normalises spellings; it never asserts a study's location.
    static NAMES: std::sync::LazyLock<std::collections::BTreeMap<String, String>> = std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("country_names.json")).expect("country lookup is valid JSON")
    });
    if let Some(code) = NAMES.get(&raw.trim().to_lowercase()) {
        return Some(code.clone());
    }
    // Registry country spellings used by CT.gov; unknown metadata stays unknown.
    let code = match raw.trim().to_lowercase().as_str() {
        "germany" | "deutschland" => "DE",
        "france" => "FR",
        "united states" | "united states of america" => "US",
        "united kingdom" | "uk" => "GB",
        "canada" => "CA",
        "australia" => "AU",
        "italy" => "IT",
        "spain" => "ES",
        "netherlands" => "NL",
        "belgium" => "BE",
        "switzerland" => "CH",
        "austria" => "AT",
        "sweden" => "SE",
        "denmark" => "DK",
        "norway" => "NO",
        "finland" => "FI",
        "poland" => "PL",
        "portugal" => "PT",
        "ireland" => "IE",
        "israel" => "IL",
        "japan" => "JP",
        "china" => "CN",
        "india" => "IN",
        "brazil" => "BR",
        "korea, republic of" | "south korea" | "republic of korea" => "KR",
        "taiwan" => "TW",
        "turkey" | "türkiye" => "TR",
        "czechia" | "czech republic" => "CZ",
        "hungary" => "HU",
        "greece" => "GR",
        "romania" => "RO",
        "slovakia" => "SK",
        "slovenia" => "SI",
        "croatia" => "HR",
        "bulgaria" => "BG",
        "ukraine" => "UA",
        "russian federation" | "russia" => "RU",
        "mexico" => "MX",
        "argentina" => "AR",
        "chile" => "CL",
        "south africa" => "ZA",
        "new zealand" => "NZ",
        "singapore" => "SG",
        "hong kong" => "HK",
        "europe" | "european union" => "EU",
        "antarctica" => "AQ",
        "bouvet island" => "BV",
        "western sahara" => "EH",
        "south georgia and the south sandwich islands" => "GS",
        "heard island and mcdonald islands" => "HM",
        "french southern territories" => "TF",
        _ => return None,
    };
    Some(code.into())
}

pub fn language(raw: &str) -> Option<String> {
    let s = raw.trim().to_lowercase();
    let first = s.split(['-', '_']).next().unwrap_or("");
    if LANGUAGES.split_whitespace().any(|v| v == first) {
        return Some(first.into());
    }
    Some(
        match s.as_str() {
            "english" => "en",
            "german" | "deutsch" => "de",
            "french" | "français" => "fr",
            "spanish" | "español" => "es",
            "italian" | "italiano" => "it",
            "portuguese" | "português" => "pt",
            "polish" | "polski" => "pl",
            "dutch" | "nederlands" => "nl",
            "russian" => "ru",
            "arabic" => "ar",
            "chinese" | "mandarin" => "zh",
            "japanese" => "ja",
            "korean" => "ko",
            "turkish" => "tr",
            "swedish" => "sv",
            "danish" => "da",
            "norwegian" => "no",
            "finnish" => "fi",
            "greek" => "el",
            "czech" => "cs",
            "hungarian" => "hu",
            "romanian" => "ro",
            "ukrainian" => "uk",
            "hebrew" => "he",
            _ => return None,
        }
        .into(),
    )
}

impl Filters {
    pub fn accepts(&self, item: &Item, skip: &str) -> bool {
        (skip == "kind" || self.kind.is_empty() || self.kind.contains(&item.kind))
            && (skip == "country"
                || self.country.is_empty()
                || !["study", "group", "funding"].contains(&item.kind.as_str())
                || item.country.iter().any(|v| self.country.contains(v)))
            && (skip == "language"
                || self.language.is_empty()
                || item.kind != "group"
                || item.languages.iter().any(|v| self.language.contains(v)))
            && (skip == "recruiting"
                || self.recruiting.is_none()
                || item.kind != "study"
                || item.recruiting == self.recruiting)
    }
    pub fn facets(&self, items: &[Item]) -> Value {
        let mut out = json!({});
        for field in ["kind", "country", "language", "recruiting"] {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for i in items.iter().filter(|i| self.accepts(i, field)) {
                let values = match field {
                    "kind" => {
                        if KINDS.contains(&i.kind.as_str()) {
                            vec![i.kind.clone()]
                        } else {
                            vec![]
                        }
                    }
                    "country" => i.country.clone(),
                    "language" => i.languages.clone(),
                    _ => i.recruiting.map(|v| vec![v.to_string()]).unwrap_or_default(),
                };
                for v in values {
                    *counts.entry(v).or_default() += 1;
                }
            }
            out[field] = json!(counts.into_iter().map(|(value,count)| {
                let key = if field == "kind" { format!("find.kind.{value}") } else { format!("find.{field}") };
                json!({"value":value,"count":count,"msg":msg(&key,json!({"code":value,"value":value,"n":count}),format!("{value}: {count} results"))})
            }).collect::<Vec<_>>());
        }
        out
    }
}

pub fn applies_to() -> Value {
    json!({"country":["study","group","funding"],"language":["group"],"recruiting":["study"]})
}
