import { useEffect, useState } from "react";
import { Logo } from "./components";
import { useStatus } from "./hooks";
import { t } from "./i18n";
import { Activity } from "./screens/Activity";
import { Advanced, TABS, type Tab } from "./screens/Advanced";
import { Apps } from "./screens/Apps";
import { Battery } from "./screens/Battery";
import { Home } from "./screens/Home";
import { Modes } from "./screens/Modes";
import { Welcome, welcomed } from "./screens/Welcome";

type Screen = "home" | "modes" | "apps" | "battery" | "activity" | "advanced";

/** Screen in the URL hash, e.g. #activity or #advanced:learning. */
function fromHash(): [Screen, string | undefined] {
  const [s, sub] = window.location.hash.replace("#", "").split(":");
  return [(["home", "modes", "apps", "battery", "activity", "advanced"].includes(s) ? s : "home") as Screen, sub];
}

export function App() {
  const [[screen, sub], setRoute] = useState(fromHash);
  const { status, error, refresh, watts } = useStatus();
  const [showWelcome, setShowWelcome] = useState(() => !welcomed());

  useEffect(() => {
    const on = () => setRoute(fromHash());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  useEffect(() => {
    document.title = `${t(`nav_${screen}`)} – ${t("app_name")}`;
    document.querySelector("main")?.focus();
  }, [screen]);

  const go = (to: string) => {
    window.location.hash = to;
  };
  const nav: [Screen, Parameters<typeof t>[0]][] = [
    ["home", "nav_home"],
    ["modes", "nav_modes"],
    ["apps", "nav_apps"],
    ["battery", "nav_battery"],
    ["activity", "nav_activity"],
  ];

  if (showWelcome && status) {
    return <Welcome status={status} onDone={() => { setShowWelcome(false); refresh(); }} />;
  }

  return (
    <div className="shell">
      <nav className="sidebar" aria-label={t("nav_main")}>
        <div className="brand">
          <Logo />
          <span>{t("app_name")}</span>
        </div>
        {nav.map(([k, label]) => (
          <a key={k} href={`#${k}`} className="nav-link" aria-current={screen === k ? "page" : undefined}>
            {t(label)}
          </a>
        ))}
        <div className="nav-sep" />
        <a href="#advanced" className="nav-link" aria-current={screen === "advanced" ? "page" : undefined}>
          {t("nav_advanced")}
        </a>
      </nav>
      <main className="main" tabIndex={-1}>
        {error && !status ? (
          <div className="card" role="alert">
            <h2>{t("error_title")}</h2>
            <p>{error}</p>
            <div><button type="button" className="btn" onClick={refresh}>{t("retry")}</button></div>
          </div>
        ) : !status ? (
          <p className="muted" role="status">{t("loading")}</p>
        ) : screen === "home" ? (
          <Home status={status} watts={watts} refresh={refresh} go={go} />
        ) : screen === "modes" ? (
          <Modes status={status} refresh={refresh} />
        ) : screen === "apps" ? (
          <Apps status={status} refresh={refresh} />
        ) : screen === "battery" ? (
          <Battery status={status} refresh={refresh} />
        ) : screen === "activity" ? (
          <Activity version={status.version} onChange={refresh} />
        ) : (
          <Advanced key={sub} status={status} refresh={refresh} initialTab={TABS.includes(sub as Tab) ? (sub as Tab) : "settings"} />
        )}
      </main>
    </div>
  );
}
