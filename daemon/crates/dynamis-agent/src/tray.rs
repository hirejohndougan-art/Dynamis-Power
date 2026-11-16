//! The panel icon ("tray"): see the power mode at a glance and change it
//! without opening the app.
//!
//! It is part of the session agent, which already runs in every desktop
//! session and sleeps until something happens, so the icon costs no extra
//! process and the app itself can stay closed (the app must cost nothing
//! when closed).
//!
//! It speaks the two standard D-Bus protocols panels use:
//!
//! - **StatusNotifierItem** (`org.kde.StatusNotifierItem`): the icon, its
//!   tooltip, and a left click (opens the app). KDE Plasma, Xfce, Cinnamon,
//!   MATE, Budgie and others show these natively; GNOME needs the
//!   AppIndicator extension (Ubuntu ships it on).
//! - **DBusMenu** (`com.canonical.dbusmenu`): the menu, which the panel
//!   draws itself.
//!
//! Every change from the menu goes to the Dynamis Power service exactly as
//! the app sends it, so polkit and every safety rail apply the same way.
//! Rust note: the `#[zbus::interface]` blocks below are what other programs
//! see on the bus; `#[zbus(property)]` items become D-Bus properties.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, Connection, OwnedObjectPath, OwnedVariant, Structure, Variant};

use crate::strings::s;

/// Icons as panels want them: (width, height, ARGB32 bytes).
pub type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

pub const ITEM_PATH: &str = "/StatusNotifierItem";
pub const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const SERVICE: &str = "net.dynamis.Power";
const SERVICE_PATH: &str = "/net/dynamis/Power";
const SERVICE_IFACE: &str = "net.dynamis.Power1";
const PPD: &str = "org.freedesktop.UPower.PowerProfiles";
const PPD_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// Menu item IDs. 0 is the root.
mod id {
    pub const HEADER: i32 = 1;
    pub const SEP1: i32 = 2;
    pub const SEP2: i32 = 3;
    pub const SEP3: i32 = 4;
    pub const POWER_SAVER: i32 = 10;
    pub const BALANCED: i32 = 11;
    pub const PERFORMANCE: i32 = 12;
    pub const AUTO: i32 = 20;
    pub const LAST: i32 = 30;
    pub const LAST_2H: i32 = 31;
    pub const LAST_4H: i32 = 32;
    pub const LAST_6H: i32 = 33;
    pub const LAST_STOP: i32 = 34;
    pub const FULL_CHARGE: i32 = 35;
    pub const OPEN: i32 = 40;
    pub const HIDE: i32 = 41;
}

/// One menu entry: its ID, parent and D-Bus properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: i32,
    pub parent: i32,
    pub label: String,
    pub kind: Kind,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Plain,
    Separator,
    Radio(bool),
    Check(bool),
    Submenu,
}

