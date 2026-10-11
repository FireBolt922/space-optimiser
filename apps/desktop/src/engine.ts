// Typed client for the engine's Tauri commands. Every call sends ids, never a
// path: the engine owns paths and re-checks them before any delete.
import { invoke } from "@tauri-apps/api/core";

export type ScanMode = "normal" | "deep";
export type Risk = "low" | "medium" | "high" | "info";
export type Action = "delete" | "run_tool" | "report" | "none";
export type Platform = "macos" | "windows" | "linux";

export interface Candidate {
  id: number;
  catalog_id: string;
  display_name: string;
  explanation: string;
  location_label: string;
  /** Allocated bytes a delete removes. 0 for tool rows. */
  estimated_bytes: number;
  /** For tool rows: the most the tool could free. Never added to the delete total. */
  upper_bound_bytes: number | null;
  /** How a tool row's folder or size was found, as a sentence. */
  found_by: string | null;
  /** Candidates this one removes too; selecting both counts them once. */
  covers: number[];
  file_count: number;
  links_not_followed: number;
  labels: ("shared" | "logical_size" | "quit_first")[];
  risk: Risk;
  bulk_safe: boolean;
  confidence: "high" | "uncertain";
  action: Action;
  platform: string;
  /** False when the row has no folder of its own to show (Docker's build cache). */
  revealable: boolean;
}

export interface SkippedView {
  display_name: string;
  reason: string;
  detail: string;
}

export interface RootView {
  location_label: string;
  skipped: string | null;
  frees_memory: boolean;
}

/** Why a row offers nothing (a tool not run, Docker elsewhere, volumes). */
export interface Note {
  catalog_id: string;
  location_label: string;
  text: string;
  bytes: number | null;
}

/** A part of a scan, in the order the engine runs them. */
export type ScanStage = "temp" | "caches" | "logs" | "tools" | "docker" | "large_files";

export interface ScanView {
  scan_id: number;
  status: "running" | "finished" | "cancelled" | "failed";
  /** The stages this scan runs, in order; `progress.stage` is the current one. */
  stages: ScanStage[];
  progress: {
    files_seen: number;
    bytes_seen: number;
    stage: ScanStage;
    /** The tool a "tools" stage is asking, such as `uv`. */
    detail: string | null;
  };
  duration_ms: number;
  error: string | null;
  candidates: Candidate[];
  skipped: SkippedView[];
  roots: RootView[];
  notes: Note[];
}

export interface PlanItem {
  candidate_id: number;
  catalog_id: string;
  display_name: string;
  action_text: string;
  estimated_bytes: number;
  upper_bound_bytes: number | null;
  file_count: number;
}

export interface Plan {
  plan_id: number;
  scan_id: number;
  items: PlanItem[];
  estimated_bytes: number;
  /** Shown on its own line, never added to `estimated_bytes`. */
  tool_upper_bound_bytes: number;
  expires_in_secs: number;
}

export type ExecuteStart =
  { status: "started" } | { status: "audit_log_unavailable"; disk_full: boolean };

export interface ReportItem {
  candidate_id: number;
  catalog_id: string;
  display_name: string;
  outcome: "removed" | "ran" | "skipped" | "failed" | "not_run";
  detail: string | null;
  bytes_removed: number;
  recovery_text: string | null;
}

export interface FreeSpace {
  location_label: string;
  before: number | null;
  after: number | null;
  after_delay_secs: number;
  source: string;
}

export interface Report {
  plan_id: number;
  items: ReportItem[];
  bytes_removed: number;
  stopped: boolean;
  free_space: FreeSpace[];
  audit_log: "written" | "memory_only";
  memory_log: string[];
}

export interface RunView {
  plan_id: number;
  status: "running" | "finished";
  items_done: number;
  items_total: number;
  report: Report | null;
}

export interface InterruptedRun {
  started_unix_secs: number;
  items_planned: number;
  actions: { display_name: string; outcome: string }[];
}

export interface AppInfo {
  version: string;
  platform: Platform;
  /** Shown so a person can delete the log themselves; the cleaner never does. */
  audit_log_path: string;
  /** Where Export report saves its file. */
  export_folder: string;
}

export interface ExportedReport {
  path: string;
  file_name: string;
}

export interface EngineError {
  kind: string;
  message: string;
}

export function isEngineError(value: unknown): value is EngineError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as EngineError).kind === "string" &&
    typeof (value as EngineError).message === "string"
  );
}

/**
 * Engine messages start in lower case so they read inside a longer sentence
 * (reveal_failed, candidate_gone, run_not_finished, export_failed,
 * running_elevated, ...). Shown on their own, they start with a capital.
 */
export function errorMessage(value: unknown): string {
  const text = isEngineError(value) ? value.message : String(value);
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export interface Engine {
  startScan(mode: ScanMode): Promise<number>;
  cancelScan(scanId: number): Promise<void>;
  getScan(scanId: number): Promise<ScanView>;
  buildPlan(scanId: number, candidateIds: number[]): Promise<Plan>;
  confirmPlan(planId: number): Promise<string>;
  executePlan(planId: number, token: string, keepLogInMemory: boolean): Promise<ExecuteStart>;
  getRun(planId: number): Promise<RunView>;
  stopRun(planId: number): Promise<void>;
  interruptedRun(): Promise<InterruptedRun | null>;
  /** Opens the system file manager at the candidate. Sends the id only. */
  reveal(candidateId: number): Promise<void>;
  /** Saves the finished run's report as a local text file. Nothing is uploaded. */
  exportReport(planId: number): Promise<ExportedReport>;
  appInfo(): Promise<AppInfo>;
}

export const tauriEngine: Engine = {
  startScan: (mode) => invoke("start_scan", { args: { mode } }),
  cancelScan: (scanId) => invoke("cancel_scan", { args: { scan_id: scanId } }),
  getScan: (scanId) => invoke("get_scan", { args: { scan_id: scanId } }),
  buildPlan: (scanId, candidateIds) =>
    invoke("build_plan", { args: { scan_id: scanId, candidate_ids: candidateIds } }),
  confirmPlan: (planId) => invoke("confirm_plan", { args: { plan_id: planId } }),
  executePlan: (planId, token, keepLogInMemory) =>
    invoke("execute_plan", {
      args: {
        plan_id: planId,
        confirmation_token: token,
        keep_log_in_memory: keepLogInMemory,
      },
    }),
  getRun: (planId) => invoke("get_run", { args: { plan_id: planId } }),
  stopRun: (planId) => invoke("stop_run", { args: { plan_id: planId } }),
  interruptedRun: () => invoke("interrupted_run"),
  reveal: (candidateId) => invoke("reveal", { args: { candidate_id: candidateId } }),
  exportReport: (planId) => invoke("export_report", { args: { plan_id: planId } }),
  appInfo: () => invoke("app_info"),
};
