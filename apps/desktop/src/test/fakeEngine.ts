// A scripted engine for component tests. Nothing here touches files.
import { vi } from "vitest";
import type {
  AppInfo,
  Candidate,
  Engine,
  EngineError,
  ExecuteStart,
  InterruptedRun,
  Plan,
  Report,
  ScanView,
} from "../engine";

export const XSS_NAME = "<img src=x onerror=alert(1)>";

function candidate(over: Partial<Candidate> & Pick<Candidate, "id" | "display_name">): Candidate {
  return {
    catalog_id: "user-temp",
    explanation: "Temporary files more than a day old. Apps recreate what they still need.",
    location_label: "Temporary files",
    estimated_bytes: 4096,
    upper_bound_bytes: null,
    found_by: null,
    covers: [],
    kind: null,
    age_days: null,
    file_count: 1,
    links_not_followed: 0,
    labels: [],
    risk: "low",
    bulk_safe: true,
    confidence: "high",
    action: "delete",
    platform: "macos",
    revealable: true,
    ...over,
  };
}

export const CANDIDATES: Candidate[] = [
  candidate({ id: 1, display_name: "old-download.bin", estimated_bytes: 2 * 1024 * 1024 }),
  candidate({ id: 2, display_name: XSS_NAME }),
  candidate({
    id: 3,
    display_name: "Browser cache",
    catalog_id: "browser-http-cache",
    location_label: "Browser cache",
    explanation: "Copies of web pages and images. Pages load from the network again.",
    risk: "medium",
    bulk_safe: false,
  }),
  candidate({
    id: 4,
    display_name: "video.mov",
    catalog_id: "large-files-report",
    location_label: "Home folder",
    explanation: "Large files in your home folder. Shown so you can decide; never removed.",
    risk: "info",
    bulk_safe: false,
    action: "report",
  }),
  candidate({
    id: 5,
    display_name: "unsure-folder",
    confidence: "uncertain",
    action: "none",
    bulk_safe: false,
  }),
  candidate({
    id: 6,
    display_name: "Firefox — abcd.default",
    catalog_id: "browser-http-cache",
    location_label: "Browser cache",
    explanation: "Copies of web pages and images. Pages load from the network again.",
    risk: "medium",
    bulk_safe: false,
    action: "none",
    labels: ["quit_first"],
  }),
];

/** Deep Scan rows: an overlapping pair, a tool row, a default folder. */
export const DEEP_CANDIDATES: Candidate[] = [
  candidate({ id: 20, display_name: "registry", estimated_bytes: 8000, covers: [21] }),
  candidate({
    id: 21,
    display_name: "~/.cargo/registry/cache",
    catalog_id: "pkg-cargo-cache",
    location_label: "Cargo downloads",
    explanation: "Downloaded Rust crates and git dependencies.",
    estimated_bytes: 5000,
    risk: "medium",
    bulk_safe: false,
  }),
  candidate({
    id: 22,
    display_name: "~/Library/pnpm/store/v3",
    catalog_id: "pkg-pnpm-store",
    location_label: "pnpm store",
    explanation: "pnpm packages that no project references.",
    estimated_bytes: 0,
    upper_bound_bytes: 3_000_000,
    found_by: "Folder from `pnpm store path`; pnpm found on PATH.",
    risk: "medium",
    bulk_safe: false,
    action: "run_tool",
  }),
  candidate({
    id: 23,
    display_name: "~/Library/Caches/pip",
    catalog_id: "pkg-pip-cache",
    location_label: "pip cache",
    explanation: "Downloaded Python wheels.",
    estimated_bytes: 0,
    upper_bound_bytes: 1_000_000,
    found_by: "Default folder; pip was not found.",
    risk: "medium",
    bulk_safe: false,
    confidence: "uncertain",
    action: "none",
  }),
  // Its path is the home folder, so the engine marks it not revealable.
  candidate({
    id: 24,
    display_name: "Docker build cache",
    catalog_id: "docker-build-cache",
    location_label: "Docker build cache",
    explanation: "Layers Docker kept from earlier image builds.",
    estimated_bytes: 0,
    upper_bound_bytes: 500_000_000,
    found_by: "Reported by `docker system df`.",
    risk: "medium",
    bulk_safe: false,
    action: "run_tool",
    revealable: false,
  }),
];

export function deepScan(): ScanView {
  return finishedScan({
    candidates: DEEP_CANDIDATES,
    notes: [
      {
        catalog_id: "docker-build-cache",
        location_label: "Docker build cache",
        text: "Docker did not respond.",
        bytes: null,
      },
      {
        catalog_id: "docker-build-cache",
        location_label: "Docker build cache",
        text: "Docker volumes could free this much, but they hold databases and are never offered.",
        bytes: 2_000_000_000,
      },
    ],
  });
}

export function finishedScan(over: Partial<ScanView> = {}): ScanView {
  return {
    scan_id: 7,
    status: "finished",
    stages: ["temp", "caches"],
    progress: { files_seen: 12, bytes_seen: 3 * 1024 * 1024, stage: "caches", detail: null },
    duration_ms: 240,
    error: null,
    candidates: CANDIDATES,
    skipped: [
      { display_name: "fresh.part", reason: "fresh", detail: "it changed within the last day" },
    ],
    roots: [{ location_label: "Temporary files", skipped: null, frees_memory: false }],
    notes: [],
    ...over,
  };
}

