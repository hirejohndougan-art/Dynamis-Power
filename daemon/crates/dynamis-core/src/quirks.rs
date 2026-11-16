//! The quirk list: settings known to cause trouble on particular machines.
//!
//! Two sources, combined:
//!
//! 1. **Built in** (`quirks/builtin.json` in this crate), compiled into the
//!    program. Trusted because it ships with the code.
//! 2. **Downloaded** by the unprivileged `dynamis-sync` helper into its own
//!    directory. Used only if its ed25519 signature checks out against a key
//!    in [`TRUSTED_KEYS`], and only if its serial number is not lower than one
//!    already accepted (so an old list cannot be replayed to undo a newer
//!    block). This is also the remote kill switch: a new list blocks a setting
//!    for affected models within a day.
//!
//! The root service never downloads anything; it only reads and verifies.
//! See docs/decisions/0009-signed-quirk-list.md.

use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;
use crate::inventory;

/// Version of the quirk list format.
pub const QUIRK_FORMAT_VERSION: u32 = 1;

/// Public keys (hex) allowed to sign downloaded quirk lists. Empty until the
/// maintainer creates a signing key; see docs/MAINTAINING.md. While empty,
/// downloaded lists are ignored and only the built-in list applies.
pub const TRUSTED_KEYS: &[&str] = &[];

/// Downloaded lists bigger than this are refused unread.
const MAX_LIST_BYTES: u64 = 1 << 20;

const BUILTIN: &str = include_str!("../quirks/builtin.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuirkList {
    pub v: u32,
    /// Increases with every published list.
    pub serial: u64,
    #[serde(default)]
    pub issued: String,
    pub quirks: Vec<Quirk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Quirk {
    pub id: String,
    /// The setting ID this quirk restricts.
    pub setting: String,
    /// Values that are blocked. Absent or empty: the setting may not be
    /// changed at all.
    #[serde(default)]
    pub values: Vec<String>,
    /// Which machines it applies to. Every field given must match; an empty
    /// match applies everywhere.
    #[serde(rename = "match", default)]
    pub matches: Match,
    pub reason: String,
    #[serde(default)]
    pub url: String,
}

/// Patterns use `*` for "anything", matched without regard to case.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Match {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi_driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
}

/// The facts quirks are matched against.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Identity {
    pub vendor: String,
    pub model: String,
    pub cpu: String,
    pub wifi_drivers: Vec<String>,
    pub kernel: String,
}

impl Identity {
    pub fn read(root: &SysRoot) -> Identity {
        let inv = inventory::read(root);
        Identity {
            vendor: inv.machine.vendor.unwrap_or_default(),
            model: inv.machine.model.unwrap_or_default(),
            cpu: inv.cpu.model.unwrap_or_default(),
            wifi_drivers: inv.wifi.into_iter().filter_map(|w| w.driver).collect(),
            kernel: inv.software.kernel.unwrap_or_default(),
        }
    }
}

impl Match {
    pub fn matches(&self, id: &Identity) -> bool {
        let one = |pat: &Option<String>, val: &str| pat.as_deref().is_none_or(|p| glob(p, val));
        one(&self.vendor, &id.vendor)
            && one(&self.model, &id.model)
            && one(&self.cpu, &id.cpu)
            && one(&self.kernel, &id.kernel)
            && self
                .wifi_driver
                .as_deref()
                .is_none_or(|p| id.wifi_drivers.iter().any(|d| glob(p, d)))
    }
}

/// Where the downloaded list came from and whether it was used.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Downloaded {
    None,
    Used { serial: u64, issued: String },
    Rejected { why: String },
}

/// The quirks that apply to this machine.
#[derive(Debug, Clone, Serialize)]
pub struct Active {
    pub quirks: Vec<Quirk>,
    pub builtin_serial: u64,
    pub downloaded: Downloaded,
}

impl Active {
    /// The quirk blocking `setting` = `value` here, if any.
    pub fn blocking(&self, setting: &str, value: &str) -> Option<&Quirk> {
        self.quirks.iter().find(|q| {
            q.setting == setting && (q.values.is_empty() || q.values.iter().any(|v| v == value))
        })
    }
}

/// Load the built-in list, add the downloaded one from `sync_dir` if it is
/// valid, and keep the quirks that match this machine. `min_serial` is the
/// highest downloaded serial accepted before.
pub fn load(root: &SysRoot, sync_dir: &Path, min_serial: u64) -> Active {
    load_with_keys(root, sync_dir, min_serial, TRUSTED_KEYS)
}

