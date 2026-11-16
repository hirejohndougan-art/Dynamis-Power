//! Applies setting changes safely. This is where the rules in
//! `docs/SAFETY.md` are enforced.
//!
//! Every change follows the same steps:
//!
//! 1. Refuse if safe mode is on, the setting is blocked, its risk tier needs an
//!    opt-in that wasn't given, or the value is outside what the probe allows.
//! 2. Record the setting's original value in `baseline.json` the first time it
//!    is ever changed (never overwritten afterwards).
//! 3. Write `pending.json` and a `start` line to `journal.jsonl`, both flushed
//!    to disk, **before** touching the kernel.
//! 4. Write, then read back. If the read-back differs, put the old value back.
//! 5. Journal `done` (or `failed`) and remove `pending.json` — unless this is a
//!    trial, which stays pending until [`Engine::keep`] or [`Engine::revert`].
//!
//! If Dynamis Power stops with `pending.json` still present — a crash, a
//! freeze, a power cut, or a trial nobody answered — the next
//! [`Engine::open`] puts the old value back, journals `recover`, and blocks the
//! setting so it is not tried again until someone unblocks it.
//!
//! All state lives in one directory (default `/var/lib/dynamis-power`). Every
//! file carries a version number; see `docs/ARCHITECTURE.md`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;
use crate::quirks;
use crate::settings::{self, Probe, Setting};
use crate::writer;

pub const DEFAULT_STATE_DIR: &str = "/var/lib/dynamis-power";
/// Present in the state directory while safe mode is switched on by
/// Dynamis Power itself (after repeated unclean shutdowns) or by
/// `dynamis safe-mode on`. Holds the reason.
pub const SAFE_MODE_FILE: &str = "safe-mode";
/// The journal is rotated when it grows past this many bytes; one previous
/// file (`journal.1.jsonl`) is kept.
pub const JOURNAL_ROTATE_BYTES: u64 = 1 << 20;
/// Creating this file turns on safe mode.
pub const DISABLE_FILE: &str = "/etc/dynamis-power/disable";
/// Adding this to the kernel command line turns on safe mode.
pub const SAFE_MODE_CMDLINE: &str = "dynamis.safe=1";
/// Where the unprivileged `dynamis-sync` helper leaves downloaded files.
pub const DEFAULT_SYNC_DIR: &str = "/var/lib/dynamis-power-sync";

/// Version of `journal.jsonl` lines and the JSON state files.
pub const STATE_FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum EngineError {
    SafeMode(String),
    Locked(String),
    UnknownSetting(String),
    Unsupported(String),
    NeedsOptIn(String),
    Advisory(String),
    Blocked { id: String, reason: String },
    Invalid(String),
    Write(String),
    State(String),
    NoTrial,
    Quirk { id: String, reason: String },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use EngineError::*;
        match self {
            SafeMode(why) => write!(
                f,
                "safe mode is on ({why}); changes are refused, restore still works"
            ),
            Locked(s) => write!(
                f,
                "another Dynamis Power process is changing settings ({s})"
            ),
            UnknownSetting(id) => write!(f, "no setting called '{id}' (see `dynamis settings`)"),
            Unsupported(s) => write!(f, "this machine does not offer that setting: {s}"),
            NeedsOptIn(id) => write!(
                f,
                "'{id}' is risky or experimental; add --allow-risky to confirm"
            ),
            Advisory(id) => write!(
                f,
                "'{id}' is advice only and is never changed automatically"
            ),
            Blocked { id, reason } => write!(
                f,
                "'{id}' is blocked: {reason} (use `dynamis unblock {id}`)"
            ),
            Invalid(s) => write!(f, "{s}"),
            Write(s) => write!(f, "could not change the setting: {s}"),
            State(s) => write!(f, "state directory problem: {s}"),
            NoTrial => write!(f, "no trial is in progress"),
            Quirk { id, reason } => write!(f, "'{id}' is not allowed on this machine: {reason}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<io::Error> for EngineError {
    fn from(e: io::Error) -> Self {
        EngineError::State(e.to_string())
    }
}

type Result<T> = std::result::Result<T, EngineError>;

/// One line of `journal.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub v: u32,
    pub time: u64,
    /// start, done, failed, keep, revert, recover, restore, block, unblock
    pub event: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// A change that has been written but not yet confirmed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pending {
    pub v: u32,
    pub id: String,
    pub from: String,
    pub to: String,
    /// Unix time by which a trial must be kept, or it is reverted.
    pub trial_deadline: Option<u64>,
    /// The process that made the change. Recovery leaves a change alone while
    /// its owner is still running (it may be mid-trial or mid-experiment).
    /// Absent in files written before this field existed: treated as gone.
    #[serde(default)]
    pub owner: Option<Owner>,
}

/// Identifies a process across PID reuse and reboots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Owner {
    pub pid: u32,
    /// Start time from /proc/PID/stat, in clock ticks since boot.
    pub start: u64,
    pub boot_id: String,
}

