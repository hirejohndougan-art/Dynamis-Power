//! `dynamis` — the Dynamis Power command line.
//!
//! Phase 1 commands are read-only. They never change a system setting.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use dynamis_core::inventory::{self, Inventory};
use dynamis_core::logfmt::{Row, LOG_FORMAT_VERSION};
use dynamis_core::sensors::{self, cpu::Turbo};
use dynamis_core::SysRoot;

#[derive(Parser)]
#[command(
    name = "dynamis",
    version,
    about = "Dynamis Power — learns your laptop to save battery and run faster.\n\nThese commands only read; they never change settings."
)]
struct Cli {
    /// Read from this directory instead of the real system (for testing).
    #[arg(long, global = true, value_name = "DIR", hide = true)]
    root: Option<PathBuf>,

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
    let result = match cli.command {
        Command::Inventory { json } => cmd_inventory(&root, json),
        Command::Status { json, over } => cmd_status(&root, json, over),
        Command::Log {
            interval,
            duration,
            output,
            format,
        } => cmd_log(&root, interval, duration, output, format),
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
) -> io::Result<()> {
    let interval = interval.clamp(0.5, 3600.0);
    let mut out: Box<dyn Write> = match &output {
        Some(p) => Box::new(BufWriter::new(File::create(p)?)),
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
    loop {
        sleep(Duration::from_secs_f64(interval));
        let cur = sensors::snapshot(root);
        let row = Row::between(&prev, &cur);
        match format {
            Format::Csv => writeln!(out, "{}", row.to_csv())?,
            Format::Jsonl => {
                serde_json::to_writer(&mut out, &row)?;
                writeln!(out)?;
            }
        }
        // Flush every row so nothing is lost if the logger is stopped.
        out.flush()?;
        prev = cur;
        if duration.is_some_and(|d| sensors::now_secs() - start >= d) {
            return Ok(());
        }
    }
}