export function planFor(ids: number[]): Plan {
  const all = [...CANDIDATES, ...DEEP_CANDIDATES];
  const items = all
    .filter((c) => ids.includes(c.id))
    .map((c) => ({
      candidate_id: c.id,
      catalog_id: c.catalog_id,
      display_name: c.display_name,
      action_text:
        c.action === "run_tool"
          ? "Run the tool's own clean command."
          : "Delete this item from your temporary folder. Everything in it is more than a day old.",
      estimated_bytes: c.estimated_bytes,
      upper_bound_bytes: c.upper_bound_bytes,
      file_count: c.file_count,
    }));
  return {
    plan_id: 11,
    scan_id: 7,
    items,
    estimated_bytes: items.reduce((sum, i) => sum + i.estimated_bytes, 0),
    tool_upper_bound_bytes: items.reduce((sum, i) => sum + (i.upper_bound_bytes ?? 0), 0),
    expires_in_secs: 900,
  };
}

export const REPORT: Report = {
  plan_id: 11,
  items: [
    {
      candidate_id: 1,
      catalog_id: "user-temp",
      display_name: "old-download.bin",
      outcome: "removed",
      detail: null,
      bytes_removed: 2 * 1024 * 1024,
      recovery_text: "Files older than a day were removed from the temporary folder.",
    },
    {
      candidate_id: 2,
      catalog_id: "user-temp",
      display_name: XSS_NAME,
      outcome: "skipped",
      detail: "it changed since the scan",
      bytes_removed: 0,
      recovery_text: null,
    },
  ],
  bytes_removed: 2 * 1024 * 1024,
  stopped: false,
  free_space: [
    {
      location_label: "Temporary files",
      before: 10 * 1024 * 1024 * 1024,
      after: 10 * 1024 * 1024 * 1024 + 50 * 1024 * 1024,
      after_delay_secs: 3,
      source: "free space reported by the system",
    },
  ],
  audit_log: "written",
  memory_log: [],
};

export const APP_INFO: AppInfo = {
  version: "0.1.0",
  platform: "macos",
  audit_log_path: "/Users/test/Library/Application Support/Space Optimiser/audit.jsonl",
  export_folder: "/Users/test/Downloads",
};

export interface FakeOptions {
  scan?: ScanView;
  /** The scan stays "running" until it is cancelled. */
  scanRuns?: boolean;
  /** Progress a running scan reports. */
  scanProgress?: ScanView["progress"];
  scanStages?: ScanView["stages"];
  execute?: ExecuteStart[];
  confirmError?: { kind: string; message: string };
  interrupted?: InterruptedRun | null;
  report?: Report;
  runStays?: boolean;
  platform?: AppInfo["platform"];
  revealError?: EngineError;
  exportError?: EngineError;
}

export function fakeEngine(options: FakeOptions = {}) {
  let cancelled = false;
  const executes = [...(options.execute ?? [{ status: "started" as const }])];
  const engine = {
    startScan: vi.fn<Engine["startScan"]>(async () => 7),
    cancelScan: vi.fn(async () => {
      cancelled = true;
    }),
    getScan: vi.fn(async (): Promise<ScanView> => {
      if (options.scanRuns) {
        return finishedScan({
          status: cancelled ? "cancelled" : "running",
          progress: options.scanProgress ?? {
            files_seen: 0,
            bytes_seen: 0,
            stage: "temp",
            detail: null,
          },
          stages: options.scanStages ?? ["temp", "caches"],
          candidates: [],
          skipped: [],
          roots: [],
        });
      }
      return options.scan ?? finishedScan();
    }),
    buildPlan: vi.fn(async (_scan: number, ids: number[]) => planFor(ids)),
    confirmPlan: vi.fn(async () => {
      if (options.confirmError) throw options.confirmError;
      return "token-1";
    }),
    executePlan: vi.fn(async () => executes.shift() ?? { status: "started" as const }),
    getRun: vi.fn(async () =>
      options.runStays
        ? { plan_id: 11, status: "running" as const, items_done: 1, items_total: 2, report: null }
        : {
            plan_id: 11,
            status: "finished" as const,
            items_done: 2,
            items_total: 2,
            report: options.report ?? REPORT,
          },
    ),
    stopRun: vi.fn(async () => {}),
    interruptedRun: vi.fn(async () => options.interrupted ?? null),
    reveal: vi.fn<Engine["reveal"]>(async () => {
      if (options.revealError) throw options.revealError;
    }),
    exportReport: vi.fn<Engine["exportReport"]>(async () => {
      if (options.exportError) throw options.exportError;
      const fileName = "space-optimiser-report.txt";
      return { path: `${APP_INFO.export_folder}/${fileName}`, file_name: fileName };
    }),
    appInfo: vi.fn(async (): Promise<AppInfo> => ({
      ...APP_INFO,
      platform: options.platform ?? APP_INFO.platform,
    })),
  } satisfies Engine;
  return engine;
}