/// The menu for a status reading (the JSON of the service's `Status`), or
/// a single line when the service is not running. Pure, for tests.
pub fn menu(status: Option<&Value>) -> Vec<Entry> {
    let e = |id, parent, label: String, kind| Entry {
        id,
        parent,
        label,
        kind,
        enabled: true,
    };
    let Some(st) = status else {
        return vec![
            Entry {
                enabled: false,
                ..e(id::HEADER, 0, s("tray_no_service").into(), Kind::Plain)
            },
            e(id::SEP3, 0, String::new(), Kind::Separator),
            e(id::OPEN, 0, s("tray_open").into(), Kind::Plain),
            e(id::HIDE, 0, s("tray_hide").into(), Kind::Plain),
        ];
    };
    let locked = !st["safe_mode"].is_null();
    let profile = st["profile"].as_str().unwrap_or("");
    let mut out = vec![
        Entry {
            enabled: false,
            ..e(id::HEADER, 0, summary(st), Kind::Plain)
        },
        e(id::SEP1, 0, String::new(), Kind::Separator),
    ];
    let offered: Vec<&str> = st["profiles"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for (pid, name, key) in [
        (id::POWER_SAVER, "power-saver", "mode_power_saver"),
        (id::BALANCED, "balanced", "mode_balanced"),
        (id::PERFORMANCE, "performance", "mode_performance"),
    ] {
        if offered.contains(&name) {
            out.push(Entry {
                enabled: !locked,
                ..e(pid, 0, s(key).into(), Kind::Radio(profile == name))
            });
        }
    }
    out.push(e(id::SEP2, 0, String::new(), Kind::Separator));
    out.push(Entry {
        enabled: !locked,
        ..e(
            id::AUTO,
            0,
            s("tray_auto").into(),
            Kind::Check(st["auto"]["on"].as_bool().unwrap_or(false)),
        )
    });
    let has_battery = st["power"]["has_battery"].as_bool().unwrap_or(false);
    if has_battery {
        out.push(Entry {
            enabled: !locked,
            ..e(id::LAST, 0, s("tray_last").into(), Kind::Submenu)
        });
        if st["last_until"].is_null() {
            out.push(e(
                id::LAST_2H,
                id::LAST,
                s("tray_last_2h").into(),
                Kind::Plain,
            ));
            out.push(e(
                id::LAST_4H,
                id::LAST,
                s("tray_last_4h").into(),
                Kind::Plain,
            ));
            out.push(e(
                id::LAST_6H,
                id::LAST,
                s("tray_last_6h").into(),
                Kind::Plain,
            ));
        } else {
            out.push(e(
                id::LAST_STOP,
                id::LAST,
                s("tray_last_stop").into(),
                Kind::Plain,
            ));
        }
    }
    // "Charge fully once" where the battery has a limit below 100.
    let fc = &st["full_charge"];
    let limited = st["power"]["charge_limit"]
        .as_f64()
        .is_some_and(|l| l < 100.0);
    if fc["supported"].as_bool().unwrap_or(false) && (limited || fc["active"] == true) {
        let active = fc["active"].as_bool().unwrap_or(false);
        out.push(Entry {
            enabled: !locked,
            ..e(
                id::FULL_CHARGE,
                0,
                s(if active {
                    "tray_full_charge_stop"
                } else {
                    "tray_full_charge"
                })
                .into(),
                Kind::Plain,
            )
        });
    }
    out.push(e(id::SEP3, 0, String::new(), Kind::Separator));
    out.push(e(id::OPEN, 0, s("tray_open").into(), Kind::Plain));
    out.push(e(id::HIDE, 0, s("tray_hide").into(), Kind::Plain));
    out
}

/// "Balanced · 79% · about 4 h 10 min left", or the safe-mode note.
pub fn summary(st: &Value) -> String {
    if !st["safe_mode"].is_null() {
        return s("tray_safe_mode").into();
    }
    let mode = match st["profile"].as_str() {
        Some("power-saver") => s("mode_power_saver"),
        Some("performance") => s("mode_performance"),
        _ => s("mode_balanced"),
    };
    let mut parts = vec![mode.to_string()];
    let p = &st["power"];
    if let Some(pct) = p["battery_pct"].as_f64() {
        parts.push(format!("{pct:.0}%"));
    }
    if let Some(h) = p["hours_left"].as_f64() {
        let mins = (h * 60.0).round() as i64;
        parts.push(
            s("tray_left")
                .replace("{h}", &(mins / 60).to_string())
                .replace("{m}", &(mins % 60).to_string()),
        );
    } else if p["on_ac"].as_bool() == Some(true) && p["has_battery"].as_bool() == Some(true) {
        parts.push(s("tray_plugged").into());
    }
    if !st["last_until"].is_null() {
        parts.push(s("tray_lasting").into());
    }
    parts.join(" · ")
}

fn props(e: &Entry) -> HashMap<String, OwnedVariant> {
    let mut m: HashMap<String, OwnedVariant> = HashMap::new();
    let mut put = |k: &str, v: Variant<'_>| {
        if let Ok(o) = OwnedVariant::try_from(v) {
            m.insert(k.to_string(), o);
        }
    };
    match e.kind {
        Kind::Separator => put("type", Variant::from("separator")),
        _ => put("label", Variant::from(e.label.as_str())),
    }
    if !e.enabled {
        put("enabled", Variant::from(false));
    }
    match e.kind {
        Kind::Radio(on) => {
            put("toggle-type", Variant::from("radio"));
            put("toggle-state", Variant::from(i32::from(on)));
        }
        Kind::Check(on) => {
            put("toggle-type", Variant::from("checkmark"));
            put("toggle-state", Variant::from(i32::from(on)));
        }
        Kind::Submenu => put("children-display", Variant::from("submenu")),
        _ => {}
    }
    m
}

