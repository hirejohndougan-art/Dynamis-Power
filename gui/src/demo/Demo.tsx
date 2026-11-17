import { useCallback, useEffect, useRef, useState } from "react";
import { App } from "../App";
import { backend, type Profile, type Status } from "../backend";
import { demoControls } from "../backend/mock";
import { profileName } from "../i18n";

const changed = () => window.dispatchEvent(new Event("dynamis:changed"));

const MODE_COLOUR: Record<string, string> = {
  "power-saver": "#2e7d32",
  balanced: "#ffef00",
  performance: "#d9480f",
  off: "#757575",
};

const BATTERY_FILL: Record<string, string> = {
  "power-saver": "#43a047",
  balanced: "#ffef00",
  performance: "#f57c00",
  off: "#9e9e9e",
};

/** The panel icon as the agent draws it (dynamis-agent/src/tray.rs): a
 * battery filled to the charge in the mode's colour (red at 20% or less),
 * a bolt while charging, and the percentage beside it. */
function TrayBattery(props: { mode: string; pct: number; charging: boolean }) {
  const fill = props.pct <= 20 && !props.charging ? "#e53935" : BATTERY_FILL[props.mode] ?? BATTERY_FILL.off;
  const w = (21.8 * props.pct) / 100;
  return (
    <span className="tray-battery">
      <svg width="26" height="26" viewBox="0 0 32 32" aria-hidden="true">
        <rect x="2.6" y="8.1" width="23.3" height="15.8" rx="2.6" fill="none" stroke="#151515" strokeOpacity="0.55" strokeWidth="4.2" />
        <rect x="2.6" y="8.1" width="23.3" height="15.8" rx="2.6" fill="none" stroke="#f5f5f5" strokeWidth="2.2" />
        <rect x="27" y="12" width="3.5" height="8" rx="1" fill="#f5f5f5" />
        {props.pct > 0 && <rect x="4.9" y="10.4" width={Math.max(1, w)} height="11.2" rx="1" fill={fill} />}
        {props.charging && <path d="M16.5 9.5 10.5 17h4l-1.5 5.5 6.5-8h-4z" fill="#1a1a1a" stroke="#f5f5f5" strokeWidth="0.9" />}
      </svg>
      <span className="tray-pct">{props.pct}%</span>
    </span>
  );
}

/** The same logo the panel icon draws (dynamis-agent/src/tray.rs). */
function TrayLogo(props: { mode: string }) {
  return (
    <svg width="20" height="20" viewBox="0 0 28 28" aria-hidden="true">
      <rect x="1" y="1" width="26" height="26" rx="8" fill={MODE_COLOUR[props.mode] ?? MODE_COLOUR.off} />
      {/* Dark bolt on yellow, white on the darker modes (as the real panel icon). */}
      <path d="M15.5 6 9 15.5h5l-1.5 6.5L19 12.5h-5z" fill={props.mode === "balanced" ? "#1b1d1f" : "#fff"} />
    </svg>
  );
}

function summary(s: Status): string {
  if (s.safe_mode) return "Safe mode is on: nothing is changed";
  const parts = [profileName(s.profile)];
  if (s.power.battery_pct != null) parts.push(`${Math.round(s.power.battery_pct)}%`);
  if (s.power.hours_left != null) {
    const m = Math.round(s.power.hours_left * 60);
    parts.push(`about ${Math.floor(m / 60)} h ${m % 60} min left`);
  }
  if (s.last_until) parts.push("making it last");
  return parts.join(" · ");
}

