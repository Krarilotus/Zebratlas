//! Document → plain text, in memory (D41). The format is sniffed from the bytes (magic numbers), never
//! taken from the file name or the client's content type. Nothing is written to disk.

use std::io::{Cursor, Read};

use serde::Serialize;

use crate::{IntakeError, Limits};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
    Docx,
    Txt,
    /// Pasted text (no file).
    Paste,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Txt => "txt",
            Self::Paste => "paste",
        }
    }
}

/// Extracted text of one document.
#[derive(Clone, Debug)]
pub struct Extracted {
    pub format: Format,
    pub text: String,
    pub pages: Option<usize>,
}

/// What the bytes are. Images and other binaries are refused (no OCR in v1).
pub fn sniff(bytes: &[u8]) -> Result<Format, IntakeError> {
    const IMAGES: [&[u8]; 6] = [b"\x89PNG", b"\xff\xd8\xff", b"GIF8", b"II*\0", b"MM\0*", b"RIFF"];
    if bytes.starts_with(b"%PDF-") || bytes.get(..1024).is_some_and(|h| find(h, b"%PDF-").is_some()) {
        return Ok(Format::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok(Format::Docx); // checked for word/document.xml when opened
    }
    if IMAGES.iter().any(|m| bytes.starts_with(m)) || bytes.get(4..12).is_some_and(|b| b.starts_with(b"ftyp")) {
        return Err(IntakeError::Unsupported);
    }
    // text: no NUL bytes in the first 8 KiB (UTF-16 files carry a BOM and NULs: refuse rather than guess)
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return Err(IntakeError::Unsupported);
    }
    Ok(Format::Txt)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Extract the text of an uploaded file within the limits.
pub fn extract(bytes: &[u8], limits: &Limits) -> Result<Extracted, IntakeError> {
    if bytes.len() > limits.max_bytes {
        return Err(IntakeError::TooLarge);
    }
    if bytes.is_empty() {
        return Err(IntakeError::Empty);
    }
    let out = match sniff(bytes)? {
        Format::Pdf => pdf(bytes, limits)?,
        Format::Docx => docx(bytes, limits)?,
        _ => Extracted {
            format: Format::Txt,
            text: decode_text(bytes),
            pages: None,
        },
    };
    finish(out)
}

/// Pasted text within the limits.
pub fn pasted(text: &str, limits: &Limits) -> Result<Extracted, IntakeError> {
    if text.len() > limits.max_bytes {
        return Err(IntakeError::TooLarge);
    }
    finish(Extracted {
        format: Format::Paste,
        text: text.to_owned(),
        pages: None,
    })
}

fn finish(mut e: Extracted) -> Result<Extracted, IntakeError> {
    e.text = normalise(&e.text);
    if e.text.trim().is_empty() {
        return Err(if e.format == Format::Pdf {
            IntakeError::NoText
        } else {
            IntakeError::Empty
        });
    }
    Ok(e)
}

/// UTF-8 (BOM stripped), else Windows-1252/Latin-1 byte by byte.
fn decode_text(bytes: &[u8]) -> String {
    let b = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => b.iter().map(|&c| char::from(c)).collect(),
    }
}

