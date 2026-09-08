//! LSP UTF-16 coordinates and local file URIs. No byte offsets cross the wire.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn path(uri: &str) -> Result<PathBuf> {
    let rest = uri
        .strip_prefix("file://")
        .context("Only local file URIs are supported")?;
    let rest = rest
        .strip_prefix("localhost/")
        .map(|s| format!("/{s}"))
        .unwrap_or_else(|| rest.into());
    if !rest.starts_with('/') || rest.contains(['?', '#']) {
        bail!("Expected an absolute local file URI");
    }
    let mut bytes = Vec::new();
    let raw = rest.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' {
            let pair = raw
                .get(index + 1..index + 3)
                .context("Truncated URI escape")?;
            let hex = std::str::from_utf8(pair)?;
            bytes.push(u8::from_str_radix(hex, 16).context("Invalid URI escape")?);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(bytes)?;
    if decoded.contains('\0') {
        bail!("NUL is not allowed in file paths");
    }
    let decoded = if cfg!(windows) && decoded.as_bytes().get(2) == Some(&b':') {
        &decoded[1..]
    } else {
        &decoded
    };
    crate::paths::resolve_buffer_path(Path::new(decoded))
}

pub fn uri(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.into_owned()
    };
    let mut result = if path.starts_with('/') {
        "file://".to_string()
    } else {
        "file:///".to_string()
    };
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(result, "%{byte:02X}").unwrap();
        }
    }
    result
}

pub fn offset(text: &str, position: &Value) -> Result<usize> {
    let line = position["line"].as_u64().context("Missing line")? as usize;
    let column = position["character"]
        .as_u64()
        .context("Missing character")? as usize;
    let start = if line == 0 {
        0
    } else {
        text.match_indices('\n')
            .nth(line - 1)
            .context("Line outside buffer")?
            .0
            + 1
    };
    let end = text[start..].find('\n').map_or(text.len(), |n| start + n);
    let row = text[start..end].trim_end_matches('\r');
    let mut units = 0;
    for (index, c) in row.char_indices() {
        if units == column {
            return Ok(start + index);
        }
        units += c.len_utf16();
        if units > column {
            bail!("Position splits a UTF-16 surrogate pair");
        }
    }
    // LSP positions past a line's end refer to its end.
    Ok(start + row.len())
}

pub fn position(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    json!({"line": before.bytes().filter(|b| *b == b'\n').count(),
        "character": before.rsplit('\n').next().unwrap_or("").encode_utf16().count()})
}

pub fn range(text: &str, start: usize, end: usize) -> Value {
    json!({"start":position(text,start),"end":position(text,end)})
}

pub fn change(text: &str, changes: &[Value]) -> Result<String> {
    let mut updated = text.to_string();
    for edit in changes {
        let replacement = edit["text"].as_str().context("Missing replacement text")?;
        if let Some(range) = edit.get("range") {
            let start = offset(&updated, &range["start"])?;
            let end = offset(&updated, &range["end"])?;
            if start > end {
                bail!("Reversed change range");
            }
            updated.replace_range(start..end, replacement);
        } else {
            updated = replacement.into();
        }
        if updated.len() > super::MAX_BUFFER {
            bail!("Document exceeds 8 MiB");
        }
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_reject_split_surrogates_and_apply_sequential_crlf_changes() {
        let text = "a😀b\r\nnext\n";
        assert_eq!(offset(text, &json!({"line":0,"character":3})).unwrap(), 5);
        assert!(offset(text, &json!({"line":0,"character":2})).is_err());
        assert!(offset(text, &json!({"line":99,"character":0})).is_err());
        assert_eq!(offset(text, &json!({"line":0,"character":99})).unwrap(), 6);
        assert_eq!(change(text,&[json!({"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":3}},"text":"é"}),json!({"range":{"start":{"line":0,"character":2},"end":{"line":0,"character":3}},"text":"!"})]).unwrap(),"aé!\r\nnext\n");
    }
    #[test]
    fn local_uri_roundtrips_and_rejects_remote_and_invalid_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("é #%.mk");
        assert_eq!(
            path(&uri(&p)).unwrap(),
            dunce::canonicalize(dir.path()).unwrap().join("é #%.mk")
        );
        for invalid in [
            "https://host/Makefile",
            "file://remote/Makefile",
            "file:///a%",
            "file:///a%GG",
            "file:///a%00",
            "file:///a#b",
        ] {
            assert!(path(invalid).is_err(), "{invalid}");
        }
    }
}
