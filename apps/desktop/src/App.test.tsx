// Component tests with a fake engine (phase 7 tests 1-10, plus the M3 run
// screens). No test touches a file.
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import { XSS_NAME, fakeEngine, finishedScan } from "./test/fakeEngine";

afterEach(cleanup);

const ABSOLUTE_PROMISE = "will not delete personal data";

async function toResults(engine = fakeEngine()) {
  render(<App engine={engine} />);
  fireEvent.click(await screen.findByRole("button", { name: "Normal Scan" }));
  await screen.findByRole("heading", { name: "Results" });
  return engine;
}

function box(name: string): HTMLInputElement {
  return screen.getByRole("checkbox", { name }) as HTMLInputElement;
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
    const buttons = await screen.findAllByRole("button");
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
});

describe("results", () => {
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
    expect(screen.queryByRole("checkbox", { name: "video.mov" })).toBeNull();
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
    expect(screen.getByText(/Scan took 0\.2 seconds/)).toBeTruthy();
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
    expect(document.activeElement).toBe(screen.getByRole("heading", { name: "Results" }));
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
    expect(screen.getByText("1 of 2 items done")).toBeTruthy();
  });
});

// Phase 7 test 10.
it('no screen says "Select All"', async () => {
  await toReview();
  expect(document.body.textContent).not.toMatch(/select all/i);
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  await screen.findByRole("heading", { name: "Results" });
  expect(document.body.textContent).not.toMatch(/select all/i);
});