/** The panel icon's menu, as the panel draws it from the agent's DBusMenu. */
function TrayMenu(props: { status: Status; onOpen: () => void; onClose: () => void }) {
  const s = props.status;
  const locked = !!s.safe_mode;
  const ref = useRef<HTMLDivElement>(null);
  const [lastOpen, setLastOpen] = useState(false);
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, []);
  const act = async (f: () => Promise<unknown>) => {
    props.onClose();
    try {
      await f();
    } catch {
      /* refused (safe mode): the menu simply shows what is true */
    }
    changed();
  };
  const hours = (h: number) => Date.now() / 1000 + h * 3600;
  return (
    <div
      ref={ref}
      className="tray-menu"
      role="menu"
      aria-label="Dynamis Power"
      onKeyDown={(e) => {
        if (e.key === "Escape") props.onClose();
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          const items = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
          const i = items.indexOf(document.activeElement as HTMLButtonElement);
          items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
          e.preventDefault();
        }
      }}
    >
      <div className="tray-summary">{summary(s)}</div>
      <div className="tray-sep" role="separator" />
      {s.profiles.map((p: Profile) => (
        <button key={p} type="button" role="menuitemradio" aria-checked={s.profile === p} disabled={locked} onClick={() => act(() => backend.setProfile(p))}>
          <span className="tray-mark" aria-hidden="true">{s.profile === p ? "●" : ""}</span>
          {profileName(p)}
          {s.last_until && p !== "power-saver" && s.profile !== p && <span className="muted small">&nbsp;(stops Make it last)</span>}
        </button>
      ))}
      <div className="tray-sep" role="separator" />
      <button type="button" role="menuitemcheckbox" aria-checked={s.auto.on} disabled={locked} onClick={() => act(() => backend.setAuto(!s.auto.on))}>
        <span className="tray-mark" aria-hidden="true">{s.auto.on ? "✓" : ""}</span>
        Choose automatically
      </button>
      {s.power.has_battery && (
        <>
          <button type="button" role="menuitem" aria-haspopup="menu" aria-expanded={lastOpen} disabled={locked} onClick={() => setLastOpen(!lastOpen)}>
            <span className="tray-mark" aria-hidden="true" />
            Make it last <span className="tray-more" aria-hidden="true">›</span>
          </button>
          {lastOpen && (
            <div className="tray-sub" role="menu" aria-label="Make it last">
              {s.last_until ? (
                <button type="button" role="menuitem" onClick={() => act(() => backend.setTarget(0))}>Stop making it last</button>
              ) : (
                [2, 4, 6].map((h) => (
                  <button key={h} type="button" role="menuitem" onClick={() => act(() => backend.setTarget(hours(h)))}>
                    {h} hours
                  </button>
                ))
              )}
            </div>
          )}
        </>
      )}
      <button type="button" role="menuitem" disabled={locked} onClick={() => act(() => (s.relief ? backend.endRelief() : backend.feelsSlow(false)))}>
        <span className="tray-mark" aria-hidden="true" />
        {s.relief ? "Back to normal settings" : "It feels slow"}
      </button>
      <button type="button" role="menuitem" onClick={() => { void act(async () => { if (!s.trouble) await backend.somethingBroke(); }); window.location.hash = "#home"; props.onOpen(); }}>
        <span className="tray-mark" aria-hidden="true" />
        Something stopped working…
      </button>
      <button type="button" role="menuitem" onClick={() => { props.onClose(); window.location.hash = "#activity:report"; props.onOpen(); }}>
        <span className="tray-mark" aria-hidden="true" />
        Report a problem…
      </button>
      <div className="tray-sep" role="separator" />
      <button type="button" role="menuitem" onClick={() => { props.onClose(); props.onOpen(); }}>
        <span className="tray-mark" aria-hidden="true" />
        Open Dynamis Power
      </button>
      <button type="button" role="menuitem" onClick={() => act(() => backend.setTray(false))}>
        <span className="tray-mark" aria-hidden="true" />
        Hide this icon
      </button>
    </div>
  );
}

function Clock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 15_000);
    return () => window.clearInterval(id);
  }, []);
  return <span className="panel-clock">{now.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}</span>;
}