impl Owner {
    /// This process. Process identity is a property of the real machine, not
    /// of the (possibly fixture) SysRoot, so the real /proc is right here.
    pub fn current() -> Owner {
        let pid = std::process::id();
        Owner {
            pid,
            start: proc_start(pid).unwrap_or(0),
            boot_id: boot_id(),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.boot_id == boot_id() && proc_start(self.pid).is_some_and(|s| s == self.start)
    }
}

fn boot_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn proc_start(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name (field 2) is in parentheses and may contain spaces;
    // fields after the last ')' are plain. Start time is field 22.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Baseline {
    v: u32,
    values: BTreeMap<String, String>,
}

/// App cgroups whose CPU weight was changed, with the original value.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Touched {
    v: u32,
    weights: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Blocked {
    v: u32,
    blocked: BTreeMap<String, String>,
}

/// What [`Engine::apply`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Changed { from: String, to: String },
    AlreadySet(String),
}

/// What [`Engine::open`] found and fixed from an earlier run.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    pub id: String,
    pub restored_to: String,
    pub ok: bool,
}

#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub id: &'a str,
    pub value: &'a str,
    pub allow_risky: bool,
    /// Seconds before an unconfirmed change reverts. `None` applies directly.
    pub trial_secs: Option<u64>,
    pub reason: &'a str,
}

pub struct Engine {
    root: SysRoot,
    dir: PathBuf,
    _lock: Lock,
    recovered: Option<Recovered>,
    quirks: quirks::Active,
}

impl Engine {
    /// Open the state directory, take the lock, and recover from any change
    /// an earlier run left unfinished.
    pub fn open(root: SysRoot, state_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with_keys(root, state_dir, quirks::TRUSTED_KEYS)
    }

