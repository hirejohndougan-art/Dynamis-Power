import { useMemo, useState } from "react";
import { backend, type JournalEntry } from "../backend";
import { describe, groupAuto, type Category, type Item } from "../activity";
import { Choice, Dialog } from "../components";
import { build, summary, type ExportOptions } from "../exporter";
import { useAction, useLoad } from "../hooks";
import { formatTime, t } from "../i18n";

const FILTERS: { key: Category | "all"; label: Parameters<typeof t>[0] }[] = [
  { key: "all", label: "filter_all" },
  { key: "auto", label: "filter_auto" },
  { key: "mine", label: "filter_mine" },
  { key: "tests", label: "filter_tests" },
  { key: "rollbacks", label: "filter_rollbacks" },
  { key: "problems", label: "filter_problems" },
];

export function Activity(props: { version: string; onChange: () => void }) {
  const journal = useLoad(() => backend.journal(500), []);
  const { run, toast } = useAction(() => {
    journal.reload();
    props.onChange();
  });
  const [tab, setTab] = useState<"activity" | "logs">("activity");
  const [filter, setFilter] = useState<Category | "all">("all");
  const [exporting, setExporting] = useState(false);
  const [reporting, setReporting] = useState(false);

  const items = useMemo(
    () => groupAuto((journal.data ?? []).map(describe).filter((i): i is Item => !!i && (filter === "all" || i.category === filter))),
    [journal.data, filter],
  );

  return (
    <>
      <header className="page-head">
        <h1>{t("activity_title")}</h1>
        <div className="row">
          <button type="button" className="btn" onClick={() => setReporting(true)}>{t("report_button")}</button>
          <button type="button" className="btn" onClick={() => setExporting(true)}>{t("export")}</button>
        </div>
      </header>

      <div className="tabs" role="tablist">
        {(["activity", "logs"] as const).map((k) => (
          <button key={k} type="button" role="tab" className="tab" aria-selected={tab === k} aria-controls={`panel-${k}`} id={`tab-${k}`} onClick={() => setTab(k)}>
            {t(k === "activity" ? "tab_activity" : "tab_logs")}
          </button>
        ))}
      </div>

      {tab === "activity" ? (
        <section id="panel-activity" role="tabpanel" aria-labelledby="tab-activity" className="card">
          <div className="chips" role="group" aria-label={t("tab_activity")}>
            {FILTERS.map((f) => (
              <button key={f.key} type="button" className="chip" aria-pressed={filter === f.key} onClick={() => setFilter(f.key)}>
                {t(f.label)}
              </button>
            ))}
          </div>
          {items.length === 0 ? (
            <p className="muted">{t("activity_empty")}</p>
          ) : (
            <ul className="entries">
              {items.map((i, n) =>
                i ? (
                  <li key={n} className="entry">
                    <div className="stack">
                      <span>{i.title}</span>
                      <span className="muted small">
                        {formatTime(i.entry.time)}
                        {i.why && ` · ${i.why}`}
                      </span>
                      {i.group && (
                        <details>
                          <summary className="small">{t("ev_auto_group_show")}</summary>
                          <ul className="small muted">
                            {i.group.map((g, k) => (
                              <li key={k}>
                                {formatTime(g.entry.time)}: {g.title}
                                {g.why && ` · ${g.why}`}
                              </li>
                            ))}
                          </ul>
                        </details>
                      )}
                    </div>
                    {i.undoable && (
                      <button type="button" className="btn btn-small" aria-label={`${t("undo")}: ${i.title}`} onClick={() => run(() => backend.undo(i.entry.id).then(() => t("undone", { label: i.entry.label })))}>
                        {t("undo")}
                      </button>
                    )}
                  </li>
                ) : null,
              )}
            </ul>
          )}
        </section>
      ) : (
        <Logs entries={journal.data ?? []} />
      )}

      {reporting && <ReportDialog onClose={() => setReporting(false)} version={props.version} journal={journal.data ?? []} />}
      <ExportDialog open={exporting} onClose={() => setExporting(false)} version={props.version} journal={journal.data ?? []} />
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

function Logs(props: { entries: JournalEntry[] }) {
  const [q, setQ] = useState("");
  const rows = props.entries.filter((e) => !q || JSON.stringify(e).toLowerCase().includes(q.toLowerCase()));
  return (
    <section id="panel-logs" role="tabpanel" aria-labelledby="tab-logs" className="card">
      <label className="field">
        <span className="visually-hidden">{t("logs_search")}</span>
        <input type="search" placeholder={t("logs_search")} value={q} onChange={(e) => setQ(e.target.value)} />
      </label>
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t("logs_col_time")}</th>
              <th>{t("logs_col_event")}</th>
              <th>{t("logs_col_setting")}</th>
              <th>{t("logs_col_change")}</th>
              <th>{t("logs_col_reason")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((e, n) => (
              <tr key={n}>
                <td className="small">{formatTime(e.time)}</td>
                <td><code>{e.event}</code></td>
                <td><code>{e.id}</code></td>
                <td className="small">{e.from || e.to ? `${e.from ?? ""} → ${e.to ?? ""}` : ""}</td>
                <td className="small">{e.reason}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function ExportDialog(props: { open: boolean; onClose: () => void; version: string; journal: JournalEntry[] }) {
  const [o, setO] = useState<ExportOptions>({ journal: true, results: true, hardware: true, settings: true, diagnostics: true, rangeDays: 7, format: "json", privacy: true });
  const extra = useLoad(
    async () => (props.open ? { learned: await backend.learned(), inventory: await backend.inventory(), settings: await backend.settings(), diagnostics: await backend.diagnostics().catch(() => null) } : null),
    [props.open],
  );
  const [note, setNote] = useState<string | null>(null);
  const data = { journal: props.journal, learned: extra.data?.learned ?? null, inventory: extra.data?.inventory ?? null, settings: extra.data?.settings ?? [], diagnostics: extra.data?.diagnostics ?? null, version: props.version };
  const text = build(o, data);
  const set = <K extends keyof ExportOptions>(k: K, v: ExportOptions[K]) => setO({ ...o, [k]: v });
  const check = (k: "journal" | "results" | "hardware" | "settings" | "diagnostics", label: Parameters<typeof t>[0]) => (
    <label className="check">
      <input type="checkbox" checked={o[k]} onChange={(e) => set(k, e.target.checked)} /> {t(label)}
    </label>
  );

  return (
    <Dialog
      open={props.open}
      onClose={props.onClose}
      title={t("export_title")}
      actions={
        <>
          <button type="button" className="btn" onClick={async () => { await navigator.clipboard?.writeText(summary(data)); setNote(t("export_copied")); }}>
            {t("export_copy")}
          </button>
          <button type="button" className="btn" onClick={props.onClose}>{t("close")}</button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={async () => {
              const stamp = new Date().toISOString().slice(0, 10);
              const where = await backend.saveFile(`dynamis-logs-${stamp}.${o.format === "json" ? "json" : "txt"}`, text).catch((e) => String(e));
              setNote(t("export_saved", { where }));
            }}
          >
            {t("export_save")}
          </button>
        </>
      }
    >
      <fieldset>
        <legend className="field-label">{t("export_include")}</legend>
        {check("journal", "export_journal")}
        {check("results", "export_results")}
        {check("hardware", "export_hardware")}
        {check("settings", "export_settings")}
        {check("diagnostics", "export_diagnostics")}
      </fieldset>
      <div className="row">
        <Choice
          name="export-range"
          label={t("export_range")}
          value={o.rangeDays ?? 0}
          options={[[1, t("range_24h")], [7, t("range_7d")], [30, t("range_30d")], [0, t("range_all")]]}
          onChange={(v) => set("rangeDays", v === 0 ? null : v)}
        />
        <Choice
          name="export-format"
          label={t("export_format")}
          value={o.format}
          options={[["json", t("format_json")], ["text", t("format_text")]]}
          onChange={(v) => set("format", v)}
        />
      </div>
      <label className="check" style={{ marginTop: 14 }}>
        <input type="checkbox" checked={o.privacy} onChange={(e) => set("privacy", e.target.checked)} /> {t("export_privacy")}
      </label>
      <p className="muted small">{t("export_privacy_desc")}</p>
      <p className="field-label" style={{ marginTop: 14 }}>{t("export_preview")}</p>
      <pre className="preview" tabIndex={0}>{text.slice(0, 4000)}</pre>
      <p className="muted small">{note ?? t("export_note")}</p>
    </Dialog>
  );
}

/** "Report a problem" (everyday problem §10): one file with everything
 * useful, privacy filter on, then a filled-in issue page to attach it to. */
function ReportDialog(props: { onClose: () => void; version: string; journal: JournalEntry[] }) {
  const [what, setWhat] = useState("");
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const send = async () => {
    setError(null);
    try {
      const [learned, inventory, settings, diagnostics] = await Promise.all([
        backend.learned(),
        backend.inventory(),
        backend.settings(),
        backend.diagnostics().catch(() => null),
      ]);
      const data = { journal: props.journal, learned, inventory, settings, diagnostics, version: props.version };
      const opts: ExportOptions = { journal: true, results: true, hardware: true, settings: true, diagnostics: true, rangeDays: 7, format: "text", privacy: true };
      const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, "-");
      const name = `dynamis-problem-${stamp}.txt`;
      const where = await backend.saveFile(name, build(opts, data));
      const first = what.trim().split("\n")[0].slice(0, 80);
      const body = [
        "**What went wrong**",
        what.trim() || "(not described)",
        "",
        "**Summary** (no personal data)",
        "```",
        summary(data),
        "```",
        "",
        `Please attach the file \`${name}\` (saved by the app; personal data removed).`,
      ].join("\n");
      await backend.openIssue(first ? `Problem: ${first}` : "A problem with Dynamis Power", body);
      setDone(t("report_done", { where }));
    } catch (e) {
      setError(String((e as Error)?.message ?? e));
    }
  };
  return (
    <Dialog
      open
      onClose={props.onClose}
      title={t("report_title")}
      actions={
        done ? (
          <button type="button" className="btn btn-primary" onClick={props.onClose}>{t("close")}</button>
        ) : (
          <>
            <button type="button" className="btn" onClick={props.onClose}>{t("cancel")}</button>
            <button type="button" className="btn btn-primary" onClick={send}>{t("report_send")}</button>
          </>
        )
      }
    >
      {done ? (
        <p role="status">{done}</p>
      ) : (
        <>
          <p>{t("report_desc")}</p>
          <label className="field">
            <span className="field-label">{t("report_what")}</span>
            <textarea rows={4} value={what} onChange={(e) => setWhat(e.target.value)} />
          </label>
          {error && <p className="notice" role="alert">{error}</p>}
        </>
      )}
    </Dialog>
  );
}
