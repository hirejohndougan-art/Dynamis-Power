import { useState } from "react";
import { backend, type Profile, type Status } from "../backend";
import { Dialog, PowerChart, ProgressBar, Switch } from "../components";
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

const DISMISSED = "dynamis.unclean-seen";

/** After an unclean shutdown: what was happening, in plain words. */
function UncleanNotice({ s }: { s: Status }) {
  const last = s.unclean?.last;
  const [seen, setSeen] = useState<number | null>(() => {
    try {
      return Number(localStorage.getItem(DISMISSED)) || null;
    } catch {
      return null;
    }
  });
  if (!last || seen === last.found_at || Date.now() / 1000 - last.found_at > 3 * 86400) return null;
  const names = (l: { label: string; id: string }[]) => l.map((x) => x.label || x.id).join(", ");
  const dismiss = () => {
    try {
      localStorage.setItem(DISMISSED, String(last.found_at));
    } catch {
      /* per-viewer convenience only */
    }
    setSeen(last.found_at);
  };
  return (
    <div className="notice notice-info" role="status" style={{ flexDirection: "column", alignItems: "flex-start", gap: 4 }}>
      <strong>{t("unclean_heading")}</strong>
      <span>{t(`unclean_kernel_${last.kernel}` as "unclean_kernel_nothing")}</span>
      {last.in_safe_mode ? (
        <span>{t("unclean_in_safe_mode")}</span>
      ) : (
        <>
          {last.in_progress && <span>{t("unclean_in_progress", { label: last.in_progress.label || last.in_progress.id })}</span>}
          {last.recent.length > 0 ? (
            <span>{t("unclean_recent", { labels: names(last.recent) })}</span>
          ) : (
            last.ended_at != null && !last.in_progress && <span>{t("unclean_none_recent")}</span>
          )}
        </>
      )}
      {last.detail && (
        <details>
          <summary>{t("details")}</summary>
          <code className="small">{last.detail}</code>
        </details>
      )}
      <button type="button" className="btn btn-small" onClick={dismiss}>
        {t("got_it")}
      </button>
    </div>
  );
}

/** Power use now, with a chart of the last half hour (readings the app has
 * seen since it was opened, every 5 seconds). */
function PowerCard(props: { s: Status; watts: number[] }) {
  const { s, watts } = props;
  const now = s.power.on_ac ? null : s.power.watts;
  const avg = watts.length ? watts.reduce((a, b) => a + b, 0) / watts.length : null;
  const minutes = Math.max(1, Math.round(((watts.length - 1) * 5) / 60));
  return (
    <section className="card" aria-labelledby="draw-heading">
      <h2 id="draw-heading">{t("draw_heading")}</h2>
      <div className="stat-row">
        {now != null ? <p className="big">{t("draw_watts", { w: now.toFixed(1) })}</p> : <p className="muted">{t("draw_none")}</p>}
        {now != null && avg != null && watts.length > 1 && (
          <p className="muted">{t("draw_avg", { w: avg.toFixed(1), m: minutes })}</p>
        )}
      </div>
      <PowerChart
        values={s.power.on_ac ? [] : watts}
        stepS={5}
        label={
          watts.length > 1
            ? t("draw_chart_label", { m: minutes, min: Math.min(...watts).toFixed(1), max: Math.max(...watts).toFixed(1), now: (now ?? watts[watts.length - 1]).toFixed(1) })
            : t("draw_chart_empty")
        }
        empty={s.power.on_ac ? t("draw_chart_plugged") : t("draw_chart_empty")}
      />
    </section>
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

      <UncleanNotice s={s} />
      {s.safe_mode && (
        <div className="notice" role="alert">
          <div className="stack" style={{ gap: 4 }}>
            <span>{t("safe_mode_on", { why: s.safe_mode })}</span>
            {(s.unclean?.in_safe_mode ?? 0) > 0 ? (
              <span>
                {s.unclean!.in_safe_mode >= 2
                  ? t("safe_mode_still_froze", { n: s.unclean!.in_safe_mode })
                  : t("safe_mode_froze_once")}
              </span>
            ) : (
              (s.unclean?.suspects ?? []).length > 0 && (
                <span>{t("safe_mode_suspects", { labels: s.unclean!.suspects.map((x) => x.label || x.id).join(", ") })}</span>
              )
            )}
          </div>
          <button type="button" className="btn btn-small" onClick={() => run(async () => { await backend.setSafeMode(false); })}>
            {t("safe_mode_off_button")}
          </button>
        </div>
      )}
      {s.pending?.trial && <div className="notice notice-info">{t("trial_running", { label: s.pending.label })}</div>}
      {(s.kept?.held_back ?? []).length > 0 && (
        <div className="notice">
          {t("kept_held_back", { labels: (s.kept?.held_back ?? []).map((id) => s.kept?.settings.find((k) => k.id === id)?.label || id).join(", ") })}
        </div>
      )}
      {s.dry_run && (
        <div className="notice notice-info">
          {t("dry_run_on")} <a href="#advanced:developer">{t("tab_developer")}</a>
        </div>
      )}

      <PowerCard s={s} watts={props.watts} />

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
        <section className="card" aria-labelledby="learn-heading">
          <div className="row spread">
            <h2 id="learn-heading">{t("learn_heading")}</h2>
            <Switch checked={s.learning.on} label={t("learn_switch")} onChange={(on) => run(() => backend.setLearning(on))} />
          </div>
          {s.learning.progress && (
            <>
              <ProgressBar pct={s.learning.progress.pct} />
              <p className="small">{t("learn_pct", { pct: s.learning.progress.pct })}</p>
            </>
          )}
          {s.learning.on ? (
            <>
              <p>{tn("learn_progress", s.learning.tested_settings, { checked: s.learning.tested_settings, kept: s.learning.kept.length })}</p>
              {s.learning.saves_w > 0 && <p>{t("learn_saves", { w: s.learning.saves_w.toFixed(2), min: savedMinutes })}</p>}
              <p className="muted small">{s.learning.status}</p>
            </>
          ) : (
            <p className="muted">{t("learn_off")}</p>
          )}
          <button type="button" className="btn-link" onClick={() => props.go("learning")}>
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
