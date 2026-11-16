//! `dynamis` — the Dynamis Power command line.
//!
//! Most commands only read. `set`, `restore` and `unblock` change settings,
//! always through the engine's safety checks (see `changes.rs`).

mod changes;

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use dynamis_core::gauge;
use dynamis_core::inventory::{self, Inventory};
use dynamis_core::logfmt::{Row, LOG_FORMAT_VERSION};
use dynamis_core::report::{self, Report, Stat};
use dynamis_core::selfstat::SelfUsage;
use dynamis_core::sensors::{self, cpu::Turbo};
use dynamis_core::SysRoot;

#[derive(Parser)]
#[command(
    name = "dynamis",
    version,
    about = "Dynamis Power — learns your laptop to save battery and run faster.\n\nOnly `set`, `restore` and `unblock` change anything, and every change can be undone with `dynamis restore`."
)]
struct Cli {
    /// Read from this directory instead of the real system (for testing).
    #[arg(long, global = true, value_name = "DIR", hide = true)]
    root: Option<PathBuf>,

    /// Keep the journal and baseline here instead of /var/lib/dynamis-power.
    #[arg(long, global = true, value_name = "DIR", hide = true)]
    state_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Describe this machine and which settings it lets Dynamis Power tune.
    Inventory {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show power, speed, lag and temperature right now.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Seconds to measure over (CPU power and lag need two readings).
        #[arg(long, default_value_t = 2.0)]
        over: f64,
    },
    /// Record readings at an interval, for later analysis.
    Log {
        /// Seconds between readings.
        #[arg(long, default_value_t = 5.0)]
        interval: f64,
        /// Stop after this many seconds (default: run until Ctrl+C).
        #[arg(long)]
        duration: Option<f64>,
        /// File to write. Default: standard output.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Csv)]
        format: Format,
        /// Force each reading onto the disk as it is written, so the log
        /// survives a hard freeze or power cut (costs one small disk write per
        /// reading).
        #[arg(long)]
        sync: bool,
    },
    /// Summarise a CSV log recorded with `dynamis log`.
    Report {
        /// The CSV log file.
        file: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Measure how often the battery gauge updates, and how finely.
    /// Run it on battery for the most useful answer.
    Gauge {
        /// Seconds to watch.
        #[arg(long, default_value_t = 120.0)]
        duration: f64,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print a hardware report to paste into a GitHub "Hardware report" issue.
    /// Contains no serial numbers, usernames or network names.
    HardwareReport,
    /// List the settings Dynamis Power can change on this machine.
    Settings {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Change one setting (needs sudo). Use --trial to have it undone
    /// automatically unless you confirm it.
    Set {
        /// Setting ID, as shown by `dynamis settings`.
        id: String,
        /// New value.
        value: String,
        /// Undo the change after this many seconds unless you type y.
        #[arg(long, value_name = "SECONDS")]
        trial: Option<u64>,
        /// Confirm a change to a Risky or Experimental setting.
        #[arg(long)]
        allow_risky: bool,
        /// Why, for the journal.
        #[arg(long, default_value = "set by hand")]
        reason: String,
    },
    /// Put every setting Dynamis Power changed back to how it was. Always works,
    /// even in safe mode.
    Restore,
    /// Show the record of every change.
    Journal {
        /// How many entries to show.
        #[arg(long, default_value_t = 30)]
        last: usize,
        /// Print JSON Lines instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Allow a setting that was blocked after a failed or unfinished change.
    Unblock {
        /// Setting ID.
        id: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Csv,
    Jsonl,
}

fn main() {
    let cli = Cli::parse();
    let root = cli.root.map(SysRoot::at).unwrap_or_else(SysRoot::system);
    let state = cli
        .state_dir
        .unwrap_or_else(|| PathBuf::from(dynamis_core::engine::DEFAULT_STATE_DIR));
    let result = match cli.command {
        Command::Inventory { json } => cmd_inventory(&root, json),
        Command::Status { json, over } => cmd_status(&root, json, over),
        Command::Log {
            interval,
            duration,
            output,
            format,
            sync,
        } => cmd_log(&root, interval, duration, output, format, sync),
        Command::Report { file, json } => cmd_report(&root, &file, json),
        Command::Gauge { duration, json } => cmd_gauge(&root, duration, json),
        Command::HardwareReport => cmd_hardware_report(&root),
        Command::Settings { json } => changes::cmd_settings(&root, &state, json),
        Command::Set {
            id,
            value,
            trial,
            allow_risky,
            reason,
        } => changes::cmd_set(
            &root,
            &state,
            changes::SetArgs {
                id: &id,
                value: &value,
                trial,
                allow_risky,
                reason: &reason,
            },
        ),
        Command::Restore => changes::cmd_restore(&root, &state),
        Command::Journal { last, json } => changes::cmd_journal(&root, &state, last, json),
        Command::Unblock { id } => changes::cmd_unblock(&root, &state, &id),
    };
    if let Err(e) = result {
        // A closed pipe (for example `dynamis log | head`) is a normal way to stop.
        if e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("dynamis: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_inventory(root: &SysRoot, json: bool) -> io::Result<()> {
    let inv = inventory::read(root);
    let mut out = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut out, &inv)?;
        writeln!(out)?;
        return Ok(());
    }
    print_inventory(&mut out, &inv)
}

fn print_inventory(out: &mut impl Write, inv: &Inventory) -> io::Result<()> {
    let s = |v: &Option<String>| v.clone().unwrap_or_else(|| "unknown".into());
    let m = &inv.machine;
    writeln!(out, "This machine")?;
    writeln!(out, "  Model        {} {}", s(&m.vendor), s(&m.model))?;
    writeln!(out, "  Type         {}", m.form_factor)?;
    writeln!(
        out,
        "  Processor    {} ({} cores, {} threads)",
        s(&inv.cpu.model),
        inv.cpu.cores.map_or("?".into(), |c| c.to_string()),
        inv.cpu.threads.map_or("?".into(), |c| c.to_string())
    )?;
    writeln!(
        out,
        "  Memory       {}",
        inv.ram_gb.map_or("unknown".into(), |g| format!("{g} GB"))
    )?;
    for d in &inv.storage {
        writeln!(
            out,
            "  Storage      {} {} GB ({})",
            storage_label(&d.kind),
            d.size_gb,
            d.device
        )?;
    }
    if let Some(z) = &inv.zram {
        writeln!(
            out,
            "  Compressed swap  {} GB in memory ({})",
            z.size_gb,
            z.algorithm.clone().unwrap_or_default()
        )?;
    }
    for w in &inv.wifi {
        writeln!(
            out,
            "  Wi-Fi        {} (driver {})",
            w.interface,
            s(&w.driver)
        )?;
    }
    if let Some(wh) = inv.battery_design_wh {
        writeln!(out, "  Battery      {wh:.0} Wh when new")?;
    }
    writeln!(
        out,
        "  System       {}, kernel {}",
        s(&inv.software.distro),
        s(&inv.software.kernel)
    )?;
    writeln!(out)?;

    let available = inv.levers.iter().filter(|l| l.available).count();
    writeln!(
        out,
        "Settings Dynamis Power can tune here: {available} of {}",
        inv.levers.len()
    )?;
    for l in &inv.levers {
        let mark = if l.available { "yes" } else { " no" };
        let detail = match (&l.current, &l.note) {
            (Some(v), _) => format!("now: {}", friendly_value(l.id, v)),
            (None, Some(n)) => n.clone(),
            _ => String::new(),
        };
        writeln!(out, "  {mark}  {:<34} {detail}", l.label)?;
    }
    Ok(())
}

/// Show raw kernel values in units people recognise.
fn friendly_value(id: &str, raw: &str) -> String {
    match id {
        "cpu.rapl_long" => raw
            .parse::<f64>()
            .map(|uw| format!("{:.1} W", uw / 1e6))
            .unwrap_or_else(|_| raw.to_string()),
        "cpu.max_perf_pct" | "battery.charge_limit" => format!("{raw}%"),
        "vm.dirty_writeback" => raw
            .parse::<f64>()
            .map(|cs| format!("every {:.0} s", cs / 100.0))
            .unwrap_or_else(|_| raw.to_string()),
        "kernel.nmi_watchdog" | "audio.power_save" if raw == "0" => "off".into(),
        "kernel.nmi_watchdog" => "on".into(),
        "audio.power_save" => format!("after {raw} s idle"),
        _ => raw.to_string(),
    }
}

fn storage_label(kind: &str) -> &'static str {
    match kind {
        "emmc" => "eMMC",
        "sd" => "SD card",
        "nvme" => "NVMe SSD",
        "sata_ssd" => "SATA SSD",
        "hdd" => "Hard disk",
        _ => "Disk",
    }
}

fn cmd_status(root: &SysRoot, json: bool, over: f64) -> io::Result<()> {
    let before = sensors::snapshot(root);
    sleep(Duration::from_secs_f64(over.clamp(0.2, 60.0)));
    let after = sensors::snapshot(root);
    let row = Row::between(&before, &after);
    let mut out = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut out, &row)?;
        return writeln!(out);
    }
    let w = |v: Option<f64>| v.map_or("not available".into(), |x| format!("{x:.2} W"));
    let pct = |v: Option<f64>| v.map_or("not available".into(), |x| format!("{x:.1}%"));
    writeln!(
        out,
        "Power source   {}",
        if after.power.batteries.is_empty() {
            "mains (no battery found)"
        } else if row.on_ac {
            "charger"
        } else {
            "battery"
        }
    )?;
    if let Some(b) = row.battery_pct {
        writeln!(out, "Battery        {b}%")?;
    }
    writeln!(
        out,
        "Whole system   {}{}",
        w(row.battery_w),
        if row.on_ac {
            "  (only accurate on battery)"
        } else {
            ""
        }
    )?;
    writeln!(
        out,
        "Processor      {}{}",
        w(row.cpu_pkg_w),
        if row.cpu_pkg_w.is_none() && after.rapl.iter().any(|d| d.unreadable.is_some()) {
            "  (run as root to read)"
        } else {
            ""
        }
    )?;
    if let Some(mhz) = row.cpu_avg_mhz {
        writeln!(out, "CPU speed      {mhz} MHz average")?;
    }
    writeln!(
        out,
        "Turbo          {}",
        match after.cpu.turbo {
            Turbo::On => "on",
            Turbo::Off => "off",
            Turbo::Unsupported => "not available",
        }
    )?;
    writeln!(
        out,
        "Waiting for    CPU {}  memory {}  disk {}",
        pct(row.stall_cpu_pct),
        pct(row.stall_mem_pct),
        pct(row.stall_io_pct)
    )?;
    if let Some(t) = row.max_temp_c {
        writeln!(out, "Hottest sensor {t:.0} °C")?;
    }
    if let Some(b) = row.brightness_pct {
        writeln!(out, "Screen         {b:.0}% brightness")?;
    }
    Ok(())
}

fn cmd_log(
    root: &SysRoot,
    interval: f64,
    duration: Option<f64>,
    output: Option<PathBuf>,
    format: Format,
    sync: bool,
) -> io::Result<()> {
    let interval = interval.clamp(0.5, 3600.0);
    // A second handle to the same file, used only to force data to disk.
    let mut disk: Option<File> = None;
    let mut out: Box<dyn Write> = match &output {
        Some(p) => {
            let f = File::create(p)?;
            if sync {
                disk = Some(f.try_clone()?);
            }
            Box::new(BufWriter::new(f))
        }
        None => Box::new(io::stdout().lock()),
    };
    if let Format::Csv = format {
        writeln!(out, "# dynamis log format {LOG_FORMAT_VERSION}")?;
        writeln!(out, "{}", Row::CSV_HEADER)?;
        out.flush()?;
    }
    if output.is_some() {
        eprintln!("Recording every {interval} s. Press Ctrl+C to stop.");
    }
    let start = sensors::now_secs();
    let mut prev = sensors::snapshot(root);
    let mut prev_self = SelfUsage::read();
    loop {
        sleep(Duration::from_secs_f64(interval));
        let cur = sensors::snapshot(root);
        let cur_self = SelfUsage::read();
        let mut row = Row::between(&prev, &cur);
        if let (Some(a), Some(b)) = (prev_self, cur_self) {
            row.self_cpu_pct = b.cpu_pct_since(&a, cur.time - prev.time);
            row.self_rss_mb = b.rss_mb;
        }
        prev_self = cur_self;
        match format {
            Format::Csv => writeln!(out, "{}", row.to_csv())?,
            Format::Jsonl => {
                serde_json::to_writer(&mut out, &row)?;
                writeln!(out)?;
            }
        }
        // Flush every row so nothing is lost if the logger is stopped. With
        // --sync, also wait until it is on the disk itself, not just in the
        // kernel's cache, so a hard freeze loses at most the current reading.
        out.flush()?;
        if let Some(f) = &disk {
            f.sync_data()?;
        }
        prev = cur;
        if duration.is_some_and(|d| sensors::now_secs() - start >= d) {
            return Ok(());
        }
    }
}

fn cmd_report(root: &SysRoot, file: &PathBuf, json: bool) -> io::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let rep = report::from_csv(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut out = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut out, &rep)?;
        return writeln!(out);
    }
    print_report(&mut out, root, &rep)
}

