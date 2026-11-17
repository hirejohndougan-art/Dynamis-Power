//! `dynamis bench`: the head-to-head benchmark (see `dynamis_core::bench`
//! and docs/TESTING-ON-HARDWARE.md). Runs the workloads on battery and saves
//! the results; `compare` prints them side by side. Changes no setting.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use dynamis_core::bench::{self, Run, Sample, Workload, SETTLE_S};
use dynamis_core::{sensors, SysRoot};

/// Heavy work: this many fixed units, shared by every core. About two to
/// four minutes on a two-core Celeron, well under a minute on a fast laptop.
const HEAVY_UNITS: usize = 600;
/// One unit of heavy work (iterations of a small integer loop).
const UNIT_ITERS: u64 = 40_000_000;
const SAMPLE_EVERY_S: f64 = 10.0;

pub struct RunArgs<'a> {
    pub label: &'a str,
    pub only: &'a [Workload],
    pub minutes: f64,
    pub video: Option<&'a Path>,
    pub output: Option<PathBuf>,
    /// Testing only: seconds per workload and sampling interval.
    pub test_seconds: Option<f64>,
}

/// A small steady load (10 ms of work every 100 ms, about 10% of one core):
/// reading and light browsing.
struct Light(Arc<AtomicBool>);

impl Light {
    fn start() -> Light {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let mut x: u64 = 1;
            while !flag.load(Ordering::Relaxed) {
                let t = Instant::now();
                while t.elapsed() < Duration::from_millis(10) {
                    x = x
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    std::hint::black_box(x);
                }
                sleep(Duration::from_millis(90));
            }
        });
        Light(stop)
    }
}

