//! The start page's background: a picture the user chose (for example the game's cover), kept as config\cover.<ext> in
//! the package. The console never ships one; the page darkens it. The UI gets it as a data: URL (the CSP allows data:
//! images only), and sends a new one as base64.
use crate::paths::Layout;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const EXTS: &[&str] = &["jpg", "png", "webp"];
const MAX: usize = 24 << 20;

fn kind(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}
fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn b64(b: &[u8]) -> String {
    let mut o = String::with_capacity((b.len() + 2) / 3 * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            o.push(if i <= c.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    o
}
pub fn unb64(s: &str) -> Option<Vec<u8>> {
    let mut o = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if ch == b'=' {
            break;
        }
        let v = B64.iter().position(|&x| x == ch)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            o.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(o)
}

fn current(l: &Layout) -> Option<PathBuf> {
    EXTS.iter().map(|e| l.config_dir().join(format!("cover.{}", e))).find(|p| p.is_file())
}

pub fn get(l: &Layout) -> Result<Value, String> {
    let Some(p) = current(l) else {
        return Ok(json!({ "url": null }));
    };
    let b = fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
    let Some(k) = kind(&b).filter(|_| b.len() <= MAX) else {
        return Ok(json!({ "url": null, "error": format!("{} 不是 JPEG、PNG 或 WebP 图片，或者太大", p.display()) }));
    };
    Ok(json!({ "url": format!("data:{};base64,{}", mime(k), b64(&b)), "path": p.to_string_lossy() }))
}

/// `data` = the picture as base64, or None to go back to the plain background.
pub fn set(l: &Layout, data: Option<&str>) -> Result<Value, String> {
    let new = match data {
        Some(d) => {
            let b = unb64(d).ok_or("图片数据无效")?;
            if b.len() > MAX {
                return Err("图片太大（最多 24 MB）".into());
            }
            let k = kind(&b).ok_or("只支持 JPEG、PNG 和 WebP 图片")?;
            Some((b, k))
        }
        None => None,
    };
    fs::create_dir_all(l.config_dir()).map_err(|e| e.to_string())?;
    for e in EXTS {
        let _ = fs::remove_file(l.config_dir().join(format!("cover.{}", e)));
    }
    if let Some((b, k)) = new {
        let p = l.config_dir().join(format!("cover.{}", k));
        let tmp = p.with_extension("tmp");
        fs::write(&tmp, &b).and_then(|_| fs::rename(&tmp, &p)).map_err(|e| format!("{}: {}", p.display(), e))?;
    }
    get(l)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_round_trip_and_types() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(unb64(&b64(s)).unwrap(), s);
        }
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert!(unb64("Zm9v!").is_none());
        assert_eq!(kind(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(kind(b"GIF89a"), None);
    }
}