/// Line endings to `\n`, control characters (except tab/newline) dropped, runs of blank lines collapsed.
fn normalise(text: &str) -> String {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(t.len());
    let mut blank = 0;
    for line in t.lines() {
        let line: String = line
            .chars()
            .map(|c| if c == '\u{a0}' { ' ' } else { c })
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn pdf(bytes: &[u8], limits: &Limits) -> Result<Extracted, IntakeError> {
    let pages = lopdf::Document::load_mem(bytes)
        .map_err(|_| IntakeError::Unreadable)?
        .get_pages()
        .len();
    if pages > limits.max_pages {
        return Err(IntakeError::TooManyPages(pages));
    }
    // pdf-extract can panic on malformed files: contain it (the caller runs this on a blocking thread)
    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| IntakeError::Unreadable)?
        .map_err(|_| IntakeError::Unreadable)?;
    Ok(Extracted {
        format: Format::Pdf,
        text,
        pages: Some(pages),
    })
}

/// DOCX: the paragraphs of `word/document.xml` (`w:t` runs, `w:tab`, `w:br`, paragraph ends).
fn docx(bytes: &[u8], limits: &Limits) -> Result<Extracted, IntakeError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| IntakeError::Unsupported)?;
    if zip.len() > 1024 {
        return Err(IntakeError::Unreadable);
    }
    let mut xml = Vec::new();
    {
        let entry = zip.by_name("word/document.xml").map_err(|_| IntakeError::Unsupported)?;
        // zip bomb guard: never inflate more than 8× the upload limit
        let cap = (limits.max_bytes as u64) * 8;
        if entry.size() > cap {
            return Err(IntakeError::TooLarge);
        }
        entry
            .take(cap + 1)
            .read_to_end(&mut xml)
            .map_err(|_| IntakeError::Unreadable)?;
        if xml.len() as u64 > cap {
            return Err(IntakeError::TooLarge);
        }
    }
    let text = docx_text(&xml)?;
    // pages are not stored in DOCX; estimate from explicit page breaks
    let breaks = count(&xml, b"w:type=\"page\"") + count(&xml, b"<w:lastRenderedPageBreak");
    let pages = breaks + 1;
    if pages > limits.max_pages {
        return Err(IntakeError::TooManyPages(pages));
    }
    Ok(Extracted {
        format: Format::Docx,
        text,
        pages: Some(pages),
    })
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn docx_text(xml: &[u8]) -> Result<String, IntakeError> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"w:t" {
                    in_text = true;
                }
            }
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:tab" => out.push('\t'),
                b"w:br" | b"w:cr" => out.push('\n'),
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                let s = t.decode().map_err(|_| IntakeError::Unreadable)?;
                out.push_str(&quick_xml::escape::unescape(&s).map_err(|_| IntakeError::Unreadable)?);
            }
            Ok(Event::GeneralRef(r)) if in_text => {
                // quick-xml 0.38 reports entity references separately
                let name = r.decode().map_err(|_| IntakeError::Unreadable)?;
                let ch = match name.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                };
                out.push_str(ch);
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(IntakeError::Unreadable),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal DOCX built in memory.
    pub fn docx_bytes(paragraphs: &[&str]) -> Vec<u8> {
        use std::io::Write;
        let mut body = String::new();
        for p in paragraphs {
            body.push_str(&format!("<w:p><w:r><w:t xml:space=\"preserve\">{p}</w:t></w:r></w:p>"));
        }
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        );
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("word/document.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(xml.as_bytes()).unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff(b"%PDF-1.7\n...").unwrap(), Format::Pdf);
        assert_eq!(sniff(b"Hello").unwrap(), Format::Txt);
        assert!(matches!(sniff(b"\x89PNG\r\n\x1a\n"), Err(IntakeError::Unsupported)));
        assert!(matches!(sniff(b"\xff\xd8\xff\xe0"), Err(IntakeError::Unsupported)));
    }

    #[test]
    fn reads_docx_paragraphs() {
        let bytes = docx_bytes(&["Befund", "STXBP1 c.1162C&gt;T"]);
        let e = extract(&bytes, &Limits::default()).unwrap();
        assert_eq!(e.format, Format::Docx);
        assert_eq!(e.text, "Befund\nSTXBP1 c.1162C>T\n");
    }

    #[test]
    fn rejects_docx_inflation_over_limit_instead_of_partial_text() {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer
            .write_all(format!("<w:document><w:t>{}</w:t></w:document>", "x".repeat(10_000)).as_bytes())
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let limits = Limits {
            max_bytes: bytes.len(),
            ..Limits::default()
        };
        assert!(matches!(extract(&bytes, &limits), Err(IntakeError::TooLarge)));
    }

    #[test]
    fn limits_size_and_empty() {
        let l = Limits {
            max_bytes: 10,
            ..Limits::default()
        };
        assert!(matches!(extract(&[b'a'; 11], &l), Err(IntakeError::TooLarge)));
        assert!(matches!(extract(b"  \n ", &Limits::default()), Err(IntakeError::Empty)));
    }

    #[test]
    fn latin1_fallback() {
        let e = extract(b"Gr\xf6\xdfe", &Limits::default()).unwrap();
        assert_eq!(e.text.trim(), "Größe");
    }
}
