//! Commands that list, change and restore settings. All changes go through
//! `dynamis_core::engine::Engine`, which enforces the safety rules.

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use dynamis_core::engine::{Engine, EngineError, Outcome, Request};
use dynamis_core::health::{Limits, Monitor, Problem};
use dynamis_core::quirks;
use dynamis_core::settings::{self, Probe};
use dynamis_core::SysRoot;

fn err(e: EngineError) -> io::Error {
    io::Error::other(e.to_string())
}

fn open(root: &SysRoot, state: &Path) -> io::Result<Engine> {
    let engine = Engine::open(root.clone(), state).map_err(|e| {
        if let EngineError::State(s) = &e {
            if s.contains("ermission denied") {
                return io::Error::other(format!(
                    "cannot open {} (run with sudo): {s}",
                    state.display()
                ));
            }
        }
        err(e)
    })?;
    if let Some(r) = engine.recovered() {
        eprintln!(
            "Note: a change to '{}' did not finish last time (crash, freeze or no answer). \
             It was put back to '{}'{} and blocked. See `dynamis journal`.",
            r.id,
            r.restored_to,
            if r.ok {
                ""
            } else {
                " — but that failed; check it by hand"
            }
        );
    }
    Ok(engine)
}

pub fn cmd_settings(root: &SysRoot, state: &Path, json: bool) -> io::Result<()> {
    // Listing only reads, so it does not need the state directory to be
    // writable; it shows blocked settings when the state can be read.
    let (blocked, quirks) = match Engine::open(root.clone(), state) {
        Ok(e) => (e.blocked(), e.quirks().clone()),
        // Without access to the state (not root): built-in quirks only.
        Err(_) => (
            Default::default(),
            quirks::load(root, Path::new("/nonexistent"), 0),
        ),
    };
    let quirked = |id: &str| -> Vec<&quirks::Quirk> {
        quirks.quirks.iter().filter(|q| q.setting == id).collect()
    };
    let mut out = io::stdout().lock();
    if json {
        let rows: Vec<_> = settings::all()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id(),
                    "label": s.label(),
                    "risk": s.risk(),
                    "probe": s.probe(root),
                    "current": s.read(root).ok(),
                    "blocked": blocked.get(s.id()),
                    "quirks": quirked(s.id()),
                })
            })
            .collect();
        serde_json::to_writer_pretty(&mut out, &rows)?;
        return writeln!(out);
    }
    writeln!(
        out,
        "SETTING                RISK          NOW                    ALLOWED"
    )?;
    for s in settings::all() {
        let now = s.read(root).unwrap_or_else(|e| format!("({e})"));
        let allowed = match s.probe(root) {
            Probe::Supported { allowed } => allowed.describe(),
            Probe::Unsupported { reason } => format!("not available: {reason}"),
        };
        writeln!(
            out,
            "{:<22} {:<13} {:<22} {}",
            s.id(),
            s.risk().label(),
            now,
            allowed
        )?;
        writeln!(out, "  {}", s.label())?;
        if let Some(why) = blocked.get(s.id()) {
            writeln!(out, "  BLOCKED: {why}")?;
        }
        for q in quirked(s.id()) {
            let what = if q.values.is_empty() {
                "any change".to_string()
            } else {
                q.values.join(", ")
            };
            writeln!(out, "  NOT ALLOWED ON THIS MACHINE: {what} ({})", q.reason)?;
        }
    }
    Ok(())
}

pub fn cmd_quirks(root: &SysRoot, state: &Path, json: bool) -> io::Result<()> {
    let engine = open(root, state)?;
    let q = engine.quirks();
    let mut out = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut out, q)?;
        return writeln!(out);
    }
    writeln!(out, "Built-in list:   serial {}", q.builtin_serial)?;
    let downloaded = match &q.downloaded {
        quirks::Downloaded::None => "none yet".to_string(),
        quirks::Downloaded::Used { serial, issued } => {
            format!("serial {serial}, issued {issued}, signature OK")
        }
        quirks::Downloaded::Rejected { why } => format!("IGNORED: {why}"),
    };
    writeln!(out, "Downloaded list: {downloaded}")?;
    if q.quirks.is_empty() {
        writeln!(out, "No quirks apply to this machine.")?;
    }
    for x in &q.quirks {
        let what = if x.values.is_empty() {
            "any value".to_string()
        } else {
            x.values.join(", ")
        };
        writeln!(out, "\n{}  {} = {}", x.id, x.setting, what)?;
        writeln!(out, "  {}", x.reason)?;
        if !x.url.is_empty() {
            writeln!(out, "  {}", x.url)?;
        }
    }
    Ok(())
}

pub struct SetArgs<'a> {
    pub id: &'a str,
    pub value: &'a str,
    pub trial: Option<u64>,
    pub allow_risky: bool,
    pub reason: &'a str,
}

