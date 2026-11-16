import { useEffect, useState } from "react";
import { backend, type SleepTestInfo } from "../backend";
import { Choice, Dialog } from "../components";
import { useLoad } from "../hooks";
import { t } from "../i18n";

const sleepName = (kind: string | null) => (kind ? (kind === "s2idle" || kind === "deep" ? t(`sleep_kind_${kind}`) : kind) : t("unknown"));

/** The guided sleep test: which kind of sleep uses least and wakes cleanly. */
export function SleepTest(props: { locked: boolean; run: (a: () => Promise<string | void>) => void }) {
  const info = useLoad<SleepTestInfo>(() => backend.sleepTest(), []);
  const [asking, setAsking] = useState(false);
  const [minutes, setMinutes] = useState(30);
  const d = info.data;
  const running = !!d && ["ready", "sleeping", "settling"].includes(d.phase);
  // While it runs, look again every few seconds (the app may be open as the
  // laptop wakes between sleeps).
  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(info.reload, 3000);
    return () => window.clearInterval(id);
  }, [running, info.reload]);
  useEffect(() => {
    const again = () => info.reload();
    window.addEventListener("dynamis:changed", again);
    return () => window.removeEventListener("dynamis:changed", again);
  }, [info.reload]);
  if (!d || !d.alarm || d.offered.length === 0) return null;
  const types = Math.max(d.offered.length, 1);
  const act = (a: () => Promise<void>) =>
    props.run(async () => {
      await a();
      info.reload();
    });
  return (
    <section className="card" aria-labelledby="sleep-heading">
      <h2 id="sleep-heading">{t("sleep_heading")}</h2>
      <p className="muted">{t("sleep_intro")}</p>
      <p>{t("sleep_now", { kind: sleepName(d.active) })}</p>
      {running ? (
        <div className="notice notice-info">
          <span>
            {d.phase === "sleeping"
              ? t("sleep_running", { kind: sleepName(d.current) })
              : d.phase === "settling"
                ? t("sleep_settling", { kind: sleepName(d.current) })
                : d.note ?? t("sleep_next")}
          </span>
          <button type="button" className="btn btn-small" onClick={() => act(() => backend.stopSleepTest())}>
            {t("sleep_stop")}
          </button>
        </div>
      ) : (
        <button type="button" className="btn" disabled={props.locked} onClick={() => setAsking(true)}>
          {d.results.length ? t("sleep_again") : t("sleep_start")}
        </button>
      )}
      {d.results.length > 0 && (
        <ul className="entries" aria-label={t("sleep_results")}>
          {d.results.map((r, i) => (
            <li key={i} className="entry">
              <div className="stack" style={{ gap: 2 }}>
                <strong>{sleepName(r.kind)}</strong>
                <span className="muted small">
                  {r.pct_per_h != null ? t("sleep_used", { pct: r.pct_per_h, wh: r.wh_per_h ?? "?" }) : t("sleep_not_measured")}
                  {r.invalid && ` · ${t("sleep_invalid", { why: r.invalid })}`}
                </span>
                {r.problems.length > 0 && (
                  <details>
                    <summary>{t("details")}</summary>
                    <ul className="small">
                      {r.problems.map((p) => (
                        <li key={p}>{p}</li>
                      ))}
                    </ul>
                  </details>
                )}
              </div>
              <span className={`badge ${r.clean ? "badge-safe" : "badge-risk"}`}>{r.clean ? t("sleep_clean") : t("sleep_not_clean")}</span>
            </li>
          ))}
        </ul>
      )}
      {!running && d.phase === "done" && d.note && <p className="muted small">{t("sleep_ended", { why: d.note })}</p>}
      {!running && d.recommended && (
        d.recommended === d.active ? (
          <p>{t("sleep_best_already", { kind: sleepName(d.recommended) })}</p>
        ) : (
          <div className="row">
            <span>{t("sleep_recommend", { kind: sleepName(d.recommended) })}</span>
            <button
              type="button"
              className="btn btn-primary"
              disabled={props.locked}
              onClick={() =>
                act(async () => {
                  await backend.setSetting("system.mem_sleep", d.recommended!, true);
                  await backend.setKept("system.mem_sleep", true);
                })
              }
            >
              {t("sleep_use", { kind: sleepName(d.recommended) })}
            </button>
          </div>
        )
      )}
      {d.hibernate && (
        <div className="stack" style={{ gap: 4 }}>
          <h3>{t("hib_heading")}</h3>
          {d.hibernate.ready ? (
            <p className="small">{t("hib_ready")}</p>
          ) : (
            <>
              <p className="small muted">{t("hib_not_ready")}</p>
              <ul className="small muted">
                {d.hibernate.missing.map((m) => (
                  <li key={m}>{t(`hib_missing_${m}`)}</li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
      <Dialog
        open={asking}
        onClose={() => setAsking(false)}
        title={t("sleep_confirm_title")}
        actions={
          <>
            <button type="button" className="btn" onClick={() => setAsking(false)}>{t("cancel")}</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => {
                setAsking(false);
                act(() => backend.startSleepTest(minutes));
              }}
            >
              {t("sleep_confirm")}
            </button>
          </>
        }
      >
        <p>{t("sleep_confirm_desc", { total: Math.round((minutes * types) / 6) / 10 })}</p>
        <Choice
          name="sleep-minutes"
          label={t("sleep_minutes")}
          value={minutes}
          options={[
            [15, t("sleep_minutes_n", { n: 15 })],
            [30, t("sleep_minutes_n", { n: 30 })],
            [60, t("sleep_minutes_n", { n: 60 })],
          ]}
          onChange={setMinutes}
        />
      </Dialog>
    </section>
  );
}