pub fn load_with_keys(root: &SysRoot, sync_dir: &Path, min_serial: u64, keys: &[&str]) -> Active {
    let builtin: QuirkList =
        serde_json::from_str(BUILTIN).expect("built-in quirk list is valid JSON");
    let mut all = builtin.quirks.clone();
    let downloaded = match read_verified(sync_dir, keys) {
        Ok(None) => Downloaded::None,
        Ok(Some(list)) if list.serial < min_serial => Downloaded::Rejected {
            why: format!(
                "serial {} is older than {min_serial} already accepted",
                list.serial
            ),
        },
        Ok(Some(list)) => {
            let d = Downloaded::Used {
                serial: list.serial,
                issued: list.issued.clone(),
            };
            all.extend(list.quirks);
            d
        }
        Err(why) => Downloaded::Rejected { why },
    };
    let id = Identity::read(root);
    Active {
        quirks: all.into_iter().filter(|q| q.matches.matches(&id)).collect(),
        builtin_serial: builtin.serial,
        downloaded,
    }
}

/// Read `quirks.json` and `quirks.json.sig` from `dir`. `Ok(None)` if there
/// is no list; `Err` if there is one but it may not be used.
pub fn read_verified(dir: &Path, keys: &[&str]) -> Result<Option<QuirkList>, String> {
    let list_path = dir.join("quirks.json");
    let Ok(meta) = std::fs::metadata(&list_path) else {
        return Ok(None);
    };
    if meta.len() > MAX_LIST_BYTES {
        return Err("downloaded list is too large".into());
    }
    let bytes = std::fs::read(&list_path).map_err(|e| e.to_string())?;
    let sig_hex = std::fs::read_to_string(dir.join("quirks.json.sig"))
        .map_err(|_| "downloaded list has no signature".to_string())?;
    verify(&bytes, sig_hex.trim(), keys)?;
    parse(&bytes).map(Some)
}

/// Parse a list. Call only on bytes whose signature has been verified.
pub fn parse(bytes: &[u8]) -> Result<QuirkList, String> {
    let list: QuirkList =
        serde_json::from_slice(bytes).map_err(|e| format!("unreadable list: {e}"))?;
    if list.v != QUIRK_FORMAT_VERSION {
        return Err(format!("list format {} is not supported", list.v));
    }
    Ok(list)
}

/// Check a detached ed25519 signature (hex) over `data` against any of `keys`
/// (hex public keys). Verification happens before the data is parsed.
pub fn verify(data: &[u8], sig_hex: &str, keys: &[&str]) -> Result<(), String> {
    if keys.is_empty() {
        return Err("no trusted signing key is configured yet".into());
    }
    let sig_bytes: [u8; 64] = hex(sig_hex)
        .and_then(|b| b.try_into().ok())
        .ok_or("signature is not 64 hex-encoded bytes")?;
    let sig = Signature::from_bytes(&sig_bytes);
    for k in keys {
        let Some(Ok(key)) = hex(k)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .map(|b| VerifyingKey::from_bytes(&b))
        else {
            continue;
        };
        // verify_strict also rejects weak keys and malleable signatures.
        if key.verify_strict(data, &sig).is_ok() {
            return Ok(());
        }
    }
    Err("signature does not match any trusted key".into())
}

pub fn hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Case-insensitive match where `*` stands for any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let p = pattern.to_ascii_lowercase();
    let t = text.to_ascii_lowercase();
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == t;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !t.starts_with(first) || !t.ends_with(last) || t.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &t[first.len()..t.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globbing() {
        assert!(glob("ASUSTeK*", "ASUSTeK COMPUTER INC."));
        assert!(glob("*vivobook*", "VivoBook_ASUSLaptop E410MA_E410MA"));
        assert!(glob("rtw*8821ce", "rtw_8821ce"));
        assert!(glob("exact", "EXACT"));
        assert!(!glob("exact", "exactly"));
        assert!(!glob("a*b*c", "acb"));
        assert!(glob("*", ""));
    }

    #[test]
    fn builtin_list_is_valid() {
        let l: QuirkList = serde_json::from_str(BUILTIN).unwrap();
        assert_eq!(l.v, QUIRK_FORMAT_VERSION);
        for q in &l.quirks {
            assert!(
                crate::settings::by_id(&q.setting).is_some(),
                "{} names unknown setting {}",
                q.id,
                q.setting
            );
            assert!(!q.reason.is_empty());
        }
    }
}
