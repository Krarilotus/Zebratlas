//! Reading the source caches of design/SOURCES.md: envelopes (schema/version checked), per-record
//! checksums, and re-reading one record for verification.
//!
//! Record checksums: a JSON-lines record hashes its line bytes (without the newline), a JSON
//! envelope record hashes its canonical JSON (sorted keys, `,`/`:` separators, UTF-8; the form of
//! the SOURCES.md header checksum), a TSV row hashes its line bytes.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use atlas_core::graph::{RecordHash, Sha256};
use atlas_core::provenance::Locator;
use serde_json::Value;
use sha2::Digest;

use crate::error::IngestError;

/// A parsed JSON cache file.
pub struct Envelope {
    pub schema: String,
    pub version: u64,
    pub header: Value,
    pub records: Vec<Value>,
    /// `sha256(canonical(records))` equals `header.sha256`.
    pub header_verified: bool,
}

impl Envelope {
    pub fn header_str(&self, key: &str) -> Option<&str> {
        self.header.get(key).and_then(Value::as_str)
    }
}

pub fn sha256(bytes: &[u8]) -> Sha256 {
    sha2::Sha256::digest(bytes).into()
}

/// Canonical JSON (Python `json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`).
pub fn canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push(b'{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut *out, k).expect("string serialises");
                out.push(b':');
                canonical(&map[k], out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonical(item, out);
            }
            out.push(b']');
        }
        Value::Number(number) if number.is_f64() => python_float(number, out),
        other => serde_json::to_writer(&mut *out, other).expect("scalar serialises"),
    }
}

/// Python's shortest binary64 spelling uses scientific notation below 1e-4
/// and from 1e16, a signed exponent with at least two digits, and `.0` for
/// integral floats in fixed notation. serde_json's shortest digits describe
/// the same value; only their decimal placement and exponent spelling change.
/// Parsing requires serde_json's float_roundtrip feature (workspace manifest).
fn python_float(number: &serde_json::Number, out: &mut Vec<u8>) {
    let value = number.as_f64().expect("float number");
    if value == 0.0 {
        out.extend_from_slice(if value.is_sign_negative() { b"-0.0" } else { b"0.0" });
        return;
    }
    let raw = number.to_string();
    let unsigned = raw.strip_prefix('-').unwrap_or(&raw);
    if raw.starts_with('-') {
        out.push(b'-');
    }
    let (mantissa, exponent) = unsigned
        .split_once('e')
        .map_or((unsigned, 0), |(m, e)| (m, e.parse::<i32>().expect("JSON exponent")));
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let leading = digits.len() - digits.trim_start_matches('0').len();
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    let exponent = exponent + point - leading as i32 - 1;
    if !(-4..16).contains(&exponent) {
        out.push(significant.as_bytes()[0]);
        if significant.len() > 1 {
            out.push(b'.');
            out.extend_from_slice(&significant.as_bytes()[1..]);
        }
        out.extend_from_slice(format!("e{exponent:+03}").as_bytes());
    } else {
        let point = exponent + 1;
        if point <= 0 {
            out.extend_from_slice(b"0.");
            out.extend(std::iter::repeat_n(b'0', (-point) as usize));
            out.extend_from_slice(significant.as_bytes());
        } else if point as usize >= significant.len() {
            out.extend_from_slice(significant.as_bytes());
            out.extend(std::iter::repeat_n(b'0', point as usize - significant.len()));
            out.extend_from_slice(b".0");
        } else {
            out.extend_from_slice(&significant.as_bytes()[..point as usize]);
            out.push(b'.');
            out.extend_from_slice(&significant.as_bytes()[point as usize..]);
        }
    }
}

pub fn canonical_sha256(v: &Value) -> Sha256 {
    let mut buf = Vec::with_capacity(4096);
    canonical(v, &mut buf);
    sha256(&buf)
}

pub fn hex(d: &Sha256) -> String {
    atlas_core::graph::hex(d)
}

