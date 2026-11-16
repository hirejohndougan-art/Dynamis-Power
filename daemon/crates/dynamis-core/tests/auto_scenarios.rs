//! The phase 3 gate: every scenario in tests/scenarios/*.json must choose
//! the expected profile at every step. See tests/scenarios/README.md.

use std::path::Path;

use dynamis_core::auto::{Auto, Context, Prefs, Workload};
use dynamis_core::profiles::Profile;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    prefs: Map<String, Value>,
    available: Option<Vec<Profile>>,
    steps: Vec<StepSpec>,
    max_switches: Option<usize>,
}

#[derive(Deserialize)]
struct StepSpec {
    t: f64,
    #[serde(default)]
    set: Map<String, Value>,
    expect: Option<Profile>,
    workload: Option<Workload>,
    repeat: Option<Repeat>,
}

#[derive(Deserialize)]
struct Repeat {
    every: f64,
    times: usize,
    toggle: String,
}

fn merge(base: &mut Map<String, Value>, set: &Map<String, Value>) {
    for (k, v) in set {
        base.insert(k.clone(), v.clone());
    }
}

fn run(path: &Path) -> Result<(), String> {
    let s: Scenario = serde_json::from_str(&std::fs::read_to_string(path).unwrap())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut prefs = serde_json::to_value(Prefs::default()).unwrap();
    merge(prefs.as_object_mut().unwrap(), &s.prefs);
    let prefs: Prefs =
        serde_json::from_value(prefs).map_err(|e| format!("{}: prefs: {e}", s.name))?;
    let available = s.available.clone().unwrap_or_else(|| Profile::ALL.to_vec());
    let mut auto = Auto::new(prefs);
    let mut ctx = Map::new();
    let mut switches = 0;

    for (i, step) in s.steps.iter().enumerate() {
        let (every, times, toggle) = match &step.repeat {
            Some(r) => (r.every, r.times, Some(r.toggle.as_str())),
            None => (0.0, 1, None),
        };
        merge(&mut ctx, &step.set);
        for n in 0..times {
            if n > 0 {
                if let Some(key) = toggle {
                    let v = ctx.get(key).and_then(Value::as_bool).unwrap_or(false);
                    ctx.insert(key.into(), Value::Bool(!v));
                }
            }
            let c: Context = serde_json::from_value(Value::Object(ctx.clone()))
                .map_err(|e| format!("{}: step {i}: {e}", s.name))?;
            let r = auto.step(step.t + every * n as f64, &c, &available);
            if r.switch_to.is_some() {
                switches += 1;
            }
            if times > 1 {
                continue;
            }
            let active = auto.current();
            if let Some(want) = step.expect {
                if active != Some(want) {
                    return Err(format!(
                        "{}: step {i} (t={}): expected {} but got {:?} — {} [{}]",
                        s.name,
                        step.t,
                        want.as_str(),
                        active.map(|p| p.as_str()),
                        r.decision.reason,
                        r.waiting.unwrap_or_default()
                    ));
                }
            }
            if let Some(w) = step.workload {
                if r.decision.workload != w {
                    return Err(format!(
                        "{}: step {i} (t={}): expected workload {w:?} but detected {:?}",
                        s.name, step.t, r.decision.workload
                    ));
                }
            }
        }
    }
    if let Some(max) = s.max_switches {
        if switches > max {
            return Err(format!(
                "{}: {switches} switches, more than the {max} allowed",
                s.name
            ));
        }
    }
    Ok(())
}

#[test]
fn every_scenario_chooses_the_expected_profile() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/scenarios");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 8, "scenarios missing from {}", dir.display());
    let failures: Vec<String> = files.iter().filter_map(|f| run(f).err()).collect();
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

#[test]
fn rate_limit_actually_holds_back() {
    // Without the limit, the flapping scenario would switch on every toggle.
    let mut auto = Auto::new(Prefs {
        on_battery: Profile::PowerSaver,
        ..Prefs::default()
    });
    let mut ctx = Context {
        battery_pct: Some(90.0),
        ..Context::default()
    };
    let mut switches = 0;
    let mut held = 0;
    // 51 readings, 70 s apart: just under an hour, so the rolling limit applies to all.
    for i in 0..51 {
        ctx.media_playing = i % 2 == 1;
        let r = auto.step(i as f64 * 70.0, &ctx, &Profile::ALL);
        switches += r.switch_to.is_some() as usize;
        held += r.waiting.is_some() as usize;
    }
    assert!(switches <= 13, "{switches}");
    assert!(held > 0);
}