pub fn cmd_set(root: &SysRoot, state: &Path, a: SetArgs) -> io::Result<()> {
    let mut engine = open(root, state)?;
    // Start watching before the write, so trouble the write itself causes is
    // caught.
    let mut monitor = a.trial.map(|_| Monitor::start(root, Limits::default()));
    let outcome = engine
        .apply(&Request {
            id: a.id,
            value: a.value,
            allow_risky: a.allow_risky,
            trial_secs: a.trial,
            reason: a.reason,
        })
        .map_err(err)?;
    let (from, to) = match outcome {
        Outcome::AlreadySet(v) => {
            println!("{} is already {v}. Nothing changed.", a.id);
            return Ok(());
        }
        Outcome::Changed { from, to } => (from, to),
    };
    println!("Changed {}: {from} → {to}", a.id);
    let Some(secs) = a.trial else {
        println!("Undo with `dynamis restore`.");
        return Ok(());
    };

    let mut monitor = monitor.take().expect("monitor starts with every trial");
    if !monitor.watching_kernel_log() {
        println!("(Not watching the kernel log: run with sudo for the full health check.)");
    }
    print!("Watching for problems. Keep this change? Type y and Enter within {secs} seconds, or it is undone: ");
    io::stdout().flush()?;
    match wait_for_answer(Duration::from_secs(secs), &mut monitor) {
        Answer::Yes => {
            engine.keep().map_err(err)?;
            println!("Kept.");
        }
        Answer::No => {
            println!();
            engine
                .revert("not confirmed (no, no answer, or timed out)")
                .map_err(err)?;
            println!("Undone: {} is back to {from}.", a.id);
        }
        Answer::Unhealthy(problems) => {
            let why: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
            println!();
            engine
                .revert_unhealthy(&format!("health check: {}", why.join("; ")))
                .map_err(err)?;
            println!("Undone because of a problem during the trial:");
            for w in why {
                println!("  - {w}");
            }
            println!(
                "{} is back to {from} and blocked until `dynamis unblock {}`. See `dynamis journal`.",
                a.id, a.id
            );
        }
    }
    Ok(())
}

enum Answer {
    Yes,
    No,
    Unhealthy(Vec<Problem>),
}

/// Wait for a "y" on standard input, checking the machine's health every
/// second. Anything but "y", end of input, the timeout, or a health problem
/// counts as no — the safe answer.
fn wait_for_answer(timeout: Duration, monitor: &mut Monitor) -> Answer {
    let (tx, rx) = mpsc::channel();
    // The reading thread may outlive the trial; it is abandoned when the
    // program exits, which is fine for a one-shot command.
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);
        let _ = tx.send(line);
    });
    let deadline = Instant::now() + timeout;
    loop {
        let problems = monitor.check();
        if !problems.is_empty() {
            return Answer::Unhealthy(problems);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Answer::No;
        }
        match rx.recv_timeout(left.min(Duration::from_secs(1))) {
            Ok(l) => {
                let l = l.trim().to_ascii_lowercase();
                return if l == "y" || l == "yes" {
                    Answer::Yes
                } else {
                    Answer::No
                };
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Answer::No,
        }
    }
}

pub fn cmd_restore(root: &SysRoot, state: &Path) -> io::Result<()> {
    let mut engine = open(root, state)?;
    let done = engine.restore().map_err(err)?;
    if done.is_empty() {
        println!("Everything is already at its original setting.");
    }
    let mut failed = false;
    for (id, original, ok) in done {
        if ok {
            println!("Restored {id} to {original}");
        } else {
            failed = true;
            println!("Could NOT restore {id} to {original}; see `dynamis journal`");
        }
    }
    if failed {
        return Err(io::Error::other("some settings could not be restored"));
    }
    Ok(())
}

pub fn cmd_journal(root: &SysRoot, state: &Path, last: usize, json: bool) -> io::Result<()> {
    let engine = open(root, state)?;
    let mut out = io::stdout().lock();
    for e in engine.journal(last) {
        if json {
            serde_json::to_writer(&mut out, &e)?;
            writeln!(out)?;
            continue;
        }
        let change = match (&e.from, &e.to) {
            (Some(f), Some(t)) => format!("{f} → {t}"),
            (None, Some(t)) => format!("→ {t}"),
            _ => String::new(),
        };
        writeln!(
            out,
            "{}  {:<8} {:<22} {:<28} {}",
            e.time, e.event, e.id, change, e.reason
        )?;
    }
    Ok(())
}

pub fn cmd_unblock(root: &SysRoot, state: &Path, id: &str) -> io::Result<()> {
    let mut engine = open(root, state)?;
    if engine.unblock(id).map_err(err)? {
        println!("{id} may be changed again.");
    } else {
        println!("{id} was not blocked.");
    }
    Ok(())
}

/// `switch`: None = show status, Some(true) = on, Some(false) = off.
pub fn cmd_safe_mode(root: &SysRoot, state: &Path, switch: Option<bool>) -> io::Result<()> {
    let mut engine = open(root, state)?;
    match switch {
        Some(true) => engine
            .set_safe_mode(Some("switched on by hand"))
            .map_err(err)?,
        Some(false) => engine.set_safe_mode(None).map_err(err)?,
        None => {}
    }
    match engine.safe_mode() {
        Some(why) => {
            println!("Safe mode is ON: {why}.");
            println!("No setting will be changed. `dynamis restore` still works.");
            if switch == Some(false) {
                println!("It is still on because of the disable file or the boot option; remove that to switch it off.");
            }
        }
        None => println!("Safe mode is off."),
    }
    Ok(())
}