/// Read a JSON envelope; rejects unknown schemas or versions.
pub fn read_envelope(path: &Path, schema: &str, versions: &[u64]) -> Result<Envelope, IngestError> {
    let bytes = std::fs::read(path).map_err(IngestError::io(path))?;
    let mut v: Value = serde_json::from_slice(&bytes).map_err(|e| IngestError::Json {
        path: path.to_owned(),
        source: e,
    })?;
    let found = v.get("schema").and_then(Value::as_str).unwrap_or("").to_owned();
    let version = v.get("version").and_then(Value::as_u64).unwrap_or(0);
    if (!schema.is_empty() && found != schema) || !versions.contains(&version) {
        return Err(IngestError::Schema {
            path: path.to_owned(),
            found: format!("{found} v{version}"),
            expected: format!("{schema} v{versions:?}"),
        });
    }
    let header = v.get_mut("header").map(Value::take).unwrap_or(Value::Null);
    let records = match v.get_mut("records").map(Value::take) {
        Some(Value::Array(r)) => r,
        _ => Vec::new(),
    };
    let expected = header.get("sha256").and_then(Value::as_str).unwrap_or("");
    let all = Value::Array(records);
    let header_verified = hex(&canonical_sha256(&all)) == expected;
    let Value::Array(records) = all else {
        unreachable!("built as an array")
    };
    Ok(Envelope {
        schema: found,
        version,
        header,
        records,
        header_verified,
    })
}

/// Lines of a JSON-lines file, gzip or plain (detected by the gzip magic bytes), newline
/// stripped, with 1-based line numbers.
pub fn gz_lines(path: &Path) -> Result<impl Iterator<Item = std::io::Result<(u32, Vec<u8>)>>, IngestError> {
    let mut file = File::open(path).map_err(IngestError::io(path))?;
    let mut magic = [0u8; 2];
    let gzip = file.read_exact(&mut magic).is_ok() && magic == [0x1f, 0x8b];
    let file = File::open(path).map_err(IngestError::io(path))?;
    let inner: Box<dyn Read> = if gzip {
        Box::new(flate2::read::GzDecoder::new(BufReader::new(file)))
    } else {
        Box::new(file)
    };
    let reader = BufReader::with_capacity(1 << 20, inner);
    let mut n = 0u32;
    Ok(reader.split(b'\n').map(move |line| {
        n += 1;
        line.map(|l| (n, l))
    }))
}

/// Outcome of re-reading and re-hashing one record.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordCheck {
    pub computed_sha256: Option<String>,
    /// Record id found at the locator (must equal the stored id).
    pub found_id: Option<String>,
    pub error: Option<String>,
}

