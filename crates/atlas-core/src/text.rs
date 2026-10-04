//! Text normalisation shared by identity (label merge) and search.

/// Python `re` `\w` for str patterns: alphanumeric or underscore.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Case-folded word tokens joined by one space: `"Dravet  Syndrome!"` -> `"dravet syndrome"`.
///
/// Port of `normalize_label`: `casefold`, non-word runs -> space, whitespace collapsed.
pub fn normalize_label(text: &str) -> String {
    let folded = caseless::default_case_fold_str(text);
    let mut out = String::with_capacity(folded.len());
    for token in tokens(&folded) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token);
    }
    out
}

/// Maximal runs of word characters.
pub fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !is_word(c)).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_like_python() {
        assert_eq!(normalize_label("  Dravet--Syndrome, type 1 "), "dravet syndrome type 1");
        assert_eq!(normalize_label("Straße"), "strasse");
        assert_eq!(normalize_label("snake_case"), "snake_case");
        assert_eq!(normalize_label("!!"), "");
    }
}
