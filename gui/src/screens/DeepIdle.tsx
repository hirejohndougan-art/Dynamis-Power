import { useEffect } from "react";
import { backend, type IdleBlocker, type Status } from "../backend";
import { ProgressBar } from "../components";
import { formatTime, t } from "../i18n";

/** "Check deep idle": one quiet, read-only minute, then how deeply the
 * processor rested and which devices may be keeping it awake. */
export function DeepIdleCheck(props: { status: Status; refresh: () => void; run: (a: () => Promise<string | void>) => void }) {
  const d = props.status.deep_idle;
  const running = !!d?.running;
  const { refresh } = props;
  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(refresh, 2000);
    return () => window.clearInterval(id);
  }, [running, refresh]);
  const r = d?.report;
  const name = (b: IdleBlocker) => b.name || t(`dev_what_${b.what}` as "dev_what_other");
  const list = (items: IdleBlocker[], heading: string, hint: string, href: string) =>
    items.length > 0 && (
      <div className="stack" style={{ gap: 2 }}>
        <strong>{heading}</strong>
        <ul className="plain-list small">
          {items.map((b) => (
            <li key={b.id}>{name(b)}</li>
          ))}
        </ul>
        <p className="muted small">
          {hint} <a className="btn-link" href={href}>{t("idle_open")}</a>
        </p>
      </div>
    );
  return (
    <section className="card" aria-labelledby="idle-heading">
      <h2 id="idle-heading">{t("idle_heading")}</h2>
      <p className="muted">{t("idle_intro")}</p>
      {running ? (
        <div className="stack" role="status" style={{ gap: 4 }}>
          <p>{t("idle_running", { s: d?.seconds_left ?? 60 })}</p>
          <ProgressBar pct={Math.round(((60 - (d?.seconds_left ?? 60)) / 60) * 100)} />
        </div>
      ) : (
        <div>
          <button type="button" className="btn" disabled={!!props.status.safe_mode} onClick={() => props.run(() => backend.checkDeepIdle())}>
            {t(r ? "idle_again" : "idle_start")}
          </button>
        </div>
      )}
      {r && !running && (
        <div className="stack" style={{ gap: 8 }} aria-label={t("idle_result")}>
          {r.core_deepest_pct == null ? (
            <p>{t("idle_unknown")}</p>
          ) : (
            <p role="note">
              {t(r.core_deepest_pct >= 80 ? "idle_good" : "idle_low", { pct: Math.round(r.core_deepest_pct) })}
              {r.package_pct != null && ` ${t("idle_package", { pct: Math.round(r.package_pct) })}`}
            </p>
          )}
          {r.touched && <p className="notice notice-info small">{t("idle_touched")}</p>}
          {list(r.held_awake, t("idle_held_awake"), t("idle_held_awake_hint"), "#devices")}
          {list(r.no_deeper_link_sleep, t("idle_no_deeper"), t("idle_no_deeper_hint"), "#devices")}
          {list(r.no_link_sleep, t("idle_no_link"), t("idle_no_link_hint"), "#advanced:settings")}
          {r.held_awake.length + r.no_deeper_link_sleep.length + r.no_link_sleep.length === 0 && <p className="muted">{t("idle_nothing")}</p>}
          <p className="muted small">{t("idle_maybe")}</p>
          <details>
            <summary className="small">{t("details")}</summary>
            <p className="muted small">
              {d?.at ? formatTime(d.at) : ""}
              {r.deepest && ` · ${t("idle_deepest_state", { name: r.deepest })}`}
              {r.platform_pct != null && ` · ${t("idle_platform", { pct: Math.round(r.platform_pct) })}`}
            </p>
          </details>
        </div>
      )}
    </section>
  );
}
