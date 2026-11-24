// A scripted engine for component tests. Nothing here touches files.
import { vi } from "vitest";
import type {
  AppInfo,
  Candidate,
  DevAssets,
  DiskUsage,
  Engine,
  EngineError,
  ExecuteStart,
  InterruptedRun,
  Plan,
  Report,
  RunView,
  ScanView,
  SystemUsage,
  UpdateView,
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
    detail: null,
    file_count: 1,
    links_not_followed: 0,
    labels: [],
    quit_app: null,
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
    quit_app: "Firefox",
  }),
];

/** Deep Scan rows: an overlapping pair, a tool row, a default folder, editor caches. */
export const DEEP_CANDIDATES: Candidate[] = [
  candidate({
    id: 40,
    display_name: "VS Code — CachedData",
    catalog_id: "ide-index-caches",
    location_label: "Editor cache",
    explanation: "Editor caches and search indexes. The editor rebuilds them on its next start.",
    risk: "medium",
    bulk_safe: false,
    action: "none",
    labels: ["quit_first"],
    quit_app: "VS Code",
  }),
  candidate({
    id: 41,
    display_name: "IntelliJ IDEA 2024.3 — index",
    catalog_id: "ide-index-caches",
    location_label: "Editor cache",
    explanation: "Editor caches and search indexes. The editor rebuilds them on its next start.",
    risk: "medium",
    bulk_safe: false,
    quit_app: "IntelliJ IDEA",
  }),
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

const PROJECT_ROW = {
  catalog_id: "project-build-folders",
  location_label: "Project folders",
  explanation:
    "Dependency and build folders inside your projects: node_modules, Rust target, Python virtual environments, and .next.",
  bulk_safe: false,
} as const;

/** The Projects tool: an old project, an active one, and a folder outside source control. */
export const PROJECT_CANDIDATES: Candidate[] = [
  candidate({
    ...PROJECT_ROW,
    id: 60,
    display_name: "~/Projects/old-shop/node_modules",
    estimated_bytes: 1_800_000_000,
    risk: "medium",
    labels: ["old_project"],
    age_days: 94,
    detail: "Old project: nothing in it changed for 94 days.",
  }),
  candidate({
    ...PROJECT_ROW,
    id: 61,
    display_name: "~/Projects/engine/target",
    estimated_bytes: 5_500_000_000,
    risk: "high",
    labels: ["active_project"],
    age_days: 2,
    detail: "Active project: last changed 2 days ago.",
  }),
  candidate({
    ...PROJECT_ROW,
    id: 62,
    display_name: "~/Tools/some-app/resources/app/node_modules",
    estimated_bytes: 300_000_000,
    risk: "high",
    confidence: "uncertain",
    action: "none",
    detail:
      "Not inside a source-control folder (.git), so Space Optimiser cannot tell that it belongs to a project of yours. Not offered.",
  }),
];

/** The Android tool: a blocked snapshot, an unused image, an image in use, an emulator. */
export const ANDROID_CANDIDATES: Candidate[] = [
  candidate({
    id: 70,
    display_name: "Pixel 9 — Quick Boot snapshot",
    catalog_id: "android-avd-snapshots",
    location_label: "Emulator snapshots",
    explanation: "The saved state an Android emulator uses to start quickly.",
    estimated_bytes: 2_600_000_000,
    risk: "medium",
    bulk_safe: false,
    action: "none",
    labels: ["quit_first"],
    quit_app: "Android Emulator",
  }),
  candidate({
    id: 71,
    display_name: "android-30 — default — x86_64",
    catalog_id: "android-unreferenced-images",
    location_label: "Android system images",
    explanation: "Android system images in your SDK.",
    estimated_bytes: 4_100_000_000,
    risk: "high",
    bulk_safe: false,
    detail: "No emulator uses this image.",
  }),
  candidate({
    id: 72,
    display_name: "android-36.1 — google_apis_playstore — arm64-v8a",
    catalog_id: "android-unreferenced-images",
    location_label: "Android system images",
    explanation: "Android system images in your SDK.",
    estimated_bytes: 6_200_000_000,
    risk: "info",
    bulk_safe: false,
    action: "report",
    detail: "In use by Pixel 9.",
  }),
  candidate({
    id: 73,
    display_name: "Pixel 9",
    catalog_id: "android-emulators-report",
    location_label: "Android emulators",
    explanation: "Your Android emulators and the space each takes.",
    estimated_bytes: 18_000_000_000,
    risk: "info",
    bulk_safe: false,
    action: "report",
    detail:
      "Uses the system image android-36.1 — google_apis_playstore — arm64-v8a. Apps and data inside the emulator are never removed.",
  }),
];

/** Chrome's on-device AI model, as Normal Scan and Deep Scan report it. */
export const ON_DEVICE_MODEL: Candidate = candidate({
  id: 80,
  display_name: "Google Chrome — on-device AI model",
  catalog_id: "chrome-on-device-model",
  location_label: "On-device AI model",
  explanation:
    'An AI model that Chrome downloaded to this computer for its built-in AI features. To remove it, open Chrome\'s settings, search for "On-device AI", and turn it off. Chrome then deletes the model. If you delete the folder yourself, Chrome downloads it again.',
  estimated_bytes: 4_000_000_000,
  file_count: 6,
  risk: "info",
  bulk_safe: false,
  action: "report",
});

export function projectsScan(): ScanView {
  return finishedScan({
    stages: ["projects"],
    progress: { files_seen: 48_210, bytes_seen: 7_600_000_000, stage: "projects", detail: null },
    candidates: PROJECT_CANDIDATES,
    skipped: [],
    roots: [],
  });
}

export function androidScan(): ScanView {
  return finishedScan({
    stages: ["android"],
    progress: { files_seen: 312, bytes_seen: 30_900_000_000, stage: "android", detail: null },
    candidates: ANDROID_CANDIDATES,
    skipped: [],
    roots: [],
  });
}

export const DEV_ASSETS: DevAssets = {
  docker: {
    images: [
      {
        repository: "example/worker",
        tag: "check",
        size_bytes: 955_000_000,
        unique_bytes: 795_800_000,
        containers: 0,
      },
      {
        repository: "example/backend",
        tag: "latest",
        size_bytes: 1_400_000_000,
        unique_bytes: 1_291_000_000,
        containers: 1,
      },
      {
        repository: "<none>",
        tag: "<none>",
        size_bytes: 754_000_000,
        unique_bytes: 753_800_000,
        containers: 0,
      },
    ],
    containers: [
      {
        name: "example-backend-1",
        image: "example/backend:latest",
        state: "running",
        status: "Up 25 minutes",
        size_bytes: 57_300,
      },
    ],
    volumes: [
      { name: "example_db", size_bytes: 14_200_000_000, in_use_by: 1 },
      { name: "example_cache", size_bytes: 482_000_000, in_use_by: 0 },
      {
        name: "dc487f67a8810ad2570e670787bbda542c3fdd4fce4ddfd02c57293ef3ba59cf",
        size_bytes: 0,
        in_use_by: 0,
      },
    ],
    disk_file: {
      location_label: "~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw",
      allocated_bytes: 30_600_000_000,
      logical_bytes: 1_099_511_627_776,
    },
    note: null,
  },
};

export function deepScan(): ScanView {
  return finishedScan({
    candidates: DEEP_CANDIDATES,
    dev_assets: DEV_ASSETS,
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
    dev_assets: null,
    ...over,
  };
}

/** The engine's review sentence for the rows that have their own. */
const ACTION_TEXT: Record<string, string> = {
  "project-build-folders":
    "Delete this folder. The project will not build or run until you run its install or build command again.",
  "android-unreferenced-images":
    "Delete this Android system image. No emulator uses it, and Android Studio can download it again.",
};

export function planFor(ids: number[]): Plan {
  const all = [...CANDIDATES, ...DEEP_CANDIDATES, ...PROJECT_CANDIDATES, ...ANDROID_CANDIDATES];
  const items = all
    .filter((c) => ids.includes(c.id))
    .map((c) => ({
      candidate_id: c.id,
      catalog_id: c.catalog_id,
      display_name: c.display_name,
      action_text:
        c.action === "run_tool"
          ? "Run the tool's own clean command."
          : (ACTION_TEXT[c.catalog_id] ??
            "Delete this item from your temporary folder. Everything in it is more than a day old."),
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
      reason: null,
      detail: null,
      bytes_removed: 2 * 1024 * 1024,
      recovery_text: "Files older than a day were removed from the temporary folder.",
      hint: null,
    },
    {
      candidate_id: 2,
      catalog_id: "user-temp",
      display_name: XSS_NAME,
      outcome: "skipped",
      reason: "changed_since_scan",
      detail: "it changed since the scan",
      bytes_removed: 0,
      recovery_text: null,
      hint: null,
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

/** A run on its second item: npm was busy, uv is printing. */
export const RUNNING: RunView = {
  plan_id: 11,
  status: "running",
  items_done: 1,
  items_total: 2,
  elapsed_ms: 62_000,
  current: {
    index: 1,
    display_name: "~/.cache/uv",
    catalog_id: "pkg-uv-cache",
    elapsed_ms: 41_000,
    output: [
      "Running /opt/homebrew/bin/uv cache clean",
      "Clearing cache at: /Users/test/.cache/uv",
    ],
  },
  recent: [
    {
      candidate_id: 30,
      catalog_id: "pkg-npm-cache",
      display_name: "~/.npm",
      outcome: "skipped",
      reason: "tool_busy",
      detail: "the tool is busy with another process. Try again later",
      bytes_removed: 0,
      recovery_text: null,
      hint: "Another install is using the cache. Let it finish or close it, then run the cleanup again.",
    },
  ],
  counts: { removed: 0, skipped: 1, failed: 0 },
  report: null,
};

export const APP_INFO: AppInfo = {
  version: "0.1.0",
  platform: "macos",
  audit_log_path: "/Users/test/Library/Application Support/Space Optimiser/audit.jsonl",
  export_folder: "/Users/test/Downloads",
};

const GIB = 1024 ** 3;

/** A second reading: memory, swap, CPU, and both process lists. */
export const SYSTEM_USAGE: SystemUsage = {
  memory: { total_bytes: 16 * GIB, used_bytes: 11 * GIB },
  swap: { total_bytes: 2 * GIB, used_bytes: GIB / 2 },
  cpu_percent: 23.4,
  logical_cpus: 10,
  top_memory: [
    { name: "Google Chrome Helper (Renderer)", memory_bytes: 3 * GIB, cpu_percent: 4.2, count: 14 },
    { name: XSS_NAME, memory_bytes: 2 * GIB, cpu_percent: 0.1, count: 1 },
    { name: "Code Helper", memory_bytes: GIB, cpu_percent: 1.5, count: 6 },
  ],
  top_cpu: [
    { name: "rustc", memory_bytes: GIB / 4, cpu_percent: 12.5, count: 3 },
    { name: "Google Chrome Helper (Renderer)", memory_bytes: 3 * GIB, cpu_percent: 4.2, count: 14 },
  ],
};

/** 494 GB disk with 182 GB free. */
export const DISK_USAGE: DiskUsage = {
  location_label: "The disk that holds your home folder",
  total_bytes: 494_000_000_000,
  free_bytes: 182_000_000_000,
  used_bytes: 312_000_000_000,
};

/** A check that found 0.6.0, installable from the app. */
export const UPDATE_AVAILABLE: Extract<UpdateView, { state: "available" }> = {
  state: "available",
  version: "0.6.0",
  current_version: "0.1.0",
  notes: `### Added\n\n- **Overview**, a new first page. ${XSS_NAME}`,
  date: "2026-11-24",
  route: "in_app",
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
  systemUsage?: SystemUsage;
  systemError?: EngineError;
  diskError?: EngineError;
  /** What check_for_update returns; up to date by default. */
  updateCheck?: UpdateView;
  /** What update_status returns after Install, one per poll; the last repeats. */
  updateProgress?: UpdateView[];
  installError?: EngineError;
  restartError?: EngineError;
}

export function fakeEngine(options: FakeOptions = {}) {
  let cancelled = false;
  let update: UpdateView = { state: "idle" };
  let installed = false;
  const progress = [...(options.updateProgress ?? [])];
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
    getRun: vi.fn(async (): Promise<RunView> =>
      options.runStays
        ? RUNNING
        : {
            plan_id: 11,
            status: "finished",
            items_done: 2,
            items_total: 2,
            elapsed_ms: 3000,
            current: null,
            recent: [],
            counts: { removed: 1, skipped: 1, failed: 0 },
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
    systemUsage: vi.fn(async (): Promise<SystemUsage> => {
      if (options.systemError) throw options.systemError;
      return options.systemUsage ?? SYSTEM_USAGE;
    }),
    diskUsage: vi.fn(async (): Promise<DiskUsage> => {
      if (options.diskError) throw options.diskError;
      return DISK_USAGE;
    }),
    checkForUpdate: vi.fn(async (): Promise<UpdateView> => {
      update = options.updateCheck ?? { state: "up_to_date", current_version: "0.1.0" };
      return update;
    }),
    updateStatus: vi.fn(async (): Promise<UpdateView> => {
      if (installed && progress.length > 0)
        update = progress.length > 1 ? progress.shift()! : progress[0];
      return update;
    }),
    installUpdate: vi.fn(async (): Promise<UpdateView> => {
      if (options.installError) throw options.installError;
      installed = true;
      update = progress.length > 1 ? progress.shift()! : (progress[0] ?? update);
      return update;
    }),
    restartApp: vi.fn(async () => {
      if (options.restartError) throw options.restartError;
    }),
    openDownloadPage: vi.fn(async () => {}),
  } satisfies Engine;
  return engine;
}