fn print_report(out: &mut impl Write, root: &SysRoot, r: &Report) -> io::Result<()> {
    fn line(
        out: &mut impl Write,
        label: &str,
        s: &Option<Stat>,
        unit: &str,
        places: usize,
    ) -> io::Result<()> {
        match s {
            Some(s) => writeln!(
                out,
                "  {label:<22} average {:.p$}{unit}   typical {:.p$}{unit}   range {:.p$}–{:.p$}{unit}",
                s.mean,
                s.median,
                s.min,
                s.max,
                p = places
            ),
            None => writeln!(out, "  {label:<22} not recorded"),
        }
    }
    let hours = r.duration_h;
    let span = if hours >= 1.0 {
        format!("{hours:.1} hours")
    } else {
        format!("{:.0} minutes", hours * 60.0)
    };
    writeln!(
        out,
        "Power report: {span}, {} readings, {:.0}% on battery",
        r.rows, r.on_battery_share_pct
    )?;
    writeln!(out)?;
    writeln!(out, "Battery (readings taken on battery only)")?;
    line(out, "Whole system", &r.battery_w, " W", 2)?;
    if let Some(p) = r.battery_pct_used {
        writeln!(out, "  {:<22} {p} percentage points", "Charge used")?;
    }
    // Estimate using this machine's current full capacity, when there is a battery.
    let cap = sensors::power::read(root)
        .batteries
        .first()
        .and_then(|b| b.energy_full_wh);
    if let Some(h) = cap.and_then(|c| report::hours_at_mean(r, c)) {
        writeln!(
            out,
            "  {:<22} about {h} h from full at this rate (using this laptop's current capacity)",
            "Estimated battery life"
        )?;
    }
    writeln!(out)?;
    writeln!(out, "Processor")?;
    line(out, "CPU power", &r.cpu_pkg_w, " W", 2)?;
    line(out, "CPU speed", &r.cpu_avg_mhz, " MHz", 0)?;
    writeln!(out)?;
    writeln!(out, "Lag (share of time apps waited)")?;
    line(out, "Waiting for CPU", &r.stall_cpu_pct, "%", 1)?;
    line(out, "Waiting for memory", &r.stall_mem_pct, "%", 1)?;
    line(out, "Waiting for disk", &r.stall_io_pct, "%", 1)?;
    writeln!(out)?;
    writeln!(out, "Other")?;
    line(out, "Hottest sensor", &r.max_temp_c, " °C", 0)?;
    line(out, "Screen brightness", &r.brightness_pct, "%", 0)?;
    writeln!(out)?;
    writeln!(out, "Dynamis Power's own cost while logging")?;
    line(out, "CPU", &r.self_cpu_pct, "% of one core", 2)?;
    line(out, "Memory", &r.self_rss_mb, " MB", 1)?;
    if let (Some(c), Some(m)) = (&r.self_cpu_pct, &r.self_rss_mb) {
        let ok = c.mean < 0.5 && m.max < 20.0;
        writeln!(
            out,
            "  {:<22} {}",
            "Budget (0.5% CPU, 20 MB)",
            if ok { "within budget" } else { "OVER BUDGET" }
        )?;
    }
    writeln!(out)?;
    writeln!(out, "Battery gauge")?;
    writeln!(out, "  {}", r.gauge.verdict)?;
    Ok(())
}