/// Re-read the record at `locator` in `path` and hash it in the stored form.
pub fn rehash(path: &Path, locator: &Locator, form: RecordHash) -> RecordCheck {
    let fail = |e: String| RecordCheck {
        computed_sha256: None,
        found_id: None,
        error: Some(e),
    };
    match (form, locator) {
        (RecordHash::JsonLine, Locator::Line(n)) => {
            let lines = match gz_lines(path) {
                Ok(l) => l,
                Err(e) => return fail(e.to_string()),
            };
            for line in lines {
                match line {
                    Ok((i, bytes)) if i == *n => {
                        let id = serde_json::from_slice::<Value>(&bytes)
                            .ok()
                            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned));
                        return RecordCheck {
                            computed_sha256: Some(hex(&sha256(&bytes))),
                            found_id: id,
                            error: None,
                        };
                    }
                    Ok(_) => {}
                    Err(e) => return fail(e.to_string()),
                }
            }
            fail(format!("line {n} not found"))
        }
        (RecordHash::TsvLine, Locator::Line(n)) => {
            let file = match File::open(path) {
                Ok(f) => f,
                Err(e) => return fail(e.to_string()),
            };
            let reader = BufReader::with_capacity(1 << 20, file);
            for (i, line) in reader.split(b'\n').enumerate() {
                if i + 1 == *n as usize {
                    return match line {
                        Ok(mut bytes) => {
                            if bytes.last() == Some(&b'\r') {
                                bytes.pop();
                            }
                            let id = bytes
                                .split(|b| *b == b'\t')
                                .next()
                                .map(|f| String::from_utf8_lossy(f).into_owned());
                            RecordCheck {
                                computed_sha256: Some(hex(&sha256(&bytes))),
                                found_id: id,
                                error: None,
                            }
                        }
                        Err(e) => fail(e.to_string()),
                    };
                }
            }
            fail(format!("line {n} not found"))
        }
        (RecordHash::CanonicalJson, Locator::Record(r)) => {
            let mut parts = r.split('.');
            let index = |part: &str, key: &str| {
                part.strip_prefix(key)
                    .and_then(|s| s.strip_prefix('['))
                    .and_then(|s| s.strip_suffix(']'))
                    .and_then(|s| s.parse::<usize>().ok())
            };
            let Some(i) = parts.next().and_then(|s| index(s, "records")) else {
                return fail(format!("bad locator {r}"));
            };
            let author = parts.next();
            if parts.next().is_some() {
                return fail(format!("bad locator {r}"));
            }
            let mut text = Vec::new();
            if let Err(e) = File::open(path).and_then(|mut f| f.read_to_end(&mut text)) {
                return fail(e.to_string());
            }
            let v: Value = match serde_json::from_slice(&text) {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            let value = v.get("records").and_then(|r| r.get(i));
            let value = if let Some(part) = author {
                let Some(j) = index(part, "authors") else {
                    return fail(format!("bad author locator {r}"));
                };
                value.and_then(|v| v.get("authors")).and_then(|v| v.get(j))
            } else {
                value
            };
            match value {
                Some(rec) => RecordCheck {
                    computed_sha256: Some(hex(&canonical_sha256(rec))),
                    found_id: rec.get("id").and_then(Value::as_str).map(str::to_owned),
                    error: None,
                },
                None => fail(format!("records[{i}] not found")),
            }
        }
        (form, loc) => fail(format!("locator {loc} does not fit hash form {form:?}")),
    }
}

/// `data/<file>` for a cache-relative entity file.
pub fn entity_path(data: &Path, file: &str) -> PathBuf {
    data.join(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_matches_python() {
        // python: json.dumps({"b": [1, "ä\n"], "a": None}, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        let v: Value = serde_json::from_str(r#"{"b": [1, "ä\n"], "a": null}"#).unwrap();
        let mut out = Vec::new();
        canonical(&v, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "{\"a\":null,\"b\":[1,\"ä\\n\"]}");
    }

    #[test]
    fn canonical_python_float_notation_and_boundaries() {
        // Expected spellings from Python 3.13 json.dumps, including binary64
        // extremes, exponent padding and the integer-versus-float distinction.
        for (input, expected) in [
            ("1", "1"),
            ("1.0", "1.0"),
            ("-0.0", "-0.0"),
            ("0.0001", "0.0001"),
            ("0.00001", "1e-05"),
            ("1.2345678901234567e-7", "1.2345678901234566e-07"),
            ("3.61315170428691e-05", "3.61315170428691e-05"),
            ("1e15", "1000000000000000.0"),
            ("1e16", "1e+16"),
            ("1e23", "1e+23"),
            ("5e-324", "5e-324"),
            ("2.2250738585072014e-308", "2.2250738585072014e-308"),
            ("1.7976931348623157e308", "1.7976931348623157e+308"),
        ] {
            let mut out = vec![];
            canonical(&serde_json::from_str(input).unwrap(), &mut out);
            assert_eq!(String::from_utf8(out).unwrap(), expected, "input: {input}");
        }
    }

    #[test]
    fn canonical_python_float_preserves_parse_roundtrip() {
        // Reproducible finite synthetic bit-pattern sample, unrelated to source
        // records. The default serde_json parser changes its final decimal digit.
        let value: Value = serde_json::from_str("8.086635942097233e+69").unwrap();
        let mut out = vec![];
        canonical(&value, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "8.086635942097233e+69");
    }
}
