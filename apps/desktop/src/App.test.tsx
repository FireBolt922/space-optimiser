// Component tests with a fake engine (phase 7 tests 1-10, plus the M3 run
// screens). No test touches a file.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import {
  CANDIDATES,
  REPORT,
  XSS_NAME,
  deepScan,
  fakeEngine,
  finishedScan,
} from "./test/fakeEngine";

afterEach(cleanup);
beforeEach(() => {
  try {
    localStorage.clear();
  } catch {
    // Storage can be unavailable; the app must cope.
  }
  delete document.documentElement.dataset.theme;
});

const ABSOLUTE_PROMISE = "will not delete personal data";

async function toResults(engine = fakeEngine()) {
  render(<App engine={engine} />);
  fireEvent.click(await screen.findByRole("button", { name: "Normal Scan" }));
  await screen.findByRole("heading", { name: "Results" });
  return engine;
}

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** A result checkbox, found by the row name its accessible name starts with. */
function box(name: string): HTMLInputElement {
  return screen.getByRole("checkbox", {
    name: new RegExp(`^${escape(name)}, `),
  }) as HTMLInputElement;
}

async function toReview(engine = fakeEngine()) {
  await toResults(engine);
  fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
  fireEvent.click(screen.getByRole("button", { name: "Review" }));
  await screen.findByRole("heading", { name: "Review" });
  return engine;
}

describe("start", () => {
  it("leads with Deep Scan and offers Normal Scan", async () => {
    render(<App engine={fakeEngine()} />);
    const main = await screen.findByRole("main");
    const buttons = await within(main).findAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["Deep Scan", "Normal Scan"]);
  });

  it("reports an interrupted run from an earlier session", async () => {
    render(
      <App
        engine={fakeEngine({
          interrupted: {
            started_unix_secs: 1_760_000_000,
            items_planned: 3,
            actions: [{ display_name: "old-build", outcome: "removed" }],
          },
        })}
      />,
    );
    expect(await screen.findByText("The last cleanup was interrupted")).toBeTruthy();
    expect(screen.getByText(/old-build/)).toBeTruthy();
  });
});

