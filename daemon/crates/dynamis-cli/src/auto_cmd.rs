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
