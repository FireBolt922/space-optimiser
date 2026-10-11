// Scan, review, confirm, run, and report, plus Settings, Help, and About.
// The UI holds ids only. It never builds a path, and file names are always
// rendered as text; dangerouslySetInnerHTML is banned by the lint config.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  errorMessage,
  isEngineError,
  type AppInfo,
  type Engine,
  type InterruptedRun,
  type Plan,
  type Report,
  type ScanMode,
  type ScanView,
} from "./engine";
import { Start } from "./screens/Start";
import { Scanning } from "./screens/Scanning";
import { Results } from "./screens/Results";
import { Review } from "./screens/Review";
import { AuditWarning } from "./screens/AuditWarning";
import { Running } from "./screens/Running";
import { ReportScreen } from "./screens/ReportScreen";
import { Settings } from "./screens/Settings";
import { Help } from "./screens/Help";
import { About } from "./screens/About";
import { Sidebar, type View } from "./components/Sidebar";
import { useTheme } from "./theme";

const POLL_MS = 200;

type Screen =
  | { kind: "start"; notice?: string }
  | { kind: "scanning"; scanId: number; view?: ScanView; cancelling: boolean }
  | { kind: "results"; scan: ScanView; selected: number[] }
  | {
      kind: "review";
      scan: ScanView;
      selected: number[];
      plan: Plan;
      expired: boolean;
      busy: boolean;
      error?: string;
    }
  | {
      kind: "audit";
      scan: ScanView;
      selected: number[];
      plan: Plan;
      token: string;
      diskFull: boolean;
    }
  | { kind: "running"; planId: number; done: number; total: number; stopping: boolean }
  | { kind: "report"; report: Report };

