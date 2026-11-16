#!/bin/sh
# The whole phase 1 real-hardware test in one command. It only READS: no
# setting on the laptop is changed.
#
#   cd daemon
#   cargo build --release
#   sudo ./scripts/phase1-test.sh            # 1 hour of logging (default)
#   sudo ./scripts/phase1-test.sh 120        # or a different number of minutes
#
# Steps: inventory, hardware report, 5 minutes of battery-gauge timing, then
# logging on battery while you use the laptop normally (Ctrl+C ends it early
# and still produces the report). Results go in one folder and one .tar.gz to
# attach to a GitHub "Hardware report" issue. See docs/TESTING-ON-HARDWARE.md.
set -eu

minutes=${1:-60}
here=$(cd "$(dirname "$0")/.." && pwd)
dynamis="$here/target/release/dynamis"
out="$PWD/phase1-results-$(date +%Y%m%d-%H%M)"
# For testing this script on a fixture copy (not for normal use):
#   DYNAMIS_ROOT=dir  GAUGE_SECONDS=n  LOG_SECONDS=n
root=${DYNAMIS_ROOT:-}
gauge_s=${GAUGE_SECONDS:-300}
log_s=${LOG_SECONDS:-$((${1:-60} * 60))}
if [ -n "$root" ]; then
    run() { "$dynamis" --root "$root" "$@"; }
else
    run() { "$dynamis" "$@"; }
fi

[ "$(id -u)" -eq 0 ] || { echo "Run with sudo (CPU power can only be read as root): sudo $0"; exit 1; }
[ -x "$dynamis" ] || { echo "Build first, as your normal user: cd $here && cargo build --release"; exit 1; }
case "$minutes" in ''|*[!0-9]*) echo "Minutes must be a whole number."; exit 1 ;; esac

on_mains() {
    for d in "$root"/sys/class/power_supply/*; do
        [ "$(cat "$d/type" 2>/dev/null)" = Mains ] && [ "$(cat "$d/online" 2>/dev/null)" = 1 ] && return 0
    done
    return 1
}

wait_for_battery() {
    while on_mains; do
        printf "Unplug the charger, then press Enter. "
        read -r _
    done
}

mkdir -p "$out"
echo "Saving results in $out"
echo

echo "== 1/5 Inventory"
run inventory | tee "$out/inventory.txt"
run inventory --json > "$out/inventory.json"
uname -r > "$out/kernel.txt"
echo

echo "== 2/5 Hardware report"
run hardware-report > "$out/hardware-report.md"
echo "Written to hardware-report.md (no serial numbers, usernames or network names)."
echo

echo "== 3/5 Battery gauge (5 minutes, on battery)"
wait_for_battery
run gauge --duration "$gauge_s" | tee "$out/gauge.txt"
echo

echo "== 4/5 Power log ($minutes minutes, on battery)"
echo "Use the laptop normally: browsing, documents, a video. Ctrl+C stops early."
wait_for_battery
# Ctrl+C should end the logger, not this script, so the report still runs.
trap 'echo; echo "Stopped early."' INT
run log --sync --duration "$log_s" -o "$out/power.csv" || true
trap - INT
echo

echo "== 5/5 Report"
if run report "$out/power.csv" | tee "$out/report.txt"; then
    run report "$out/power.csv" --json > "$out/report.json"
else
    echo "No report: the log has too few readings. Run the test for longer."
fi
echo

if [ -f "$out/report.json" ] && command -v python3 >/dev/null; then
    echo "== Automatic checks"
    python3 - "$out/report.json" <<'PY' | tee "$out/checks.txt"
import json, sys
r = json.load(open(sys.argv[1]))
def mean(k):
    v = r.get(k)
    return v["mean"] if v else None
def line(ok, text):
    print(("PASS  " if ok else ("FAIL  " if ok is False else "CHECK ")) + text)
bw, cpu = mean("battery_w"), mean("cpu_pkg_w")
line(bw is not None and 1 <= bw <= 40, f"Whole-system power on battery: {bw} W (expect roughly 3-10 W on a small laptop)")
line(None if cpu is None else cpu > 0, f"CPU power: {cpu} W" + (" (not recorded: no readable energy counters)" if cpu is None else ""))
line(r["duration_h"] >= 0.75, f"Logged {r['duration_h']} h (aim for 1 h or more)")
line(r["on_battery_share_pct"] >= 90, f"{r['on_battery_share_pct']}% of readings on battery")
sc, sm = mean("self_cpu_pct"), (r.get("self_rss_mb") or {}).get("max")
line(sc is not None and sc < 0.5 and sm is not None and sm < 20,
     f"Dynamis Power's own cost: {sc}% CPU, {sm} MB (budget: under 0.5% and 20 MB)")
g = r.get("gauge") or {}
print(f"INFO  Battery gauge in the log: {g.get('verdict', 'unknown')}")
print("Also check by eye: does the battery estimate match your panel? Do lag and temperature rise when you work harder?")
PY
    echo
fi

# Hand the results to the person who ran sudo, so they can open and attach them.
tar -czf "$out.tar.gz" -C "$(dirname "$out")" "$(basename "$out")"
if [ -n "${SUDO_USER:-}" ]; then
    chown -R "$SUDO_USER" "$out" "$out.tar.gz" 2>/dev/null || true
fi
echo "Done. Attach $out.tar.gz to a Hardware report issue:"
echo "https://github.com/hirejohndougan-art/Dynamis-Power/issues/new?template=hardware_report.yml"