/// A menu node as `GetLayout` returns it: `(ia{sv}av)`, children wrapped
/// in variants (that is how DBusMenu nests).
#[derive(Debug, serde::Serialize, zbus::Type)]
pub struct Layout {
    id: i32,
    props: HashMap<String, OwnedVariant>,
    children: Vec<OwnedVariant>,
}

fn layout(entries: &[Entry], parent: i32, depth: i32) -> zbus::Result<Layout> {
    let node = node(entries, parent, depth)?;
    let mut f = node.into_fields().into_iter();
    let id = entries.iter().find(|e| e.id == parent).map_or(0, |e| e.id);
    let _ = f.next();
    let props = match f.next() {
        Some(v) => HashMap::<String, OwnedVariant>::try_from(v)?,
        None => HashMap::new(),
    };
    let children = match f.next() {
        Some(Variant::Array(a)) => a
            .iter()
            .map(|v| OwnedVariant::try_from(v.try_clone()?))
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    Ok(Layout {
        id,
        props,
        children,
    })
}

fn node(entries: &[Entry], parent: i32, depth: i32) -> zbus::Result<Structure<'static>> {
    let props_of = |id: i32| {
        entries
            .iter()
            .find(|e| e.id == id)
            .map(props)
            .unwrap_or_else(|| {
                // The root: shows its children as a menu.
                let mut m = HashMap::new();
                if let Ok(v) = OwnedVariant::try_from(Variant::from("submenu")) {
                    m.insert("children-display".to_string(), v);
                }
                m
            })
    };
    let mut children: Vec<Variant<'static>> = Vec::new();
    if depth != 0 {
        for c in entries.iter().filter(|e| e.parent == parent) {
            children.push(Variant::from(node(entries, c.id, depth - 1)?));
        }
    }
    Structure::builder()
        .add_field(parent)
        .add_field(props_of(parent))
        .add_field(children)
        .build()
}

/// What the icon and the menu show, shared with the D-Bus objects.
pub struct Shared {
    /// The connection the service is reached on (the system bus).
    service: Connection,
    status: Mutex<Option<Value>>,
    revision: AtomicU32,
    pub verbose: bool,
}