    /// Like [`Engine::open`], trusting `keys` for downloaded quirk lists
    /// instead of the built-in [`quirks::TRUSTED_KEYS`]. For tests.
    pub fn open_with_keys(
        root: SysRoot,
        state_dir: impl Into<PathBuf>,
        keys: &[&str],
    ) -> Result<Self> {
        let dir = state_dir.into();
        fs::create_dir_all(&dir)?;
        let lock = Lock::take(&dir.join("lock"))?;
        // The real service reads the helper's directory; anything else (tests,
        // `--state-dir`) keeps its downloads next to its own state.
        let sync_dir = if dir == Path::new(DEFAULT_STATE_DIR) {
            PathBuf::from(DEFAULT_SYNC_DIR)
        } else {
            dir.join("sync")
        };
        let min_serial = fs::read_to_string(dir.join("quirk-serial"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let quirks = quirks::load_with_keys(&root, &sync_dir, min_serial, keys);
        if let quirks::Downloaded::Used { serial, .. } = quirks.downloaded {
            if serial > min_serial {
                fs::write(dir.join("quirk-serial"), serial.to_string())?;
            }
        }
        let mut engine = Engine {
            root,
            dir,
            _lock: lock,
            recovered: None,
            quirks,
        };
        engine.recover()?;
        engine.enforce_quirks()?;
        Ok(engine)
    }

    /// The quirks that apply to this machine, and where they came from.
    pub fn quirks(&self) -> &quirks::Active {
        &self.quirks
    }

    /// Set if [`Engine::open`] found an unfinished change and undid it.
    pub fn recovered(&self) -> Option<&Recovered> {
        self.recovered.as_ref()
    }

    /// Why safe mode is on, if it is.
    pub fn safe_mode(&self) -> Option<String> {
        if let Ok(why) = fs::read_to_string(self.dir.join(SAFE_MODE_FILE)) {
            return Some(why.trim().to_string());
        }
        if self.root.exists(DISABLE_FILE) {
            return Some(format!("{DISABLE_FILE} exists"));
        }
        let cmdline = self.root.read_string("/proc/cmdline").unwrap_or_default();
        cmdline
            .split_whitespace()
            .any(|w| w == SAFE_MODE_CMDLINE)
            .then(|| format!("{SAFE_MODE_CMDLINE} on the kernel command line"))
    }

    /// Switch safe mode on (`Some(reason)`) or off (`None`). Switching it off
    /// also resets the unclean-shutdown count. Only affects the safe mode kept
    /// in the state directory, not the `/etc` file or the boot option.
    pub fn set_safe_mode(&mut self, on: Option<&str>) -> Result<()> {
        let path = self.dir.join(SAFE_MODE_FILE);
        match on {
            Some(reason) => {
                fs::write(&path, format!("{reason}\n"))?;
                self.log("safe-mode-on", "", None, None, reason)
            }
            None => {
                if path.exists() {
                    fs::remove_file(&path)?;
                    crate::boots::reset(&self.dir)?;
                    self.log("safe-mode-off", "", None, None, "")?;
                }
                Ok(())
            }
        }
    }

    pub fn pending(&self) -> Option<Pending> {
        read_json(&self.dir.join("pending.json"))
    }

    pub fn blocked(&self) -> BTreeMap<String, String> {
        read_json::<Blocked>(&self.dir.join("blocked.json"))
            .unwrap_or_default()
            .blocked
    }

    pub fn baseline(&self) -> BTreeMap<String, String> {
        read_json::<Baseline>(&self.dir.join("baseline.json"))
            .unwrap_or_default()
            .values
    }

    /// The last `n` journal entries, oldest first. Lines that fail to parse
    /// (a torn final line after a power cut) are skipped.
    pub fn journal(&self, n: usize) -> Vec<Entry> {
        let mut text = fs::read_to_string(self.dir.join("journal.1.jsonl")).unwrap_or_default();
        text.push_str(&fs::read_to_string(self.dir.join("journal.jsonl")).unwrap_or_default());
        let all: Vec<Entry> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let skip = all.len().saturating_sub(n);
        all.into_iter().skip(skip).collect()
    }

    /// Change one setting, following the steps in the module docs.
    pub fn apply(&mut self, req: &Request) -> Result<Outcome> {
        if let Some(why) = self.safe_mode() {
            return Err(EngineError::SafeMode(why));
        }
        if let Some(p) = self.pending() {
            return Err(EngineError::Invalid(format!(
                "a trial of '{}' is still in progress; keep or revert it first",
                p.id
            )));
        }
        let s =
            settings::by_id(req.id).ok_or_else(|| EngineError::UnknownSetting(req.id.into()))?;
        match s.risk() {
            settings::RiskTier::Advisory => return Err(EngineError::Advisory(req.id.into())),
            r if r.needs_opt_in() && !req.allow_risky => {
                return Err(EngineError::NeedsOptIn(req.id.into()))
            }
            _ => {}
        }
        if let Some(reason) = self.blocked().get(req.id) {
            return Err(EngineError::Blocked {
                id: req.id.into(),
                reason: reason.clone(),
            });
        }
        let allowed = match s.probe(&self.root) {
            Probe::Supported { allowed } => allowed,
            Probe::Unsupported { reason } => return Err(EngineError::Unsupported(reason)),
        };
        let to = allowed.check(req.value).map_err(EngineError::Invalid)?;
        if let Some(q) = self.quirks.blocking(req.id, &to) {
            return Err(EngineError::Quirk {
                id: req.id.into(),
                reason: q.reason.clone(),
            });
        }
        let from = s
            .read(&self.root)
            .map_err(|e| EngineError::Unsupported(format!("cannot read current value: {e}")))?;
        if from == "mixed" {
            return Err(EngineError::Invalid(format!(
                "'{}' differs between CPUs; restore it first so there is one value to return to",
                req.id
            )));
        }
        if from == to {
            return Ok(Outcome::AlreadySet(to));
        }
        let writes = s.plan(&self.root, &to).map_err(EngineError::Invalid)?;

        self.remember_baseline(req.id, &from)?;
        let deadline = req.trial_secs.map(|t| now() + t);
        write_json(
            &self.dir.join("pending.json"),
            &Pending {
                v: STATE_FORMAT_VERSION,
                id: req.id.into(),
                from: from.clone(),
                to: to.clone(),
                trial_deadline: deadline,
                owner: Some(Owner::current()),
            },
        )?;
        self.log("start", req.id, Some(&from), Some(&to), req.reason)?;

        if let Err(e) = self.write_and_verify(s, &writes, &to) {
            let back = self.put_back(s, &from);
            self.log(
                "failed",
                req.id,
                Some(&from),
                Some(&to),
                &format!("{e}; put back: {back}"),
            )?;
            self.clear_pending()?;
            return Err(EngineError::Write(e));
        }

        if deadline.is_none() {
            self.log("done", req.id, Some(&from), Some(&to), "")?;
            self.clear_pending()?;
        }
        Ok(Outcome::Changed { from, to })
    }

    /// Confirm the trial in progress.
    pub fn keep(&mut self) -> Result<()> {
        let p = self.pending().ok_or(EngineError::NoTrial)?;
        self.log("keep", &p.id, Some(&p.from), Some(&p.to), "")?;
        self.clear_pending()
    }

    /// Undo the trial in progress.
    pub fn revert(&mut self, reason: &str) -> Result<()> {
        let p = self.pending().ok_or(EngineError::NoTrial)?;
        let s = settings::by_id(&p.id).ok_or_else(|| EngineError::UnknownSetting(p.id.clone()))?;
        let result = self.put_back(s, &p.from);
        self.log(
            "revert",
            &p.id,
            Some(&p.to),
            Some(&p.from),
            &format!("{reason}; {result}"),
        )?;
        self.clear_pending()?;
        if result == "ok" {
            Ok(())
        } else {
            Err(EngineError::Write(result))
        }
    }

    /// Undo the trial in progress because it made the machine unhealthy, and
    /// block the setting so it is not tried again until someone unblocks it.
    pub fn revert_unhealthy(&mut self, reason: &str) -> Result<()> {
        let p = self.pending().ok_or(EngineError::NoTrial)?;
        let result = self.revert(reason);
        self.block(&p.id, reason)?;
        result
    }

    // --- Per-app CPU weights (phase 6) ----------------------------------

    /// Set the CPU weight (1–10000, normal 100) of one app's cgroup, given as
    /// it appears in /proc/PID/cgroup (`/user.slice/…/app-x.scope`). A
    /// weight is a share when apps compete for the CPU, never a cap, so this
    /// cannot stop an app from running. Journaled; the first value seen is
    /// kept so `restore` can put it back. Returns whether anything changed.
    pub fn set_app_weight(
        &mut self,
        app: &str,
        cgroup: &str,
        weight: u32,
        reason: &str,
    ) -> Result<bool> {
        if let Some(why) = self.safe_mode() {
            return Err(EngineError::SafeMode(why));
        }
        if !(1..=10_000).contains(&weight) {
            return Err(EngineError::Invalid(format!(
                "weight {weight} is outside 1–10000"
            )));
        }
        if !cgroup.starts_with("/user.slice/") || cgroup.contains("..") {
            return Err(EngineError::Invalid(format!(
                "not a user app group: {cgroup}"
            )));
        }
        let path = format!("/sys/fs/cgroup{cgroup}/cpu.weight");
        let current = self
            .root
            .read_string(&path)
            .map_err(|e| EngineError::Unsupported(format!("{path}: {e}")))?;
        if current == weight.to_string() {
            return Ok(false);
        }
        let file = self.dir.join("cgroups.json");
        let mut touched: Touched = read_json(&file).unwrap_or_default();
        if !touched.weights.contains_key(cgroup) {
            touched.v = STATE_FORMAT_VERSION;
            touched.weights.insert(cgroup.to_string(), current.clone());
            write_json(&file, &touched)?;
        }
        self.log(
            "app-weight",
            app,
            Some(&current),
            Some(&weight.to_string()),
            reason,
        )?;
        let w = settings::Write {
            path: path.clone(),
            contents: weight.to_string(),
        };
        writer::write_all(&self.root, &[w]).map_err(EngineError::Write)?;
        Ok(true)
    }

    /// Put every app weight changed back. Apps that have since closed are
    /// simply skipped. Returns how many were restored.
    pub fn restore_app_weights(&mut self) -> Result<usize> {
        let file = self.dir.join("cgroups.json");
        let touched: Touched = read_json(&file).unwrap_or_default();
        let mut n = 0;
        for (cgroup, original) in &touched.weights {
            let path = format!("/sys/fs/cgroup{cgroup}/cpu.weight");
            match self.root.read_string(&path) {
                Ok(v) if v != *original => {
                    let w = settings::Write {
                        path,
                        contents: original.clone(),
                    };
                    if writer::write_all(&self.root, &[w]).is_ok() {
                        n += 1;
                    }
                }
                _ => {}
            }
        }
        if n > 0 {
            self.log(
                "restore",
                "app weights",
                None,
                None,
                &format!("{n} apps back to normal"),
            )?;
        }
        let _ = fs::remove_file(&file);
        Ok(n)
    }

    /// Put every changed setting back to its original value. Works in safe
    /// mode and on blocked settings. Returns (id, original, ok) per setting
    /// that needed changing. Also puts app CPU weights back.
    pub fn restore(&mut self) -> Result<Vec<(String, String, bool)>> {
        if self.pending().is_some() {
            self.revert("restore requested")?;
        }
        self.restore_app_weights()?;
        let mut done = Vec::new();
        for (id, original) in self.baseline() {
            let Some(s) = settings::by_id(&id) else {
                continue;
            };
            if s.read(&self.root).ok().as_deref() == Some(original.as_str()) {
                continue;
            }
            let result = self.put_back(s, &original);
            self.log("restore", &id, None, Some(&original), &result)?;
            done.push((id, original, result == "ok"));
        }
        Ok(done)
    }

    /// Put one setting back to its original value (Undo in the app).
    /// Returns the original value, or `None` if it was never changed.
    pub fn restore_one(&mut self, id: &str) -> Result<Option<String>> {
        if self.pending().is_some_and(|p| p.id == id) {
            self.revert("undo requested")?;
        }
        let Some(original) = self.baseline().get(id).cloned() else {
            return Ok(None);
        };
        let s = settings::by_id(id).ok_or_else(|| EngineError::UnknownSetting(id.into()))?;
        if s.read(&self.root).ok().as_deref() == Some(original.as_str()) {
            return Ok(Some(original));
        }
        let result = self.put_back(s, &original);
        self.log(
            "restore",
            id,
            None,
            Some(&original),
            &format!("undo; {result}"),
        )?;
        if result == "ok" {
            Ok(Some(original))
        } else {
            Err(EngineError::Write(result))
        }
    }

    pub fn unblock(&mut self, id: &str) -> Result<bool> {
        let mut b: Blocked = read_json(&self.dir.join("blocked.json")).unwrap_or_default();
        let was = b.blocked.remove(id).is_some();
        if was {
            b.v = STATE_FORMAT_VERSION;
            write_json(&self.dir.join("blocked.json"), &b)?;
            self.log("unblock", id, None, None, "")?;
        }
        Ok(was)
    }

    // --- internals ---------------------------------------------------------

    fn recover(&mut self) -> Result<()> {
        let Some(p) = self.pending() else {
            return Ok(());
        };
        if p.owner.as_ref().is_some_and(|o| o.is_alive()) {
            // In flight in a process that is still running: not ours to undo.
            return Ok(());
        }
        let reason = match p.trial_deadline {
            Some(_) => "a trial did not finish (crash, freeze or no answer)",
            None => "a change did not finish (crash or power cut)",
        };
        let result = match settings::by_id(&p.id) {
            Some(s) => self.put_back(s, &p.from),
            None => "unknown setting".into(),
        };
        self.log(
            "recover",
            &p.id,
            Some(&p.to),
            Some(&p.from),
            &format!("{reason}; {result}"),
        )?;
        self.block(&p.id, reason)?;
        self.clear_pending()?;
        self.recovered = Some(Recovered {
            id: p.id,
            restored_to: p.from,
            ok: result == "ok",
        });
        Ok(())
    }

    /// A quirk may have arrived after a setting was changed: put any setting
    /// whose current value is now blocked back to its original value.
    fn enforce_quirks(&mut self) -> Result<()> {
        for (id, original) in self.baseline() {
            let Some(s) = settings::by_id(&id) else {
                continue;
            };
            let Ok(current) = s.read(&self.root) else {
                continue;
            };
            if current == original {
                continue;
            }
            let Some(q) = self.quirks.blocking(&id, &current) else {
                continue;
            };
            let reason = format!("quirk {}: {}", q.id, q.reason);
            let result = self.put_back(s, &original);
            self.log(
                "quirk",
                &id,
                Some(&current),
                Some(&original),
                &format!("{reason}; {result}"),
            )?;
        }
        Ok(())
    }

    fn block(&mut self, id: &str, reason: &str) -> Result<()> {
        let mut b: Blocked = read_json(&self.dir.join("blocked.json")).unwrap_or_default();
        b.v = STATE_FORMAT_VERSION;
        b.blocked.insert(id.into(), reason.into());
        write_json(&self.dir.join("blocked.json"), &b)?;
        self.log("block", id, None, None, reason)
    }

    fn write_and_verify(
        &self,
        s: &dyn Setting,
        writes: &[settings::Write],
        to: &str,
    ) -> std::result::Result<(), String> {
        writer::write_all(&self.root, writes)?;
        match s.read(&self.root) {
            Ok(v) if v == to => Ok(()),
            Ok(v) => Err(format!("read back '{v}' instead of '{to}'")),
            Err(e) => Err(format!("could not read back: {e}")),
        }
    }

    /// Write `value` back without the checks in `apply` (it was the value
    /// before, so it is known-good). Returns "ok" or what went wrong.
    fn put_back(&self, s: &dyn Setting, value: &str) -> String {
        match s
            .plan(&self.root, value)
            .and_then(|w| self.write_and_verify(s, &w, value))
        {
            Ok(()) => "ok".into(),
            Err(e) => e,
        }
    }

    fn remember_baseline(&self, id: &str, value: &str) -> Result<()> {
        let path = self.dir.join("baseline.json");
        let mut b: Baseline = read_json(&path).unwrap_or_default();
        if b.values.contains_key(id) {
            return Ok(());
        }
        b.v = STATE_FORMAT_VERSION;
        b.values.insert(id.into(), value.into());
        write_json(&path, &b)
    }

    fn clear_pending(&self) -> Result<()> {
        match fs::remove_file(self.dir.join("pending.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn log(
        &self,
        event: &str,
        id: &str,
        from: Option<&str>,
        to: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        let e = Entry {
            v: STATE_FORMAT_VERSION,
            time: now(),
            event: event.into(),
            id: id.into(),
            from: from.map(String::from),
            to: to.map(String::from),
            reason: reason.into(),
        };
        let mut line = serde_json::to_string(&e).map_err(|e| EngineError::State(e.to_string()))?;
        line.push('\n');
        // Rotation never affects recovery, which uses pending.json, not the
        // journal; it only bounds the history kept on disk.
        let current = self.dir.join("journal.jsonl");
        if fs::metadata(&current).is_ok_and(|m| m.len() > JOURNAL_ROTATE_BYTES) {
            fs::rename(&current, self.dir.join("journal.1.jsonl"))?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("journal.jsonl"))?;
        f.write_all(line.as_bytes())?;
        // Flushed to disk before the kernel is touched, so the record survives
        // a freeze that follows the write.
        f.sync_all()?;
        Ok(())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Write via a temporary file and rename, so a crash never leaves half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let text =
        serde_json::to_string_pretty(value).map_err(|e| EngineError::State(e.to_string()))?;
    let mut f = File::create(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Only one process may change settings at a time.
///
/// This uses the kernel's advisory file lock (`flock`) on `lock`. The kernel
/// releases it the moment the owning process exits, even after a crash or
/// `kill -9`, so a stale lock can never keep everyone out. The file itself is
/// never deleted (deleting a lock file lets two processes each lock a different
/// file); it holds the owner's PID only as a hint for people.
struct Lock {
    // Held only for its lock; dropping the file releases it.
    _file: File,
}

impl Lock {
    fn take(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                let pid = fs::read_to_string(path).unwrap_or_default();
                return Err(EngineError::Locked(format!("process {}", pid.trim())));
            }
            Err(fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        file.set_len(0)?;
        write!(file, "{}", std::process::id())?;
        Ok(Lock { _file: file })
    }
}
