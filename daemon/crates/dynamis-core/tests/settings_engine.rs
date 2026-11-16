//! Setting plugins and the engine, tested on a **temporary copy** of the
//! synthetic-laptop fixture. Nothing here writes to the checked-in fixture or
//! to the real `/sys` and `/proc`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use dynamis_core::engine::{Engine, EngineError, Outcome, Request};
use dynamis_core::settings::{self, Allowed, Probe};
use dynamis_core::SysRoot;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A fresh copy of the fixture plus an empty state directory, removed on drop.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("dynamis-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sysfs/synthetic-laptop");
        copy_tree(&src, &dir.join("root"));
        fs::create_dir_all(dir.join("state")).unwrap();
        Sandbox { dir }
    }
    fn root(&self) -> SysRoot {
        SysRoot::at(self.dir.join("root"))
    }
    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }
    fn engine(&self) -> Engine {
        Engine::open(self.root(), self.state()).unwrap()
    }
    fn file(&self, kernel_path: &str) -> String {
        self.root().read_string(kernel_path).unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let ty = entry.file_type().unwrap();
        if ty.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&from).unwrap(), &to).unwrap();
        } else if ty.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

fn req<'a>(id: &'a str, value: &'a str) -> Request<'a> {
    Request {
        id,
        value,
        allow_risky: false,
        trial_secs: None,
        reason: "test",
    }
}

#[test]
fn probes_what_the_machine_offers() {
    let sb = Sandbox::new();
    let root = sb.root();
    let probe = |id| settings::by_id(id).unwrap().probe(&root);

    match probe("cpu.epp") {
        Probe::Supported {
            allowed: Allowed::Choices { values },
        } => {
            assert!(values.contains(&"balance_power".to_string()));
            assert!(
                !values.contains(&"default".to_string()),
                "'default' cannot be read back"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        probe("cpu.turbo"),
        Probe::Supported {
            allowed: Allowed::OnOff
        }
    );
    match probe("cpu.max_perf_pct") {
        Probe::Supported {
            allowed: Allowed::Range { min, max, .. },
        } => assert_eq!((min, max), (30, 100)),
        other => panic!("{other:?}"),
    }
    match probe("pcie.aspm") {
        Probe::Supported {
            allowed: Allowed::Choices { values },
        } => {
            assert!(!values.contains(&"powersupersave".to_string()))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        probe("platform.profile"),
        Probe::Unsupported { .. }
    ));
    assert!(matches!(
        probe("battery.charge_limit"),
        Probe::Unsupported { .. }
    ));

    // Reads use the same words people type.
    let read = |id| settings::by_id(id).unwrap().read(&root).unwrap();
    assert_eq!(read("cpu.turbo"), "on");
    assert_eq!(read("kernel.nmi_watchdog"), "on");
    assert_eq!(read("pcie.aspm"), "default");
    assert_eq!(read("cpu.epp"), "balance_performance");
}

#[test]
fn every_setting_has_a_unique_documented_id() {
    let docs =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/SETTINGS.md"))
            .unwrap();
    let mut seen = std::collections::HashSet::new();
    for s in settings::all() {
        assert!(seen.insert(s.id()), "duplicate id {}", s.id());
        assert!(
            docs.contains(&format!("`{}`", s.id())),
            "{} is not in docs/SETTINGS.md",
            s.id()
        );
    }
}

#[test]
fn apply_journals_records_baseline_and_restore_puts_it_back() {
    let sb = Sandbox::new();
    let mut e = sb.engine();

    let out = e.apply(&req("vm.swappiness", "10")).unwrap();
    assert_eq!(
        out,
        Outcome::Changed {
            from: "60".into(),
            to: "10".into()
        }
    );
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "10");

    // A second change keeps the *original* baseline.
    e.apply(&req("vm.swappiness", "20")).unwrap();
    assert_eq!(
        e.baseline().get("vm.swappiness").map(String::as_str),
        Some("60")
    );

    let events: Vec<String> = e.journal(10).into_iter().map(|j| j.event).collect();
    assert_eq!(events, ["start", "done", "start", "done"]);
    assert!(e.pending().is_none());

    assert_eq!(
        e.apply(&req("vm.swappiness", "20")).unwrap(),
        Outcome::AlreadySet("20".into())
    );

    let restored = e.restore().unwrap();
    assert_eq!(
        restored,
        vec![("vm.swappiness".to_string(), "60".to_string(), true)]
    );
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "60");
}

