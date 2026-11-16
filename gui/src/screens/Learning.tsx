import { useEffect, useState } from "react";
import { backend, type LearnSession, type Status } from "../backend";
import { Dialog, ProgressBar, Switch } from "../components";
import { SleepTest } from "./SleepTest";
import { useAction, useLoad } from "../hooks";
import { duration, formatTime, t, tn, valueName } from "../i18n";

/** Learning, in plain language: how far it has got, what it kept, and the
 * tests so far. The technical table stays in Advanced → Learning. */
export function Learning(props: { status: Status; refresh: () => void }) {
  const s = props.status.learning;
  const learned = useLoad(() => backend.learned(), [s.tests]);
  const settings = useLoad(() => backend.settings(), []);
  const { run, toast } = useAction(() => {
    props.refresh();
    learned.reload();
  });
  const label = (id: string) => settings.data?.find((r) => r.id === id)?.label ?? id;
  const p = s.progress;
  const records = [...(learned.data?.records ?? [])].reverse();

  return (
    <>
      <header className="page-head">
        <h1>{t("learning_title")}</h1>
      </header>

      <section className="card" aria-labelledby="lprog-heading">
        <div className="row spread">
          <h2 id="lprog-heading">{p ? t("learn_pct", { pct: p.pct }) : t("learn_heading")}</h2>
          <Switch checked={s.on} label={t("learn_switch")} disabled={!!props.status.safe_mode} onChange={(on) => run(() => backend.setLearning(on))} />
        </div>
        {p && <ProgressBar pct={p.pct} />}
        <p className="muted">{t("learning_intro")}</p>
        {!props.status.power.has_battery && <p className="notice notice-info">{t("learning_no_battery")}</p>}
        {p && (
          <>
            <p>{t(`learning_stage_${p.stage}` as "learning_stage_single")}</p>
            {p.left > 0 && <p>{tn("learning_left", p.left, { n: p.left, time: duration(p.hours_left) })}</p>}
          </>
        )}
        {!s.on && p && p.left > 0 && <p className="muted">{t("learning_paused")}</p>}
        {s.on && (
          <p className="small">
            <span className="muted">{t("learning_now")}: </span>
            {s.status}
          </p>
        )}
        {s.on && s.waiting && s.waiting.share_pct >= 25 && p && p.left > 0 && (
          <div className="notice notice-info" role="note" style={{ flexDirection: "column", alignItems: "flex-start", gap: 4 }}>
            <strong>{t("learn_wait_heading", { pct: s.waiting.share_pct })}</strong>
            <p>{t(`learn_wait_${s.waiting.reason}` as "learn_wait_busy")}</p>
          </div>
        )}
        {s.power_update_s != null && s.power_update_s >= 20 && (
          <p className="muted small">{t("learn_slow_battery", { s: Math.round(s.power_update_s) })}</p>
        )}
      </section>

      {props.status.power.has_battery && <LearnNow status={props.status} run={run} />}
      {props.status.power.has_battery && <SleepTest locked={!!props.status.safe_mode} run={run} />}

      <section className="card" aria-labelledby="lkept-heading">
        <h2 id="lkept-heading">{t("learning_kept_heading")}</h2>
        {s.kept.length === 0 ? (
          <p className="muted">{t("learning_kept_none")}</p>
        ) : (
          <>
            <ul className="entries">
              {s.kept.map((k) => (
                <li key={k.setting} className="entry">
                  <div className="stack" style={{ gap: 2 }}>
                    <strong>{k.label || label(k.setting)}: {valueName(k.setting, k.value)}</strong>
                    <span className="muted small">
                      {t("learning_kept_was", { from: valueName(k.setting, k.baseline) })} · {t("learning_kept_saves", { w: k.saves_w.toFixed(2) })}
                    </span>
                  </div>
                  <button
                    type="button"
                    className="btn btn-small"
                    aria-label={`${t("forget")}: ${k.label || label(k.setting)}`}
                    onClick={() => run(() => backend.forget(k.setting).then(() => undefined))}
                  >
                    {t("forget")}
                  </button>
                </li>
              ))}
            </ul>
            {s.saves_w > 0 && <p>{t("learning_kept_total", { w: s.saves_w.toFixed(2) })}</p>}
          </>
        )}
      </section>

      <section className="card" aria-labelledby="ltests-heading">
        <h2 id="ltests-heading">{t("learning_tests_heading")}</h2>
        {records.length === 0 ? (
          <p className="muted">{t("learning_tests_none")}</p>
        ) : (
          <ul className="entries">
            {records.map((r, i) => (
              <li key={i} className="entry">
                <div className="stack" style={{ gap: 2 }}>
                  <span>
                    {label(r.setting)}: {valueName(r.setting, r.baseline)} → {valueName(r.setting, r.value)}
                  </span>
                  <span className="muted small">
                    {r.reason}
                    {r.time ? ` · ${formatTime(r.time)}` : ""}
                  </span>
                </div>
                <span className={`badge ${r.verdict === "keep" ? "badge-safe" : ""}`}>{t(`verdict_${r.verdict}` as "verdict_keep")}</span>
              </li>
            ))}
          </ul>
        )}
        <p className="muted small">
          <a className="btn-link" href="#advanced:learning">{t("learning_more")}</a>
        </p>
      </section>
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

const VERDICT_BADGE: Record<string, string> = { keep: "badge-safe", reject: "badge-risky", unclear: "" };

/** "Learn now": the person leaves the laptop alone and every test runs back
 * to back to set a baseline; everyday learning carries on afterwards. */
function LearnNow(props: { status: Status; run: (a: () => Promise<string | void>) => void }) {
  const [d, setD] = useState<LearnSession | null>(null);
  const [asking, setAsking] = useState(false);
  const [workload, setWorkload] = useState(true);
  const load = () => backend.learnSession().then(setD).catch(() => setD(null));
  useEffect(() => {
    load();
    const again = () => load();
    window.addEventListener("dynamis:changed", again);
    const id = window.setInterval(load, 3000);
    return () => {
      window.removeEventListener("dynamis:changed", again);
      window.clearInterval(id);
    };
  }, []);
  if (!d) return null;
  const pw = props.status.power;
  const running = d.phase === "waiting_screen" || d.phase === "testing";
  const ready = !pw.on_ac && (pw.battery_pct ?? 0) >= 40 && !props.status.safe_mode;
  const act = (a: () => Promise<void>) =>
    props.run(async () => {
      await a();
      await load();
    });
  const minutesSaved = pw.full_wh && d.saves_w > 0 ? Math.round((pw.full_wh / Math.max(0.5, (pw.watts ?? 6) - d.saves_w) - pw.full_wh / (pw.watts ?? 6)) * 60) : null;
  return (
    <section className="card" aria-labelledby="lnow-heading">
      <h2 id="lnow-heading">{t("lnow_heading")}</h2>
      <p className="muted">{t("lnow_intro")}</p>

      {running && (
        <>
          <ProgressBar pct={Math.round((d.done / d.total) * 100)} />
          <div className="notice notice-info" role="status">
            <span>
              {d.phase === "waiting_screen"
                ? t(d.agent_missing ? "lnow_waiting_agent" : "lnow_waiting_screen")
                : t("lnow_testing", { n: d.done + 1, total: d.total, label: d.current?.label ?? "", value: valueName(d.current?.setting ?? "", d.current?.value ?? null) })}
              {d.current?.under_load && ` ${t("lnow_under_load")}`}
            </span>
            <button type="button" className="btn btn-small" onClick={() => act(() => backend.stopLearnSession())}>
              {t("lnow_stop")}
            </button>
          </div>
          <p className="small muted">
            {d.minutes_left != null && t("lnow_left", { time: duration(d.minutes_left / 60) })} {t("lnow_touch_stops")}
          </p>
        </>
      )}

      {!running && (
        <>
          {d.phase === "done" && (
            <p>
              <strong>{t("lnow_done", { kept: d.results.filter((r) => r.verdict === "keep").length, w: d.saves_w.toFixed(2) })}</strong>
              {minutesSaved != null && minutesSaved > 0 && ` ${t("lnow_done_minutes", { min: minutesSaved })}`} {t("lnow_continues")}
            </p>
          )}
          {d.phase === "stopped" && (
            <p className="notice">{t("lnow_stopped", { why: d.stopped_reason ?? "", done: d.done, total: d.total })}</p>
          )}
          <ul className="small muted" aria-label={t("lnow_needs")}>
            <li>{pw.on_ac ? "✗" : "✓"} {t("lnow_need_battery")}</li>
            <li>{(pw.battery_pct ?? 0) >= 40 ? "✓" : "✗"} {t("lnow_need_charge")}</li>
            <li>{t("lnow_need_alone")}</li>
          </ul>
          <button type="button" className="btn btn-primary" disabled={!ready} onClick={() => setAsking(true)}>
            {d.phase === "idle" ? t("lnow_start") : t("lnow_again")}
          </button>
        </>
      )}

      {d.results.length > 0 && (
        <ul className="entries" aria-label={t("lnow_results")}>
          {d.results.map((r, i) => (
            <li key={`${i}-${r.setting}-${r.value}`} className="entry">
              <div className="stack" style={{ gap: 2 }}>
                <span>
                  {r.label}: {valueName(r.setting, r.value)}
                  {r.under_load && <span className="muted small"> · {t("lnow_with_workload")}</span>}
                </span>
                <span className="muted small">{r.reason}</span>
              </div>
              <span className={`badge ${VERDICT_BADGE[r.verdict]}`}>{t(`lnow_verdict_${r.verdict}` as "lnow_verdict_keep")}</span>
            </li>
          ))}
        </ul>
      )}

      <Dialog
        open={asking}
        onClose={() => setAsking(false)}
        title={t("lnow_confirm_title")}
        actions={
          <>
            <button type="button" className="btn" onClick={() => setAsking(false)}>{t("cancel")}</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => {
                setAsking(false);
                act(() => backend.startLearnSession(workload));
              }}
            >
              {t("lnow_confirm")}
            </button>
          </>
        }
      >
        <p>{t("lnow_confirm_desc", { time: duration((d.total * 9 + 5) / 60) })}</p>
        <label className="check">
          <input type="checkbox" checked={workload} onChange={(e) => setWorkload(e.target.checked)} />
          {t("lnow_workload")}
        </label>
        <p className="muted small">{t("lnow_workload_desc")}</p>
      </Dialog>
    </section>
  );
}