describe("scanning", () => {
  // Phase 7 test 6.
  it("Cancel calls cancel_scan", async () => {
    const engine = fakeEngine({ scanRuns: true });
    render(<App engine={engine} />);
    fireEvent.click(await screen.findByRole("button", { name: "Normal Scan" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(engine.cancelScan).toHaveBeenCalledWith(7));
    await screen.findByRole("button", { name: "Normal Scan" });
  });

  it("names the stage it is in, the tool it is asking, and each stage's state", async () => {
    const engine = fakeEngine({
      scanRuns: true,
      scanProgress: {
        files_seen: 1_203_836,
        bytes_seen: 39_300_000_000,
        stage: "tools",
        detail: "uv",
      },
      scanStages: ["temp", "caches", "logs", "tools", "docker", "large_files"],
    });
    render(<App engine={engine} />);
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    expect(await screen.findByText("Asking uv where its cache is.")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("Asking uv where its cache is.");
    expect(screen.getByText("1,203,836 files so far, 39.3 GB.")).toBeTruthy();
    const stages = within(screen.getByRole("list", { name: "Scan stages" }))
      .getAllByRole("listitem")
      .map((li) => li.textContent);
    expect(stages).toEqual([
      "Temporary filesDone",
      "App and browser cachesDone",
      "Old logsDone",
      "Developer tool cachesChecking",
      "DockerWaiting",
      "Large filesWaiting",
    ]);
  });

  it("shows how long the scan has been running", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<App engine={fakeEngine({ scanRuns: true })} />);
      fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
      expect(await screen.findByText(/^Running for /)).toBeTruthy();
      await act(async () => {
        vi.advanceTimersByTime(72_000);
      });
      expect(screen.getByText(/^Running for 1 minute 1[23] seconds$/)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("results", () => {
  it("the header says when the scan ran and offers Scan again in the same mode", async () => {
    const engine = fakeEngine({
      scan: finishedScan({
        duration_ms: 90_900,
        progress: { files_seen: 2_203_944, bytes_seen: 1, stage: "caches", detail: null },
      }),
    });
    render(<App engine={engine} />);
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    await screen.findByRole("heading", { name: "Results" });
    expect(
      screen.getByText(/^Scanned at .+ · took 1 minute 30 seconds · checked 2,203,944 files$/),
    ).toBeTruthy();
    const header = screen.getByRole("heading", { name: "Results" }).parentElement!;
    fireEvent.click(within(header).getByRole("button", { name: "Scan again" }));
    await waitFor(() => expect(engine.startScan).toHaveBeenCalledTimes(2));
    expect(engine.startScan.mock.calls.map((c) => c[0])).toEqual(["deep", "deep"]);
  });

  // Phase 7 tests 1 and 2.
  it("Select safe items checks only bulk-safe rows", async () => {
    await toResults();
    fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
    expect(box("old-download.bin").checked).toBe(true);
    expect(box(XSS_NAME).checked).toBe(true);
    expect(box("Browser cache").checked).toBe(false);
    expect(box("unsure-folder").checked).toBe(false);
    expect(screen.getByText(/1 other item was left unselected/)).toBeTruthy();
  });

  // Phase 7 test 3.
  it("Review is disabled until something deletable is selected", async () => {
    await toResults();
    const review = screen.getByRole("button", { name: "Review" }) as HTMLButtonElement;
    expect(review.disabled).toBe(true);
    fireEvent.click(box("Browser cache"));
    expect(review.disabled).toBe(false);
  });

  // Phase 7 test 4.
  it("an info row has no checkbox", async () => {
    await toResults();
    expect(screen.getByText("video.mov")).toBeTruthy();
    expect(screen.queryByRole("checkbox", { name: /video\.mov/ })).toBeNull();
  });

  // Phase 7 test 7.
  it("a file name is rendered as text, never as markup", async () => {
    await toResults();
    expect(screen.getAllByText(XSS_NAME).length).toBeGreaterThan(0);
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  // Phase 7 test 8.
  it("an uncertain row's checkbox is disabled", async () => {
    await toResults();
    expect(box("unsure-folder").disabled).toBe(true);
    expect(screen.getByText("Not sure — not offered")).toBeTruthy();
  });

  // Phase 5 test 8: a running browser's cache cannot be selected.
  it("a browser that is open is shown as quit first and cannot be selected", async () => {
    await toResults();
    const firefox = box("Firefox — abcd.default");
    expect(firefox.disabled).toBe(true);
    expect(screen.getByText("Quit the browser first")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
    expect(firefox.checked).toBe(false);
  });

  // Phase 5 item 9.
  it("shows how long the scan took", async () => {
    await toResults();
    expect(screen.getByText(/took less than a second/)).toBeTruthy();
  });

  it("risk is shown as a word, and skipped entries show their reason", async () => {
    await toResults();
    expect(screen.getAllByText("Low").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Medium").length).toBeGreaterThan(0);
    expect(screen.getByText("Info")).toBeTruthy();
    expect(screen.getByText(/it changed within the last day/)).toBeTruthy();
  });

  it("focus moves to the results heading", async () => {
    await toResults();
    // The heading renders before its focus effect runs, so wait for focus.
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("heading", { name: "Results" })),
    );
  });

  it("an empty scan says there is nothing to clean", async () => {
    await toResults(fakeEngine({ scan: finishedScan({ candidates: [] }) }));
    expect(screen.getByText("Nothing to clean in this mode")).toBeTruthy();
  });

  it("only candidate ids are sent to build_plan", async () => {
    const engine = await toReview();
    expect(engine.buildPlan).toHaveBeenCalledWith(7, [1, 2]);
  });
});

describe("review", () => {
  // Phase 7 tests 5 and 9.
  it("states the safety wording and that the cleanup cannot be undone", async () => {
    await toReview();
    const text = document.body.textContent ?? "";
    expect(text).toContain("This cannot be undone.");
    expect(text).toContain("only removes files in locations it has a reviewed rule for");
    expect(text).not.toContain(ABSOLUTE_PROMISE);
  });

  it("nothing runs on arrival", async () => {
    const engine = await toReview();
    expect(engine.confirmPlan).not.toHaveBeenCalled();
    expect(engine.executePlan).not.toHaveBeenCalled();
  });

  it("Back returns to the results with the selection kept", async () => {
    await toReview();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await screen.findByRole("heading", { name: "Results" });
    expect(box("old-download.bin").checked).toBe(true);
  });

  it("an expired plan offers Scan again instead of Confirm", async () => {
    await toReview(
      fakeEngine({
        confirmError: {
          kind: "plan_expired",
          message: "the plan is more than 15 minutes old. Scan again.",
        },
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await screen.findByRole("button", { name: "Scan again" });
    expect(screen.queryByRole("button", { name: "Confirm" })).toBeNull();
  });
});

describe("running and report", () => {
  it("confirm runs the plan and shows the report", async () => {
    const engine = await toReview();
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await screen.findByRole("heading", { name: "Report" });
    expect(engine.executePlan).toHaveBeenCalledWith(11, "token-1", false);
    const removed = screen.getByRole("region", { name: "Removed" });
    expect(within(removed).getByText("old-download.bin")).toBeTruthy();
    const skipped = screen.getByRole("region", { name: "Skipped" });
    expect(within(skipped).getByText(/it changed since the scan/)).toBeTruthy();
    expect(screen.getByText(/free space reported by the system/)).toBeTruthy();
    // 50 MB more free space, 2 MB of files: the numbers differ.
    expect(screen.getByText(/These numbers differ/)).toBeTruthy();
  });

  it("a full disk asks for a second confirm and keeps the log in memory", async () => {
    const engine = await toReview(
      fakeEngine({
        execute: [{ status: "audit_log_unavailable", disk_full: true }, { status: "started" }],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(
      await screen.findByText("The audit log could not be written because the disk is full."),
    ).toBeTruthy();
    expect(engine.executePlan).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Clean without a log file" }));
    await screen.findByRole("heading", { name: "Report" });
    expect(engine.executePlan).toHaveBeenLastCalledWith(11, "token-1", true);
  });

  it("Stop calls stop_run", async () => {
    const engine = await toReview(fakeEngine({ runStays: true }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    await waitFor(() => expect(engine.stopRun).toHaveBeenCalledWith(11));
    // Progress arrives from the next poll, after the click, and must survive it.
    expect(await screen.findByText("1 of 2 items done")).toBeTruthy();
  });
});

// Phase 7 test 10.
describe("deep scan", () => {
  async function toDeepResults(engine = fakeEngine({ scan: deepScan() })) {
    render(<App engine={engine} />);
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    await screen.findByRole("heading", { name: "Results" });
    return engine;
  }

  it("the start screen says what Deep Scan adds", async () => {
    render(<App engine={fakeEngine()} />);
    expect(await screen.findByText(/package caches/)).toBeTruthy();
    expect(screen.queryByText(/later version/)).toBeNull();
  });

  it("a tool row shows its upper bound and where it was found", async () => {
    await toDeepResults();
    const row = box("~/Library/pnpm/store/v3").closest("li") as HTMLElement;
    expect(within(row).getByText("Up to 3.0 MB")).toBeTruthy();
    expect(within(row).getByText("Clean with the tool")).toBeTruthy();
    expect(
      within(row).getByText("Folder from `pnpm store path`; pnpm found on PATH."),
    ).toBeTruthy();
  });

  it("Select safe items leaves tool rows, package caches, and default folders alone", async () => {
    await toDeepResults();
    fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
    expect(box("registry").checked).toBe(true);
    expect(box("~/.cargo/registry/cache").checked).toBe(false);
    expect(box("~/Library/pnpm/store/v3").checked).toBe(false);
    expect(box("~/Library/Caches/pip").disabled).toBe(true);
  });

  it("a tool row can be selected and its bound stays out of the delete total", async () => {
    await toDeepResults();
    fireEvent.click(box("~/Library/pnpm/store/v3"));
    expect(
      screen.getByText("1 item selected, about 0 bytes. Tools may free up to 3.0 MB more."),
    ).toBeTruthy();
  });

  it("an item inside a selected folder counts once", async () => {
    await toDeepResults();
    fireEvent.click(box("registry"));
    fireEvent.click(box("~/.cargo/registry/cache"));
    expect(screen.getByText("2 items selected, about 8.0 KB")).toBeTruthy();
  });

  it("notes say why a row offers nothing, with sizes", async () => {
    await toDeepResults();
    const docker = screen.getByRole("region", { name: "Docker build cache" });
    expect(within(docker).getByText("Docker did not respond.")).toBeTruthy();
    expect(within(docker).getByText(/never offered.*2\.0 GB/)).toBeTruthy();
  });

  it("the review keeps the tool bound on its own line", async () => {
    await toDeepResults();
    fireEvent.click(box("registry"));
    fireEvent.click(box("~/Library/pnpm/store/v3"));
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("heading", { name: "Review" });
    expect(screen.getByText("Run the tool's own clean command.")).toBeTruthy();
    expect(screen.getByText(/Tools may free up to 3\.0 MB more/)).toBeTruthy();
    expect(screen.getByText("8.0 KB")).toBeTruthy();
  });

  it("the report lists tool commands that ran", async () => {
    const report = {
      ...REPORT,
      items: [
        ...REPORT.items,
        {
          candidate_id: 22,
          catalog_id: "pkg-pnpm-store",
          display_name: "~/Library/pnpm/store/v3",
          outcome: "ran" as const,
          detail: null,
          bytes_removed: 0,
          recovery_text: "The next install will download packages again.",
        },
      ],
    };
    const engine = fakeEngine({ scan: deepScan(), report });
    await toDeepResults(engine);
    fireEvent.click(box("~/Library/pnpm/store/v3"));
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
    await screen.findByRole("heading", { name: "Report" });
    const ran = screen.getByRole("region", { name: "Tool commands run" });
    expect(within(ran).getByText("~/Library/pnpm/store/v3")).toBeTruthy();
    expect(within(ran).getByText("The next install will download packages again.")).toBeTruthy();
  });
});

it('no screen says "Select All"', async () => {
  await toReview();
  expect(document.body.textContent).not.toMatch(/select all/i);
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  await screen.findByRole("heading", { name: "Results" });
  expect(document.body.textContent).not.toMatch(/select all/i);
});

// M6: layout, accessibility, Show in folder, export, theme, About, Help.
async function toReport(engine = fakeEngine()) {
  await toReview(engine);
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await screen.findByRole("heading", { name: "Report" });
  return engine;
}

function nav(name: string) {
  return within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name });
}

describe("accessibility", () => {
  // Phase 11 test 1: the checkbox exposes name, risk, and size in one name.
  it("result checkboxes are named with the row name, risk, and size", async () => {
    await toResults();
    expect(
      screen.getByRole("checkbox", { name: "old-download.bin, Low risk, 2.1 MB" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("checkbox", { name: "Browser cache, Medium risk, 4.1 KB" }),
    ).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: `${XSS_NAME}, Low risk, 4.1 KB` })).toBeTruthy();
  });

  it("a tool row's checkbox names its upper bound", async () => {
    render(<App engine={fakeEngine({ scan: deepScan() })} />);
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    await screen.findByRole("heading", { name: "Results" });
    expect(
      screen.getByRole("checkbox", { name: "~/Library/pnpm/store/v3, Medium risk, Up to 3.0 MB" }),
    ).toBeTruthy();
  });

  it("every result checkbox has an accessible name", async () => {
    await toResults();
    for (const checkbox of screen.getAllByRole("checkbox")) {
      expect(checkbox.getAttribute("aria-label")).toMatch(/, (Low|Medium|High|Info) risk, /);
    }
  });

  it("the selection count is announced politely", async () => {
    await toResults();
    const count = screen.getByText(/items? selected/);
    expect(count.getAttribute("aria-live")).toBe("polite");
  });
});

describe("show in folder", () => {
  it("sends only the candidate id", async () => {
    const engine = await toResults();
    fireEvent.click(screen.getByRole("button", { name: "Show in folder: old-download.bin" }));
    await waitFor(() => expect(engine.reveal).toHaveBeenCalledTimes(1));
    expect(engine.reveal.mock.calls[0]).toEqual([1]);
    expect(typeof engine.reveal.mock.calls[0][0]).toBe("number");
  });

  // Phase 7: info rows have a Show in folder button and no checkbox.
  it("an info row has the button and no checkbox", async () => {
    const engine = await toResults();
    expect(screen.queryByRole("checkbox", { name: /video\.mov/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show in folder: video.mov" }));
    await waitFor(() => expect(engine.reveal).toHaveBeenCalledWith(4));
  });

  it("a row that cannot be revealed has no button", async () => {
    render(<App engine={fakeEngine({ scan: deepScan() })} />);
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    await screen.findByRole("heading", { name: "Results" });
    expect(box("Docker build cache")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Show in folder: Docker build cache" })).toBeNull();
    expect(screen.getByRole("button", { name: "Show in folder: registry" })).toBeTruthy();
  });

  it("an error shows on that row", async () => {
    await toResults(
      fakeEngine({
        revealError: {
          kind: "candidate_gone",
          message: "that item is no longer where the scan found it. Scan again.",
        },
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Show in folder: old-download.bin" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("That item is no longer where the scan found it. Scan again.");
    expect(alert.closest("li")).toBe(box("old-download.bin").closest("li"));
  });
});

describe("report export and explanation", () => {
  it("Export report sends the plan id and shows where the file was saved", async () => {
    const engine = await toReport();
    fireEvent.click(screen.getByRole("button", { name: "Export report" }));
    expect(
      await screen.findByText("Saved to /Users/test/Downloads/space-optimiser-report.txt"),
    ).toBeTruthy();
    expect(engine.exportReport.mock.calls).toEqual([[11]]);
  });

  it("an export error is shown", async () => {
    await toReport(
      fakeEngine({
        exportError: { kind: "export_failed", message: "could not save the report: disk full" },
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Export report" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Could not save the report: disk full",
    );
  });

  it("Done stays the only primary button", async () => {
    await toReport();
    const primaries = document.querySelectorAll(".button-primary");
    expect([...primaries].map((b) => b.textContent)).toEqual(["Done"]);
  });

  it("on macOS the explanation names local snapshots and purgeable space", async () => {
    await toReport();
    const text = screen.getByText(/These numbers differ/).textContent ?? "";
    expect(text).toMatch(/Time Machine/);
    expect(text).toMatch(/purgeable/);
    expect(text).toMatch(/Docker/);
    expect(text).not.toMatch(/restore points/);
  });

  it("on Windows the explanation names restore points, not purgeable space", async () => {
    await toReport(fakeEngine({ platform: "windows" }));
    const text = screen.getByText(/These numbers differ/).textContent ?? "";
    expect(text).toMatch(/restore points/);
    expect(text).not.toMatch(/purgeable|Time Machine/);
  });

  it("on Linux the explanation names Btrfs or ZFS snapshots", async () => {
    await toReport(fakeEngine({ platform: "linux" }));
    const text = screen.getByText(/These numbers differ/).textContent ?? "";
    expect(text).toMatch(/Btrfs or ZFS/);
    expect(text).not.toMatch(/purgeable|Time Machine|restore points/);
  });
});

describe("sidebar", () => {
  it("has the four views and marks the current one", async () => {
    render(<App engine={fakeEngine()} />);
    const main = screen.getByRole("navigation", { name: "Main" });
    expect(
      within(main)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Clean up", "Settings", "Help", "About"]);
    expect(nav("Clean up").getAttribute("aria-current")).toBe("page");
    fireEvent.click(nav("Help"));
    expect(nav("Help").getAttribute("aria-current")).toBe("page");
    expect(nav("Clean up").getAttribute("aria-current")).toBeNull();
  });

  it("shows the version from the engine", async () => {
    render(<App engine={fakeEngine()} />);
    expect(await screen.findByText("Version 0.1.0")).toBeTruthy();
  });

  it("a view change moves focus to that view's heading", async () => {
    render(<App engine={fakeEngine()} />);
    fireEvent.click(nav("Settings"));
    expect(document.activeElement).toBe(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    );
    fireEvent.click(nav("Clean up"));
    expect(document.activeElement).toBe(
      screen.getByRole("heading", { level: 1, name: "Free up space safely" }),
    );
  });

  it("a scan keeps running while another view is open", async () => {
    const engine = fakeEngine();
    render(<App engine={engine} />);
    fireEvent.click(await screen.findByRole("button", { name: "Normal Scan" }));
    fireEvent.click(nav("Help"));
    await waitFor(() => expect(engine.getScan).toHaveBeenCalled());
    fireEvent.click(nav("Clean up"));
    await screen.findByRole("heading", { name: "Results" });
  });

  it("Settings, Help, and About have no primary button", async () => {
    render(<App engine={fakeEngine()} />);
    for (const view of ["Settings", "Help", "About"]) {
      fireEvent.click(nav(view));
      await screen.findByRole("heading", { level: 1, name: view });
      expect(document.querySelectorAll(".button-primary").length).toBe(0);
    }
  });
});

describe("theme", () => {
  function mockMatchMedia(light: boolean) {
    const listeners: ((e: { matches: boolean }) => void)[] = [];
    const query = {
      matches: light,
      media: "(prefers-color-scheme: light)",
      addEventListener: (_: string, fn: (e: { matches: boolean }) => void) => listeners.push(fn),
      removeEventListener: (_: string, fn: (e: { matches: boolean }) => void) =>
        listeners.splice(listeners.indexOf(fn), 1),
    };
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => query),
    );
    return (matches: boolean) => {
      query.matches = matches;
      for (const fn of [...listeners]) fn({ matches });
    };
  }

  afterEach(() => vi.unstubAllGlobals());

  it("defaults to dark", () => {
    render(<App engine={fakeEngine()} />);
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("the setting is applied and kept for the next launch", async () => {
    render(<App engine={fakeEngine()} />);
    fireEvent.click(nav("Settings"));
    const dark = screen.getByRole("radio", { name: /Dark/ }) as HTMLInputElement;
    expect(dark.checked).toBe(true);
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(localStorage.getItem("space-optimiser:theme")).toBe("light");
    cleanup();
    delete document.documentElement.dataset.theme;
    render(<App engine={fakeEngine()} />);
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("Match system follows the system setting as it changes", async () => {
    const change = mockMatchMedia(true);
    render(<App engine={fakeEngine()} />);
    fireEvent.click(nav("Settings"));
    fireEvent.click(screen.getByRole("radio", { name: "Match system" }));
    expect(document.documentElement.dataset.theme).toBe("light");
    act(() => change(false));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("space-optimiser:theme")).toBe("system");
  });

  it("a broken storage falls back to dark and still works", async () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    vi.stubGlobal("localStorage", broken);
    render(<App engine={fakeEngine()} />);
    expect(document.documentElement.dataset.theme).toBe("dark");
    fireEvent.click(nav("Settings"));
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("about and help", () => {
  it("About shows the version, the safety wording, no upload, and the audit log", async () => {
    render(<App engine={fakeEngine()} />);
    fireEvent.click(nav("About"));
    await screen.findByText("Version 0.1.0", { selector: "main *" });
    const text = screen.getByRole("main").textContent ?? "";
    expect(text).toContain(
      "Space Optimiser only removes files in locations it has a reviewed rule for, and only after you confirm. It does not clean your documents, photos, downloads, or source code, and it does not change system settings, passwords, or app configuration. A confirmed cleanup can still make an app rebuild a cache or download files again.",
    );
    expect(text).toContain("This app does not upload a list of your files.");
    expect(text).toContain("/Users/test/Library/Application Support/Space Optimiser/audit.jsonl");
    expect(text).toContain("You can delete this file yourself. Space Optimiser does not clean it.");
    expect(text).toContain("This version does not update itself.");
    expect(text).toContain("SIL Open Font License");
    expect(text).toContain("MIT License");
    expect(text).toContain("Permission is hereby granted, free of charge");
    expect(text).not.toContain(ABSOLUTE_PROMISE);
  });

  it("About links to the local Help page", async () => {
    render(<App engine={fakeEngine()} />);
    fireEvent.click(nav("About"));
    fireEvent.click(await screen.findByRole("button", { name: "Open Help" }));
    expect(screen.getByRole("heading", { level: 1, name: "Help" })).toBeTruthy();
  });

  it("Help explains the essentials without the absolute promise", async () => {
    render(<App engine={fakeEngine()} />);
    fireEvent.click(nav("Help"));
    const text = screen.getByRole("main").textContent ?? "";
    expect(text).toContain("Normal Scan");
    expect(text).toContain("Deep Scan");
    expect(text).toContain("Select safe items");
    expect(text).toContain("cannot be undone");
    expect(text).toContain("administrator");
    expect(text).toMatch(/does not upload/);
    expect(text).not.toContain(ABSOLUTE_PROMISE);
    expect(text).not.toMatch(/select all/i);
  });
});

// Categories, filters, folded groups, and the sticky action bar.
describe("categories and long lists", () => {
  function manyTemp(count: number) {
    return Array.from({ length: count }, (_, i) => ({
      ...CANDIDATES[0],
      id: 100 + i,
      display_name: `temp-${i}.tmp`,
      estimated_bytes: 1000,
    }));
  }

  it("category buttons show counts and filter the groups", async () => {
    await toResults();
    const categories = within(screen.getByRole("group", { name: "Categories" }));
    expect(categories.getByRole("button", { name: "All (6)" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    fireEvent.click(categories.getByRole("button", { name: /^Browsers \(2/ }));
    expect(screen.getByRole("region", { name: "Browser cache" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Temporary files" })).toBeNull();
    expect(screen.queryByText("video.mov")).toBeNull();
    fireEvent.click(categories.getByRole("button", { name: /^Large files \(1/ }));
    expect(screen.getByText("video.mov")).toBeTruthy();
    fireEvent.click(categories.getByRole("button", { name: "All (6)" }));
    expect(screen.getByRole("region", { name: "Temporary files" })).toBeTruthy();
  });

  it("a selection survives a category change and counts the whole scan", async () => {
    await toResults();
    fireEvent.click(box("old-download.bin"));
    const categories = within(screen.getByRole("group", { name: "Categories" }));
    fireEvent.click(categories.getByRole("button", { name: /^Browsers/ }));
    expect(screen.getByText(/1 item selected, about 2\.1 MB/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Review" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
    fireEvent.click(categories.getByRole("button", { name: "All (6)" }));
    expect(box("old-download.bin").checked).toBe(true);
  });

  it("the name filter narrows rows and says when nothing matches", async () => {
    await toResults();
    const filter = screen.getByLabelText("Filter by name");
    fireEvent.change(filter, { target: { value: "VIDEO" } });
    expect(screen.getByText("video.mov")).toBeTruthy();
    expect(screen.queryByText("old-download.bin")).toBeNull();
    fireEvent.change(filter, { target: { value: "nothing-here" } });
    expect(screen.getByText("No items match this filter.")).toBeTruthy();
  });

  it("a long group folds after ten rows and Show all reveals the rest", async () => {
    await toResults(fakeEngine({ scan: finishedScan({ candidates: manyTemp(14) }) }));
    expect(screen.getAllByRole("checkbox").length).toBe(10);
    const more = screen.getByRole("button", { name: "Show all 14 items in Temporary files" });
    expect(more.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(more);
    expect(screen.getAllByRole("checkbox").length).toBe(14);
    expect(screen.getByText("14 items, 14.0 KB")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Show fewer in Temporary files" }));
    expect(screen.getAllByRole("checkbox").length).toBe(10);
  });

  it("Select safe items also selects rows that are folded away", async () => {
    const engine = await toResults(
      fakeEngine({ scan: finishedScan({ candidates: manyTemp(12) }) }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
    expect(screen.getByText(/12 items selected/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("heading", { name: "Review" });
    expect(engine.buildPlan.mock.calls[0][1].length).toBe(12);
  });

  it("the action bar on Results, Review, and Report is the sticky one", async () => {
    await toResults();
    const bar = screen.getByRole("button", { name: "Review" }).parentElement!;
    expect(bar.className).toContain("actions-sticky");
    expect(within(bar).getByText(/0 items selected/)).toBeTruthy();
    fireEvent.click(box("old-download.bin"));
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("heading", { name: "Review" });
    expect(screen.getByRole("button", { name: "Confirm" }).parentElement!.className).toContain(
      "actions-sticky",
    );
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await screen.findByRole("heading", { name: "Report" });
    expect(screen.getByRole("button", { name: "Done" }).parentElement!.className).toContain(
      "actions-sticky",
    );
  });

  it("a long report section starts folded with its count and size; Failed stays open", async () => {
    const removed = Array.from({ length: 12 }, (_, i) => ({
      ...REPORT.items[0],
      candidate_id: 100 + i,
      display_name: `temp-${i}.tmp`,
      bytes_removed: 1000,
    }));
    const failed = Array.from({ length: 11 }, (_, i) => ({
      ...REPORT.items[1],
      candidate_id: 200 + i,
      display_name: `stuck-${i}.tmp`,
      outcome: "failed" as const,
      detail: "in use",
    }));
    await toReport(
      fakeEngine({ report: { ...REPORT, items: [...removed, ...failed], bytes_removed: 12_000 } }),
    );
    const removedSection = screen.getByRole("region", { name: "Removed" });
    expect(within(removedSection).getByText("Removed (12 items, 12.0 KB)")).toBeTruthy();
    expect(removedSection.querySelector("details")!.open).toBe(false);
    const failedSection = screen.getByRole("region", { name: "Failed" });
    expect(within(failedSection).getByText("Failed (11 items)")).toBeTruthy();
    expect(failedSection.querySelector("details")!.open).toBe(true);
  });
});
