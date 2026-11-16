import { useCallback, useEffect, useRef, useState } from "react";
import { backend, type Status } from "./backend";

/** Load something once (and again on reload()). */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    load().then(
      (d) => { setData(d); setError(null); },
      (e) => setError(String(e?.message ?? e)),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(reload, [reload]);
  return { data, error, reload };
}

/** The service's status, refreshed every few seconds while the window is
 * visible. Nothing runs when the window is hidden or the app is closed. */
export function useStatus(intervalMs = 5000) {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [watts, setWatts] = useState<number[]>([]);
  const timer = useRef<number | undefined>(undefined);

  const refresh = useCallback(async () => {
    try {
      const s = await backend.status();
      setStatus(s);
      setError(null);
      if (s.power.watts != null && !s.power.on_ac) {
        setWatts((w) => [...w.slice(-359), s.power.watts as number]);
      }
    } catch (e) {
      setError(String((e as Error)?.message ?? e));
    }
  }, []);

  useEffect(() => {
    const tick = () => {
      if (document.visibilityState === "visible") refresh();
    };
    tick();
    timer.current = window.setInterval(tick, intervalMs);
    document.addEventListener("visibilitychange", tick);
    // Something outside this window changed the service (the demo's panel
    // icon): read again at once instead of waiting for the next tick.
    window.addEventListener("dynamis:changed", tick);
    return () => {
      window.clearInterval(timer.current);
      document.removeEventListener("visibilitychange", tick);
      window.removeEventListener("dynamis:changed", tick);
    };
  }, [refresh, intervalMs]);

  return { status, error, refresh, watts };
}

/** Run an action, show its result or error briefly. */
export function useAction(onDone?: () => void) {
  const [toast, setToast] = useState<string | null>(null);
  const run = useCallback(
    async (action: () => Promise<string | void>) => {
      try {
        const msg = await action();
        if (msg) setToast(msg);
        onDone?.();
      } catch (e) {
        setToast(String((e as Error)?.message ?? e));
      }
      window.setTimeout(() => setToast(null), 4000);
    },
    [onDone],
  );
  return { run, toast };
}
