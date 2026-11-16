//! `dynamis auto`: switch auto mode on or off, or see what it is doing. Talks
//! to the running `dynamisd` over D-Bus (auto mode lives in the service, not
//! in this command).

use std::io;

use zbus::Connection;

const NAME: &str = "net.dynamis.Power";
const PATH: &str = "/net/dynamis/Power";
const IFACE: &str = "net.dynamis.Power1";
const PPD: &str = "org.freedesktop.UPower.PowerProfiles";
const PPD_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

fn err(e: zbus::Error) -> io::Error {
    let text = e.to_string();
    if text.contains("ServiceUnknown") || text.contains("was not provided") {
        io::Error::other("the Dynamis Power service is not running (start it with: sudo systemctl start dynamisd)")
    } else if text.contains("AccessDenied") {
        io::Error::other(format!("not allowed: {text}"))
    } else {
        io::Error::other(text)
    }
}

/// `switch`: None = status, Some(true) = on, Some(false) = off.
pub fn cmd_auto(switch: Option<bool>, session_bus: bool) -> io::Result<()> {
    zbus::block_on(async {
        let conn = if session_bus {
            Connection::session().await
        } else {
            Connection::system().await
        }
        .map_err(err)?;
        let own = zbus::Proxy::new(&conn, NAME, PATH, IFACE)
            .await
            .map_err(err)?;
        if let Some(on) = switch {
            own.set_property("AutoMode", on)
                .await
                .map_err(|e| err(e.into()))?;
        }
        let enabled: bool = own.get_property("AutoMode").await.map_err(err)?;
        let reason: String = own.get_property("AutoReason").await.map_err(err)?;
        let workload: String = own.get_property("Workload").await.map_err(err)?;
        let ppd = zbus::Proxy::new(&conn, PPD, PPD_PATH, PPD)
            .await
            .map_err(err)?;
        let active: String = ppd.get_property("ActiveProfile").await.map_err(err)?;

        println!("Auto mode:  {}", if enabled { "on" } else { "off" });
        println!("Profile:    {active}");
        if enabled {
            if !workload.is_empty() {
                println!("Workload:   {}", workload.replace('_', " "));
            }
            println!("Why:        {reason}");
        }
        Ok(())
    })
}

/// `dynamis learn [on|off|status]`.
pub fn cmd_learn(switch: Option<bool>, session_bus: bool) -> io::Result<()> {
    zbus::block_on(async {
        let conn = if session_bus {
            Connection::session().await
        } else {
            Connection::system().await
        }
        .map_err(err)?;
        let own = zbus::Proxy::new(&conn, NAME, PATH, IFACE)
            .await
            .map_err(err)?;
        if let Some(on) = switch {
            own.set_property("Learning", on)
                .await
                .map_err(|e| err(e.into()))?;
        }
        let on: bool = own.get_property("Learning").await.map_err(err)?;
        let status: String = own.get_property("LearningStatus").await.map_err(err)?;
        println!("Learning:  {}", if on { "on" } else { "off" });
        if on {
            println!("Now:       {status}");
            println!("Tests run only on battery, at quiet moments, one setting at a time.");
        }
        Ok(())
    })
}

/// `dynamis learned [--json]` and `dynamis learned forget ID`.
pub fn cmd_learned(forget: Option<&str>, json: bool, session_bus: bool) -> io::Result<()> {
    zbus::block_on(async {
        let conn = if session_bus {
            Connection::session().await
        } else {
            Connection::system().await
        }
        .map_err(err)?;
        let own = zbus::Proxy::new(&conn, NAME, PATH, IFACE)
            .await
            .map_err(err)?;
        if let Some(id) = forget {
            let done: bool = own.call("Forget", &(id,)).await.map_err(err)?;
            println!(
                "{}",
                if done {
                    format!("Forgot {id}; its original value is back.")
                } else {
                    format!("Nothing was learned about {id}.")
                }
            );
            return Ok(());
        }
        let text: String = own.call("LearnedJson", &()).await.map_err(err)?;
        if json {
            println!("{text}");
            return Ok(());
        }
        let v: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        let learned = v["learned"].as_array().cloned().unwrap_or_default();
        let records = v["records"].as_array().cloned().unwrap_or_default();
        if learned.is_empty() {
            println!("Nothing learned yet ({} tests run).", records.len());
        } else {
            println!("Learned (applied on battery):");
            for r in &learned {
                println!(
                    "  {} = {} (was {}): {}",
                    s(&r["setting"]),
                    s(&r["value"]),
                    s(&r["baseline"]),
                    s(&r["reason"])
                );
            }
        }
        if !records.is_empty() {
            println!("\nAll tests:");
            for r in &records {
                println!(
                    "  {:<22} {:<10} {:<13} {}",
                    s(&r["setting"]),
                    s(&r["value"]),
                    s(&r["verdict"]),
                    s(&r["reason"])
                );
                // Phase 5: what it was tested together with.
                if let Some(g) = r["group"].as_object().filter(|g| !g.is_empty()) {
                    let with: Vec<String> =
                        g.iter().map(|(k, v)| format!("{k}={}", s(v))).collect();
                    println!("  {:<22} with {}", "", with.join(", "));
                }
            }
        }
        match v["display"].as_object() {
            Some(d) => println!(
                "\nScreen: about {:.2} W per 10% brightness ({} readings)",
                d["w_per_pct"].as_f64().unwrap_or(0.0) * 10.0,
                d["samples"]
            ),
            None => println!(
                "\nScreen: not enough readings yet (brightness needs to vary while on battery)."
            ),
        }
        Ok(())
    })
}

fn s(v: &serde_json::Value) -> String {
    v.as_str()
        .map(String::from)
        .unwrap_or_else(|| v.to_string())
}