impl Drop for Light {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// The fixed heavy work on every core; returns when it is all done.
/// `Arc<AtomicUsize>` is a counter shared by the threads, so each unit is
/// done exactly once whichever core is free.
fn heavy(units: usize, iters: u64) -> f64 {
    let start = Instant::now();
    let next = Arc::new(AtomicUsize::new(0));
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let next = next.clone();
            std::thread::spawn(move || {
                while next.fetch_add(1, Ordering::Relaxed) < units {
                    let mut x: u64 = 7;
                    for _ in 0..iters {
                        x = x
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        x ^= x >> 29;
                    }
                    std::hint::black_box(x);
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
    start.elapsed().as_secs_f64()
}

/// A video player looping the file, if one is installed.
fn player(file: &Path) -> Option<Child> {
    let tries: [(&str, &[&str]); 2] = [
        (
            "mpv",
            &[
                "--no-terminal",
                "--loop-file=inf",
                "--fullscreen",
                "--really-quiet",
            ],
        ),
        ("ffplay", &["-loglevel", "quiet", "-loop", "0", "-fs"]),
    ];
    tries.iter().find_map(|(cmd, args)| {
        Command::new(cmd)
            .args(*args)
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    })
}

fn measure(
    root: &SysRoot,
    seconds: f64,
    every: f64,
    mut done: impl FnMut() -> bool,
) -> Vec<Sample> {
    let start = Instant::now();
    let mut samples = vec![bench::sample(root, 0.0)];
    loop {
        let t = start.elapsed().as_secs_f64();
        if t >= seconds || done() {
            if samples.last().is_some_and(|s| s.t < t) {
                samples.push(bench::sample(root, t));
            }
            return samples;
        }
        let next = (samples.len() as f64 * every).min(seconds);
        sleep(Duration::from_secs_f64((next - t).clamp(0.05, every)));
        let t = start.elapsed().as_secs_f64();
        samples.push(bench::sample(root, t));
        if samples.last().is_some_and(|s| s.on_ac) {
            eprintln!("  The charger was plugged in: this workload is stopped and marked.");
            return samples;
        }
    }
}

fn save(path: &Path, run: &Run) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(run)?)?;
    fs::rename(tmp, path)
}

pub fn cmd_run(root: &SysRoot, a: RunArgs) -> io::Result<()> {
    let ps = sensors::power::read(root);
    if ps.batteries.is_empty() {
        return Err(io::Error::other(
            "no battery found: the benchmark measures the battery",
        ));
    }
    if ps.on_ac() {
        return Err(io::Error::other(
            "unplug the charger first: the benchmark measures power from the battery",
        ));
    }
    let workloads: Vec<Workload> = if a.only.is_empty() {
        Workload::ALL.to_vec()
    } else {
        a.only.to_vec()
    };
    if workloads.contains(&Workload::Video) && a.video.is_none() {
        return Err(io::Error::other(
            "the video workload needs a video file: add --video FILE, or leave video out with --only idle,light,heavy",
        ));
    }
    let day = dynamis_core::apps::day_of(sensors::now_secs());
    let output = a
        .output
        .unwrap_or_else(|| PathBuf::from(format!("bench-{}-{day}.json", a.label)));
    let (seconds, every, settle, units, iters) = match a.test_seconds {
        Some(s) => (s, s / 5.0, 0.0, 4, 1000),
        None => (
            a.minutes.clamp(5.0, 120.0) * 60.0 + SETTLE_S,
            SAMPLE_EVERY_S,
            SETTLE_S,
            HEAVY_UNITS,
            UNIT_ITERS,
        ),
    };
    let mut run = Run {
        v: bench::VERSION,
        label: a.label.to_string(),
        day,
        machine: bench::machine(root),
        results: Vec::new(),
    };
    let full = bench::full_wh(root);
    eprintln!(
        "Benchmark \"{}\": {} — keep the laptop on battery, the brightness where it is and the screen on. Results go to {}.",
        a.label,
        workloads.iter().map(|w| w.name()).collect::<Vec<_>>().join(", "),
        output.display()
    );
    for w in workloads {
        eprintln!("- {} …", w.name());
        let (samples, work_seconds) = match w {
            Workload::Idle => (measure(root, seconds, every, || false), None),
            Workload::Light => {
                let _load = Light::start();
                (measure(root, seconds, every, || false), None)
            }
            Workload::Video => {
                let Some(mut child) = a.video.and_then(player) else {
                    eprintln!("  No video player found (mpv or ffplay): video left out.");
                    continue;
                };
                let s = measure(root, seconds, every, || false);
                let _ = child.kill();
                let _ = child.wait();
                (s, None)
            }
            Workload::Heavy => {
                // Settle first, then time the fixed work while sampling.
                let calm = measure(root, settle, every, || false);
                let finished = Arc::new(AtomicBool::new(false));
                let took = Arc::new(std::sync::Mutex::new(None));
                let (f, t) = (finished.clone(), took.clone());
                std::thread::spawn(move || {
                    let s = heavy(units, iters);
                    if let Ok(mut g) = t.lock() {
                        *g = Some(s);
                    }
                    f.store(true, Ordering::Relaxed);
                });
                let mut s = measure(root, 3600.0, every.min(5.0), || {
                    finished.load(Ordering::Relaxed)
                });
                let offset = calm.last().map_or(0.0, |x| x.t);
                for x in &mut s {
                    x.t += offset;
                }
                let mut all = calm;
                all.extend(s);
                (all, took.lock().ok().and_then(|g| *g))
            }
        };
        let r = bench::summarise(w, samples, full, work_seconds, settle);
        match (r.avg_w, r.hours_on_full) {
            (Some(p), Some(h)) => eprintln!("  {p:.2} W, about {h:.1} h on a full battery"),
            (Some(p), None) => eprintln!("  {p:.2} W"),
            _ => {}
        }
        if let Some(s) = r.work_seconds {
            eprintln!("  finished the work in {s:.0} s");
        }
        for p in &r.problems {
            eprintln!("  Problem: {p}");
        }
        run.results.push(r);
        save(&output, &run)?;
    }
    eprintln!(
        "Saved {}. Compare runs with: dynamis bench compare FILE…",
        output.display()
    );
    Ok(())
}

pub fn cmd_compare(files: &[PathBuf]) -> io::Result<()> {
    let mut runs = Vec::new();
    for f in files {
        let text = fs::read_to_string(f)?;
        let run: Run = serde_json::from_str(&text)
            .map_err(|e| io::Error::other(format!("{}: {e}", f.display())))?;
        if run.v != bench::VERSION {
            return Err(io::Error::other(format!(
                "{}: results format {} is not known to this version",
                f.display(),
                run.v
            )));
        }
        runs.push(run);
    }
    io::stdout()
        .lock()
        .write_all(bench::compare(&runs).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_every_workload_but_video_on_a_fixture_battery() {
        let dir = std::env::temp_dir().join(format!("dynamis-bench-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bat = dir.join("root/sys/class/power_supply/BAT0");
        fs::create_dir_all(&bat).unwrap();
        for (f, v) in [
            ("type", "Battery"),
            ("status", "Discharging"),
            ("capacity", "80"),
            ("power_now", "5000000"),
            ("energy_now", "30000000"),
            ("energy_full", "40000000"),
        ] {
            fs::write(bat.join(f), format!("{v}\n")).unwrap();
        }
        let root = SysRoot::at(dir.join("root"));
        let out = dir.join("r.json");
        cmd_run(
            &root,
            RunArgs {
                label: "stock",
                only: &[Workload::Idle, Workload::Light, Workload::Heavy],
                minutes: 5.0,
                video: None,
                output: Some(out.clone()),
                test_seconds: Some(0.5),
            },
        )
        .unwrap();
        let run: Run = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(run.results.len(), 3);
        assert_eq!(run.results[0].avg_w, Some(5.0));
        assert_eq!(run.results[0].hours_on_full, Some(8.0));
        assert!(run.results[2].work_seconds.is_some());
        // Needs a file for video.
        assert!(cmd_run(
            &root,
            RunArgs {
                label: "x",
                only: &[Workload::Video],
                minutes: 5.0,
                video: None,
                output: None,
                test_seconds: Some(0.5),
            }
        )
        .is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