fn cmd_gauge(root: &SysRoot, duration: f64, json: bool) -> io::Result<()> {
    let duration = duration.clamp(5.0, 3600.0);
    let supplies = sensors::power::read(root);
    if supplies.batteries.is_empty() {
        eprintln!("No battery found, so there is no gauge to measure.");
        return Ok(());
    }
    if supplies.on_ac() && !json {
        eprintln!(
            "Note: the charger is connected. Many gauges only report useful power on battery."
        );
    }
    if !json {
        eprintln!("Watching the battery gauge for {duration:.0} s (one reading every 0.5 s)…");
    }
    let start = sensors::now_secs();
    let mut series = Vec::new();
    while sensors::now_secs() - start < duration {
        if let Some(w) = sensors::power::read(root).total_battery_power_w() {
            series.push((sensors::now_secs(), w));
        }
        sleep(Duration::from_millis(500));
    }
    let g = gauge::analyse(&series);
    let mut out = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut out, &g)?;
        return writeln!(out);
    }
    writeln!(out, "Readings        {}", g.samples)?;
    writeln!(out, "Changes seen    {}", g.changes)?;
    writeln!(
        out,
        "Updates every   {}",
        g.update_interval_s
            .map_or("unknown".into(), |s| format!("about {s} s"))
    )?;
    writeln!(
        out,
        "Smallest step   {}",
        g.resolution_w
            .map_or("unknown".into(), |w| format!("{w} W"))
    )?;
    writeln!(out, "{}", g.verdict)
}

