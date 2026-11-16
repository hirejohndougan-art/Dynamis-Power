import { backend, type Status } from "../backend";
import { ProgressBar, Switch } from "../components";
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
      </section>

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
