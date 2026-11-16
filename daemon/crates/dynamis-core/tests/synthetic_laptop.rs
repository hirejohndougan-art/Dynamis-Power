//! Tests against the synthetic laptop fixture in `tests/fixtures/sysfs/`.
//! See that fixture's MACHINE.md for what each value represents.

use std::path::PathBuf;

use dynamis_core::inventory;
use dynamis_core::logfmt::Row;
use dynamis_core::sensors::{self, cpu::Turbo};
use dynamis_core::SysRoot;

fn fixture() -> SysRoot {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/sysfs/synthetic-laptop");
    assert!(p.exists(), "fixture missing at {}", p.display());
    SysRoot::at(p)
}

#[test]
fn battery_and_charger() {
    let s = sensors::power::read(&fixture());
    assert_eq!(s.batteries.len(), 1);
    let b = &s.batteries[0];
    assert_eq!(b.status.as_deref(), Some("Discharging"));
    assert_eq!(b.capacity_pct, Some(79));
    assert_eq!(b.power_w, Some(5.2));
    assert_eq!(b.energy_full_design_wh, Some(42.0));
    assert_eq!(b.health_pct, Some(90.5));
    assert_eq!(b.cycle_count, Some(212));
    assert_eq!(b.charge_limit_pct, None);
    assert!(!b.charge_behaviour_supported);
    assert!(!s.on_ac());
}

#[test]
fn cpu_policies() {
    let c = sensors::cpu::read(&fixture());
    assert_eq!(c.policies.len(), 2);
    assert_eq!(c.policies[0].epp.as_deref(), Some("balance_performance"));
    assert_eq!(c.policies[0].epp_choices.len(), 5);
    assert_eq!(c.policies[0].max_mhz, Some(2800));
    assert_eq!(c.avg_mhz(), Some(1100));
    assert_eq!(c.turbo, Turbo::On);
}

#[test]
fn pressure_thermal_rapl_backlight() {
    let root = fixture();
    let p = sensors::pressure::read(&root);
    assert_eq!(p.cpu.unwrap().some.avg10, 1.2);
    assert_eq!(p.memory.unwrap().full.unwrap().total_us, 1234);
    let t = sensors::thermal::read(&root);
    assert_eq!(sensors::thermal::max_c(&t), Some(52.0));
    let r = sensors::rapl::read(&root);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].name, "package-0");
    assert_eq!(r[0].energy_uj, Some(123_456_789));
    let b = sensors::backlight::read(&root).unwrap();
    assert_eq!(b.percent, 50.0);
}

#[test]
fn inventory_identity_and_storage() {
    let inv = inventory::read(&fixture());
    assert_eq!(
        inv.machine.vendor.as_deref(),
        Some("Example Computers Inc.")
    );
    assert_eq!(inv.machine.form_factor, "laptop");
    assert_eq!(inv.cpu.cores, Some(2));
    assert_eq!(inv.cpu.threads, Some(2));
    assert!(inv.cpu.hwp);
    assert_eq!(inv.ram_gb, Some(4));
    assert_eq!(
        inv.storage.len(),
        1,
        "boot partitions and zram must be excluded"
    );
    assert_eq!(inv.storage[0].kind, "emmc");
    assert_eq!(inv.storage[0].size_gb, 63);
    assert_eq!(inv.storage[0].scheduler.as_deref(), Some("mq-deadline"));
    assert_eq!(
        inv.zram.as_ref().unwrap().algorithm.as_deref(),
        Some("zstd")
    );
    assert_eq!(inv.wifi[0].driver.as_deref(), Some("rtw_8821ce"));
    assert_eq!(inv.software.distro.as_deref(), Some("Example Linux 24.04"));
    assert_eq!(inv.battery_design_wh, Some(42.0));
}

#[test]
fn inventory_levers() {
    let inv = inventory::read(&fixture());
    let lever = |id: &str| {
        inv.levers
            .iter()
            .find(|l| l.id == id)
            .unwrap_or_else(|| panic!("no lever {id}"))
    };
    assert!(lever("cpu.epp").available);
    assert_eq!(lever("cpu.turbo").current.as_deref(), Some("on"));
    assert_eq!(lever("cpu.rapl_long").current.as_deref(), Some("6000000"));
    assert_eq!(lever("pcie.aspm").current.as_deref(), Some("default"));
    assert!(!lever("platform.profile").available);
    assert!(!lever("battery.charge_limit").available);
    assert!(!lever("battery.charge_behaviour").available);
}

#[test]
fn log_row_from_identical_snapshots() {
    let root = fixture();
    let a = sensors::snapshot(&root);
    let mut b = a.clone();
    b.time = a.time + 2.0;
    let row = Row::between(&a, &b);
    assert_eq!(row.interval_s, 2.0);
    assert!(!row.on_ac);
    assert_eq!(row.battery_w, Some(5.2));
    // Same counters in both snapshots: zero CPU power and zero stall.
    assert_eq!(row.cpu_pkg_w, Some(0.0));
    assert_eq!(row.stall_cpu_pct, Some(0.0));
    assert_eq!(row.csv_field_count(), Row::CSV_HEADER.split(',').count());
}

#[test]
fn missing_root_is_empty_not_a_crash() {
    let root = SysRoot::at("/nonexistent/dynamis-test-root");
    let inv = inventory::read(&root);
    assert!(inv.storage.is_empty());
    assert!(inv.levers.iter().all(|l| !l.available));
    let s = sensors::snapshot(&root);
    assert!(s.power.batteries.is_empty());
}