#[test]
fn values_outside_the_allowed_range_are_refused_untouched() {
    let sb = Sandbox::new();
    let mut e = sb.engine();
    for (id, v) in [
        ("vm.swappiness", "500"),
        ("vm.swappiness", "abc"),
        ("cpu.max_perf_pct", "10"),
        ("cpu.epp", "turbo"),
        ("pcie.aspm", "powersupersave"),
    ] {
        assert!(
            matches!(e.apply(&req(id, v)), Err(EngineError::Invalid(_))),
            "{id}={v}"
        );
    }
    assert!(matches!(
        e.apply(&req("battery.charge_limit", "80")),
        Err(EngineError::Unsupported(_))
    ));
    assert!(matches!(
        e.apply(&req("no.such", "1")),
        Err(EngineError::UnknownSetting(_))
    ));
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "60");
    assert!(
        e.journal(10).is_empty(),
        "refusals happen before anything is journaled"
    );
}

#[test]
fn multi_file_and_inverted_settings_write_correctly() {
    let sb = Sandbox::new();
    let mut e = sb.engine();

    e.apply(&req("cpu.turbo", "off")).unwrap();
    assert_eq!(
        sb.file("/sys/devices/system/cpu/intel_pstate/no_turbo"),
        "1"
    );

    e.apply(&req("cpu.epp", "balance_power")).unwrap();
    for p in ["policy0", "policy1"] {
        assert_eq!(
            sb.file(&format!(
                "/sys/devices/system/cpu/cpufreq/{p}/energy_performance_preference"
            )),
            "balance_power"
        );
    }

    e.apply(&req("kernel.nmi_watchdog", "off")).unwrap();
    assert_eq!(sb.file("/proc/sys/kernel/nmi_watchdog"), "0");

    e.restore().unwrap();
    assert_eq!(
        sb.file("/sys/devices/system/cpu/intel_pstate/no_turbo"),
        "0"
    );
    assert_eq!(
        sb.file("/sys/devices/system/cpu/cpufreq/policy1/energy_performance_preference"),
        "balance_performance"
    );
    assert_eq!(sb.file("/proc/sys/kernel/nmi_watchdog"), "1");
}

#[test]
fn epp_is_refused_while_governor_is_performance() {
    let sb = Sandbox::new();
    for p in ["policy0", "policy1"] {
        fs::write(
            sb.dir.join(format!(
                "root/sys/devices/system/cpu/cpufreq/{p}/scaling_governor"
            )),
            "performance",
        )
        .unwrap();
    }
    let mut e = sb.engine();
    let err = e.apply(&req("cpu.epp", "power")).unwrap_err();
    assert!(err.to_string().contains("governor"), "{err}");
}

#[test]
fn trial_keep_and_revert() {
    let sb = Sandbox::new();
    let mut e = sb.engine();
    let trial = Request {
        trial_secs: Some(60),
        ..req("vm.dirty_writeback", "1500")
    };

    e.apply(&trial).unwrap();
    assert_eq!(e.pending().unwrap().to, "1500");
    assert!(
        e.apply(&req("vm.swappiness", "10")).is_err(),
        "one change at a time"
    );
    e.keep().unwrap();
    assert!(e.pending().is_none());
    assert_eq!(sb.file("/proc/sys/vm/dirty_writeback_centisecs"), "1500");

    e.apply(&Request {
        trial_secs: Some(60),
        ..req("vm.dirty_writeback", "3000")
    })
    .unwrap();
    e.revert("user said no").unwrap();
    assert_eq!(sb.file("/proc/sys/vm/dirty_writeback_centisecs"), "1500");
    assert!(matches!(e.keep(), Err(EngineError::NoTrial)));
}

#[test]
fn unfinished_trial_is_undone_and_blocked_on_next_start() {
    let sb = Sandbox::new();
    {
        let mut e = sb.engine();
        e.apply(&Request {
            trial_secs: Some(30),
            ..req("vm.swappiness", "5")
        })
        .unwrap();
        // Simulate a freeze: the process ends without keep or revert.
    }
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "5");

    let mut e = sb.engine();
    let r = e.recovered().expect("recovery should run").clone();
    assert_eq!(
        (r.id.as_str(), r.restored_to.as_str(), r.ok),
        ("vm.swappiness", "60", true)
    );
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "60");
    assert!(e.blocked().contains_key("vm.swappiness"));
    assert!(matches!(
        e.apply(&req("vm.swappiness", "5")),
        Err(EngineError::Blocked { .. })
    ));

    let events: Vec<String> = e.journal(10).into_iter().map(|j| j.event).collect();
    assert_eq!(events, ["start", "recover", "block"]);

    assert!(e.unblock("vm.swappiness").unwrap());
    e.apply(&req("vm.swappiness", "5")).unwrap();
}