export function Demo() {
  const [status, setStatus] = useState<Status | null>(null);
  const [trayShown, setTrayShown] = useState(true);
  const [menuOpen, setMenuOpen] = useState(false);
  const [appOpen, setAppOpen] = useState(true);
  const [run, setRun] = useState(0);
  const iconRef = useRef<HTMLButtonElement>(null);
  const holdRef = useRef<number | undefined>(undefined);

  const read = useCallback(async () => {
    setStatus(await backend.status());
    setTrayShown(await backend.trayShown());
  }, []);
  useEffect(() => {
    read();
    const id = window.setInterval(read, 5000);
    window.addEventListener("dynamis:changed", read);
    return () => {
      window.clearInterval(id);
      window.removeEventListener("dynamis:changed", read);
    };
  }, [read]);
  // The app's own changes (Modes, the panel icon switch) show on the panel too.
  useEffect(() => {
    const id = window.setInterval(read, 1500);
    return () => window.clearInterval(id);
  }, [read]);

  const mode = !status ? "off" : status.safe_mode ? "off" : status.profile;
  const restartWelcome = () => {
    try {
      localStorage.removeItem("dynamis.welcomed");
    } catch {
      /* storage blocked: the app then skips the welcome */
    }
    setAppOpen(true);
    setRun((r) => r + 1);
  };

  return (
    <div className="demo">
      <div className="demo-bar">
        <span>
          <strong>Dynamis Power demo.</strong> The real app with sample data: click anything. Nothing on your computer changes.
        </span>
        <span className="demo-actions">
          <button type="button" className="btn btn-small" onClick={restartWelcome}>Restart first-run welcome</button>
          <button
            type="button"
            className="btn btn-small"
            title="As if the laptop froze and was restarted. Three in a row switch safe mode on."
            onClick={() => {
              demoControls.freeze?.();
              changed();
            }}
          >
            Pretend the laptop froze
          </button>
          <button
            type="button"
            className="btn btn-small"
            title="As if another power tool set the speed preference back. The second time, Dynamis Power leaves the setting to it."
            onClick={() => {
              demoControls.outside?.();
              changed();
            }}
          >
            Pretend another tool changed a setting
          </button>
          <button
            type="button"
            className="btn btn-small"
            title="As if someone used the laptop during Learn now: the session stops."
            onClick={() => {
              demoControls.touch?.();
              changed();
            }}
          >
            Pretend someone touched the laptop
          </button>
          {!appOpen && <button type="button" className="btn btn-small" onClick={() => setAppOpen(true)}>Open the app</button>}
        </span>
      </div>
      <div className="panel" role="toolbar" aria-label="Desktop panel (pretend)">
        <span className="panel-left">Activities</span>
        <span className="panel-right">
          {trayShown && (
            <span className="tray-wrap">
              <button
                ref={iconRef}
                type="button"
                className="tray-icon"
                aria-label={`Dynamis Power: ${status ? summary(status) : "starting"}. Right-click or press the menu key for options.`}
                title={status ? `Dynamis Power\n${summary(status)}` : "Dynamis Power"}
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                onClick={() => setMenuOpen(!menuOpen)}
                // Right button: open on press, as panels do, so a page or
                // browser menu cannot get in first.
                onPointerDown={(e) => {
                  if (e.button === 2) {
                    e.preventDefault();
                    setMenuOpen(true);
                  } else if (e.pointerType === "touch") {
                    // Press and hold on a touch screen.
                    holdRef.current = window.setTimeout(() => setMenuOpen(true), 450);
                  }
                }}
                onPointerUp={() => window.clearTimeout(holdRef.current)}
                onPointerLeave={() => window.clearTimeout(holdRef.current)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setMenuOpen(true);
                }}
              >
                {status?.power.has_battery && status.power.battery_pct != null ? (
                  <TrayBattery mode={mode} pct={Math.round(status.power.battery_pct)} charging={!!status.power.charging} />
                ) : (
                  <TrayLogo mode={mode} />
                )}
              </button>
              {menuOpen && status && (
                <TrayMenu
                  status={status}
                  onOpen={() => setAppOpen(true)}
                  onClose={() => {
                    setMenuOpen(false);
                    iconRef.current?.focus();
                  }}
                />
              )}
            </span>
          )}
          <span className="panel-glyph" aria-hidden="true">▾</span>
          <Clock />
        </span>
      </div>
      {!trayShown && (
        <p className="demo-hint">The panel icon is hidden. Turn it back on in the app: Modes → <em>Show Dynamis Power in the panel</em>.</p>
      )}
      {appOpen ? (
        <div className="window" role="region" aria-label="Dynamis Power window">
          <div className="window-title">
            <span>Dynamis Power</span>
            <button type="button" className="window-close" aria-label="Close the app window" onClick={() => setAppOpen(false)}>×</button>
          </div>
          <div className="window-body">
            <App key={run} />
          </div>
        </div>
      ) : (
        <div className="desktop-empty">
          <p>The app is closed and costs nothing. The service keeps working, and the panel icon stays.</p>
          <p className="muted">Click the icon in the panel to change the mode, or choose <em>Open Dynamis Power</em> from its menu.</p>
        </div>
      )}
    </div>
  );
}
