import { useState } from "react";
import { backend, type SettingRow, type Status } from "../backend";
import { Dialog, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { t, valueName } from "../i18n";
import { DeveloperTab, ProfilesTab, SensorsTab, SystemTab } from "./AdvancedMore";
import { RestoreDialog } from "./Home";

export const TABS = ["settings", "learning", "profiles", "sensors", "safety", "system", "developer"] as const;
export type Tab = (typeof TABS)[number];

export function Advanced(props: { status: Status; refresh: () => void; initialTab?: Tab }) {
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "settings");
  const settings = useLoad(() => backend.settings(), []);
  const learned = useLoad(() => backend.learned(), [props.status.learning.tests]);
  const { run, toast } = useAction(() => {
    settings.reload();
    learned.reload();
    props.refresh();
  });

  return (
    <>
      <header className="page-head">
        <h1>{t("advanced_title")}</h1>
      </header>
      <p className="muted">{t("advanced_notice")}</p>
      <div className="tabs" role="tablist">
        {TABS.map((k) => (
          <button key={k} type="button" role="tab" id={`atab-${k}`} aria-controls={`apanel-${k}`} aria-selected={tab === k} className="tab" onClick={() => setTab(k)}>
            {t(`tab_${k}`)}
          </button>
        ))}
      </div>
      <section id={`apanel-${tab}`} role="tabpanel" aria-labelledby={`atab-${tab}`} className="card">
        {tab === "settings" && <SettingsTab rows={settings.data ?? []} learned={(learned.data?.learned ?? []).map((r) => r.setting)} run={run} />}
        {tab === "learning" && <LearningTab status={props.status} data={learned.data} run={run} />}
        {tab === "safety" && <SafetyTab status={props.status} rows={settings.data ?? []} run={run} />}
        {tab === "profiles" && <ProfilesTab locked={!!props.status.safe_mode} run={run} />}
        {tab === "sensors" && <SensorsTab />}
        {tab === "system" && <SystemTab />}
        {tab === "developer" && <DeveloperTab status={props.status} run={run} refresh={props.refresh} />}
      </section>
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

type Run = (a: () => Promise<string | void>) => void;

function SettingsTab(props: { rows: SettingRow[]; learned: string[]; run: Run }) {
  const [details, setDetails] = useState(false);
  const [editing, setEditing] = useState<SettingRow | null>(null);
  const [value, setValue] = useState("");
  const [confirmRisky, setConfirmRisky] = useState(false);

  const status = (r: SettingRow) => {
    if (!r.available) return t("status_not_offered");
    if (r.blocked) return t("status_blocked", { why: r.blocked });
    if (props.learned.includes(r.id)) return t("status_learned");
    if (r.original && r.original !== r.current) return t("status_changed");
    return "";
  };
  const apply = (allowRisky: boolean) => {
    const r = editing;
    if (!r) return;
    setEditing(null);
    setConfirmRisky(false);
    props.run(() => backend.setSetting(r.id, value, allowRisky));
  };

  return (
    <>
      <Switch checked={details} onChange={setDetails} label={t("details")} />
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t("col_setting")}</th>
              <th>{t("col_now")}</th>
              <th>{t("col_original")}</th>
              <th>{t("col_risk")}</th>
              <th>{t("col_status")}</th>
              <th><span className="visually-hidden">{t("change")}</span></th>
            </tr>
          </thead>
          <tbody>
            {props.rows.map((r) => (
              <tr key={r.id}>
                <td>
                  {r.label}
                  {details && (
                    <div>
                      <code>{r.id}</code> <span className="muted small">{r.available ? t("change_allowed", { allowed: r.allowed }) : r.allowed}</span>
                    </div>
                  )}
                  {r.not_allowed.map(([vals, why], i) => (
                    <div key={i} className="muted small">{t("status_not_allowed", { values: vals.map((v) => valueName(r.id, v)).join(", ") || "*" })}{details && ` — ${why}`}</div>
                  ))}
                </td>
                <td>
                  {valueName(r.id, r.current)}
                  {details && r.current && valueName(r.id, r.current) !== r.current && <div><code>{r.current}</code></div>}
                </td>
                <td>{valueName(r.id, r.original)}</td>
                <td><span className={`badge badge-${r.risk}`}>{r.risk_label}</span></td>
                <td className="small">{status(r)}</td>
                <td>
                  <div className="row" style={{ flexWrap: "nowrap" }}>
                    {r.available && r.risk !== "advisory" && (
                      <button type="button" className="btn btn-small" aria-label={`${t("change")}: ${r.label}`} onClick={() => { setEditing(r); setValue(r.current ?? ""); }}>
                        {t("change")}
                      </button>
                    )}
                    {r.original && r.original !== r.current && (
                      <button type="button" className="btn btn-small" aria-label={`${t("reset")}: ${r.label}`} onClick={() => props.run(() => backend.undo(r.id).then(() => t("undone", { label: r.label })))}>
                        {t("reset")}
                      </button>
                    )}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <Dialog
        open={!!editing && !confirmRisky}
        onClose={() => setEditing(null)}
        title={editing ? t("change_title", { label: editing.label }) : ""}
        actions={
          <>
            <button type="button" className="btn" onClick={() => setEditing(null)}>{t("cancel")}</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => (editing && ["risky", "experimental"].includes(editing.risk) ? setConfirmRisky(true) : apply(false))}
            >
              {t("change_apply")}
            </button>
          </>
        }
      >
        <label className="field">
          <span className="field-label">{t("change_value")}</span>
          <input type="text" value={value} onChange={(e) => setValue(e.target.value)} autoFocus />
        </label>
        {editing && <p className="muted small">{t("change_allowed", { allowed: editing.allowed })}</p>}
      </Dialog>

      <Dialog
        open={confirmRisky}
        onClose={() => setConfirmRisky(false)}
        title={t("risky_title")}
        actions={
          <>
            <button type="button" className="btn" onClick={() => { setConfirmRisky(false); setEditing(null); }}>{t("cancel")}</button>
            <button type="button" className="btn btn-primary" onClick={() => apply(true)}>{t("risky_confirm")}</button>
          </>
        }
      >
        <p>{editing && t("risky_body", { label: editing.label, risk: editing.risk_label.toLowerCase() })}</p>
      </Dialog>
    </>
  );
}

function LearningTab(props: { status: Status; data: Awaited<ReturnType<typeof backend.learned>> | null; run: Run }) {
  const [confirm, setConfirm] = useState(false);
  const records = props.data?.records ?? [];
  const verdict = (v: string) => t(`verdict_${v}` as "verdict_keep");
  return (
    <>
      <div className="row spread">
        <Switch checked={props.status.learning.on} label={t("learn_switch")} onChange={(on) => props.run(() => backend.setLearning(on))} />
        <span className="muted small">{props.status.learning.status}</span>
      </div>
      <h2>{t("learning_records")}</h2>
      {records.length === 0 ? (
        <p className="muted">{t("learning_none")}</p>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t("col_setting")}</th>
                <th>{t("col_value")}</th>
                <th>{t("col_result")}</th>
                <th>{t("col_effect")}</th>
                <th><span className="visually-hidden">{t("forget")}</span></th>
              </tr>
            </thead>
            <tbody>
              {[...records].reverse().map((r, i) => (
                <tr key={i}>
                  <td><code>{r.setting}</code></td>
                  <td>{r.baseline} → {r.value}</td>
                  <td>
                    <span className={`badge ${r.verdict === "keep" ? "badge-safe" : ""}`}>{verdict(r.verdict)}</span>
                    <div className="muted small">{r.reason}</div>
                    {r.group && Object.keys(r.group).length > 0 && (
                      <div className="muted small">
                        {t("learning_tested_with", { list: Object.entries(r.group).map(([k, v]) => `${k} = ${v}`).join(", ") })}
                      </div>
                    )}
                  </td>
                  <td className="small">{t("watts_delta", { w: (r.effect.mean > 0 ? "+" : "") + r.effect.mean.toFixed(2) })}</td>
                  <td>
                    {r.verdict === "keep" && (
                      <button type="button" className="btn btn-small" onClick={() => props.run(() => backend.forget(r.setting).then(() => undefined))}>
                        {t("forget")}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {records.length > 0 && (
        <button type="button" className="btn" onClick={() => setConfirm(true)}>{t("forget_all")}</button>
      )}
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title={t("forget_all_title")}
        actions={
          <>
            <button type="button" className="btn" onClick={() => setConfirm(false)}>{t("cancel")}</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => {
                setConfirm(false);
                const ids = [...new Set(records.map((r) => r.setting))];
                props.run(async () => { for (const id of ids) await backend.forget(id); });
              }}
            >
              {t("forget")}
            </button>
          </>
        }
      >
        <p>{t("forget_all_body")}</p>
      </Dialog>
    </>
  );
}

function SafetyTab(props: { status: Status; rows: SettingRow[]; run: Run }) {
  const [restoring, setRestoring] = useState(false);
  const quirks = props.rows.flatMap((r) => r.not_allowed.map(([vals, why]) => ({ label: r.label, vals, why })));
  return (
    <>
      <h2>{t("safety_safe_mode")}</h2>
      <Switch checked={!!props.status.safe_mode} label={t("safety_safe_mode")} onChange={(on) => props.run(() => backend.setSafeMode(on).then(() => undefined))} />
      <p className="muted small">{t("safety_safe_mode_desc")}</p>

      <h2 style={{ marginTop: 12 }}>{t("safety_quirks")}</h2>
      {quirks.length === 0 ? (
        <p className="muted">{t("safety_quirks_none")}</p>
      ) : (
        <ul>
          {quirks.map((q, i) => (
            <li key={i}><strong>{q.label}</strong> ({q.vals.join(", ") || "*"}): {q.why}</li>
          ))}
        </ul>
      )}

      <h2 style={{ marginTop: 12 }}>{t("home_restore")}</h2>
      <p className="muted small">{t("safety_restore_desc")}</p>
      <div>
        <button type="button" className="btn" onClick={() => setRestoring(true)}>{t("home_restore")}</button>
      </div>
      <RestoreDialog open={restoring} onClose={() => setRestoring(false)} onDone={(m) => props.run(async () => m)} />
    </>
  );
}
