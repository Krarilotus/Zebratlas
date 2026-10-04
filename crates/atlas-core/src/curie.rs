//! Compact identifiers (`PREFIX:local`). The only place prefixes are normalised.

/// Source prefix spellings -> canonical prefix (Orphanet -> ORPHA).
const PREFIX: &[(&str, &str)] = &[
    ("Orphanet", "ORPHA"),
    ("ORPHANET", "ORPHA"),
    ("OMIM", "OMIM"),
    ("MONDO", "MONDO"),
    ("DECIPHER", "DECIPHER"),
];

/// Canonical CURIE: maps known prefix spellings (`Orphanet:42` -> `ORPHA:42`).
///
/// Like the Python `partition`, an id without `:` gains one (`x` -> `x:`).
pub fn normalize(curie: &str) -> String {
    let (prefix, local) = split(curie);
    let prefix = PREFIX
        .iter()
        .find(|(from, _)| *from == prefix)
        .map_or(prefix, |(_, to)| to);
    format!("{prefix}:{local}")
}

/// Normalise a user-typed id: prefix matched case-insensitively (`orphanet:42`, `hp:0001250`).
pub fn normalize_query(curie: &str) -> Option<String> {
    let (prefix, local) = curie.trim().split_once(':')?;
    if prefix.is_empty() || local.is_empty() || prefix.contains(char::is_whitespace) {
        return None;
    }
    let upper = prefix.to_ascii_uppercase();
    let known = PREFIX
        .iter()
        .find(|(from, _)| from.eq_ignore_ascii_case(prefix))
        .map(|(_, to)| *to);
    Some(format!("{}:{}", known.unwrap_or(&upper), local.trim()))
}

/// `(prefix, local)`; the local part is empty when there is no `:`.
pub fn split(curie: &str) -> (&str, &str) {
    curie.split_once(':').unwrap_or((curie, ""))
}

/// Prefix before the first `:` (the whole id if none).
pub fn prefix(curie: &str) -> &str {
    split(curie).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_prefixes() {
        assert_eq!(normalize("Orphanet:42"), "ORPHA:42");
        assert_eq!(normalize("OMIM:1"), "OMIM:1");
        assert_eq!(normalize("x"), "x:");
        assert_eq!(normalize_query(" orphanet:42 ").as_deref(), Some("ORPHA:42"));
        assert_eq!(normalize_query("hp:0001250").as_deref(), Some("HP:0001250"));
        assert_eq!(normalize_query("seizure"), None);
    }
}
