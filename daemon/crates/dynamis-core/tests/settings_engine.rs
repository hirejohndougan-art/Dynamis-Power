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

/// Make the pending change look as if its process has died.
fn mark_owner_gone(sb: &Sandbox) {
    let path = sb.state().join("pending.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    v["owner"]["pid"] = serde_json::json!(999_999_999u32);
    fs::write(&path, v.to_string()).unwrap();
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
    assert!(e.blocked().is_empty(), "a plain no does not block");
    assert_eq!(sb.file("/proc/sys/vm/dirty_writeback_centisecs"), "1500");
    assert!(matches!(e.keep(), Err(EngineError::NoTrial)));

    e.apply(&Request {
        trial_secs: Some(60),
        ..req("vm.dirty_writeback", "3000")
    })
    .unwrap();
    e.revert_unhealthy("health check: GPU hang").unwrap();
    assert_eq!(sb.file("/proc/sys/vm/dirty_writeback_centisecs"), "1500");
    assert!(e.blocked().contains_key("vm.dirty_writeback"));
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
        // Simulate a freeze: the process ends without keep or revert. In
        // this test the process lives on, so mark the owner as gone.
    }
    mark_owner_gone(&sb);
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
fn a_live_owners_trial_is_left_alone() {
    let sb = Sandbox::new();
    {
        let mut e = sb.engine();
        e.apply(&Request {
            trial_secs: Some(30),
            ..req("vm.swappiness", "5")
        })
        .unwrap();
    }
    // Reopened by the same, still-running process (as the service does for
    // every step of an experiment): nothing is undone or blocked.
    let mut e = sb.engine();
    assert!(e.recovered().is_none());
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "5");
    assert!(e.blocked().is_empty());
    e.keep().unwrap();
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
        // Turbo and the speed limit still give Longer battery something to
        // do; Faster would be the same as Balanced (no governor choice).
        assert_eq!(
            profiles::available(&sb.root()),
            [Profile::PowerSaver, Profile::Balanced]
        );
        fs::remove_dir_all(sb.dir.join("root/sys/devices/system/cpu/intel_pstate")).unwrap();
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

mod health {
    use super::*;
    use dynamis_core::health::{Limits, Monitor, Problem};

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    #[test]
    fn only_new_kernel_trouble_counts() {
        let sb = Sandbox::new();
        let kmsg = sb.dir.join("root/dev/kmsg");
        fs::create_dir_all(kmsg.parent().unwrap()).unwrap();
        append(&kmsg, "3,1,1,-;old error from boot\n");

        let mut m = Monitor::start(&sb.root(), Limits::default());
        assert!(m.watching_kernel_log());
        assert!(
            m.check().is_empty(),
            "messages from before the trial are ignored"
        );

        append(&kmsg, "6,2,2,-;wlan0: associated\n");
        assert!(m.check().is_empty());

        append(
            &kmsg,
            "4,3,3,-;mmc0: Timeout waiting for hardware interrupt.\n SUBSYSTEM=mmc\n",
        );
        let p = m.check();
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(matches!(&p[0], Problem::Kernel { message } if message.starts_with("mmc0")));
        assert!(m.check().is_empty(), "each message is reported once");
    }

    #[test]
    fn thrashing_and_heat() {
        let sb = Sandbox::new();
        let mut m = Monitor::start(&sb.root(), Limits::default());
        assert!(
            !m.watching_kernel_log(),
            "fixture has no kernel log; other checks still run"
        );
        assert!(m.check().is_empty());

        fs::write(
            sb.dir.join("root/proc/pressure/memory"),
            "some avg10=60.00 avg60=20.00 avg300=5.00 total=1\nfull avg10=40.00 avg60=10.00 avg300=2.00 total=1\n",
        )
        .unwrap();
        let zone = fs::read_dir(sb.dir.join("root/sys/class/thermal"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.join("temp").exists())
            .unwrap();
        fs::write(zone.join("temp"), "99000").unwrap();
        let p = m.check();
        assert!(
            p.iter()
                .any(|x| matches!(x, Problem::MemoryThrashing { .. })),
            "{p:?}"
        );
        assert!(
            p.iter().any(|x| matches!(x, Problem::Overheating { .. })),
            "{p:?}"
        );
    }

    #[test]
    fn already_busy_is_not_a_failure() {
        let sb = Sandbox::new();
        fs::write(
            sb.dir.join("root/proc/pressure/io"),
            "some avg10=80.00 avg60=0 avg300=0 total=1\nfull avg10=50.00 avg60=0 avg300=0 total=1\n",
        )
        .unwrap();
        let mut m = Monitor::start(&sb.root(), Limits::default());
        assert!(m.check().is_empty(), "same stall level as before the trial");
    }
}

mod quirk_list {
    use super::*;
    use dynamis_core::quirks::{self, Downloaded};
    use ed25519_dalek::{Signer, SigningKey};

    fn key(seed: u8) -> (SigningKey, String) {
        let k = SigningKey::from_bytes(&[seed; 32]);
        let public = quirks::to_hex(k.verifying_key().as_bytes());
        (k, public)
    }

    fn publish(dir: &Path, k: &SigningKey, serial: u64, quirks_json: &str) {
        fs::create_dir_all(dir).unwrap();
        let body = format!(r#"{{"v":1,"serial":{serial},"issued":"test","quirks":{quirks_json}}}"#);
        fs::write(dir.join("quirks.json"), &body).unwrap();
        fs::write(
            dir.join("quirks.json.sig"),
            quirks::to_hex(&k.sign(body.as_bytes()).to_bytes()),
        )
        .unwrap();
    }

    const BLOCK_SWAP_10: &str = r#"[{"id":"t1","setting":"vm.swappiness","values":["10"],"match":{"model":"Synthetic*"},"reason":"test block"}]"#;

    #[test]
    fn builtin_quirk_matches_this_wifi() {
        let sb = Sandbox::new();
        let mut e = sb.engine();
        // The fixture has an RTL8821CE card, which the built-in list covers.
        let err = e.apply(&req("pcie.aspm", "powersave")).unwrap_err();
        assert!(matches!(err, EngineError::Quirk { .. }), "{err}");
        // Only the listed value is blocked. (Writing it fails the read-back on
        // this fixture, whose plain file cannot mimic the kernel's
        // "[current] other" format; what matters is that no quirk refused it.)
        let other = e.apply(&req("pcie.aspm", "performance"));
        assert!(
            !matches!(other, Err(EngineError::Quirk { .. })),
            "{other:?}"
        );
    }

    #[test]
    fn signed_list_is_used_and_tampering_is_rejected() {
        let sb = Sandbox::new();
        let (k, public) = key(7);
        let keys = [public.as_str()];
        let sync = sb.state().join("sync");

        publish(&sync, &k, 5, BLOCK_SWAP_10);
        let a = quirks::load_with_keys(&sb.root(), &sync, 0, &keys);
        assert!(matches!(a.downloaded, Downloaded::Used { serial: 5, .. }));
        assert!(a.blocking("vm.swappiness", "10").is_some());
        assert!(a.blocking("vm.swappiness", "20").is_none());

        // One changed byte breaks the signature.
        let body = fs::read_to_string(sync.join("quirks.json"))
            .unwrap()
            .replace("test block", "test blocK");
        fs::write(sync.join("quirks.json"), body).unwrap();
        let a = quirks::load_with_keys(&sb.root(), &sync, 0, &keys);
        assert!(matches!(a.downloaded, Downloaded::Rejected { .. }));
        assert!(a.blocking("vm.swappiness", "10").is_none());

        // Signed by someone else.
        let (other, _) = key(9);
        publish(&sync, &other, 6, BLOCK_SWAP_10);
        assert!(matches!(
            quirks::load_with_keys(&sb.root(), &sync, 0, &keys).downloaded,
            Downloaded::Rejected { .. }
        ));

        // No trusted key at all (the state of a fresh project).
        publish(&sync, &k, 6, BLOCK_SWAP_10);
        assert!(matches!(
            quirks::load_with_keys(&sb.root(), &sync, 0, &[]).downloaded,
            Downloaded::Rejected { .. }
        ));

        // An older list cannot be replayed after a newer one was accepted.
        assert!(matches!(
            quirks::load_with_keys(&sb.root(), &sync, 7, &keys).downloaded,
            Downloaded::Rejected { .. }
        ));
    }

    #[test]
    fn kill_switch_puts_back_an_already_applied_setting() {
        let sb = Sandbox::new();
        let (k, public) = key(7);
        let keys = [public.as_str()];
        {
            let mut e = Engine::open_with_keys(sb.root(), sb.state(), &keys).unwrap();
            e.apply(&req("vm.swappiness", "10")).unwrap();
        }
        // A new list arrives blocking that exact value on this model.
        publish(&sb.state().join("sync"), &k, 2, BLOCK_SWAP_10);
        let e = Engine::open_with_keys(sb.root(), sb.state(), &keys).unwrap();
        assert_eq!(sb.file("/proc/sys/vm/swappiness"), "60");
        let last = e.journal(1).pop().unwrap();
        assert_eq!(last.event, "quirk");
        assert_eq!(
            fs::read_to_string(sb.state().join("quirk-serial")).unwrap(),
            "2"
        );
    }
}

mod unclean_boots {
    use super::*;
    use dynamis_core::boots::{self, UNCLEAN_LIMIT};

    fn boot(sb: &Sandbox, id: &str) -> boots::BootCheck {
        let p = sb.dir.join("root/proc/sys/kernel/random");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("boot_id"), id).unwrap();
        boots::record_start(&sb.root(), &sb.state()).unwrap()
    }

    fn clean_stop(sb: &Sandbox) {
        // What the systemd unit's ExecStopPost does.
        fs::remove_file(sb.state().join("running")).unwrap();
    }

    #[test]
    fn three_unclean_boots_turn_on_safe_mode_until_switched_off() {
        let sb = Sandbox::new();
        assert_eq!(boot(&sb, "a").unclean_in_a_row, 0);
        // Freeze: no clean stop.
        assert_eq!(boot(&sb, "b").unclean_in_a_row, 1);
        // The service restarting within the same boot is not a new boot.
        assert_eq!(boot(&sb, "b").unclean_in_a_row, 1);
        clean_stop(&sb);
        assert_eq!(
            boot(&sb, "c").unclean_in_a_row,
            0,
            "a clean shutdown resets the count"
        );

        for (i, id) in ["d", "e"].into_iter().enumerate() {
            let r = boot(&sb, id);
            assert_eq!(r.unclean_in_a_row, i as u32 + 1);
            assert!(!r.safe_mode_now_on);
        }
        let r = boot(&sb, "f");
        assert_eq!(r.unclean_in_a_row, UNCLEAN_LIMIT);
        assert!(r.safe_mode_now_on);

        let mut e = sb.engine();
        assert!(e.safe_mode().unwrap().contains("unclean"));
        assert!(matches!(
            e.apply(&req("vm.swappiness", "10")),
            Err(EngineError::SafeMode(_))
        ));

        // It stays on after a clean boot...
        clean_stop(&sb);
        drop(e);
        boot(&sb, "g");
        let mut e = sb.engine();
        assert!(e.safe_mode().is_some());
        // ...until a person switches it off.
        e.set_safe_mode(None).unwrap();
        assert!(e.safe_mode().is_none());
        e.apply(&req("vm.swappiness", "10")).unwrap();
    }

    #[test]
    fn journal_rotates_and_still_reads_back() {
        let sb = Sandbox::new();
        let mut e = sb.engine();
        e.apply(&req("vm.swappiness", "10")).unwrap();
        // Make the journal look big, then log more.
        let path = sb.state().join("journal.jsonl");
        let mut big = fs::read_to_string(&path).unwrap();
        while big.len() as u64 <= dynamis_core::engine::JOURNAL_ROTATE_BYTES {
            big.push_str(&"x".repeat(1000));
            big.push('\n');
        }
        fs::write(&path, big).unwrap();
        e.apply(&req("vm.swappiness", "20")).unwrap();
        assert!(sb.state().join("journal.1.jsonl").exists());
        assert!(fs::metadata(&path).unwrap().len() < 1000);
        let events: Vec<String> = e.journal(10).into_iter().map(|j| j.event).collect();
        assert_eq!(events, ["start", "done", "start", "done"]);
    }
}

#[test]
fn undo_one_setting() {
    let sb = Sandbox::new();
    let mut e = sb.engine();
    e.apply(&req("vm.swappiness", "10")).unwrap();
    e.apply(&req("kernel.nmi_watchdog", "off")).unwrap();
    assert_eq!(e.restore_one("vm.swappiness").unwrap(), Some("60".into()));
    assert_eq!(sb.file("/proc/sys/vm/swappiness"), "60");
    assert_eq!(
        sb.file("/proc/sys/kernel/nmi_watchdog"),
        "0",
        "only the one setting"
    );
    assert_eq!(
        e.restore_one("audio.power_save").unwrap(),
        None,
        "never changed"
    );
    let rows = dynamis_core::overview::settings(&sb.root(), &e);
    let nmi = rows.iter().find(|r| r.id == "kernel.nmi_watchdog").unwrap();
    assert_eq!(
        (nmi.current.as_deref(), nmi.original.as_deref()),
        (Some("off"), Some("on"))
    );
    let aspm = rows.iter().find(|r| r.id == "pcie.aspm").unwrap();
    assert!(!aspm.not_allowed.is_empty(), "quirk shown");
}

mod app_weights {
    use super::*;

    fn app_cgroup(sb: &Sandbox, name: &str) -> String {
        let cg = format!("/user.slice/user-1000.slice/user@1000.service/app.slice/{name}");
        let dir = sb.dir.join(format!("root/sys/fs/cgroup{cg}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cpu.weight"), "100").unwrap();
        cg
    }

    #[test]
    fn weights_are_journaled_restorable_and_limited() {
        let sb = Sandbox::new();
        let ff = app_cgroup(&sb, "app-firefox-12.scope");
        let mut e = sb.engine();
        assert!(e
            .set_app_weight("firefox", &ff, 25, "background on battery")
            .unwrap());
        assert!(
            !e.set_app_weight("firefox", &ff, 25, "again").unwrap(),
            "no change, no write"
        );
        assert_eq!(
            fs::read_to_string(sb.dir.join(format!("root/sys/fs/cgroup{ff}/cpu.weight"))).unwrap(),
            "25"
        );
        assert_eq!(e.journal(5).last().unwrap().event, "app-weight");

        // Only user app groups, only sane weights.
        assert!(e
            .set_app_weight("sshd", "/system.slice/sshd.service", 1, "x")
            .is_err());
        assert!(e
            .set_app_weight("x", "/user.slice/../system.slice", 1, "x")
            .is_err());
        assert!(e.set_app_weight("firefox", &ff, 0, "x").is_err());

        e.restore().unwrap();
        assert_eq!(
            fs::read_to_string(sb.dir.join(format!("root/sys/fs/cgroup{ff}/cpu.weight"))).unwrap(),
            "100"
        );
    }

    #[test]
    fn one_app_can_be_put_back() {
        let sb = Sandbox::new();
        let ff = app_cgroup(&sb, "app-firefox-12.scope");
        let mut e = sb.engine();
        assert!(!e.reset_app_weight("firefox", &ff, "never changed").unwrap());
        e.set_app_weight("firefox", &ff, 400, "in front").unwrap();
        assert!(e.reset_app_weight("firefox", &ff, "behind").unwrap());
        assert_eq!(
            fs::read_to_string(sb.dir.join(format!("root/sys/fs/cgroup{ff}/cpu.weight"))).unwrap(),
            "100"
        );
        // Forgotten once put back: a later restore has nothing to do.
        assert!(!e.reset_app_weight("firefox", &ff, "again").unwrap());
        assert_eq!(e.restore_app_weights().unwrap(), 0);
    }

    #[test]
    fn scans_processes_into_apps() {
        let sb = Sandbox::new();
        let ff = app_cgroup(&sb, "app-firefox-12.scope");
        let mk = |pid: u32, comm: &str, ppid: u32, ticks: u64, cg: &str| {
            let d = sb.dir.join(format!("root/proc/{pid}"));
            fs::create_dir_all(&d).unwrap();
            fs::write(
                d.join("stat"),
                format!(
                    "{pid} ({comm}) S {ppid} 1 1 0 -1 0 0 0 0 0 {ticks} 0 0 0 20 0 1 0 100 0 0"
                ),
            )
            .unwrap();
            fs::write(d.join("cgroup"), format!("0::{cg}\n")).unwrap();
        };
        mk(1001, "firefox", 1, 300, &ff);
        mk(1002, "Web Content", 1001, 200, &ff);
        mk(
            1003,
            "NetworkManager",
            1,
            50,
            "/system.slice/NetworkManager.service",
        );
        mk(1004, "kworker/0:1", 2, 999, "/");
        let procs = dynamis_core::apps::scan(&sb.root());
        let ff_ticks: u64 = procs
            .iter()
            .filter(|p| p.app == "firefox")
            .map(|p| p.cpu_ticks)
            .sum();
        assert_eq!(ff_ticks, 500);
        assert!(procs.iter().any(|p| p.app == "System"));
        assert!(
            !procs.iter().any(|p| p.pid == 1004),
            "kernel threads skipped"
        );
    }
}

mod tlp_batch1 {
    //! Settings added to replace TLP (batch 1): each sets, reads back, and
    //! comes back with restore, on a copy of the fixture.
    use super::*;

    fn set_and_restore(sb: &Sandbox, id: &str, value: &str, risky: bool) {
        let mut e = sb.engine();
        let before = settings::by_id(id).unwrap().read(&sb.root()).unwrap();
        let r = e
            .apply(&Request {
                allow_risky: risky,
                ..req(id, value)
            })
            .unwrap_or_else(|err| panic!("{id} = {value}: {err}"));
        assert!(matches!(r, Outcome::Changed { .. }), "{id}: {r:?}");
        assert_eq!(
            settings::by_id(id).unwrap().read(&sb.root()).unwrap(),
            value
        );
        e.restore().unwrap();
        assert_eq!(
            settings::by_id(id).unwrap().read(&sb.root()).unwrap(),
            before,
            "{id} restored"
        );
    }

    #[test]
    fn processor_settings() {
        let sb = Sandbox::new();
        set_and_restore(&sb, "cpu.governor", "performance", false);
        assert_eq!(
            sb.file("/sys/devices/system/cpu/cpufreq/policy1/scaling_governor"),
            "powersave"
        );
        set_and_restore(&sb, "cpu.min_perf_pct", "50", false);
        assert!(sb.engine().apply(&req("cpu.min_perf_pct", "101")).is_err());
        set_and_restore(&sb, "cpu.hwp_dynamic_boost", "on", false);
        // There is a speed limit in percent here, so no top frequency.
        assert!(matches!(
            settings::by_id("cpu.max_freq").unwrap().probe(&sb.root()),
            Probe::Unsupported { .. }
        ));
    }

    #[test]
    fn top_frequency_where_there_is_no_speed_limit() {
        let sb = Sandbox::new();
        let pstate = sb.dir.join("root/sys/devices/system/cpu/intel_pstate");
        fs::remove_dir_all(&pstate).unwrap();
        for p in ["policy0", "policy1"] {
            fs::write(
                sb.dir.join(format!(
                    "root/sys/devices/system/cpu/cpufreq/{p}/scaling_available_frequencies"
                )),
                "2800000 2400000 2000000 1600000 1200000 800000",
            )
            .unwrap();
        }
        let Probe::Supported {
            allowed: Allowed::Choices { values },
        } = settings::by_id("cpu.max_freq").unwrap().probe(&sb.root())
        else {
            panic!("max_freq not offered")
        };
        assert_eq!(
            values,
            ["1200", "1600", "2000", "2400", "2800"],
            "30% floor"
        );
        set_and_restore(&sb, "cpu.max_freq", "1600", false);
        assert_eq!(
            sb.file("/sys/devices/system/cpu/cpufreq/policy0/scaling_max_freq"),
            "2800000"
        );
    }

    #[test]
    fn devices_and_storage() {
        let sb = Sandbox::new();
        set_and_restore(&sb, "audio.controller_power", "off", false);
        set_and_restore(&sb, "storage.io_scheduler", "none", false);
        // zram is never touched.
        let card = sb.dir.join("root/sys/class/drm/card1/device");
        fs::create_dir_all(&card).unwrap();
        fs::write(card.join("power_dpm_force_performance_level"), "auto").unwrap();
        set_and_restore(&sb, "gpu.amd_power_level", "low", false);
        assert!(sb
            .engine()
            .apply(&req("gpu.amd_power_level", "manual"))
            .is_err());
    }

    #[test]
    fn battery_and_sleep() {
        let sb = Sandbox::new();
        let bat = sb.dir.join("root/sys/class/power_supply/BAT0");
        fs::write(bat.join("charge_control_end_threshold"), "80").unwrap();
        fs::write(bat.join("charge_control_start_threshold"), "75").unwrap();
        set_and_restore(&sb, "battery.charge_start", "60", false);
        let err = sb
            .engine()
            .apply(&req("battery.charge_start", "85"))
            .unwrap_err();
        assert!(err.to_string().contains("below the charge limit"), "{err}");

        // Sleep type is risky: refused without an explicit yes.
        assert!(matches!(
            sb.engine().apply(&req("system.mem_sleep", "s2idle")),
            Err(EngineError::NeedsOptIn(_))
        ));
        set_and_restore(&sb, "system.mem_sleep", "s2idle", true);
    }
}

mod modes_without_epp {
    //! Laptops with no speed preference and no firmware power mode (budget
    //! and older processors, such as the VivoBook's Celeron N4020): the
    //! modes use turbo, the speed limit and the governor instead.
    use super::*;
    use dynamis_core::profiles::{self, Profile};

    /// The synthetic laptop with its speed preference taken away.
    fn budget_laptop() -> Sandbox {
        let sb = Sandbox::new();
        for p in ["policy0", "policy1"] {
            for f in [
                "energy_performance_preference",
                "energy_performance_available_preferences",
            ] {
                fs::remove_file(
                    sb.dir
                        .join(format!("root/sys/devices/system/cpu/cpufreq/{p}/{f}")),
                )
                .unwrap();
            }
        }
        let _ = fs::remove_file(sb.dir.join("root/sys/firmware/acpi/platform_profile"));
        let _ = fs::remove_file(
            sb.dir
                .join("root/sys/firmware/acpi/platform_profile_choices"),
        );
        sb
    }

    fn values(steps: &[profiles::Step]) -> Vec<(String, String)> {
        steps
            .iter()
            .map(|s| (s.id.to_string(), s.value.clone()))
            .collect()
    }

    #[test]
    fn all_three_modes_mean_something() {
        let sb = budget_laptop();
        let root = sb.root();
        assert!(profiles::uses_fallback(&root));
        assert_eq!(profiles::available(&root), Profile::ALL);
        assert!(profiles::missing(&root).is_empty());
        let saver = values(&profiles::plan(&root, Profile::PowerSaver));
        assert!(
            saver.contains(&("cpu.turbo".into(), "off".into())),
            "{saver:?}"
        );
        assert!(saver.contains(&("cpu.max_perf_pct".into(), "60".into())));
        assert!(saver.contains(&("cpu.governor".into(), "powersave".into())));
        let fast = values(&profiles::plan(&root, Profile::Performance));
        assert!(fast.contains(&("cpu.governor".into(), "performance".into())));
        assert!(fast.contains(&("cpu.turbo".into(), "on".into())));

        // Switching applies them, and back again.
        let mut e = sb.engine();
        profiles::apply(&mut e, &root, Profile::PowerSaver, "test");
        assert_eq!(
            sb.file("/sys/devices/system/cpu/intel_pstate/no_turbo"),
            "1"
        );
        assert_eq!(
            sb.file("/sys/devices/system/cpu/intel_pstate/max_perf_pct"),
            "60"
        );
        profiles::apply(&mut e, &root, Profile::Performance, "test");
        assert_eq!(
            sb.file("/sys/devices/system/cpu/cpufreq/policy0/scaling_governor"),
            "performance"
        );
        profiles::apply(&mut e, &root, Profile::Balanced, "test");
        assert_eq!(
            sb.file("/sys/devices/system/cpu/intel_pstate/no_turbo"),
            "0"
        );
        assert_eq!(
            sb.file("/sys/devices/system/cpu/intel_pstate/max_perf_pct"),
            "100"
        );
        assert_eq!(
            sb.file("/sys/devices/system/cpu/cpufreq/policy0/scaling_governor"),
            "powersave"
        );
        // Advanced → Profiles can choose these too.
        let ed = profiles::editable(&root, Profile::PowerSaver);
        let limit = ed.iter().find(|(id, _)| *id == "cpu.max_perf_pct").unwrap();
        assert!(limit.1.contains(&"50".to_string()));
    }

    #[test]
    fn the_usual_controls_win_where_they_exist() {
        let sb = Sandbox::new();
        assert!(!profiles::uses_fallback(&sb.root()));
        assert!(profiles::plan(&sb.root(), Profile::PowerSaver)
            .iter()
            .all(|s| s.id == "cpu.epp" || s.id == "platform.profile"));
    }

    #[test]
    fn says_why_a_mode_is_missing() {
        let sb = budget_laptop();
        // Nothing left to change: no turbo, no speed limit, one governor.
        fs::remove_dir_all(sb.dir.join("root/sys/devices/system/cpu/intel_pstate")).unwrap();
        for p in ["policy0", "policy1"] {
            fs::write(
                sb.dir.join(format!(
                    "root/sys/devices/system/cpu/cpufreq/{p}/scaling_available_governors"
                )),
                "schedutil",
            )
            .unwrap();
        }
        assert_eq!(profiles::available(&sb.root()), [Profile::Balanced]);
        let why = profiles::missing(&sb.root());
        assert_eq!(why.len(), 2);
        assert!(
            why.iter().all(|(_, w)| w.contains("no controls")),
            "{why:?}"
        );
    }
}
