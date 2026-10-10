// Typed client for the engine's Tauri commands. Every call sends ids, never a
// path: the engine owns paths and re-checks them before any delete.
import { invoke } from "@tauri-apps/api/core";

export type ScanMode = "normal" | "deep";
export type Risk = "low" | "medium" | "high" | "info";
export type Action = "delete" | "run_tool" | "report" | "none";

export interface Candidate {
  id: number;
  catalog_id: string;
  display_name: string;
  explanation: string;
  location_label: string;
  estimated_bytes: number;
  file_count: number;
  links_not_followed: number;
  labels: ("shared" | "logical_size" | "quit_first")[];
  risk: Risk;
  bulk_safe: boolean;
  confidence: "high" | "uncertain";
  action: Action;
  platform: string;
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

export interface ScanView {
  scan_id: number;
  status: "running" | "finished" | "cancelled" | "failed";
  progress: { files_seen: number; bytes_seen: number };
  duration_ms: number;
  error: string | null;
  candidates: Candidate[];
  skipped: SkippedView[];
  roots: RootView[];
}

export interface PlanItem {
  candidate_id: number;
  catalog_id: string;
  display_name: string;
  action_text: string;
  estimated_bytes: number;
  file_count: number;
}

export interface Plan {
  plan_id: number;
  scan_id: number;
  items: PlanItem[];
  estimated_bytes: number;
  expires_in_secs: number;
}

export type ExecuteStart =
  | { status: "started" }
  | { status: "audit_log_unavailable"; disk_full: boolean };

export interface ReportItem {
  candidate_id: number;
  catalog_id: string;
  display_name: string;
  outcome: "removed" | "skipped" | "failed" | "not_run";
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

export function errorMessage(value: unknown): string {
  if (isEngineError(value)) return value.message;
  return String(value);
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
};
