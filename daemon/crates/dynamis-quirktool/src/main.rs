//! `dynamis-quirktool` — the maintainer's tool for the signed quirk list.
//! Never installed on users' machines.
//!
//!   dynamis-quirktool keygen SECRET_FILE      create a signing key (once)
//!   dynamis-quirktool sign LIST SECRET_FILE   write LIST.sig
//!   dynamis-quirktool verify LIST PUBLIC_HEX  check LIST.sig
//!
//! Keep the secret key file off GitHub and backed up somewhere safe: anyone
//! with it can publish quirk lists that every installation will trust. See
//! docs/MAINTAINING.md, "The quirk list".

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::process::ExitCode;

use dynamis_core::quirks::{self, hex, to_hex};
use ed25519_dalek::{Signer, SigningKey};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["keygen", secret] => keygen(secret),
        ["sign", list, secret] => sign(list, secret),
        ["verify", list, public] => verify(list, public),
        _ => Err("usage: dynamis-quirktool keygen SECRET_FILE | sign LIST SECRET_FILE | verify LIST PUBLIC_HEX".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dynamis-quirktool: {e}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(secret_path: &str) -> Result<(), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no secure randomness available: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    // create_new: never overwrite an existing key by accident. Mode 0600:
    // readable by its owner only.
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(secret_path)
        .map_err(|e| format!("{secret_path}: {e}"))?;
    writeln!(f, "{}", to_hex(&seed)).map_err(|e| e.to_string())?;
    println!("Secret key written to {secret_path} (keep it private and backed up).");
    println!("Public key, to add to TRUSTED_KEYS in daemon/crates/dynamis-core/src/quirks.rs:");
    println!("{}", to_hex(key.verifying_key().as_bytes()));
    Ok(())
}

fn sign(list_path: &str, secret_path: &str) -> Result<(), String> {
    let seed: [u8; 32] = fs::read_to_string(secret_path)
        .ok()
        .and_then(|s| hex(&s))
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| format!("{secret_path} is not a key made by `keygen`"))?;
    let key = SigningKey::from_bytes(&seed);
    let data = fs::read(list_path).map_err(|e| format!("{list_path}: {e}"))?;
    // Refuse to sign a list the program could not use.
    let list = quirks::parse(&data)?;
    for q in &list.quirks {
        if dynamis_core::settings::by_id(&q.setting).is_none() {
            return Err(format!(
                "quirk {} names an unknown setting '{}'",
                q.id, q.setting
            ));
        }
    }
    let sig_path = format!("{list_path}.sig");
    fs::write(&sig_path, to_hex(&key.sign(&data).to_bytes()) + "\n").map_err(|e| e.to_string())?;
    println!(
        "Signed serial {} ({} quirks): {sig_path}",
        list.serial,
        list.quirks.len()
    );
    Ok(())
}

fn verify(list_path: &str, public: &str) -> Result<(), String> {
    let data = fs::read(list_path).map_err(|e| format!("{list_path}: {e}"))?;
    let sig = fs::read_to_string(format!("{list_path}.sig"))
        .map_err(|e| format!("{list_path}.sig: {e}"))?;
    quirks::verify(&data, sig.trim(), &[public])?;
    println!("Signature OK.");
    Ok(())
}
