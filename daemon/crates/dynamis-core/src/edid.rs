//! The built-in screen's own description (EDID), read from
//! `/sys/class/drm/card*-eDP-*/edid` (readable by anyone). Used only to tell
//! whether the screen is likely an OLED, where a dark style saves power.
//!
//! EDID has no field for the panel technology, so this is a guess from the
//! text the panel gives: its name or the maker's part number. Samsung
//! Display's OLED laptop panels have part numbers starting `ATNA`; others
//! often say `OLED` in their name. Anything else counts as not known.

use crate::fs::SysRoot;

/// The text descriptors (monitor name 0xFC, unspecified text 0xFE) of an
/// EDID base block.
pub fn texts(edid: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if edid.len() < 128 || edid[..8] != [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0] {
        return out;
    }
    for off in [54usize, 72, 90, 108] {
        let d = &edid[off..off + 18];
        // A display descriptor: pixel clock 0, then its tag in byte 3.
        if d[0] != 0 || d[1] != 0 || !matches!(d[3], 0xfc | 0xfe) {
            continue;
        }
        let t: String = d[5..]
            .iter()
            .take_while(|b| **b != 0x0a)
            .map(|b| *b as char)
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect();
        let t = t.trim().to_string();
        if !t.is_empty() {
            out.push(t);
        }
    }
    out
}

pub fn looks_oled(texts: &[String]) -> bool {
    texts.iter().any(|t| {
        let u = t.to_ascii_uppercase();
        u.contains("OLED") || u.starts_with("ATNA")
    })
}

/// Whether the built-in screen looks like an OLED.
pub fn builtin_oled(root: &SysRoot) -> bool {
    root.list_dir("/sys/class/drm")
        .into_iter()
        .filter(|c| c.starts_with("card") && c.contains("-eDP-"))
        .any(|c| {
            root.read_bytes(&format!("/sys/class/drm/{c}/edid"))
                .map(|b| looks_oled(&texts(&b)))
                .unwrap_or(false)
        })
}

#[cfg(test)]
pub fn sample(name_tag: u8, text: &str) -> Vec<u8> {
    let mut e = vec![0u8; 128];
    e[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
    let d = &mut e[72..90];
    d[3] = name_tag;
    for (i, b) in text.bytes().take(13).enumerate() {
        d[5 + i] = b;
    }
    if text.len() < 13 {
        d[5 + text.len()] = 0x0a;
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_panel_text_and_guesses_oled() {
        assert_eq!(texts(&sample(0xfe, "ATNA56AC03-0")), ["ATNA56AC03-0"]);
        assert!(looks_oled(&texts(&sample(0xfe, "ATNA56AC03-0"))));
        assert!(looks_oled(&texts(&sample(0xfc, "Dell OLED"))));
        assert!(!looks_oled(&texts(&sample(0xfe, "B140HAN04.7"))));
        assert!(texts(&[0u8; 128]).is_empty(), "not an EDID");
    }
}
