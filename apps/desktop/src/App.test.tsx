// Component tests with a fake engine (phase 7 tests 1-10, plus the M3 run
// screens). No test touches a file.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import { Scanning } from "./screens/Scanning";
import {
  CANDIDATES,
  ANDROID_CANDIDATES,
  ON_DEVICE_MODEL,
  PROJECT_CANDIDATES,
  REPORT,
  SYSTEM_USAGE,
  UPDATE_AVAILABLE,
  XSS_NAME,
  androidScan,
  deepScan,
  fakeEngine,
  finishedScan,
  projectsScan,
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

/** Renders the app and opens Clean up; the app opens on the Overview. */
function renderCleanUp(engine: ReturnType<typeof fakeEngine>) {
  const result = render(<App engine={engine} />);
  fireEvent.click(nav("Clean up"));
  return result;
}

async function toResults(engine = fakeEngine()) {
  renderCleanUp(engine);
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
  it("leads with Deep Scan, then Normal Scan, then the two tools", async () => {
    renderCleanUp(fakeEngine());
    const main = await screen.findByRole("main");
    const buttons = await within(main).findAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual([
      "Deep Scan",
      "Normal Scan",
      "Scan projects",
      "Scan Android",
    ]);
    // One primary button per view.
    expect(buttons.filter((b) => b.classList.contains("button-primary"))).toHaveLength(1);
    expect(buttons[0].classList.contains("button-primary")).toBe(true);
    expect(within(main).getByRole("heading", { name: "Scans" })).toBeTruthy();
    expect(within(main).getByRole("heading", { name: "Tools" })).toBeTruthy();
  });

  it("opens on Clean up to report an interrupted run from an earlier session", async () => {
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
    renderCleanUp(engine);
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
    renderCleanUp(engine);
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
    renderCleanUp(fakeEngine({ scanRuns: true }));
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    expect(await screen.findByText(/^Running for /)).toBeTruthy();
  });

  // The clock alone, with a fixed start and fully fake timers: no polling,
  // so the result cannot depend on how fast the runner is. (Mixing
  // shouldAdvanceTime with findBy polling failed now and then on CI.)
  it("the clock counts from the scan's start and ticks each second", () => {
    vi.useFakeTimers();
    try {
      const props = { view: undefined, cancelling: false, onCancel: () => {} };
      const { unmount } = render(<Scanning {...props} startedAt={Date.now() - 72_000} />);
      expect(screen.getByText("Running for 1 minute 12 seconds")).toBeTruthy();
      act(() => {
        vi.advanceTimersByTime(5_000);
      });
      expect(screen.getByText("Running for 1 minute 17 seconds")).toBeTruthy();
      unmount();
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
    renderCleanUp(engine);
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

  // Phase 5 test 8: a running browser's cache cannot be selected. The row
  // and the notice name the app, not "the browser".
  it("a browser that is open is shown as quit first and cannot be selected", async () => {
    await toResults();
    const firefox = box("Firefox — abcd.default");
    expect(firefox.disabled).toBe(true);
    expect(screen.getByText("Quit Firefox first")).toBeTruthy();
    expect(
      screen.getByText("Firefox is open, so its cache is not offered. Quit it, then scan again."),
    ).toBeTruthy();
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
    expect(screen.getByText("Changed in the last day (1 item)")).toBeTruthy();
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
    expect(await screen.findByText(/^1 of 2 items done/)).toBeTruthy();
  });
});

describe("live cleaning progress", () => {
  it("shows the item in progress, its tool output, and finished items with a hint", async () => {
    await toReview(fakeEngine({ runStays: true }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    const status = await screen.findByRole("status");
    await waitFor(() => expect(status.textContent).toBe("Cleaning ~/.cache/uv."));
    expect(screen.getByText("1 of 2 items done · 1 minute 2 seconds")).toBeTruthy();
    expect(screen.getByText("Running for 41 seconds")).toBeTruthy();
    const output = screen.getByLabelText("Output from ~/.cache/uv");
    expect(output.textContent).toBe(
      "Running /opt/homebrew/bin/uv cache clean\nClearing cache at: /Users/test/.cache/uv",
    );
    expect(output.getAttribute("tabindex")).toBe("0");
    const finished = screen.getByRole("list", { name: "Finished items" });
    expect(within(finished).getByText("~/.npm")).toBeTruthy();
    expect(within(finished).getByText(/Skipped: the tool is busy/)).toBeTruthy();
    expect(within(finished).getByText(/Let it finish or close it/)).toBeTruthy();
    expect(screen.getByText("0 removed, 1 skipped, 0 failed")).toBeTruthy();
  });

  it("the report repeats a busy tool's hint", async () => {
    const busy = {
      ...REPORT.items[1],
      candidate_id: 9,
      catalog_id: "pkg-uv-cache",
      display_name: "~/.cache/uv",
      reason: "tool_busy",
      detail: "the tool is busy with another process. Try again later",
      hint: "Another uv program is using the cache. Close them, then run the cleanup again.",
    };
    await toReview(fakeEngine({ report: { ...REPORT, items: [REPORT.items[0], busy] } }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await screen.findByRole("heading", { name: "Report" });
    const skipped = screen.getByRole("region", { name: "Skipped" });
    expect(within(skipped).getByText(/Another uv program is using the cache/)).toBeTruthy();
  });
});

describe("developer assets", () => {
  async function toDeep() {
    renderCleanUp(fakeEngine({ scan: deepScan() }));
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    await screen.findByRole("heading", { name: "Results" });
  }

  it("lists Docker volumes, images, containers, and the disk file for information only", async () => {
    await toDeep();
    const assets = screen.getByRole("region", { name: "Developer assets" });
    expect(within(assets).getByText(/Information only/)).toBeTruthy();
    expect(within(assets).queryAllByRole("checkbox")).toEqual([]);
    expect(within(assets).queryByRole("button", { name: /Show in folder/ })).toBeNull();

    const volumes = within(assets).getByRole("table", { name: "Docker volumes" });
    const volumeRows = within(volumes)
      .getAllByRole("row")
      .slice(1)
      .map((r) => r.textContent);
    expect(volumeRows).toEqual([
      "example_db14.2 GB1 container",
      "example_cache482 MBNot used by a container right now",
      "Unnamed volume (dc487f67a881)0 bytesNot used by a container right now",
    ]);
    expect(within(assets).getByText(/Space Optimiser never removes them/)).toBeTruthy();

    const images = within(assets).getByRole("table", { name: "Docker images" });
    const first = within(images).getAllByRole("row")[1];
    expect(first.textContent).toBe("example/backend:latest1.4 GB1.3 GB1");
    expect(within(images).getByText("Untagged image")).toBeTruthy();

    const containers = within(assets).getByRole("table", { name: "Docker containers" });
    expect(within(containers).getByText("example-backend-1")).toBeTruthy();
    expect(within(containers).getByText("Running (Up 25 minutes)")).toBeTruthy();

    expect(within(assets).getByText(/uses 30.6 GB on disk/)).toBeTruthy();
    expect(within(assets).getByText(/never deletes it/)).toBeTruthy();
  });

  it("the tool cache table says which caches are offered and why others are not", async () => {
    await toDeep();
    const assets = screen.getByRole("region", { name: "Developer assets" });
    const table = within(assets).getByRole("table", { name: "Developer tool caches" });
    const rows = within(table)
      .getAllByRole("row")
      .slice(1)
      .map((r) => r.textContent);
    expect(rows).toContain("pnpm store~/Library/pnpm/store/v3Up to 3.0 MBOffered");
    expect(rows).toContain("pip cache~/Library/Caches/pipUp to 1.0 MBNot offered: not sure");
    expect(rows).toContain("Docker build cacheDocker did not respond.");
  });

  it("the chip shows only the assets and selection totals ignore them", async () => {
    await toDeep();
    const categories = within(screen.getByRole("group", { name: "Categories" }));
    fireEvent.click(categories.getByRole("button", { name: "Developer assets" }));
    expect(screen.getByRole("region", { name: "Developer assets" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Temporary files" })).toBeNull();
    expect(screen.getByText(/0 items selected, about 0 bytes/)).toBeTruthy();
  });

  it("a normal scan has no Developer assets", async () => {
    await toResults();
    expect(screen.queryByRole("region", { name: "Developer assets" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Developer assets" })).toBeNull();
  });
});

// Phase 7 test 10.
describe("deep scan", () => {
  async function toDeepResults(engine = fakeEngine({ scan: deepScan() })) {
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Deep Scan" }));
    await screen.findByRole("heading", { name: "Results" });
    return engine;
  }

  it("the start screen says what Deep Scan adds", async () => {
    renderCleanUp(fakeEngine());
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
    expect(box("IntelliJ IDEA 2024.3 — index").checked).toBe(false);
  });

  // The ide-index-caches row: under Developer tools, named after the editor.
  it("editor caches sit under Developer tools and say which editor to quit", async () => {
    await toDeepResults();
    const code = box("VS Code — CachedData");
    expect(code.disabled).toBe(true);
    expect(screen.getByText("Quit VS Code first")).toBeTruthy();
    expect(
      screen.getByText("VS Code is open, so its cache is not offered. Quit it, then scan again."),
    ).toBeTruthy();
    const categories = within(screen.getByRole("group", { name: "Categories" }));
    fireEvent.click(categories.getByRole("button", { name: /^Developer tools/ }));
    expect(screen.getByRole("region", { name: "Editor cache" })).toBeTruthy();
    expect(box("IntelliJ IDEA 2024.3 — index").disabled).toBe(false);
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
          reason: null,
          detail: null,
          bytes_removed: 0,
          recovery_text: "The next install will download packages again.",
          hint: null,
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
    renderCleanUp(fakeEngine({ scan: deepScan() }));
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
    renderCleanUp(fakeEngine({ scan: deepScan() }));
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
  it("has the five views, opens on the Overview, and marks the current one", async () => {
    render(<App engine={fakeEngine()} />);
    const main = screen.getByRole("navigation", { name: "Main" });
    expect(
      within(main)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Overview", "Clean up", "Settings", "Help", "About"]);
    expect(nav("Overview").getAttribute("aria-current")).toBe("page");
    expect(await screen.findByRole("heading", { level: 1, name: "Overview" })).toBeTruthy();
    fireEvent.click(nav("Clean up"));
    expect(nav("Clean up").getAttribute("aria-current")).toBe("page");
    fireEvent.click(nav("Help"));
    expect(nav("Help").getAttribute("aria-current")).toBe("page");
    expect(nav("Clean up").getAttribute("aria-current")).toBeNull();
  });

  it("shows the version from the engine", async () => {
    renderCleanUp(fakeEngine());
    expect(await screen.findByText("Version 0.1.0")).toBeTruthy();
  });

  it("a view change moves focus to that view's heading", async () => {
    renderCleanUp(fakeEngine());
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
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Normal Scan" }));
    fireEvent.click(nav("Help"));
    await waitFor(() => expect(engine.getScan).toHaveBeenCalled());
    fireEvent.click(nav("Clean up"));
    await screen.findByRole("heading", { name: "Results" });
  });

  it("Settings, Help, and About have no primary button", async () => {
    renderCleanUp(fakeEngine());
    for (const view of ["Settings", "Help", "About"]) {
      fireEvent.click(nav(view));
      await screen.findByRole("heading", { level: 1, name: view });
      expect(document.querySelectorAll(".button-primary").length).toBe(0);
    }
  });
});

describe("overview", () => {
  async function openOverview(engine = fakeEngine()) {
    render(<App engine={engine} />);
    await screen.findByText("312 GB");
    await screen.findByRole("table", { name: "Most memory" });
    return engine;
  }

  it("shows memory, swap, and CPU as text and as meters", async () => {
    await openOverview();
    const main = screen.getByRole("main");
    expect(within(main).getByText("11.0 GB of 16.0 GB in use")).toBeTruthy();
    expect(within(main).getByText("512 MB of 2.0 GB in use")).toBeTruthy();
    expect(within(main).getByText("23% of 10 CPUs")).toBeTruthy();
    const memory = within(main).getByRole("meter", { name: "Memory" });
    expect(memory.getAttribute("aria-valuenow")).toBe("69");
    expect(within(main).getByRole("meter", { name: "CPU" }).getAttribute("aria-valuenow")).toBe(
      "23",
    );
    expect(within(main).getByRole("meter", { name: "Swap" })).toBeTruthy();
  });

  it("lists the processes using the most memory and the most CPU, by name only", async () => {
    await openOverview();
    const memory = screen.getByRole("table", { name: "Most memory" });
    const rows = within(memory).getAllByRole("row").slice(1);
    expect(rows.map((r) => within(r).getAllByRole("cell")[1].textContent)).toEqual([
      "3.0 GB",
      "2.0 GB",
      "1.0 GB",
    ]);
    expect(within(rows[0]).getByText("× 14")).toBeTruthy();
    const cpu = screen.getByRole("table", { name: "Most CPU" });
    const cpuRows = within(cpu).getAllByRole("row").slice(1);
    expect(cpuRows.map((r) => within(r).getAllByRole("cell")[1].textContent)).toEqual([
      "12.5%",
      "4.2%",
    ]);
  });

  it("a process name is rendered as text, never as markup", async () => {
    await openOverview();
    expect(screen.getByText(XSS_NAME)).toBeTruthy();
    expect(document.querySelector("img[src='x']")).toBeNull();
  });

  it("says CPU is still being measured on the first reading", async () => {
    render(
      <App
        engine={fakeEngine({
          systemUsage: { ...SYSTEM_USAGE, cpu_percent: null, top_cpu: [] },
        })}
      />,
    );
    expect(await screen.findAllByText("Measuring…")).not.toHaveLength(0);
    expect(screen.queryByRole("meter", { name: "CPU" })).toBeNull();
  });

  it("draws the disk as a chart with its numbers in the name and as text", async () => {
    await openOverview();
    const chart = screen.getByRole("img", { name: /^Disk:/ });
    expect(chart.getAttribute("aria-label")).toBe(
      "Disk: 312 GB used, 182 GB free, of 494 GB. 63% used.",
    );
    expect(screen.getByText("312 GB")).toBeTruthy();
    expect(screen.getByText("182 GB")).toBeTruthy();
    expect(screen.getByText("494 GB")).toBeTruthy();
  });

  it("explains purgeable space on macOS only", async () => {
    await openOverview(fakeEngine({ platform: "macos" }));
    expect(await screen.findByText(/purgeable/)).toBeTruthy();
    cleanup();
    await openOverview(fakeEngine({ platform: "linux" }));
    expect(screen.queryByText(/purgeable/)).toBeNull();
  });

  it("has one primary button, Clean up, which opens Clean up", async () => {
    await openOverview();
    const primary = document.querySelectorAll(".button-primary");
    expect(primary).toHaveLength(1);
    expect(primary[0].textContent).toBe("Clean up");
    fireEvent.click(primary[0]);
    expect(await screen.findByRole("heading", { name: "Free up space safely" })).toBeTruthy();
    expect(nav("Clean up").getAttribute("aria-current")).toBe("page");
  });

  it("shows an error for a reading that fails and keeps the other", async () => {
    render(
      <App
        engine={fakeEngine({ systemError: { kind: "unsupported", message: "not supported" } })}
      />,
    );
    expect(await screen.findByText("Could not read memory and CPU: Not supported")).toBeTruthy();
    expect(await screen.findByText("312 GB")).toBeTruthy();
  });

  it("reads again every 2 seconds and stops when another view opens", async () => {
    vi.useFakeTimers();
    try {
      const engine = fakeEngine();
      render(<App engine={engine} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(engine.systemUsage).toHaveBeenCalledTimes(1);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(engine.systemUsage).toHaveBeenCalledTimes(2);
      expect(engine.diskUsage).toHaveBeenCalledTimes(2);
      fireEvent.click(nav("Settings"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });
      expect(engine.systemUsage).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("sends no arguments to either reading", async () => {
    const engine = await openOverview();
    expect(engine.systemUsage.mock.calls[0]).toEqual([]);
    expect(engine.diskUsage.mock.calls[0]).toEqual([]);
  });
});

describe("updates", () => {
  async function openSettings(engine = fakeEngine()) {
    render(<App engine={engine} />);
    fireEvent.click(nav("Settings"));
    await screen.findByRole("heading", { level: 2, name: "Updates" });
    return engine;
  }
  const checkButton = () => screen.getByRole("button", { name: "Check for updates" });

  it("Settings shows the version and contacts nothing until asked", async () => {
    const engine = await openSettings();
    expect(screen.getByText("You have version 0.1.0.")).toBeTruthy();
    expect(engine.checkForUpdate).not.toHaveBeenCalled();
    expect(
      (screen.getByRole("checkbox", { name: /when Space Optimiser starts/ }) as HTMLInputElement)
        .checked,
    ).toBe(false);
  });

  it("says when the app is up to date", async () => {
    const engine = await openSettings();
    fireEvent.click(checkButton());
    expect(await screen.findByText("Space Optimiser is up to date (version 0.1.0).")).toBeTruthy();
    expect(engine.checkForUpdate).toHaveBeenCalledTimes(1);
    expect(engine.checkForUpdate.mock.calls[0]).toEqual([]);
  });

  it("shows a new version's notes as text and installs it only when asked", async () => {
    const engine = await openSettings(
      fakeEngine({
        updateCheck: UPDATE_AVAILABLE,
        updateProgress: [
          { state: "downloading", version: "0.6.0", downloaded: 3_000_000, total: 12_000_000 },
          { state: "installed", version: "0.6.0" },
        ],
      }),
    );
    fireEvent.click(checkButton());
    expect(await screen.findByText("Version 0.6.0 is available. You have 0.1.0.")).toBeTruthy();
    expect(screen.getByText(/Overview\*\*, a new first page/)).toBeTruthy();
    expect(document.querySelector("img[src='x']")).toBeNull();
    expect(engine.installUpdate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Download and install" }));
    expect(await screen.findByText("Downloading version 0.6.0: 3.0 MB of 12.0 MB")).toBeTruthy();
    expect(screen.getByRole("progressbar", { name: "Download progress" })).toBeTruthy();
    expect(
      await screen.findByText("Version 0.6.0 is installed. Restart Space Optimiser to use it."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(engine.restartApp).toHaveBeenCalledTimes(1));
  });

  it("says why it waits after the download", async () => {
    await openSettings(
      fakeEngine({
        updateCheck: UPDATE_AVAILABLE,
        updateProgress: [{ state: "waiting", version: "0.6.0" }],
      }),
    );
    fireEvent.click(checkButton());
    fireEvent.click(await screen.findByRole("button", { name: "Download and install" }));
    expect(
      await screen.findByText(
        "Version 0.6.0 is downloaded and verified. It installs when the scan or cleanup finishes.",
      ),
    ).toBeTruthy();
  });

  it("an install refused during a cleanup says so", async () => {
    await openSettings(
      fakeEngine({
        updateCheck: UPDATE_AVAILABLE,
        installError: {
          kind: "busy",
          message: "a scan or cleanup is running. Try again when it has finished.",
        },
      }),
    );
    fireEvent.click(checkButton());
    fireEvent.click(await screen.findByRole("button", { name: "Download and install" }));
    expect(
      await screen.findByText("A scan or cleanup is running. Try again when it has finished."),
    ).toBeTruthy();
  });

  it("on Linux it links to the download page instead of installing", async () => {
    const engine = await openSettings(
      fakeEngine({ updateCheck: { ...UPDATE_AVAILABLE, route: "linux_package" } }),
    );
    fireEvent.click(checkButton());
    expect(await screen.findByText(/needs an administrator password/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Download and install" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open download page" }));
    await waitFor(() => expect(engine.openDownloadPage).toHaveBeenCalledTimes(1));
    expect(engine.openDownloadPage.mock.calls[0]).toEqual([]);
  });

  it("on a Mac whose app folder is locked it links to the download page", async () => {
    await openSettings(
      fakeEngine({ updateCheck: { ...UPDATE_AVAILABLE, route: "app_folder_locked" } }),
    );
    fireEvent.click(checkButton());
    expect(await screen.findByText(/cannot change the folder Space Optimiser is in/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open download page" })).toBeTruthy();
  });

  it("a failed check shows the reason", async () => {
    await openSettings(
      fakeEngine({
        updateCheck: { state: "failed", message: "could not check for updates: timed out" },
      }),
    );
    fireEvent.click(checkButton());
    expect(await screen.findByText("Could not check for updates: timed out")).toBeTruthy();
  });

  it("does not check at start unless that setting is on", async () => {
    const engine = fakeEngine({ updateCheck: UPDATE_AVAILABLE });
    render(<App engine={engine} />);
    await screen.findByRole("heading", { level: 1, name: "Overview" });
    expect(engine.checkForUpdate).not.toHaveBeenCalled();
  });

  it("the start-up check is saved, runs once, and the sidebar points to the update", async () => {
    await openSettings();
    fireEvent.click(screen.getByRole("checkbox", { name: /when Space Optimiser starts/ }));
    expect(localStorage.getItem("space-optimiser:check-updates-at-start")).toBe("on");
    cleanup();

    const engine = fakeEngine({ updateCheck: UPDATE_AVAILABLE });
    render(<App engine={engine} />);
    const notice = await screen.findByRole("button", { name: "Version 0.6.0 is available" });
    expect(engine.checkForUpdate).toHaveBeenCalledTimes(1);
    fireEvent.click(notice);
    expect(await screen.findByText("Version 0.6.0 is available. You have 0.1.0.")).toBeTruthy();
    expect(nav("Settings").getAttribute("aria-current")).toBe("page");
    expect(engine.checkForUpdate).toHaveBeenCalledTimes(1);
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
    renderCleanUp(fakeEngine());
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("the setting is applied and kept for the next launch", async () => {
    renderCleanUp(fakeEngine());
    fireEvent.click(nav("Settings"));
    const dark = screen.getByRole("radio", { name: /Dark/ }) as HTMLInputElement;
    expect(dark.checked).toBe(true);
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(localStorage.getItem("space-optimiser:theme")).toBe("light");
    cleanup();
    delete document.documentElement.dataset.theme;
    renderCleanUp(fakeEngine());
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("Match system follows the system setting as it changes", async () => {
    const change = mockMatchMedia(true);
    renderCleanUp(fakeEngine());
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
    renderCleanUp(fakeEngine());
    expect(document.documentElement.dataset.theme).toBe("dark");
    fireEvent.click(nav("Settings"));
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("about and help", () => {
  it("About shows the version, the safety wording, no upload, and the audit log", async () => {
    renderCleanUp(fakeEngine());
    fireEvent.click(nav("About"));
    await screen.findByText("Version 0.1.0", { selector: "main *" });
    const text = screen.getByRole("main").textContent ?? "";
    expect(text).toContain(
      "Space Optimiser only removes files in locations it has a reviewed rule for, and only after you confirm. It does not clean your documents, photos, downloads, or source code, and it does not change system settings, passwords, or app configuration. A confirmed cleanup can still make an app rebuild a cache or download files again.",
    );
    expect(text).toContain("This app does not upload a list of your files.");
    expect(text).toContain("/Users/test/Library/Application Support/Space Optimiser/audit.jsonl");
    expect(text).toContain("You can delete this file yourself. Space Optimiser does not clean it.");
    expect(text).toContain("Check for updates in Settings.");
    expect(text).toContain("SIL Open Font License");
    expect(text).toContain("MIT License");
    expect(text).toContain("Permission is hereby granted, free of charge");
    expect(text).not.toContain(ABSOLUTE_PROMISE);
  });

  it("About links to the local Help page", async () => {
    renderCleanUp(fakeEngine());
    fireEvent.click(nav("About"));
    fireEvent.click(await screen.findByRole("button", { name: "Open Help" }));
    expect(screen.getByRole("heading", { level: 1, name: "Help" })).toBeTruthy();
  });

  it("Help explains the essentials without the absolute promise", async () => {
    renderCleanUp(fakeEngine());
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
describe("readable temporary files and Left alone", () => {
  const temp = (id: number, display_name: string, over = {}) => ({
    ...CANDIDATES[0],
    id,
    display_name,
    estimated_bytes: 5000,
    kind: "folder" as const,
    age_days: 3,
    file_count: 12,
    ...over,
  });
  const scanWith = () =>
    finishedScan({
      candidates: [
        temp(40, "com.apple.avconferenced"),
        temp(41, ".com.openai.codex.9XF9pN", { kind: "file", file_count: 1, age_days: 1 }),
        temp(42, ".tmpOJTzZZ", { estimated_bytes: 0 }),
        temp(43, "com.apple.ThreadCommissionerService", { estimated_bytes: 0 }),
        temp(44, "installer-log", { age_days: 90 }),
      ],
      skipped: [
        { display_name: "fresh-1", reason: "fresh", detail: "it changed within the last day" },
        { display_name: "fresh-2", reason: "fresh", detail: "it changed within the last day" },
        { display_name: "my-repo", reason: "project", detail: "it looks like a project folder" },
      ],
    });

  it("groups temporary rows by source and says what each row is", async () => {
    await toResults(fakeEngine({ scan: scanWith() }));
    const group = screen.getByRole("region", { name: "Temporary files" });
    const headings = within(group)
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(headings).toEqual(["App temporary folders", "macOS system services", "Other"]);
    expect(within(group).getByText("File · 1 day old")).toBeTruthy();
    expect(within(group).getByText("Folder, 12 files · 3 months old")).toBeTruthy();
  });

  it("folds empty items into one line that Select safe items still selects", async () => {
    const engine = await toResults(fakeEngine({ scan: scanWith() }));
    expect(screen.queryByText(".tmpOJTzZZ")).toBeNull();
    const fold = screen.getByRole("button", { name: "Show 2 empty items (0 bytes)" });
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
    expect(screen.getByText(/5 items selected/)).toBeTruthy();
    fireEvent.click(fold);
    expect(fold.getAttribute("aria-expanded")).toBe("true");
    expect(box(".tmpOJTzZZ").checked).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("heading", { name: "Review" });
    expect(engine.buildPlan.mock.calls[0][1]).toEqual(expect.arrayContaining([42, 43]));
  });

  it("the name filter finds a folded empty item", async () => {
    await toResults(fakeEngine({ scan: scanWith() }));
    fireEvent.change(screen.getByLabelText("Filter by name"), { target: { value: "tmpOJ" } });
    expect(screen.getByText(".tmpOJTzZZ")).toBeTruthy();
  });

  it("Left alone groups items by reason with a plain explanation", async () => {
    await toResults(fakeEngine({ scan: scanWith() }));
    const left = screen.getByText("Left alone (3 items)").closest("details")!;
    const reasons = within(left)
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(reasons).toEqual(["Changed in the last day (2 items)", "Looks like a project (1 item)"]);
    expect(within(left).getByText(/Nothing here is removed/)).toBeTruthy();
    expect(within(left).getByText(/may be your own work/)).toBeTruthy();
    expect(within(left).getByText("my-repo")).toBeTruthy();
  });
});

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

// The tools added in v0.5: Projects, Android, and the on-device AI model row.
describe("projects tool", () => {
  async function toProjects(engine = fakeEngine({ scan: projectsScan() })) {
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Scan projects" }));
    await screen.findByRole("heading", { name: "Results" });
    return engine;
  }

  it("starts the projects mode, and Scan again reruns it", async () => {
    const engine = await toProjects();
    const header = screen.getByRole("heading", { name: "Results" }).parentElement!;
    fireEvent.click(within(header).getByRole("button", { name: "Scan again" }));
    await waitFor(() => expect(engine.startScan).toHaveBeenCalledTimes(2));
    expect(engine.startScan.mock.calls.map((c) => c[0])).toEqual(["projects", "projects"]);
  });

  it("says what it is doing while it scans", async () => {
    const engine = fakeEngine({
      scanRuns: true,
      scanProgress: {
        files_seen: 9_001,
        bytes_seen: 1_200_000_000,
        stage: "projects",
        detail: null,
      },
      scanStages: ["projects"],
    });
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Scan projects" }));
    expect(
      await screen.findByText("Looking for dependency and build folders in your projects."),
    ).toBeTruthy();
    const stages = within(screen.getByRole("list", { name: "Scan stages" }))
      .getAllByRole("listitem")
      .map((li) => li.textContent);
    expect(stages).toEqual(["Project foldersChecking"]);
  });

  it("groups folders into old projects, active projects, and not offered", async () => {
    await toProjects();
    const group = screen.getByRole("region", { name: "Project folders" });
    const headings = within(group)
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(headings).toEqual(["Old projects", "Active projects", "Not offered"]);
    expect(within(group).getByText("1 folder, 1.8 GB")).toBeTruthy();
    expect(within(group).getByText("1 folder, 5.5 GB")).toBeTruthy();
  });

  it("each row says how old its project is, with its risk as a word", async () => {
    await toProjects();
    expect(screen.getByText("Old project: nothing in it changed for 94 days.")).toBeTruthy();
    expect(screen.getByText("Active project: last changed 2 days ago.")).toBeTruthy();
    expect(
      screen.getByRole("checkbox", {
        name: "~/Projects/old-shop/node_modules, Medium risk, 1.8 GB",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("checkbox", { name: "~/Projects/engine/target, High risk, 5.5 GB" }),
    ).toBeTruthy();
  });

  it("a project folder's path is shown whole, not cut short", async () => {
    await toProjects();
    const row = box("~/Tools/some-app/resources/app/node_modules").closest("li")!;
    expect(row.classList.contains("row-path")).toBe(true);
  });

  // pnpm hard-links packages from its store: those files are not freed and
  // not counted, but the rest of the folder is.
  it("a folder with files shared from a package store says they are not counted", async () => {
    const shared = PROJECT_CANDIDATES.map((c) =>
      c.id === 60 ? { ...c, labels: [...c.labels, "shared" as const], file_count: 41_000 } : c,
    );
    await toProjects(fakeEngine({ scan: finishedScan({ candidates: shared, skipped: [] }) }));
    const row = box("~/Projects/old-shop/node_modules").closest("li")!;
    expect(row.textContent).toContain("1.8 GB (files shared with another folder are not counted)");
    expect(row.textContent).not.toContain("frees nothing");
  });

  it("a folder outside source control is shown, says why, and cannot be chosen", async () => {
    await toProjects();
    const row = box("~/Tools/some-app/resources/app/node_modules");
    expect(row.disabled).toBe(true);
    expect(screen.getByText(/Not inside a source-control folder/)).toBeTruthy();
    expect(screen.getByText("Not sure — not offered")).toBeTruthy();
  });

  it("nothing is preselected and Select safe items has nothing to select", async () => {
    await toProjects();
    for (const checkbox of screen.getAllByRole("checkbox") as HTMLInputElement[]) {
      expect(checkbox.checked).toBe(false);
    }
    const safe = screen.getByRole("button", { name: "Select safe items" }) as HTMLButtonElement;
    expect(safe.disabled).toBe(true);
    expect(
      screen.getByText("Nothing here is selected for you. Tick each item you want removed."),
    ).toBeTruthy();
    expect(screen.queryByText(/left unselected/)).toBeNull();
    expect((screen.getByRole("button", { name: "Review" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("a chosen folder goes to review with the consequence written out", async () => {
    const engine = await toProjects();
    fireEvent.click(box("~/Projects/old-shop/node_modules"));
    expect(screen.getByText(/^1 item selected, about 1\.8 GB/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("heading", { name: "Review" });
    expect(engine.buildPlan).toHaveBeenCalledWith(7, [60]);
    expect(
      screen.getByText(/will not build or run until you run its install or build command again/),
    ).toBeTruthy();
    expect(screen.getByText(/This cannot be undone/)).toBeTruthy();
  });
});

describe("android tool", () => {
  async function toAndroid(engine = fakeEngine({ scan: androidScan() })) {
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Scan Android" }));
    await screen.findByRole("heading", { name: "Results" });
    return engine;
  }

  it("starts the android mode and names its stage", async () => {
    const engine = fakeEngine({
      scanRuns: true,
      scanProgress: { files_seen: 12, bytes_seen: 1, stage: "android", detail: null },
      scanStages: ["android"],
    });
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Scan Android" }));
    expect(await screen.findByText("Checking Android emulators and system images.")).toBeTruthy();
    expect(engine.startScan).toHaveBeenCalledWith("android");
  });

  it("an image in use names its emulator and has no checkbox", async () => {
    await toAndroid();
    expect(screen.getByText("In use by Pixel 9.")).toBeTruthy();
    expect(
      screen.queryByRole("checkbox", {
        name: /^android-36\.1 — google_apis_playstore — arm64-v8a, /,
      }),
    ).toBeNull();
  });

  it("an unused image can be chosen and is marked High", async () => {
    await toAndroid();
    expect(screen.getByText("No emulator uses this image.")).toBeTruthy();
    const unused = screen.getByRole("checkbox", {
      name: "android-30 — default — x86_64, High risk, 4.1 GB",
    }) as HTMLInputElement;
    expect(unused.disabled).toBe(false);
    expect(unused.checked).toBe(false);
  });

  it("a running emulator keeps its snapshot back", async () => {
    await toAndroid();
    expect(box("Pixel 9 — Quick Boot snapshot").disabled).toBe(true);
    expect(screen.getByText("Quit Android Emulator first")).toBeTruthy();
  });

  it("each emulator is listed for information and says its data is never removed", async () => {
    await toAndroid();
    const group = screen.getByRole("region", { name: "Android emulators" });
    expect(within(group).queryByRole("checkbox")).toBeNull();
    expect(
      within(group).getByText(/Apps and data inside the emulator are never removed/),
    ).toBeTruthy();
    expect(within(group).getByText("18.0 GB")).toBeTruthy();
  });

  it("the rows sit under Android and Projects categories of their own", async () => {
    const mixed = finishedScan({
      candidates: [...CANDIDATES, ...ANDROID_CANDIDATES, ...PROJECT_CANDIDATES],
    });
    await toAndroid(fakeEngine({ scan: mixed }));
    const chips = within(screen.getByRole("group", { name: "Categories" }))
      .getAllByRole("button")
      .map((b) => b.textContent);
    expect(chips).toContain("Android (4, 30.9 GB)");
    expect(chips).toContain("Projects (3, 7.6 GB)");
    fireEvent.click(screen.getByRole("button", { name: /^Android \(/ }));
    expect(screen.getByRole("region", { name: "Android system images" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Project folders" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Temporary files" })).toBeNull();
  });
});

describe("on-device AI model", () => {
  async function toModel() {
    const engine = fakeEngine({
      scan: finishedScan({ candidates: [...CANDIDATES, ON_DEVICE_MODEL] }),
    });
    renderCleanUp(engine);
    fireEvent.click(await screen.findByRole("button", { name: "Normal Scan" }));
    await screen.findByRole("heading", { name: "Results" });
    return engine;
  }

  it("is shown with its size, what it is, and how to turn it off", async () => {
    await toModel();
    const group = screen.getByRole("region", { name: "On-device AI model" });
    expect(within(group).getByText("1 item, 4.0 GB")).toBeTruthy();
    const text = group.textContent ?? "";
    expect(text).toContain("An AI model that Chrome downloaded to this computer");
    expect(text).toContain('search for "On-device AI"');
    expect(text).toContain("Chrome downloads it again");
  });

  it("cannot be chosen, and Select safe items leaves it alone", async () => {
    const engine = await toModel();
    const group = screen.getByRole("region", { name: "On-device AI model" });
    expect(within(group).queryByRole("checkbox")).toBeNull();
    expect(within(group).getByText("Show only")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Select safe items" }));
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("heading", { name: "Review" });
    const ids = engine.buildPlan.mock.calls[0][1];
    expect(ids).not.toContain(ON_DEVICE_MODEL.id);
  });

  it("can be shown in its folder and sits under Browsers", async () => {
    const engine = await toModel();
    fireEvent.click(
      screen.getByRole("button", { name: "Show in folder: Google Chrome — on-device AI model" }),
    );
    await waitFor(() => expect(engine.reveal).toHaveBeenCalledWith(ON_DEVICE_MODEL.id));
    fireEvent.click(screen.getByRole("button", { name: /^Browsers \(/ }));
    expect(screen.getByRole("region", { name: "On-device AI model" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Temporary files" })).toBeNull();
  });
});

describe("help for the tools", () => {
  it("explains Projects, Android, and the on-device model", async () => {
    renderCleanUp(fakeEngine());
    fireEvent.click(await screen.findByRole("button", { name: "Help" }));
    const text = screen.getByRole("main").textContent ?? "";
    expect(text).toContain("Projects and Android");
    expect(text).toContain("source-control folder (.git)");
    expect(text).toContain("Quick Boot snapshot");
    expect(text).toContain("never removed");
    expect(text).toContain("Chrome would download it again");
    expect(text).not.toContain(ABSOLUTE_PROMISE);
  });
});
