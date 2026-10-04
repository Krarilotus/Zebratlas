//! Text helpers for matching: label normalisation, HTML to text, quote search.

/// Lower-case letters and digits, every other run of characters becomes one space.
/// `"STXBP1 Foundation, Inc."` → `"stxbp1 foundation inc"`.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.extend(c.to_lowercase());
        } else {
            gap = true;
        }
    }
    out
}

/// Visible text of an HTML page, roughly: drops `<script>`, `<style>`, `<noscript>` and comments,
/// strips tags, decodes common entities. Good enough to find a quote; not a renderer.
pub fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if lower[i..].starts_with("<!--") {
                i = lower[i..].find("-->").map_or(bytes.len(), |e| i + e + 3);
                continue;
            }
            let skipped = ["script", "style", "noscript", "template"].iter().find_map(|tag| {
                let open = format!("<{tag}");
                let next = lower.as_bytes().get(i + open.len()).copied();
                (lower[i..].starts_with(&open) && matches!(next, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')))
                    .then(|| {
                        let close = format!("</{tag}");
                        lower[i..].find(&close).map_or(bytes.len(), |e| {
                            let after = i + e;
                            lower[after..].find('>').map_or(bytes.len(), |g| after + g + 1)
                        })
                    })
            });
            if let Some(end) = skipped {
                i = end;
                out.push(' ');
                continue;
            }
            i = lower[i..].find('>').map_or(bytes.len(), |e| i + e + 1);
            out.push(' ');
            continue;
        }
        let next = lower[i..].find('<').map_or(bytes.len(), |e| i + e);
        out.push_str(&decode_entities(&html[i..next]));
        i = next;
    }
    out
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // Byte search: slicing at a fixed length could split a multi-byte character.
        let Some(semi) = rest.bytes().take(12).position(|b| b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            "rsquo" | "lsquo" => Some('\''),
            "rdquo" | "ldquo" => Some('"'),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether `quote` appears in `page_text`, ignoring case, punctuation, quotes and whitespace.
pub fn contains_quote(page_text: &str, quote: &str) -> bool {
    let q = normalize(quote);
    if q.is_empty() {
        return false;
    }
    let page = format!(" {} ", normalize(page_text));
    page.contains(&format!(" {q} "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_labels() {
        assert_eq!(normalize("  STXBP1 Foundation, Inc. "), "stxbp1 foundation inc");
        assert_eq!(normalize("Ärzte–Netz"), "ärzte netz");
    }

    #[test]
    fn strips_html_and_finds_quotes() {
        let html = r#"<html><head><style>p{}</style><script>var x = "<p>fake</p>";</script></head>
            <body><!-- hidden --><p>We support families with <b>STXBP1&nbsp;disorders</b> &amp; related
            conditions.</p></body></html>"#;
        let text = html_to_text(html);
        assert!(!text.contains("fake"));
        assert!(!text.contains("hidden"));
        assert!(contains_quote(
            &text,
            "support families with STXBP1 disorders & related conditions"
        ));
        assert!(contains_quote(&text, "“We support families”"));
        assert!(!contains_quote(&text, "support families with SCN2A"));
        // Whole words only.
        assert!(!contains_quote(&text, "port families"));
    }

    #[test]
    fn decodes_numeric_entities() {
        assert_eq!(decode_entities("a&#38;b&#x26;c &unknown; d"), "a&b&c &unknown; d");
    }
}
