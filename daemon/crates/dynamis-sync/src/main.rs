//! `dynamis-sync` — downloads the signed quirk list for Dynamis Power.
//!
//! This is the **only** part of Dynamis Power that uses the network, and it
//! runs as an unprivileged, throwaway user (systemd `DynamicUser=`), never as
//! root. It fetches `quirks.json` and its signature, checks the signature, and
//! saves both in its own directory. The root service reads them from there and
//! checks the signature again before using anything.
//!
//! It refuses to run as root.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use dynamis_core::engine::DEFAULT_SYNC_DIR;
use dynamis_core::quirks::{self, QuirkList, TRUSTED_KEYS};

const DEFAULT_BASE: &str =
    "https://raw.githubusercontent.com/hirejohndougan-art/Dynamis-Power/main/quirks/";
const MAX_BYTES: u64 = 1 << 20;

#[derive(Parser)]
#[command(
    name = "dynamis-sync",
    version,
    about = "Download the signed Dynamis Power quirk list"
)]
struct Args {
    /// Where to save the list. Default: $STATE_DIRECTORY (set by systemd) or
    /// /var/lib/dynamis-power-sync.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Base URL holding quirks.json and quirks.json.sig.
    #[arg(long, default_value = DEFAULT_BASE)]
    url: String,
}

fn main() -> ExitCode {
    let args = Args::parse();
    if is_root() {
        eprintln!(
            "dynamis-sync: refusing to run as root; it is meant to run as an unprivileged user"
        );
        return ExitCode::FAILURE;
    }
    let dir = args
        .dir
        .or_else(|| std::env::var_os("STATE_DIRECTORY").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SYNC_DIR));
    match run(&dir, &args.url, TRUSTED_KEYS) {
        Ok(msg) => {
            println!("dynamis-sync: {msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dynamis-sync: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(dir: &Path, base: &str, keys: &[&str]) -> Result<String, String> {
    if keys.is_empty() {
        return Ok(
            "no trusted signing key is built in yet, so there is nothing to download".into(),
        );
    }
    let base = if base.ends_with('/') {
        base.to_string()
    } else {
        format!("{base}/")
    };
    let Some(list) = fetch(&format!("{base}quirks.json"))? else {
        return Ok("no quirk list is published yet".into());
    };
    let sig = fetch(&format!("{base}quirks.json.sig"))?
        .ok_or("the list is published without a signature")?;
    let sig = String::from_utf8(sig).map_err(|_| "signature is not text")?;
    quirks::verify(&list, sig.trim(), keys)?;
    let parsed: QuirkList = serde_json_from(&list)?;

    if let Ok(Some(current)) = quirks::read_verified(dir, keys) {
        if current.serial >= parsed.serial {
            return Ok(format!("already up to date (serial {})", current.serial));
        }
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Signature first, then the list. A reader that catches the moment in
    // between sees a mismatch and ignores the list until next time: the safe
    // failure.
    write_atomic(&dir.join("quirks.json.sig"), sig.trim().as_bytes())?;
    write_atomic(&dir.join("quirks.json"), &list)?;
    Ok(format!(
        "saved quirk list serial {} ({} quirks)",
        parsed.serial,
        parsed.quirks.len()
    ))
}

fn serde_json_from(bytes: &[u8]) -> Result<QuirkList, String> {
    // Parsed only after the signature checked out.
    dynamis_core::quirks::parse(bytes)
}

/// `Ok(None)` for "not found".
fn fetch(url: &str) -> Result<Option<Vec<u8>>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .https_only(true)
        .build()
        .into();
    match agent.get(url).call() {
        Ok(mut resp) => resp
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
            .map(Some)
            .map_err(|e| format!("{url}: {e}")),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(format!("{url}: {e}")),
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let w = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    };
    w().map_err(|e| format!("{}: {e}", path.display()))
}

fn is_root() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(2).map(|u| u == "0"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_key_it_does_nothing() {
        let dir = std::env::temp_dir().join(format!("dynamis-sync-{}", std::process::id()));
        let msg = run(&dir, "https://invalid.example/", &[]).unwrap();
        assert!(msg.contains("no trusted signing key"));
        assert!(!dir.exists(), "nothing is written");
    }

    /// Talks to GitHub, so not run by default: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reports_when_nothing_is_published() {
        let dir = std::env::temp_dir().join(format!("dynamis-sync-net-{}", std::process::id()));
        let key = "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29";
        let msg = run(&dir, DEFAULT_BASE, &[key]).unwrap();
        assert!(msg.contains("no quirk list is published yet"), "{msg}");
        assert!(
            fetch("http://example.com/").is_err(),
            "plain http is refused"
        );
    }
}
