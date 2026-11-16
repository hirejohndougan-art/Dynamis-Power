import { useState } from "react";
import { backend, type Profile, type Status } from "../backend";
import { Dialog, Sparkline, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { clock, duration, formatTime, profileName, t, tn } from "../i18n";
import { describe } from "../activity";

function Estimate({ s }: { s: Status }) {
  const p = s.power;
  let text: string;
  if (!p.has_battery) text = t("est_no_battery");
  else if (p.charging && p.hours_to_full != null) text = t("est_to_full", { time: duration(p.hours_to_full) });
  else if (p.on_ac) text = p.battery_pct === 100 ? t("est_full") : t("est_plugged");
  else if (p.hours_left != null) text = t("est_left", { time: duration(p.hours_left) });
  else text = t("est_unknown");
  return (
    <div className="stack">
      <p className="big">{text}</p>
      {p.battery_pct != null && <p className="muted">{t("battery_pct", { pct: p.battery_pct })}</p>}
    </div>
  );
}

export function Home(props: { status: Status; watts: number[]; refresh: () => void; go: (screen: string) => void }) {
  const s = props.status;
  const journal = useLoad(() => backend.journal(30), [s.profile, s.learning.tests]);
  const { run, toast } = useAction(() => {
    props.refresh();
    journal.reload();
  });
  const [restoring, setRestoring] = useState(false);

  const last = (journal.data ?? []).map(describe).find((i) => i !== null) ?? null;
  const held = s.holds[0];
  const nowLine = s.last_until
    ? t("lu_home", { time: clock(s.last_until.until), reason: s.last_until.reason ?? "" })
    : held
    ? t("mode_held", { app: held.app, mode: profileName(held.profile), reason: held.reason })
    : s.auto.on
      ? t("mode_now_auto", { reason: s.auto.reason })
      : t("mode_now_manual", { mode: profileName(s.profile) });

  const savedMinutes =
    s.learning.saves_w > 0 && s.power.watts && s.power.hours_left
      ? Math.round(s.power.hours_left * 60 * (s.learning.saves_w / s.power.watts))
      : 0;

  return (
    <>
      <header className="page-head">
        <h1>{t("home_title")}</h1>
        <button type="button" className="btn" onClick={() => setRestoring(true)}>
          {t("home_restore")}
        </button>
      </header>

      {s.safe_mode && (
        <div className="notice" role="alert">
          <span>{t("safe_mode_on", { why: s.safe_mode })}</span>
          <button type="button" className="btn btn-small" onClick={() => run(async () => { await backend.setSafeMode(false); })}>
            {t("safe_mode_off_button")}
          </button>
        </div>
      )}
      {s.pending?.trial && <div className="notice notice-info">{t("trial_running", { label: s.pending.label })}</div>}
      {s.dry_run && (
        <div className="notice notice-info">
          {t("dry_run_on")} <a href="#advanced:developer">{t("tab_developer")}</a>
        </div>
      )}

      <section className="card" aria-label={t("battery_title")}>
        <Estimate s={s} />
        <p>{nowLine}</p>
      </section>

      <section className="card" aria-labelledby="mode-heading">
        <div className="row spread">
          <h2 id="mode-heading">{t("mode_heading")}</h2>
          <Switch checked={s.auto.on} label={t("mode_auto")} disabled={!!s.safe_mode} onChange={(on) => run(() => backend.setAuto(on))} />
        </div>
        <div className="segmented" role="radiogroup" aria-labelledby="mode-heading">
          {s.profiles.map((p: Profile) => (
            <button
              key={p}
              type="button"
              role="radio"
              aria-checked={s.profile === p}
              className="segment"
              disabled={!!s.safe_mode}
              onClick={() => run(() => backend.setProfile(p))}
            >
              <span className="segment-title">{profileName(p)}</span>
              <span className="muted small">{t(`profile_${p.replace("-", "_")}_desc` as "profile_balanced_desc")}</span>
            </button>
          ))}
        </div>
        <p className="muted small">{t("mode_auto_desc")}</p>
      </section>

      <div className="grid">
        <section className="card" aria-labelledby="draw-heading">
          <h2 id="draw-heading">{t("draw_heading")}</h2>
          {s.power.watts != null && !s.power.on_ac ? (
            <>
              <p className="big">{t("draw_watts", { w: s.power.watts.toFixed(1) })}</p>
              <Sparkline values={props.watts} label={t("draw_chart", { m: Math.round((props.watts.length * 5) / 60) })} />
            </>
          ) : (
            <p className="muted">{t("draw_none")}</p>
          )}
        </section>

        <section className="card" aria-labelledby="learn-heading">
          <div className="row spread">
            <h2 id="learn-heading">{t("learn_heading")}</h2>
            <Switch checked={s.learning.on} label={t("learn_switch")} onChange={(on) => run(() => backend.setLearning(on))} />
          </div>
          {s.learning.on ? (
            <>
              <p>{tn("learn_progress", s.learning.tested_settings, { checked: s.learning.tested_settings, kept: s.learning.kept.length })}</p>
              {s.learning.saves_w > 0 && <p>{t("learn_saves", { w: s.learning.saves_w.toFixed(2), min: savedMinutes })}</p>}
              <p className="muted small">{s.learning.status}</p>
            </>
          ) : (
            <p className="muted">{t("learn_off")}</p>
          )}
          <button type="button" className="btn-link" onClick={() => props.go("advanced:learning")}>
            {t("learn_details")}
          </button>
        </section>
      </div>

      <section className="card" aria-labelledby="last-heading">
        <h2 id="last-heading">{t("last_heading")}</h2>
        {last ? (
          <div className="row spread">
            <div className="stack">
              <p>{last.title}</p>
              <p className="muted small">
                {formatTime(last.entry.time)}
                {last.why && ` · ${last.why}`}
              </p>
            </div>
            {last.undoable && (
              <button type="button" className="btn btn-small" onClick={() => run(() => backend.undo(last.entry.id).then(() => t("undone", { label: last.entry.label })))}>
                {t("undo")}
              </button>
            )}
          </div>
        ) : (
          <p className="muted">{t("last_none")}</p>
        )}
      </section>

      <RestoreDialog open={restoring} onClose={() => setRestoring(false)} onDone={(m) => run(async () => m)} />
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

export function RestoreDialog(props: { open: boolean; onClose: () => void; onDone: (msg: string) => void }) {
  return (
    <Dialog
      open={props.open}
      onClose={props.onClose}
      title={t("restore_title")}
      actions={
        <>
          <button type="button" className="btn" onClick={props.onClose}>{t("cancel")}</button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={async () => {
              props.onClose();
              try {
                const done = await backend.restore();
                props.onDone(done.length ? tn("restore_done", done.length) : t("restore_nothing"));
              } catch (e) {
                props.onDone(String((e as Error)?.message ?? e));
              }
            }}
          >
            {t("restore_confirm")}
          </button>
        </>
      }
    >
      <p>{t("restore_body")}</p>
    </Dialog>
  );
}
