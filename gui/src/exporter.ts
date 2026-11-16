// Building an exported log file. Nothing here sends anything anywhere.
import type { JournalEntry, Learned, SettingRow } from "./backend";

export interface ExportOptions {
  journal: boolean;
  results: boolean;
  hardware: boolean;
  settings: boolean;
  /** The full diagnostic report (`dynamis stat`). */
  diagnostics: boolean;
  rangeDays: number | null; // null = all
  format: "json" | "text";
  privacy: boolean;
}

export interface ExportData {
  journal: JournalEntry[];
  learned: Learned | null;
  inventory: unknown;
  settings: SettingRow[];
  diagnostics?: string | null;
  version: string;
}

/** Remove things that could identify a person or a machine. Our data
 * normally has none of these, but exported files get pasted into public
 * bug reports, so check anyway. */
export function scrub(text: string): string {
  return text
    .replace(/\b([0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b/gi, "[mac]")
    .replace(/\b(\d{1,3}\.){3}\d{1,3}\b/g, "[ip]")
    .replace(/\/home\/[^/\s"']+/g, "/home/[user]")
    .replace(/("(?:hostname|host_name|serial|serial_number|user|username|ssid)"\s*:\s*)"[^"]*"/gi, '$1"[removed]"');
}

export function build(o: ExportOptions, d: ExportData, now = Date.now() / 1000): string {
  const since = o.rangeDays == null ? 0 : now - o.rangeDays * 86400;
  const journal = d.journal.filter((e) => e.time >= since);
  const results = (d.learned?.records ?? []).filter((r) => r.time >= since);
  const bundle: Record<string, unknown> = { exported_by: `Dynamis Power ${d.version}`, exported_at: new Date(now * 1000).toISOString() };
  if (o.journal) bundle.journal = journal;
  if (o.results) bundle.results = { records: results, display: d.learned?.display ?? null };
  if (o.hardware) bundle.hardware = d.inventory;
  if (o.settings) bundle.settings = d.settings.map(({ id, current, original, blocked }) => ({ id, current, original, blocked }));
  if (o.diagnostics && d.diagnostics) bundle.diagnostics = d.diagnostics;

  let out: string;
  if (o.format === "json") {
    out = JSON.stringify(bundle, null, 2);
  } else {
    const lines = [String(bundle.exported_by), String(bundle.exported_at), ""];
    if (o.journal) {
      lines.push("== Change journal");
      for (const e of journal) {
        const change = e.from || e.to ? ` ${e.from ?? ""} -> ${e.to ?? ""}` : "";
        lines.push(`${new Date(e.time * 1000).toISOString()} ${e.event} ${e.id}${change} ${e.reason ?? ""}`.trimEnd());
      }
      lines.push("");
    }
    if (o.results) {
      lines.push("== Test results");
      for (const r of results) lines.push(`${r.setting} = ${r.value} (was ${r.baseline}): ${r.verdict}, ${r.reason}`);
      lines.push("");
    }
    if (o.hardware) lines.push("== Hardware", JSON.stringify(d.inventory, null, 2), "");
    if (o.settings) {
      lines.push("== Settings");
      for (const s of d.settings) lines.push(`${s.id}: ${s.current ?? "-"}${s.original ? ` (originally ${s.original})` : ""}${s.blocked ? " BLOCKED" : ""}`);
      lines.push("");
    }
    if (o.diagnostics && d.diagnostics) lines.push("== Diagnostic report", d.diagnostics);
    out = lines.join("\n");
  }
  return o.privacy ? scrub(out) : out;
}

/** A few lines to paste into a GitHub issue. */
export function summary(d: ExportData): string {
  const inv = d.inventory as { machine?: { vendor?: string; model?: string }; cpu?: { model?: string } } | null;
  const problems = d.journal.filter((e) => ["recover", "failed", "block"].includes(e.event));
  return [
    `Dynamis Power ${d.version}`,
    `Machine: ${inv?.machine?.vendor ?? "?"} ${inv?.machine?.model ?? ""}`.trim(),
    `CPU: ${inv?.cpu?.model ?? "?"}`,
    `Changes in journal: ${d.journal.length}; problems: ${problems.length}`,
    ...problems.slice(0, 5).map((e) => `- ${e.event} ${e.id}: ${e.reason ?? ""}`),
    `Learned: ${(d.learned?.learned ?? []).map((r) => `${r.setting}=${r.value}`).join(", ") || "nothing yet"}`,
  ].join("\n");
}
