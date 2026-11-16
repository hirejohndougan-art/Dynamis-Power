import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../App";

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
    expect(screen.queryByText("kernel.nmi_watchdog")).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: "Details" })));
    expect(screen.getByText("kernel.nmi_watchdog")).toBeTruthy();
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
    await waitFor(() => expect(screen.getByRole("heading", { name: "Home" })).toBeTruthy());
    expect(localStorage.getItem("dynamis.welcomed")).toBe("1");
    expect(screen.getByRole("switch", { name: "Learn" }).getAttribute("aria-checked")).toBe("false");
  });

  it("saves auto mode preferences and explains a refusal", async () => {
    window.location.hash = "#modes";
    render(<App />);
    const battery = (await screen.findByLabelText("On battery")) as HTMLSelectElement;
    await act(async () => fireEvent.change(battery, { target: { value: "power-saver" } }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Saved."));
  });
});
