import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../App";
import { backend } from "../backend";
import { demoControls } from "../backend/mock";

// jsdom has no modal dialogs; open/close by attribute instead.
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); this.dispatchEvent(new Event("close")); };
});
beforeEach(() => {
  window.location.hash = "";
  localStorage.setItem("dynamis.welcomed", "1");
});

describe("the app with the stand-in backend", () => {
  it("shows the battery estimate and the current mode", async () => {
    render(<App />);
    expect(await screen.findByText(/^About .* left$/)).toBeTruthy();
    const modes = screen.getByRole("radiogroup");
    expect(within(modes).getByRole("radio", { name: /Balanced/ }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("switch", { name: "Choose automatically" }).getAttribute("aria-checked")).toBe("true");
  });

  it("switches mode and can undo the last change", async () => {
    render(<App />);
    const faster = await screen.findByRole("radio", { name: /Faster/ });
    await act(async () => fireEvent.click(faster));
    await waitFor(() => expect(screen.getByRole("radio", { name: /Faster/ }).getAttribute("aria-checked")).toBe("true"));
    await waitFor(() => expect(screen.getByText("CPU speed preference: leaning to speed → full speed")).toBeTruthy());
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Undo" })));
    await waitFor(() => expect(screen.getByRole("status").textContent).toMatch(/CPU speed preference is back to its original value/));
  });

  it("asks before restoring original settings", async () => {
    render(<App />);
    const el1 = await screen.findByRole("button", { name: "Restore original settings" });
    await act(async () => fireEvent.click(el1));
    const dialog = screen.getByRole("dialog", { hidden: true });
    expect(within(dialog).getByText(/Every setting Dynamis Power changed/)).toBeTruthy();
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Restore", hidden: true })));
    await waitFor(() => expect(screen.getByRole("switch", { name: "Choose automatically" }).getAttribute("aria-checked")).toBe("false"));
  });

  it("filters activity and opens the export dialog with a preview", async () => {
    window.location.hash = "#activity";
    render(<App />);
    const el2 = await screen.findByRole("button", { name: "Tests" });
    await act(async () => fireEvent.click(el2));
    expect(await screen.findByText(/Kept Watchdog wakeups at off/)).toBeTruthy();
    expect(screen.queryByText(/Automatic: On battery/)).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Export logs" })));
    await waitFor(() => expect(document.querySelector("pre.preview")?.textContent).toContain('"journal"'));
  });

  it("shows advanced settings with risk and per-laptop restrictions", async () => {
    window.location.hash = "#advanced";
    render(<App />);
    expect(await screen.findByText("These controls are for experienced users. Everything here can be undone.")).toBeTruthy();
    await waitFor(() => expect(screen.getByText("Watchdog wakeups")).toBeTruthy());
    expect(screen.getByText(/Not allowed here: on/)).toBeTruthy();
    // Advanced always shows the details: no toggle.
    expect(screen.getByText("kernel.nmi_watchdog")).toBeTruthy();
    expect(screen.queryByRole("switch", { name: "Details" })).toBeNull();
  });

  it("blocks changes in safe mode and says so", async () => {
    window.location.hash = "#advanced:safety";
    render(<App />);
    const el3 = await screen.findByRole("switch", { name: "Safe mode" });
    await act(async () => fireEvent.click(el3));
    window.location.hash = "#home";
    await act(async () => window.dispatchEvent(new HashChangeEvent("hashchange")));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toMatch(/Safe mode is on/), { timeout: 7000 });
    expect((screen.getByRole("radio", { name: /Faster/ }) as HTMLButtonElement).disabled).toBe(true);
    // Leave the shared stand-in backend as we found it.
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Switch safe mode off" })));
  }, 10000);

  it("welcomes a first-time user and changes nothing until Start", async () => {
    localStorage.removeItem("dynamis.welcomed");
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Welcome to Dynamis Power" })).toBeTruthy();
    expect(screen.getByText("Nothing changes until you press Start.")).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Next" })));
    expect(screen.getByRole("heading", { name: "Checking this laptop" })).toBeTruthy();
    await waitFor(() => expect(screen.getByText(/Synthetic Laptop 14/)).toBeTruthy());
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Next" })));
    const off = await screen.findByRole("button", { name: "Switch off: TLP" });
    await act(async () => fireEvent.click(off));
    await waitFor(() => expect(screen.getByText("No other power tools found. Good.")).toBeTruthy());
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Next" })));
    const learn = screen.getByRole("switch", { name: "Learn" });
    await act(async () => fireEvent.click(learn));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Start" })));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Dashboard" })).toBeTruthy());
    expect(localStorage.getItem("dynamis.welcomed")).toBe("1");
    expect(screen.getByRole("switch", { name: "Learn" }).getAttribute("aria-checked")).toBe("false");
  });

  it("saves auto mode preferences and explains a refusal", async () => {
    window.location.hash = "#modes";
    render(<App />);
    // All three modes are visible choices, not a drop-down.
    const battery = await screen.findByRole("group", { name: "On battery" });
    expect(within(battery).getAllByRole("radio").map((r) => r.parentElement?.textContent)).toEqual(["Longer battery", "Balanced", "Faster"]);
    await act(async () => fireEvent.click(within(battery).getByRole("radio", { name: "Longer battery" })));
    expect(within(battery).getByRole("radio", { name: "Longer battery" })).toHaveProperty("checked", true);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Saved."));
  });

  it("shows battery use by app and saves per-app options", async () => {
    window.location.hash = "#apps";
    render(<App />);
    expect(await screen.findByText("About 4.1 Wh")).toBeTruthy();
    expect(screen.getByText(/In front/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Options: Thunderbird" })));
    const dialog = screen.getByRole("dialog", { hidden: true });
    await act(async () => fireEvent.click(within(dialog).getByRole("radio", { name: "Always slow it down", hidden: true })));
    await act(async () => fireEvent.click(within(dialog).getByRole("radio", { name: "Longer battery", hidden: true })));
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Save", hidden: true })));
    await waitFor(() => expect(screen.getByText(/Uses Longer battery while open/)).toBeTruthy());
    expect(screen.getByText(/Slowed down/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: "Give the app in front priority" })));
    await waitFor(() => expect(screen.getByText(/Priority/)).toBeTruthy());
  });

  it("makes the battery last until a time, and follows the charging routine", async () => {
    window.location.hash = "#battery";
    render(<App />);
    expect(await screen.findByText(/The battery should last until about/)).toBeTruthy();
    expect(screen.getByText(/You usually unplug around/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "4 hours" })));
    await waitFor(() => expect(screen.getByText(/^Lasting until/)).toBeTruthy());
    expect(screen.getByText(/Full speed is off/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: "Follow my routine" })));
    await waitFor(() => expect(screen.getByText(/Holding at 80%/)).toBeTruthy());
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop" })));
    await waitFor(() => expect(screen.queryByText(/^Lasting until/)).toBeNull());
  });

  it("has every Advanced tab, and the Developer tools work", async () => {
    window.location.hash = "#advanced:developer";
    render(<App />);
    for (const name of ["Settings", "Learning", "Profiles", "Sensors", "Safety", "System", "Developer"]) {
      expect(await screen.findByRole("tab", { name })).toBeTruthy();
    }
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "As TLP" })));
    await waitFor(() => expect(screen.getByLabelText("Configuration for another tool").textContent).toMatch(/NMI_WATCHDOG=0/));
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: /Dry run/ })));
    await waitFor(() => expect(screen.getByRole("switch", { name: /Dry run/ }).getAttribute("aria-checked")).toBe("true"));
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: /Dry run/ })));

    await act(async () => fireEvent.click(screen.getByRole("tab", { name: "Profiles" })));
    const balanced = await screen.findByRole("group", { name: "Balanced" });
    await act(async () => fireEvent.click(within(within(balanced).getByRole("group")).getAllByRole("radio")[2]));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })));
    await waitFor(() => expect(screen.getByRole("status").textContent).toMatch(/Profile settings saved/));

    await act(async () => fireEvent.click(screen.getByRole("tab", { name: "System" })));
    expect(await screen.findByText(/Example Linux/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("tab", { name: "Sensors" })));
    expect(await screen.findByText(/Whole laptop/)).toBeTruthy();
  });

  it("has Learning in the main menu, with progress as a percentage", async () => {
    render(<App />);
    const link = await screen.findByRole("link", { name: "Learning" });
    await act(async () => fireEvent.click(link));
    window.location.hash = "#learning";
    await act(async () => window.dispatchEvent(new HashChangeEvent("hashchange")));
    expect(await screen.findByRole("heading", { name: /^\d+% learned$/ })).toBeTruthy();
    const bar = screen.getByRole("progressbar");
    const pct = Number(bar.getAttribute("aria-valuenow"));
    expect(pct).toBeGreaterThan(0);
    expect(pct).toBeLessThan(100);
    expect(screen.getByText(/tests left, roughly/)).toBeTruthy();
    expect(screen.getByText("What it kept")).toBeTruthy();
    // Why it waits, with advice, and the battery's update rate. (An
    // earlier test restored everything, which switches learning off.)
    const sw = screen.getByRole("switch", { name: /learn/i });
    if (sw.getAttribute("aria-checked") !== "true") await act(async () => fireEvent.click(sw));
    await waitFor(() => expect(screen.getByRole("note")).toBeTruthy());
    expect(screen.getByRole("note").textContent).toMatch(/62%.*steady use/);
    expect(screen.queryByText(/updates its power reading/)).toBeNull();
    const forget = screen.getAllByRole("button", { name: /^Forget:/ })[0];
    await act(async () => fireEvent.click(forget));
    await waitFor(() => expect(screen.getAllByRole("button", { name: /^Forget:/ }).length).toBeLessThan(3));
  });

  it("keeps a setting after restarts when asked", async () => {
    window.location.hash = "#advanced:settings";
    render(<App />);
    await waitFor(() => expect(screen.getByText("Swap eagerness")).toBeTruthy());
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Change: Swap eagerness" })));
    const dialog = screen.getByRole("dialog", { name: "Change Swap eagerness", hidden: true });
    fireEvent.change(within(dialog).getByRole("textbox", { hidden: true }), { target: { value: "30" } });
    await act(async () => fireEvent.click(within(dialog).getByRole("checkbox", { name: "Keep after restart", hidden: true })));
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Apply", hidden: true })));
    await waitFor(() => expect(screen.getByText("Kept after restarts")).toBeTruthy());
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop keeping: Swap eagerness" })));
    await waitFor(() => expect(screen.queryByText("Kept after restarts")).toBeNull());
  });

  it("can ignore a charger that reports wrongly", async () => {
    window.location.hash = "#advanced:system";
    render(<App />);
    const mains = await screen.findByRole("radio", { name: "Always treat as plugged in" });
    await act(async () => fireEvent.click(mains));
    await waitFor(() => expect((screen.getByRole("radio", { name: "Always treat as plugged in" }) as HTMLInputElement).checked).toBe(true));
    expect((await backend.status()).power.on_ac).toBe(true);
    await act(async () => fireEvent.click(screen.getByRole("radio", { name: "As the charger reports" })));
    await waitFor(async () => expect((await backend.status()).power.on_ac).toBe(false));
  });

  it("runs Learn now, and stops when the laptop is touched", async () => {
    window.location.hash = "#learning";
    render(<App />);
    const start = await screen.findByRole("button", { name: "Start learning now" });
    await act(async () => fireEvent.click(start));
    const dialog = screen.getByRole("dialog", { name: "Learn now?", hidden: true });
    expect((within(dialog).getByRole("checkbox", { hidden: true }) as HTMLInputElement).checked).toBe(true);
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Start", hidden: true })));
    expect(await screen.findByText(/Waiting for the screen to turn off/)).toBeTruthy();
    expect(await screen.findByText(/Test 2 of 8: Sound chip sleep/, {}, { timeout: 5000 })).toBeTruthy();
    expect(within(screen.getByRole("list", { name: "Results of this session" })).getAllByText("Kept").length).toBe(1);
    await act(async () => { demoControls.touch!(); window.dispatchEvent(new Event("dynamis:changed")); });
    expect(await screen.findByText(/Stopped: you used the laptop\. 1 of 8 tests finished/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run again" })).toBeTruthy();
  }, 15000);

  it("runs the sleep test and uses the kind it recommends", async () => {
    // The sleep test lives under Learning.
    window.location.hash = "#learning";
    render(<App />);
    const start = await screen.findByRole("button", { name: "Start the sleep test" });
    await act(async () => fireEvent.click(start));
    const dialog = screen.getByRole("dialog", { name: "Start the sleep test?", hidden: true });
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Start", hidden: true })));
    expect(await screen.findByText(/Testing Light sleep \(modern standby\)/, {}, { timeout: 3000 })).toBeTruthy();
    const use = await screen.findByRole("button", { name: "Use Deep sleep" }, { timeout: 6000 });
    expect(screen.getAllByText("Woke cleanly").length).toBe(2);
    await act(async () => fireEvent.click(use));
    await waitFor(() => expect(screen.getByText("Deep sleep is already the best here.")).toBeTruthy());
    expect((await backend.status()).kept?.settings.some((k) => k.id === "system.mem_sleep")).toBe(true);
    await backend.restore();
  }, 15000);

  it("charges fully once, then puts the limit back", async () => {
    await backend.setSetting("battery.charge_limit", "80", false);
    window.location.hash = "#battery";
    render(<App />);
    const button = await screen.findByRole("button", { name: "Charge fully once" });
    await act(async () => fireEvent.click(button));
    await act(async () => { await new Promise((r) => setTimeout(r, 300)); });
    expect(await screen.findByText(/Charging fully this once\. The limit goes back to 80%/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop" })));
    expect(await screen.findByText("Charging fully ended: you stopped it.")).toBeTruthy();
    expect((await backend.status()).power.charge_limit).toBe(80);
    await backend.restore();
  });

  it("lets idle devices sleep, never the sensitive ones", async () => {
    // Devices has its own screen, with every device listed (nothing folded away).
    window.location.hash = "#devices";
    render(<App />);
    const usb = await screen.findByRole("switch", { name: "Let idle USB devices sleep" });
    expect(usb.getAttribute("aria-checked")).toBe("false");
    await act(async () => fireEvent.click(usb));
    await waitFor(() => expect(screen.getByRole("switch", { name: "Let idle USB devices sleep" }).getAttribute("aria-checked")).toBe("true"));
    expect(screen.getByText(/1-2 · kept awake: input devices/)).toBeTruthy();
    expect(screen.getAllByText("Changed").length).toBe(2);
    expect(screen.getByRole("link", { name: "Devices" }).getAttribute("aria-current")).toBe("page");
    expect(document.querySelector("details")).toBeNull();
    // Friendly names, with the kind of device and its address beside them.
    const xhci = screen.getByText(/0000:00:14.0/).closest("li")?.textContent ?? "";
    expect(xhci).toContain("Intel Celeron N4020 USB 3.0 xHCI Controller");
    expect(xhci).toContain("USB controller · 0000:00:14.0");
    // Keep one device awake by hand.
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Keep awake: Integrated Camera" })));
    await waitFor(() => expect(screen.getByText(/1-3 · you keep it awake/)).toBeTruthy());
    expect(screen.getByRole("button", { name: "Keep awake: Integrated Camera" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.queryByRole("button", { name: /Keep awake: Logitech/ })).toBeNull();
    // Restore switches it off again.
    await backend.restore();
  });

  it("shows settings changed by another tool, and leaves them to it the second time", async () => {
    demoControls.outside!();
    window.location.hash = "#activity";
    const first = render(<App />);
    const problems = await screen.findByRole("button", { name: "Problems" });
    await act(async () => fireEvent.click(problems));
    expect(await screen.findByText(/CPU speed preference was changed by something else/)).toBeTruthy();
    expect(screen.getByText(/Most likely TuneD\. Dynamis Power does not change it back/)).toBeTruthy();
    expect(screen.queryByText("Blocked CPU speed preference")).toBeNull();
    first.unmount();

    demoControls.outside!();
    render(<App />);
    const problems2 = await screen.findByRole("button", { name: "Problems" });
    await act(async () => fireEvent.click(problems2));
    expect(await screen.findByText("Blocked CPU speed preference")).toBeTruthy();
    expect(screen.getByText(/another tool changes this setting: TuneD/)).toBeTruthy();
  });

  // Last: it switches safe mode on and off in the shared stand-in.
  it("explains a freeze, and uses safe mode as a test", async () => {
    await backend.setSetting("vm.swappiness", "30", false);
    for (let i = 0; i < 3; i++) demoControls.freeze!();
    render(<App />);
    expect(await screen.findByText("The laptop did not shut down normally last time")).toBeTruthy();
    expect(screen.getByText(/changed these in the ten minutes before: .*Swap eagerness/)).toBeTruthy();
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toMatch(/Safe mode is on.*stay off when you switch safe mode off: .*Swap eagerness/);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Got it" })));
    expect(screen.queryByText("The laptop did not shut down normally last time")).toBeNull();

    // It froze again with safe mode on: probably not Dynamis Power.
    demoControls.freeze!();
    demoControls.freeze!();
    await act(async () => window.dispatchEvent(new Event("dynamis:changed")));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toMatch(/still stopped 2 times/));
    expect(screen.getByText(/Safe mode was on, so Dynamis Power was changing nothing/)).toBeTruthy();
    await act(async () => fireEvent.click(within(screen.getByRole("alert")).getByRole("button", { name: "Switch safe mode off" })));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    // The suspects were let go, not blocked.
    const rows = await backend.settings();
    expect(rows.find((r) => r.id === "vm.swappiness")?.blocked).toBeNull();
  });
  it("sets the screen brightness chosen for each mode", async () => {
    window.location.hash = "#modes";
    render(<App />);
    const set = await screen.findAllByRole("switch", { name: "Set the screen brightness for this mode" });
    expect(set).toHaveLength(3);
    // Longer battery is the first card.
    await act(async () => fireEvent.click(set[0]));
    const slider = await screen.findByRole("slider", { name: "Screen brightness, Longer battery" });
    await act(async () => fireEvent.change(slider, { target: { value: "30" } }));
    await waitFor(async () => expect((await backend.brightness()).levels["power-saver"]).toBe(30), { timeout: 2000 });
    expect(screen.getByText(/min more on battery than at the brightness now/)).toBeTruthy();
    // Switching to that mode sets it once.
    await act(async () => backend.setProfile("power-saver"));
    expect((await backend.brightness()).now_pct).toBe(30);
    const j = await backend.journal(5);
    expect(j.some((e) => e.event === "brightness" && e.to === "30%")).toBe(true);
    await act(async () => backend.setProfile("balanced"));
    await act(async () => backend.setBrightnessPrefs({ levels: {} }));
  });

  it("shows what uses the battery", async () => {
    window.location.hash = "#battery";
    render(<App />);
    const card = await screen.findByRole("region", { name: "What uses the battery" });
    await waitFor(() => expect(within(card).getByText("Everything else")).toBeTruthy(), { timeout: 5000 });
    expect(within(card).getByText("Processor")).toBeTruthy();
    expect(within(card).getByText("Graphics")).toBeTruthy();
    expect(within(card).getByText("Screen")).toBeTruthy();
  }, 10000);
  it("hides the battery features on a computer without a battery", async () => {
    const real = backend.status;
    const spy = vi.spyOn(backend, "status").mockImplementation(async () => {
      const s = await real();
      return { ...s, power: { ...s.power, has_battery: false, battery_pct: null, hours_left: null, on_ac: true } };
    });
    try {
      window.location.hash = "#battery";
      render(<App />);
      expect(await screen.findByText("No battery: running on mains power")).toBeTruthy();
      expect(screen.queryByRole("link", { name: "Battery" })).toBeNull();
      await act(async () => fireEvent.click(screen.getByRole("link", { name: "Learning" })));
      expect(await screen.findByText(/This computer has no battery/)).toBeTruthy();
      expect(screen.queryByRole("heading", { name: "Learn now" })).toBeNull();
    } finally {
      spy.mockRestore();
    }
  });
  it("imports a TLP configuration after a preview", async () => {
    window.location.hash = "#advanced:system";
    render(<App />);
    const list = await screen.findByRole("list", { name: "Values to import" });
    expect(within(list).getAllByRole("checkbox")).toHaveLength(4);
    expect(screen.getByText(/use Devices → Let idle USB devices sleep/)).toBeTruthy();
    // Untick the sound card line.
    await act(async () => fireEvent.click(within(list).getAllByRole("checkbox")[3]));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Import 3 values" })));
    expect(await screen.findByText(/Imported 3 values/)).toBeTruthy();
  });
  it("recalibrates the battery while plugged in, and stops at once", async () => {
    const before = await backend.autoPrefs();
    await backend.setAutoPrefs({ ...before, power_source: "mains" });
    try {
      window.location.hash = "#battery";
      render(<App />);
      const card = await screen.findByRole("region", { name: "Recalibrate the battery" });
      await waitFor(() => expect(within(card).getByRole("button", { name: "Recalibrate…" }).hasAttribute("disabled")).toBe(false));
      await act(async () => fireEvent.click(within(card).getByRole("button", { name: "Recalibrate…" })));
      const dialog = screen.getByRole("dialog", { hidden: true });
      await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Start", hidden: true })));
      expect(await within(card).findByText(/Running the battery down while plugged in/)).toBeTruthy();
      await act(async () => fireEvent.click(within(card).getByRole("button", { name: "Stop" })));
      expect(await within(card).findByText(/Stopped: you stopped it/)).toBeTruthy();
    } finally {
      await backend.setAutoPrefs(before);
    }
  });
});
