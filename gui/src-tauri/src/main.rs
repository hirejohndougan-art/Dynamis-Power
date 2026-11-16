//! The Dynamis Power app's native shell. It only forwards calls from the
//! window to the Dynamis Power service, through `dynamis-client`, plus one
//! local action: saving an exported log file. All the logic is in the
//! service and in the React code; keep this file thin.
//!
//! Rust note: `#[tauri::command]` turns a function into something the
//! React side can call with `invoke("name", { args })`. Argument names are
//! converted from camelCase (JavaScript) to snake_case (Rust).

use std::sync::Mutex;

use dynamis_client::Client;
use serde_json::Value;
use tauri::State;

/// One connection to the service, made on first use and reused.
#[derive(Default)]
struct Conn(Mutex<Option<Client>>);

async fn client(conn: &Conn) -> Result<Client, String> {
    if let Some(c) = conn.0.lock().map_err(|e| e.to_string())?.clone() {
        return Ok(c);
    }
    let c = Client::connect().await?;
    *conn.0.lock().map_err(|e| e.to_string())? = Some(c.clone());
    Ok(c)
}

#[tauri::command]
async fn status(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.status().await
}

#[tauri::command]
async fn journal(conn: State<'_, Conn>, n: u32) -> Result<Value, String> {
    client(&conn).await?.journal(n).await
}

#[tauri::command]
async fn settings(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.settings().await
}

#[tauri::command]
async fn inventory(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.inventory().await
}

#[tauri::command]
async fn learned(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.learned().await
}

#[tauri::command]
async fn set_profile(conn: State<'_, Conn>, profile: String) -> Result<(), String> {
    client(&conn).await?.set_profile(&profile).await
}

#[tauri::command]
async fn set_auto(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.set_auto(on).await
}

#[tauri::command]
async fn set_learning(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.set_learning(on).await
}

#[tauri::command]
async fn undo(conn: State<'_, Conn>, setting: String) -> Result<String, String> {
    client(&conn).await?.undo(&setting).await
}

#[tauri::command]
async fn restore(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.restore().await
}

#[tauri::command]
async fn set_setting(
    conn: State<'_, Conn>,
    setting: String,
    value: String,
    allow_risky: bool,
) -> Result<(), String> {
    client(&conn)
        .await?
        .set_setting(&setting, &value, allow_risky)
        .await
}

#[tauri::command]
async fn set_safe_mode(conn: State<'_, Conn>, on: bool) -> Result<String, String> {
    client(&conn).await?.set_safe_mode(on).await
}

#[tauri::command]
async fn forget(conn: State<'_, Conn>, setting: String) -> Result<bool, String> {
    client(&conn).await?.forget(&setting).await
}

/// Save an exported log into the person's Downloads folder (or home folder
/// if there is none). Returns the full path, to show to them.
#[tauri::command]
fn save_file(name: String, contents: String) -> Result<String, String> {
    // Only a plain file name: no folders, nothing hidden.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_start_matches('.').to_string();
    if safe.is_empty() {
        return Err("invalid file name".into());
    }
    let home = std::env::var_os("HOME").ok_or("no home folder")?;
    let downloads = std::path::Path::new(&home).join("Downloads");
    let dir = if downloads.is_dir() {
        downloads
    } else {
        home.into()
    };
    let path = dir.join(safe);
    std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(Conn::default())
        .invoke_handler(tauri::generate_handler![
            status,
            journal,
            settings,
            inventory,
            learned,
            set_profile,
            set_auto,
            set_learning,
            undo,
            restore,
            set_setting,
            set_safe_mode,
            forget,
            save_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Dynamis Power app");
}