fn cmd_hardware_report(root: &SysRoot) -> io::Result<()> {
    let inv = inventory::read(root);
    let mut out = io::stdout().lock();
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "unknown".into());
    writeln!(out, "<!-- Paste this into a new \"Hardware report\" issue. It contains no serial numbers, usernames or network names. -->")?;
    writeln!(
        out,
        "**Laptop:** {} {}",
        inv.machine.vendor.clone().unwrap_or_default(),
        inv.machine.model.clone().unwrap_or_default()
    )?;
    writeln!(
        out,
        "**System:** {}, kernel {}, desktop {} on {}",
        inv.software.distro.clone().unwrap_or_default(),
        inv.software.kernel.clone().unwrap_or_default(),
        env("XDG_CURRENT_DESKTOP"),
        env("XDG_SESSION_TYPE")
    )?;
    writeln!(out, "**Dynamis Power:** {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out)?;
    writeln!(out, "```text")?;
    print_inventory(&mut out, &inv)?;
    writeln!(out, "```")?;
    writeln!(out)?;
    writeln!(out, "<details><summary>Full inventory (JSON)</summary>")?;
    writeln!(out)?;
    writeln!(out, "```json")?;
    serde_json::to_writer_pretty(&mut out, &inv)?;
    writeln!(out)?;
    writeln!(out, "```")?;
    writeln!(out, "</details>")
}
