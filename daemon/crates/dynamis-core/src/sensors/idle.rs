//! CPU idle states (cpuidle): how long the processors spent in each sleep
//! state since boot. Deeper states save more power; a laptop that rarely
//! reaches them is usually being woken by something.
//!
//! Each state's `time` is cumulative microseconds per CPU. A single reading
//! is totals; the share of time in each state comes from two readings.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::fs::SysRoot;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct State {
    /// "POLL", "C1", "C6", … as the driver names it.
    pub name: String,
    /// Index of the state: higher is deeper.
    pub index: usize,
    /// Microseconds, summed over every CPU.
    pub time_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Idle {
    pub cpus: usize,
    pub states: Vec<State>,
}

pub fn read(root: &SysRoot) -> Option<Idle> {
    let mut by_index: BTreeMap<usize, State> = BTreeMap::new();
    let mut cpus = 0;
    for cpu in root.list_dir("/sys/devices/system/cpu") {
        if !cpu.starts_with("cpu")
            || !cpu[3..].chars().all(|c| c.is_ascii_digit())
            || cpu.len() == 3
        {
            continue;
        }
        let base = format!("/sys/devices/system/cpu/{cpu}/cpuidle");
        let states = root.list_dir(&base);
        if states.is_empty() {
            continue;
        }
        cpus += 1;
        for st in states {
            let Some(index) = st.strip_prefix("state").and_then(|n| n.parse().ok()) else {
                continue;
            };
            let time = root.read_u64(&format!("{base}/{st}/time")).unwrap_or(0);
            let name = root
                .read_string(&format!("{base}/{st}/name"))
                .unwrap_or_else(|_| st.clone());
            by_index
                .entry(index)
                .and_modify(|s| s.time_us += time)
                .or_insert(State {
                    name,
                    index,
                    time_us: time,
                });
        }
    }
    (cpus > 0).then(|| Idle {
        cpus,
        states: by_index.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_states_over_cpus() {
        let dir = std::env::temp_dir().join(format!("dynamis-idle-{}", std::process::id()));
        for cpu in ["cpu0", "cpu1"] {
            for (i, name, t) in [(0, "POLL", 10), (1, "C1", 100), (2, "C6", 1000)] {
                let d = dir.join(format!("sys/devices/system/cpu/{cpu}/cpuidle/state{i}"));
                std::fs::create_dir_all(&d).unwrap();
                std::fs::write(d.join("name"), name).unwrap();
                std::fs::write(d.join("time"), t.to_string()).unwrap();
            }
        }
        std::fs::create_dir_all(dir.join("sys/devices/system/cpu/cpufreq")).unwrap();
        let idle = read(&SysRoot::at(&dir)).unwrap();
        assert_eq!(idle.cpus, 2);
        assert_eq!(idle.states[2].name, "C6");
        assert_eq!(idle.states[2].time_us, 2000);
        std::fs::remove_dir_all(&dir).ok();
    }
}