#[test]
fn safe_mode_refuses_changes_but_restore_works() {
    let sb = Sandbox::new();
    let mut e = sb.engine();
    e.apply(&req("vm.swappiness", "10")).unwrap();

    fs::create_dir_all(sb.dir.join("root/etc/dynamis-power")).unwrap();
    fs::write(sb.dir.join("root/etc/dynamis-power/disable"), "").unwrap();
    assert!(e.safe_mode().is_some());
    assert!(matches!(
        e.apply(&req("vm.swappiness", "20")),
        Err(EngineError::SafeMode(_))
    ));
    e.restore().unwrap();
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "60");

    fs::remove_file(sb.dir.join("root/etc/dynamis-power/disable")).unwrap();
    fs::write(
        sb.dir.join("root/proc/cmdline"),
        "quiet splash dynamis.safe=1",
    )
    .unwrap();
    assert!(e.safe_mode().unwrap().contains("command line"));
}

#[test]
fn only_one_engine_at_a_time() {
    let sb = Sandbox::new();
    let first = sb.engine();
    assert!(matches!(
        Engine::open(sb.root(), sb.state()),
        Err(EngineError::Locked(_))
    ));
    drop(first);
    sb.engine();

    // A lock left by a process that no longer exists is taken over.
    fs::write(sb.state().join("lock"), "999999999").unwrap();
    sb.engine();
}

#[test]
fn a_torn_journal_line_is_skipped() {
    let sb = Sandbox::new();
    let mut e = sb.engine();
    e.apply(&req("vm.swappiness", "10")).unwrap();
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(sb.state().join("journal.jsonl"))
        .unwrap();
    std::io::Write::write_all(&mut f, b"{\"v\":1,\"time\":").unwrap();
    assert_eq!(e.journal(100).len(), 2);
}

mod profiles {
    use super::*;
    use dynamis_core::profiles::{self, Profile};

    #[test]
    fn plans_from_what_the_machine_offers() {
        let sb = Sandbox::new();
        let root = sb.root();
        // The fixture has CPU speed preferences but no firmware profile.
        assert_eq!(
            profiles::plan(&root, Profile::PowerSaver)
                .iter()
                .map(|s| (s.id, s.value.as_str()))
                .collect::<Vec<_>>(),
            [("cpu.epp", "power")]
        );
        assert_eq!(profiles::available(&root), Profile::ALL);
        let (cpu, platform) = profiles::drivers(&root);
        assert!(cpu.is_some());
        assert!(platform.is_none());
    }

    #[test]
    fn firmware_profile_is_used_when_present() {
        let sb = Sandbox::new();
        let acpi = sb.dir.join("root/sys/firmware/acpi");
        fs::create_dir_all(&acpi).unwrap();
        fs::write(
            acpi.join("platform_profile_choices"),
            "quiet balanced performance",
        )
        .unwrap();
        fs::write(acpi.join("platform_profile"), "balanced").unwrap();
        let steps = profiles::plan(&sb.root(), Profile::PowerSaver);
        assert!(steps
            .iter()
            .any(|s| s.id == "platform.profile" && s.value == "quiet"));
    }

    #[test]
    fn nothing_to_tune_means_balanced_only() {
        let sb = Sandbox::new();
        fs::remove_dir_all(sb.dir.join("root/sys/devices/system/cpu/cpufreq")).unwrap();
        assert_eq!(profiles::available(&sb.root()), [Profile::Balanced]);
    }

    #[test]
    fn apply_goes_through_the_engine_and_restores() {
        let sb = Sandbox::new();
        let mut e = sb.engine();
        let root = sb.root();
        let done = profiles::apply(&mut e, &root, Profile::Performance, "test");
        assert!(done.iter().all(|a| a.result.is_ok()), "{done:?}");
        assert_eq!(
            sb.file("/sys/devices/system/cpu/cpufreq/policy1/energy_performance_preference"),
            "performance"
        );
        assert!(e
            .journal(5)
            .iter()
            .any(|j| j.event == "start" && j.reason == "test"));
        e.restore().unwrap();
        assert_eq!(
            sb.file("/sys/devices/system/cpu/cpufreq/policy1/energy_performance_preference"),
            "balance_performance"
        );
        assert_eq!(Profile::parse("power-saver"), Some(Profile::PowerSaver));
        assert_eq!(Profile::parse("turbo"), None);
    }
}
