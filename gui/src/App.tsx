import { lazy, Suspense, useEffect, useState } from "react";
import { backend } from "./backend";
import { Logo } from "./components";
import { useStatus } from "./hooks";
import { t } from "./i18n";
import { Home } from "./screens/Home";
import { TABS, type Tab } from "./screens/advancedTabs";
import { QuickPanel } from "./screens/QuickPanel";
import { Welcome, welcomed } from "./screens/Welcome";

// Only the Dashboard is read when the window opens; every other screen's
// code is loaded the first time it is shown. On a slow laptop that halves
// the script read before the first screen appears.
const Activity = lazy(() => import("./screens/Activity").then((m) => ({ default: m.Activity })));
const Advanced = lazy(() => import("./screens/Advanced").then((m) => ({ default: m.Advanced })));
const Apps = lazy(() => import("./screens/Apps").then((m) => ({ default: m.Apps })));
const Battery = lazy(() => import("./screens/Battery").then((m) => ({ default: m.Battery })));
const Devices = lazy(() => import("./screens/Devices").then((m) => ({ default: m.Devices })));
const Learning = lazy(() => import("./screens/Learning").then((m) => ({ default: m.Learning })));
const Modes = lazy(() => import("./screens/Modes").then((m) => ({ default: m.Modes })));
const Screen = lazy(() => import("./screens/Screen").then((m) => ({ default: m.Screen })));


type Page = "home" | "modes" | "apps" | "battery" | "screen" | "devices" | "learning" | "activity" | "advanced" | "quick";

/** Screen in the URL hash, e.g. #activity or #advanced:learning. */
function fromHash(): [Page, string | undefined] {
  const [s, sub] = window.location.hash.replace("#", "").split(":");
  return [(["home", "modes", "apps", "battery", "screen", "devices", "learning", "activity", "advanced", "quick"].includes(s) ? s : "home") as Page, sub];
}

export function App() {
  const [[screen, sub], setRoute] = useState(fromHash);
  const { status, error, refresh, watts, temps } = useStatus();
  const [showWelcome, setShowWelcome] = useState(() => !welcomed());

  // Started from the panel menu's "Report a problem…": open that dialog.
  useEffect(() => {
    void backend.launchView?.().then((v) => {
      if (v) window.location.hash = v;
    });
  }, []);
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
  const nav: [Page, Parameters<typeof t>[0]][] = [
    ["home", "nav_home"],
    ["modes", "nav_modes"],
    ["apps", "nav_apps"],
    ["battery", "nav_battery"],
    ["screen", "nav_screen"],
    ["devices", "nav_devices"],
    ["learning", "nav_learning"],
    ["activity", "nav_activity"],
  ];

  // The quick panel (a left click on the panel icon): its own small window.
  if (screen === "quick") {
    return <QuickPanel status={status} refresh={refresh} />;
  }

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
        {nav
          // A computer without a battery has no Battery screen (decision 0023).
          .filter(([k]) => k !== "battery" || status?.power.has_battery !== false)
          .map(([k, label]) => (
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
        <Suspense fallback={<p className="muted" role="status">{t("loading")}</p>}>
        {error && !status ? (
          <div className="card" role="alert">
            <h2>{t("error_title")}</h2>
            <p>{error}</p>
            <div><button type="button" className="btn" onClick={refresh}>{t("retry")}</button></div>
          </div>
        ) : !status ? (
          <p className="muted" role="status">{t("loading")}</p>
        ) : screen === "home" || (screen === "battery" && !status.power.has_battery) ? (
          <Home status={status} watts={watts} temps={temps} refresh={refresh} go={go} />
        ) : screen === "modes" ? (
          <Modes status={status} refresh={refresh} />
        ) : screen === "apps" ? (
          <Apps status={status} refresh={refresh} />
        ) : screen === "battery" && status.power.has_battery ? (
          <Battery status={status} refresh={refresh} go={go} />
        ) : screen === "screen" ? (
          <Screen status={status} refresh={refresh} />
        ) : screen === "devices" ? (
          <Devices status={status} refresh={refresh} />
        ) : screen === "learning" ? (
          <Learning status={status} refresh={refresh} />
        ) : screen === "activity" ? (
          <Activity key={sub} version={status.version} onChange={refresh} report={sub === "report"} />
        ) : (
          <Advanced key={sub} status={status} refresh={refresh} initialTab={TABS.includes(sub as Tab) ? (sub as Tab) : "settings"} />
        )}
        </Suspense>
      </main>
    </div>
  );
}
