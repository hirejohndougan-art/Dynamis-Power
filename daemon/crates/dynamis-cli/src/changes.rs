//! Commands that list, change and restore settings. All changes go through
//! `dynamis_core::engine::Engine`, which enforces the safety rules.

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use dynamis_core::engine::{Engine, EngineError, Outcome, Request};
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
    let blocked = Engine::open(root.clone(), state)
        .map(|e| e.blocked())
        .unwrap_or_default();
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

    print!("Keep this change? Type y and Enter within {secs} seconds, or it is undone: ");
    io::stdout().flush()?;
    if ask_yes(Duration::from_secs(secs)) {
        engine.keep().map_err(err)?;
        println!("Kept.");
    } else {
        println!();
        engine
            .revert("not confirmed (no, no answer, or timed out)")
            .map_err(err)?;
        println!("Undone: {} is back to {from}.", a.id);
    }
    Ok(())
}

/// Wait for a "y" on standard input. Anything else, end of input, or the
/// timeout counts as no — the safe answer.
fn ask_yes(timeout: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    // The reading thread may outlive the timeout; it is abandoned when the
    // program exits, which is fine for a one-shot command.
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);
        let _ = tx.send(line);
    });
    matches!(
        rx.recv_timeout(timeout).map(|l| l.trim().to_ascii_lowercase()),
        Ok(ref l) if l == "y" || l == "yes"
    )
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
