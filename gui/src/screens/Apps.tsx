import { useState } from "react";
import { backend, type AppsInfo, type Profile, type Status } from "../backend";
import { appName, bars, shareOf, withRule, withShare, type Share } from "../apps";
import { Choice, Dialog, Switch } from "../components";
import { useAction, useLoad } from "../hooks";
import { duration, profileName, t } from "../i18n";

export function Apps(props: { status: Status; refresh: () => void }) {
  const s = props.status;
  const info = useLoad(() => backend.apps(), []);
  const [period, setPeriod] = useState<"today" | "week">("today");
  const [editing, setEditing] = useState<string | null>(null);
  const { run, toast } = useAction(() => {
    info.reload();
    props.refresh();
  });
  const a = info.data;
  const locked = !!s.safe_mode;

  const setPolicy = (change: Partial<AppsInfo["policy"]>) => a && run(() => backend.setAppPolicy({ ...a.policy, ...change }));

  return (
    <>
      <header className="page-head">
        <h1>{t("apps_title")}</h1>
      </header>
      {info.error && !a && <p role="alert">{info.error}</p>}
      {!a ? (
        !info.error && <p className="muted" role="status">{t("loading")}</p>
      ) : (
        <>
          <section className="card" aria-labelledby="use-heading">
            <div className="row spread">
              <h2 id="use-heading">{t("apps_use_heading")}</h2>
              <div className="chips" role="group" aria-label={t("apps_use_heading")}>
                {(["today", "week"] as const).map((p) => (
                  <button key={p} type="button" className="chip" aria-pressed={period === p} onClick={() => setPeriod(p)}>
                    {t(p === "today" ? "apps_today" : "apps_week")}
                  </button>
                ))}
              </div>
            </div>
            <p className="muted small">{t("apps_estimate_note")}</p>
            {!a.policy.track ? (
              <p className="muted">{t("apps_tracking_off")}</p>
            ) : a[period].length === 0 ? (
              <p className="muted">{t("apps_none")}</p>
            ) : (
              <ul className="entries">
                {bars(a[period]).map(({ row, pct, byEnergy }) => {
                  const rule = a.rules.find((r) => r.app === row.app);
                  const share = shareOf(a.policy, row.app);
                  const live = a.now.find((n) => n.app === row.app)?.share;
                  const tags = [
                    row.app === a.front ? t("apps_in_front") : a.running.includes(row.app) ? t("apps_open") : null,
                    rule ? t("apps_mode_rule", { mode: profileName(rule.profile) }) : null,
                    live != null && live < 100 ? t("apps_slowed") : live != null && live > 100 ? t("apps_priority") : null,
                  ].filter(Boolean);
                  return (
                    <li key={row.app} className="entry app-row">
                      <div className="stack" style={{ flex: 1, gap: 4 }}>
                        <div className="row spread">
                          <strong>{appName(row.app)}</strong>
                          <span className="small">
                            {byEnergy ? t("apps_wh", { wh: row.wh.toFixed(1) }) : t("apps_cpu_only", { time: duration(row.cpu_s / 3600) })}
                          </span>
                        </div>
                        <div className="bar" aria-hidden="true">
                          <span style={{ width: `${Math.max(pct, 2)}%` }} />
                        </div>
                        {tags.length > 0 && <span className="muted small">{tags.join(" · ")}</span>}
                      </div>
                      {row.app !== "System" && (
                        <button
                          type="button"
                          className="btn btn-small"
                          aria-label={`${t("apps_choose")}: ${appName(row.app)}`}
                          data-share={share}
                          onClick={() => setEditing(row.app)}
                        >
                          {t("apps_choose")}
                        </button>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
            {a.policy.track &&
              (a.trends ?? []).map((tr) => (
                <p key={tr.app} className="notice notice-info small">
                  {t(tr.change_pct > 0 ? "apps_trend_more" : "apps_trend_less", {
                    app: appName(tr.app),
                    pct: Math.round(Math.abs(tr.change_pct)),
                    from: tr.last_w.toFixed(1),
                    to: tr.this_w.toFixed(1),
                  })}
                </p>
              ))}
          </section>

          <section className="card" aria-labelledby="sharing-heading">
            <h2 id="sharing-heading">{t("apps_sharing_heading")}</h2>
            <p className="muted">{t("apps_sharing_intro")}</p>
            <div className="stack">
              <Switch checked={a.policy.slow_background} disabled={locked} label={t("apps_slow_background")} onChange={(v) => setPolicy({ slow_background: v })} />
              <Switch checked={a.policy.boost_front} disabled={locked} label={t("apps_boost_front")} onChange={(v) => setPolicy({ boost_front: v })} />
              <Switch checked={a.policy.track} label={t("apps_track")} onChange={(v) => setPolicy({ track: v })} />
            </div>
            {(a.policy.slow_background || a.policy.boost_front) && !a.front && (
              <p className="notice notice-info">{t("apps_front_unknown")}</p>
            )}
            <div>
              <button type="button" className="btn-link" onClick={() => run(() => backend.forgetAppUsage().then(() => t("apps_forgotten")))}>
                {t("apps_forget")}
              </button>
            </div>
          </section>

          {editing && (
            <AppOptions
              key={editing}
              app={editing}
              info={a}
              profiles={s.profiles}
              disabled={locked}
              onClose={() => setEditing(null)}
              onSave={(share, mode) =>
                run(async () => {
                  setEditing(null);
                  await backend.setAppPolicy(withShare(a.policy, editing, share));
                  const prefs = await backend.autoPrefs();
                  await backend.setAutoPrefs({ ...prefs, app_rules: withRule(prefs.app_rules ?? [], editing, mode) });
                  return t("apps_saved");
                })
              }
            />
          )}
        </>
      )}
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

function AppOptions(props: {
  app: string;
  info: AppsInfo;
  profiles: Profile[];
  disabled: boolean;
  onClose: () => void;
  onSave: (share: Share, mode: Profile | null) => void;
}) {
  const [share, setShare] = useState<Share>(shareOf(props.info.policy, props.app));
  const [mode, setMode] = useState<Profile | "">(props.info.rules.find((r) => r.app === props.app)?.profile ?? "");
  const options: [Share, Parameters<typeof t>[0]][] = [
    ["normal", "apps_share_normal"],
    ["never", "apps_share_never"],
    ["always", "apps_share_always"],
  ];
  return (
    <Dialog
      open
      onClose={props.onClose}
      title={appName(props.app)}
      actions={
        <>
          <button type="button" className="btn" onClick={props.onClose}>{t("cancel")}</button>
          <button type="button" className="btn btn-primary" disabled={props.disabled} onClick={() => props.onSave(share, mode || null)}>
            {t("modes_save")}
          </button>
        </>
      }
    >
      <fieldset className="field">
        <legend>{t("apps_share_label")}</legend>
        {options.map(([v, label]) => (
          <label key={v} className="check">
            <input type="radio" name="share" value={v} checked={share === v} onChange={() => setShare(v)} />
            {t(label)}
          </label>
        ))}
        {share === "always" && <p className="muted small">{t("apps_share_always_note")}</p>}
      </fieldset>
      <Choice
        name="app-mode"
        label={t("apps_mode_label")}
        value={mode}
        options={[["", t("apps_mode_none")] as [Profile | "", string], ...props.profiles.map((p): [Profile | "", string] => [p, profileName(p)])]}
        onChange={(v) => setMode(v)}
      />
    </Dialog>
  );
}