export default function App({ engine }: { engine: Engine }) {
  const [screen, setScreen] = useState<Screen>({ kind: "start" });
  const [interrupted, setInterrupted] = useState<InterruptedRun | null>(null);
  // The view is separate from the clean-up screen, so a scan or a run keeps
  // polling while Settings, Help, or About is open.
  const [view, setView] = useState<View>("cleanup");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [theme, setTheme] = useTheme();
  const main = useRef<HTMLElement>(null);
  const shownView = useRef(view);

  useEffect(() => {
    engine.interruptedRun().then(setInterrupted, () => setInterrupted(null));
    engine.appInfo().then(setInfo, () => setInfo(null));
  }, [engine]);

  // A view change moves focus to that view's heading.
  useEffect(() => {
    if (shownView.current === view) return;
    shownView.current = view;
    main.current?.querySelector<HTMLElement>("h1")?.focus();
  }, [view]);

  const startScan = useCallback(
    async (mode: ScanMode) => {
      try {
        const scanId = await engine.startScan(mode);
        setScreen({ kind: "scanning", scanId, cancelling: false });
      } catch (e) {
        setScreen({ kind: "start", notice: errorMessage(e) });
      }
    },
    [engine],
  );

  // Scan progress: poll until the scan stops running.
  const scanId = screen.kind === "scanning" ? screen.scanId : null;
  useEffect(() => {
    if (scanId === null) return;
    let stopped = false;
    const tick = async () => {
      try {
        const view = await engine.getScan(scanId);
        if (stopped) return;
        if (view.status === "running") {
          setScreen((s) => (s.kind === "scanning" ? { ...s, view } : s));
          timer = setTimeout(tick, POLL_MS);
        } else if (view.status === "finished") {
          setScreen({ kind: "results", scan: view, selected: [] });
        } else if (view.status === "cancelled") {
          setScreen({ kind: "start", notice: "The scan was cancelled." });
        } else {
          setScreen({ kind: "start", notice: view.error ?? "The scan failed." });
        }
      } catch (e) {
        if (!stopped) setScreen({ kind: "start", notice: errorMessage(e) });
      }
    };
    let timer = setTimeout(tick, 0);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [engine, scanId]);

  // Run progress: poll until the report arrives.
  const planId = screen.kind === "running" ? screen.planId : null;
  useEffect(() => {
    if (planId === null) return;
    let stopped = false;
    const tick = async () => {
      try {
        const run = await engine.getRun(planId);
        if (stopped) return;
        if (run.report) {
          setScreen({ kind: "report", report: run.report });
        } else {
          setScreen((s) =>
            s.kind === "running" ? { ...s, done: run.items_done, total: run.items_total } : s,
          );
          timer = setTimeout(tick, POLL_MS);
        }
      } catch {
        if (!stopped) timer = setTimeout(tick, POLL_MS);
      }
    };
    let timer = setTimeout(tick, 0);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [engine, planId]);

  const cancelScan = async () => {
    if (screen.kind !== "scanning") return;
    setScreen({ ...screen, cancelling: true });
    await engine.cancelScan(screen.scanId).catch(() => undefined);
  };

  const review = async (scan: ScanView, selected: number[]) => {
    try {
      const plan = await engine.buildPlan(scan.scan_id, selected);
      setScreen({ kind: "review", scan, selected, plan, expired: false, busy: false });
    } catch (e) {
      setScreen({ kind: "start", notice: errorMessage(e) });
    }
  };

  const execute = async (plan: Plan, token: string, keepLogInMemory: boolean) => {
    const start = await engine.executePlan(plan.plan_id, token, keepLogInMemory);
    if (start.status === "started") {
      setScreen({
        kind: "running",
        planId: plan.plan_id,
        done: 0,
        total: plan.items.length,
        stopping: false,
      });
    }
    return start;
  };

  const confirm = async () => {
    if (screen.kind !== "review") return;
    const current = screen;
    setScreen({ ...current, busy: true, error: undefined });
    try {
      const token = await engine.confirmPlan(current.plan.plan_id);
      const start = await execute(current.plan, token, false);
      if (start.status === "audit_log_unavailable") {
        setScreen({
          kind: "audit",
          scan: current.scan,
          selected: current.selected,
          plan: current.plan,
          token,
          diskFull: start.disk_full,
        });
      }
    } catch (e) {
      const expired = isEngineError(e) && (e.kind === "plan_expired" || e.kind === "unknown_plan");
      setScreen({ ...current, busy: false, expired, error: errorMessage(e) });
    }
  };

  const confirmWithoutLog = async () => {
    if (screen.kind !== "audit") return;
    const current = screen;
    try {
      await execute(current.plan, current.token, true);
    } catch (e) {
      setScreen({
        kind: "review",
        scan: current.scan,
        selected: current.selected,
        plan: current.plan,
        expired: isEngineError(e) && e.kind === "plan_expired",
        busy: false,
        error: errorMessage(e),
      });
    }
  };

  const stop = async () => {
    if (screen.kind !== "running") return;
    // A functional update keeps progress a poll delivered after this render.
    setScreen((s) => (s.kind === "running" ? { ...s, stopping: true } : s));
    await engine.stopRun(screen.planId).catch(() => undefined);
  };

  const backToResults = (scan: ScanView, selected: number[]) =>
    setScreen({ kind: "results", scan, selected });

  const cleanupScreen = (
    <>
      {screen.kind === "start" && (
        <Start
          notice={screen.notice}
          interrupted={interrupted}
          onDismissInterrupted={() => setInterrupted(null)}
          onScan={startScan}
        />
      )}
      {screen.kind === "scanning" && (
        <Scanning view={screen.view} cancelling={screen.cancelling} onCancel={cancelScan} />
      )}
      {screen.kind === "results" && (
        <Results
          scan={screen.scan}
          selected={screen.selected}
          onSelect={(selected) => setScreen({ ...screen, selected })}
          onReview={() => review(screen.scan, screen.selected)}
          onScanAgain={() => setScreen({ kind: "start" })}
          onReveal={(id) => engine.reveal(id)}
        />
      )}
      {screen.kind === "review" && (
        <Review
          plan={screen.plan}
          expired={screen.expired}
          busy={screen.busy}
          error={screen.error}
          onBack={() => backToResults(screen.scan, screen.selected)}
          onConfirm={confirm}
          onScanAgain={() => setScreen({ kind: "start" })}
        />
      )}
      {screen.kind === "audit" && (
        <AuditWarning
          diskFull={screen.diskFull}
          onBack={() => backToResults(screen.scan, screen.selected)}
          onContinue={confirmWithoutLog}
        />
      )}
      {screen.kind === "running" && (
        <Running done={screen.done} total={screen.total} stopping={screen.stopping} onStop={stop} />
      )}
      {screen.kind === "report" && (
        <ReportScreen
          report={screen.report}
          platform={info?.platform ?? null}
          onExport={(planId) => engine.exportReport(planId)}
          onDone={() => setScreen({ kind: "start" })}
        />
      )}
    </>
  );

  return (
    <div className="app">
      <Sidebar view={view} version={info?.version ?? null} onNavigate={setView} />
      <main className="app-main" ref={main}>
        {view === "cleanup" && cleanupScreen}
        {view === "settings" && <Settings theme={theme} onTheme={setTheme} />}
        {view === "help" && <Help info={info} />}
        {view === "about" && <About info={info} onHelp={() => setView("help")} />}
      </main>
    </div>
  );
}