impl Shared {
    fn status(&self) -> Option<Value> {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn entries(&self) -> Vec<Entry> {
        menu(self.status().as_ref())
    }

    /// Read the service's status again. Returns whether it changed.
    async fn reload(&self) -> bool {
        let fresh = self
            .service
            .call_method(
                Some(SERVICE),
                SERVICE_PATH,
                Some(SERVICE_IFACE),
                "Status",
                &(),
            )
            .await
            .ok()
            .and_then(|m| m.body().deserialize::<String>().ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let mut cur = self.status.lock().unwrap_or_else(|p| p.into_inner());
        // Battery level and watts change all the time; compare what the menu
        // and icon show.
        let shown = |v: &Option<Value>| v.as_ref().map(|v| (menu(Some(v)), mode_of(v).to_string()));
        let changed = shown(&cur) != shown(&fresh);
        *cur = fresh;
        if changed {
            self.revision.fetch_add(1, Ordering::Relaxed);
        }
        changed
    }

    async fn act(&self, item: i32) {
        let set_prop =
            |path: &'static str, iface: &'static str, name: &'static str, v: Variant<'static>| {
                let conn = self.service.clone();
                async move {
                    conn.call_method(
                        Some(if iface == SERVICE_IFACE { SERVICE } else { PPD }),
                        path,
                        Some("org.freedesktop.DBus.Properties"),
                        "Set",
                        &(iface, name, v),
                    )
                    .await
                    .map(|_| ())
                }
            };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let target = |h: f64| {
            let conn = self.service.clone();
            async move {
                conn.call_method(
                    Some(SERVICE),
                    SERVICE_PATH,
                    Some(SERVICE_IFACE),
                    "SetTarget",
                    &(if h > 0.0 { now + h * 3600.0 } else { 0.0 },),
                )
                .await
                .map(|_| ())
            }
        };
        let auto_on = self
            .status()
            .and_then(|s| s["auto"]["on"].as_bool())
            .unwrap_or(false);
        let charging_fully = self
            .status()
            .and_then(|s| s["full_charge"]["active"].as_bool())
            .unwrap_or(false);
        let result = match item {
            id::POWER_SAVER => set_prop(PPD_PATH, PPD, "ActiveProfile", "power-saver".into()).await,
            id::BALANCED => set_prop(PPD_PATH, PPD, "ActiveProfile", "balanced".into()).await,
            id::PERFORMANCE => set_prop(PPD_PATH, PPD, "ActiveProfile", "performance".into()).await,
            id::AUTO => set_prop(SERVICE_PATH, SERVICE_IFACE, "AutoMode", (!auto_on).into()).await,
            id::LAST_2H => target(2.0).await,
            id::LAST_4H => target(4.0).await,
            id::LAST_6H => target(6.0).await,
            id::LAST_STOP => target(0.0).await,
            id::FULL_CHARGE => self
                .service
                .call_method(
                    Some(SERVICE),
                    SERVICE_PATH,
                    Some(SERVICE_IFACE),
                    "ChargeFullyOnce",
                    &(!charging_fully,),
                )
                .await
                .map(|_| ()),
            id::OPEN => {
                open_app();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            // Refused by polkit or by the service (safe mode): the menu
            // simply keeps showing what is true.
            if self.verbose {
                eprintln!("dynamis-agent: tray: {e}");
            }
        }
    }
}

/// The mode the icon shows: "power-saver", "balanced", "performance" or
/// "off" (service not running, or safe mode).
fn mode_of(st: &Value) -> &str {
    if !st["safe_mode"].is_null() {
        return "off";
    }
    st["profile"].as_str().unwrap_or("off")
}

/// Open the app, whatever name it was installed under.
fn open_app() {
    for cmd in ["dynamis-power-app", "dynamis-power"] {
        if std::process::Command::new(cmd).spawn().is_ok() {
            return;
        }
    }
    let _ = std::process::Command::new("gtk-launch")
        .arg("dynamis-power")
        .spawn();
}

// --- The icon ---------------------------------------------------------------

/// ARGB32 (network byte order) pixmaps of the app's logo, coloured by mode.
pub fn pixmaps(mode: &str) -> Pixmaps {
    let (r, g, b) = match mode {
        "power-saver" => (0x2e, 0x7d, 0x32),
        "performance" => (0xd9, 0x48, 0x0f),
        "off" => (0x75, 0x75, 0x75),
        // Balanced: the app's canary yellow.
        _ => (0xff, 0xef, 0x00),
    };
    [16, 22, 24, 32, 48]
        .iter()
        .map(|&n| (n, n, draw(n as usize, (r, g, b))))
        .collect()
}

/// The logo (a rounded square with a lightning bolt, as in the app) drawn
/// with 4×4 supersampling, so it stays crisp at small sizes.
fn draw(n: usize, (r, g, b): (u8, u8, u8)) -> Vec<u8> {
    // In the logo's own 28×28 units.
    let bolt = [
        (15.5, 6.0),
        (9.0, 15.5),
        (14.0, 15.5),
        (12.5, 22.0),
        (19.0, 12.5),
        (14.0, 12.5),
    ];
    let inside_bolt = |x: f64, y: f64| {
        let mut c = false;
        let mut j = bolt.len() - 1;
        for i in 0..bolt.len() {
            let ((xi, yi), (xj, yj)) = (bolt[i], bolt[j]);
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                c = !c;
            }
            j = i;
        }
        c
    };
    let inside_square = |x: f64, y: f64| {
        let (lo, hi, rad) = (1.0, 27.0, 8.0);
        if !(lo..=hi).contains(&x) || !(lo..=hi).contains(&y) {
            return false;
        }
        let cx = x.clamp(lo + rad, hi - rad);
        let cy = y.clamp(lo + rad, hi - rad);
        (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad
    };
    let mut out = Vec::with_capacity(n * n * 4);
    let ss = 4;
    for py in 0..n {
        for px in 0..n {
            let (mut sq, mut bo) = (0u32, 0u32);
            for sy in 0..ss {
                for sx in 0..ss {
                    let x = (px as f64 + (sx as f64 + 0.5) / ss as f64) * 28.0 / n as f64;
                    let y = (py as f64 + (sy as f64 + 0.5) / ss as f64) * 28.0 / n as f64;
                    if inside_square(x, y) {
                        sq += 1;
                        if inside_bolt(x, y) {
                            bo += 1;
                        }
                    }
                }
            }
            let total = (ss * ss) as f64;
            let alpha = sq as f64 / total;
            // Within the square: blend the mode colour towards the bolt's
            // colour by the bolt's share of the covered samples. The bolt is
            // white on dark colours and near-black on light ones (yellow).
            let w = if sq > 0 { bo as f64 / sq as f64 } else { 0.0 };
            let light = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64 > 160.0;
            let bolt = if light { 27.0 } else { 255.0 };
            let mix = |c: u8| (c as f64 * (1.0 - w) + bolt * w).round() as u8;
            out.extend_from_slice(&[(alpha * 255.0).round() as u8, mix(r), mix(g), mix(b)]);
        }
    }
    out
}

// --- D-Bus objects -----------------------------------------------------------

pub struct Item(pub Arc<Shared>);

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> String {
        "Hardware".into()
    }
    #[zbus(property)]
    fn id(&self) -> String {
        "dynamis-power".into()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        s("app_name").into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        let st = self.0.status();
        pixmaps(st.as_ref().map_or("off", mode_of))
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        let body = self
            .0
            .status()
            .as_ref()
            .map_or_else(|| s("tray_no_service").to_string(), summary);
        (String::new(), Vec::new(), s("app_name").into(), body)
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("valid path")
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    /// Left click: open the app.
    fn activate(&self, _x: i32, _y: i32) {
        open_app();
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {}
    /// The panel shows the menu itself (from `Menu`).
    fn context_menu(&self, _x: i32, _y: i32) {}
    fn scroll(&self, _delta: i32, _orientation: String) {}
}

pub struct Menu(pub Arc<Shared>);

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }
    #[zbus(property)]
    fn text_direction(&self) -> String {
        "ltr".into()
    }
    #[zbus(property, name = "Status")]
    fn menu_status(&self) -> String {
        "normal".into()
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> fdo::Result<(u32, Layout)> {
        let entries = self.0.entries();
        let l = layout(&entries, parent_id, recursion_depth)
            .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        Ok((self.0.revision.load(Ordering::Relaxed), l))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedVariant>)> {
        let entries = self.0.entries();
        entries
            .iter()
            .filter(|e| ids.is_empty() || ids.contains(&e.id))
            .map(|e| (e.id, props(e)))
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> fdo::Result<OwnedVariant> {
        self.0
            .entries()
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| props(e).remove(&name))
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no property {name} on {id}")))
    }

    async fn event(
        &self,
        id: i32,
        event_id: String,
        _data: OwnedVariant,
        _timestamp: u32,
        #[zbus(connection)] conn: &Connection,
    ) {
        if event_id != "clicked" {
            return;
        }
        if id == id::HIDE {
            crate::set_tray_enabled(conn, false).await;
            return;
        }
        self.0.act(id).await;
        refresh(conn, &self.0).await;
    }

    async fn event_group(
        &self,
        events: Vec<(i32, String, OwnedVariant, u32)>,
        #[zbus(connection)] conn: &Connection,
    ) -> Vec<i32> {
        for (id, ev, data, ts) in events {
            self.event(id, ev, data, ts, conn).await;
        }
        Vec::new()
    }

    /// Called just before the menu opens: a cheap moment to read the latest
    /// status, so the battery line is current.
    async fn about_to_show(&self, _id: i32, #[zbus(connection)] conn: &Connection) -> bool {
        refresh(conn, &self.0).await
    }

    async fn about_to_show_group(
        &self,
        ids: Vec<i32>,
        #[zbus(connection)] conn: &Connection,
    ) -> (Vec<i32>, Vec<i32>) {
        let changed = refresh(conn, &self.0).await;
        (if changed { ids } else { Vec::new() }, Vec::new())
    }
}

/// Read the status again and tell the panel about anything that changed.
/// Returns whether something changed.
pub async fn refresh(session: &Connection, shared: &Shared) -> bool {
    if !shared.reload().await {
        return false;
    }
    let rev = shared.revision.load(Ordering::Relaxed);
    let result = async {
        let menu = SignalEmitter::new(session, MENU_PATH)?;
        menu.emit("com.canonical.dbusmenu", "LayoutUpdated", &(rev, 0i32))
            .await?;
        let item = SignalEmitter::new(session, ITEM_PATH)?;
        item.emit("org.kde.StatusNotifierItem", "NewIcon", &())
            .await?;
        item.emit("org.kde.StatusNotifierItem", "NewToolTip", &())
            .await
    }
    .await;
    if let (Err(e), true) = (result, shared.verbose) {
        eprintln!("dynamis-agent: tray: {e}");
    }
    true
}

/// The tray's objects on the session bus, and its name with the panel.
pub struct Tray {
    pub shared: Arc<Shared>,
    name: String,
}

impl Tray {
    pub async fn start(
        session: &Connection,
        service: Connection,
        verbose: bool,
    ) -> zbus::Result<Tray> {
        let shared = Arc::new(Shared {
            service,
            status: Mutex::new(None),
            revision: AtomicU32::new(1),
            verbose,
        });
        shared.reload().await;
        session
            .object_server()
            .at(ITEM_PATH, Item(shared.clone()))
            .await?;
        session
            .object_server()
            .at(MENU_PATH, Menu(shared.clone()))
            .await?;
        let tray = Tray {
            shared,
            name: format!("org.kde.StatusNotifierItem-{}-1", std::process::id()),
        };
        tray.show(session).await;
        Ok(tray)
    }

    /// Take the icon's name and tell the panel. Called again when a panel
    /// starts after the agent.
    pub async fn show(&self, session: &Connection) {
        if session.request_name(self.name.as_str()).await.is_err() {
            return;
        }
        let r = session
            .call_method(
                Some(WATCHER),
                "/StatusNotifierWatcher",
                Some(WATCHER),
                "RegisterStatusNotifierItem",
                &(self.name.as_str(),),
            )
            .await;
        if let (Err(e), true) = (r, self.shared.verbose) {
            eprintln!("dynamis-agent: no panel to show the icon in yet ({e})");
        }
    }

    /// Remove the icon from the panel (it notices the name going away).
    pub async fn hide(&self, session: &Connection) {
        let _ = session.release_name(self.name.as_str()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status() -> Value {
        json!({
            "profile": "balanced",
            "profiles": ["power-saver", "balanced", "performance"],
            "auto": {"on": true},
            "power": {"on_ac": false, "has_battery": true, "battery_pct": 79.0, "hours_left": 4.2},
            "safe_mode": null,
            "last_until": null,
        })
    }

    #[test]
    fn menu_shows_modes_auto_and_last_until() {
        let m = menu(Some(&status()));
        let find = |id| m.iter().find(|e| e.id == id).unwrap();
        assert_eq!(
            find(id::HEADER).label,
            "Balanced · 79% · about 4 h 12 min left"
        );
        assert!(!find(id::HEADER).enabled);
        assert_eq!(find(id::BALANCED).kind, Kind::Radio(true));
        assert_eq!(find(id::PERFORMANCE).kind, Kind::Radio(false));
        assert_eq!(find(id::AUTO).kind, Kind::Check(true));
        assert_eq!(find(id::LAST_4H).parent, id::LAST);
        assert!(m.iter().all(|e| e.id != id::LAST_STOP));
    }

    #[test]
    fn safe_mode_and_no_service() {
        let mut st = status();
        st["safe_mode"] = json!("switched on by hand");
        st["last_until"] = json!({"until": 1.0});
        let m = menu(Some(&st));
        assert!(m
            .iter()
            .filter(|e| matches!(e.kind, Kind::Radio(_) | Kind::Check(_)))
            .all(|e| !e.enabled));
        assert_eq!(mode_of(&st), "off");
        assert!(menu(None).iter().any(|e| e.id == id::OPEN));
        let mut st = status();
        st["last_until"] = json!({"until": 1.0});
        assert!(menu(Some(&st)).iter().any(|e| e.id == id::LAST_STOP));
    }

    #[test]
    fn charge_fully_once_where_there_is_a_limit() {
        let mut st = status();
        let item = |st: &Value| menu(Some(st)).into_iter().find(|e| e.id == id::FULL_CHARGE);
        assert!(item(&st).is_none());
        st["full_charge"] = json!({"supported": true, "active": false});
        st["power"]["charge_limit"] = json!(80);
        assert_eq!(item(&st).unwrap().label, "Charge fully once");
        st["full_charge"]["active"] = json!(true);
        st["power"]["charge_limit"] = json!(100);
        assert_eq!(item(&st).unwrap().label, "Stop charging fully");
    }

    #[test]
    fn layout_nests_the_submenu() {
        let m = menu(Some(&status()));
        let l = layout(&m, 0, -1).unwrap();
        assert_eq!(<Layout as zbus::Type>::SIGNATURE.to_string(), "(ia{sv}av)");
        assert_eq!(l.children.len(), m.iter().filter(|e| e.parent == 0).count());
        let sub = node(&m, id::LAST, -1).unwrap();
        assert_eq!(sub.signature().to_string(), "(ia{sv}av)");
    }

    #[test]
    fn icon_is_the_logo() {
        let p = pixmaps("balanced");
        let (w, h, data) = &p[1];
        assert_eq!((*w, *h, data.len()), (22, 22, 22 * 22 * 4));
        // Corner transparent, square edge canary yellow, and the bolt dark on
        // yellow (white on the darker modes).
        assert_eq!(data[0], 0);
        let at = |x: usize, y: usize| &data[(y * 22 + x) * 4..(y * 22 + x) * 4 + 4];
        assert_eq!(at(11, 11)[0], 255);
        assert!(at(11, 11)[1] < 60, "{:?}", at(11, 11));
        assert_eq!(at(3, 11), &[255, 0xff, 0xef, 0x00]);
        let saver = pixmaps("power-saver");
        let d = &saver[1].2;
        assert!(d[(11 * 22 + 11) * 4 + 1] > 200, "white bolt on green");
    }
}
