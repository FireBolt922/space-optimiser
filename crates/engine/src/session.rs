//! The engine as the desktop shell sees it: one scan, its plans, and at most
//! one running plan. Commands take ids and return views; paths stay here.
//! Scans and runs happen on worker threads; the UI polls `get_scan` and
//! `get_run`, so the webview needs no event permission.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::audit::{self, Audit};
use crate::clock::{Clock, PLAN_LIFETIME, Stamp, SystemClock};
use crate::discover::{self, CancelToken, NormalScan, Progress, RootOutcome, ScanError};
use crate::execute::{self, Measure, PlanEntry};
use crate::model::{
    Action, BuildPlanArgs, Candidate, ConfirmationToken, ExecutePlanArgs, ExecuteStart,
    InterruptedRun, Plan, PlanId, PlanItem, PlanRef, Report, RevealArgs, RootView, RunStatus,
    RunView, ScanId, ScanRef, ScanStatus, ScanView, SkippedView, StartScanArgs,
};
use crate::policy::{CatalogId, Context};
use crate::{EngineError, Result};

/// How long after the last delete the "after" free-space reading waits.
const SETTLE: Duration = Duration::from_secs(3);

type ContextFn = dyn Fn() -> io::Result<Context> + Send + Sync;
type FreeSpaceFn = dyn Fn(&Path) -> io::Result<u64> + Send + Sync;
type AfterItemFn = dyn Fn(usize, &CancelToken) + Send + Sync;

/// Where the engine gets its context, clock, and measurements. Production
/// uses [`Engine::for_this_machine`]; tests point it at a fixture home.
pub struct Config {
    context: Box<ContextFn>,
    audit_dir: PathBuf,
    clock: Box<dyn Clock>,
    free_space: Box<FreeSpaceFn>,
    settle: Duration,
    after_item: Option<Box<AfterItemFn>>,
}

impl Config {
    /// `context` is resolved again for every scan and every run, so the
    /// executor re-checks against the folders as they are now.
    pub fn new(
        context: impl Fn() -> io::Result<Context> + Send + Sync + 'static,
        audit_dir: PathBuf,
    ) -> Self {
        Self {
            context: Box::new(context),
            audit_dir,
            clock: Box::new(SystemClock),
            free_space: Box::new(crate::measure::free_space),
            settle: SETTLE,
            after_item: None,
        }
    }

    #[doc(hidden)]
    pub fn with_clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    #[doc(hidden)]
    pub fn with_free_space(
        mut self,
        free_space: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    ) -> Self {
        self.free_space = Box::new(free_space);
        self
    }

    #[doc(hidden)]
    pub fn with_settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// Called after each planned item with the number done and the run's
    /// stop flag.
    #[doc(hidden)]
    pub fn with_after_item(
        mut self,
        hook: impl Fn(usize, &CancelToken) + Send + Sync + 'static,
    ) -> Self {
        self.after_item = Some(Box::new(hook));
        self
    }
}

#[derive(Clone)]
pub struct Engine(Arc<Shared>);

struct Shared {
    config: Config,
    state: Mutex<State>,
    changed: Condvar,
    interrupted: Option<InterruptedRun>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    scan: Option<ScanSlot>,
    plans: HashMap<PlanId, PlanRecord>,
    runs: HashMap<PlanId, RunSlot>,
    running: Option<PlanId>,
}

struct ScanSlot {
    cancel: CancelToken,
    view: ScanView,
}

struct PlanRecord {
    entries: Vec<PlanEntry>,
    built: Stamp,
    token: ConfirmationToken,
}

struct RunSlot {
    stop: CancelToken,
    done: usize,
    total: usize,
    report: Option<Report>,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let interrupted = audit::take_interrupted(&config.audit_dir);
        Self(Arc::new(Shared {
            config,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            interrupted,
        }))
    }

    /// This computer's folders, with the audit log in `audit_dir`.
    pub fn for_this_machine(audit_dir: PathBuf) -> Self {
        Self::new(Config::new(crate::platform::resolve_context, audit_dir))
    }

    /// A run that started in an earlier session and never finished.
    pub fn interrupted_run(&self) -> Option<InterruptedRun> {
        self.0.interrupted.clone()
    }

    /// Starts a scan on a worker thread. A new scan cancels the previous
    /// one and discards its plans.
    pub fn start_scan(&self, args: StartScanArgs) -> Result<ScanId> {
        // Both modes run the Normal Scan rows until M5 adds the Deep rows.
        let _ = args.mode;
        let mut state = self.state();
        if state.running.is_some() {
            return Err(EngineError::RunInProgress);
        }
        if let Some(old) = &state.scan {
            old.cancel.cancel();
        }
        state.next_id += 1;
        let scan_id = ScanId::new(state.next_id);
        let cancel = CancelToken::new();
        state.plans.clear();
        state.scan = Some(ScanSlot {
            cancel: cancel.clone(),
            view: ScanView {
                scan_id,
                status: ScanStatus::Running,
                progress: Progress {
                    files_seen: 0,
                    bytes_seen: 0,
                },
                duration_ms: 0,
                error: None,
                candidates: Vec::new(),
                skipped: Vec::new(),
                roots: Vec::new(),
            },
        });
        drop(state);
        let engine = self.clone();
        std::thread::spawn(move || engine.run_scan(scan_id, &cancel));
        Ok(scan_id)
    }

    pub fn cancel_scan(&self, args: ScanRef) -> Result<()> {
        let state = self.state();
        let slot = scan_slot(&state, args.scan_id)?;
        slot.cancel.cancel();
        Ok(())
    }

    pub fn get_scan(&self, args: ScanRef) -> Result<ScanView> {
        Ok(scan_slot(&self.state(), args.scan_id)?.view.clone())
    }

    /// Blocks until the scan is no longer running.
    #[doc(hidden)]
    pub fn wait_for_scan(&self, args: ScanRef) -> Result<ScanView> {
        let mut state = self.state();
        loop {
            let view = &scan_slot(&state, args.scan_id)?.view;
            if view.status != ScanStatus::Running {
                return Ok(view.clone());
            }
            state = self
                .0
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Freezes the selected candidates into a plan. Only candidates whose
    /// action is a delete can be planned.
    pub fn build_plan(&self, args: BuildPlanArgs) -> Result<Plan> {
        let mut state = self.state();
        let view = &scan_slot(&state, args.scan_id)?.view;
        if view.status != ScanStatus::Finished {
            return Err(EngineError::ScanNotFinished);
        }
        let mut ids = args.candidate_ids;
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(*id));
        if ids.is_empty() {
            return Err(EngineError::EmptyPlan);
        }
        let mut chosen: Vec<&Candidate> = Vec::with_capacity(ids.len());
        for id in &ids {
            let candidate = view
                .candidates
                .iter()
                .find(|c| c.id == *id)
                .ok_or(EngineError::UnknownCandidate)?;
            match candidate.action {
                Action::Delete => chosen.push(candidate),
                Action::RunTool => return Err(EngineError::Unsupported),
                Action::Report | Action::None => return Err(EngineError::NotActionable),
            }
        }
        let entries: Vec<PlanEntry> = chosen
            .iter()
            .map(|c| PlanEntry::from_candidate(c).ok_or(EngineError::NotActionable))
            .collect::<Result<_>>()?;
        let items = chosen
            .iter()
            .map(|c| PlanItem {
                candidate_id: c.id,
                catalog_id: c.catalog_id,
                display_name: c.display_name.clone(),
                action_text: c.catalog_id.action_text(),
                estimated_bytes: c.estimated_bytes,
                file_count: c.file_count,
            })
            .collect();
        let estimated_bytes = chosen.iter().map(|c| c.estimated_bytes).sum();
        let scan_id = view.scan_id;
        state.next_id += 1;
        let plan_id = PlanId::new(state.next_id);
        let built = self.0.config.clock.stamp();
        let plan = Plan {
            plan_id,
            scan_id,
            items,
            estimated_bytes,
            expires_in_secs: PLAN_LIFETIME.as_secs(),
        };
        let token = execute::token(plan_id, scan_id, &entries, built);
        state.plans.insert(
            plan_id,
            PlanRecord {
                entries,
                built,
                token,
            },
        );
        Ok(plan)
    }

    /// Issues the token for a plan the person confirmed on the review
    /// screen. An expired plan is discarded.
    pub fn confirm_plan(&self, args: PlanRef) -> Result<ConfirmationToken> {
        let mut state = self.state();
        let record = self.fresh_plan(&mut state, args.plan_id)?;
        Ok(record.token.clone())
    }

    /// Starts a confirmed plan on a worker thread. Nothing runs when the
    /// token is wrong, the plan expired, or the audit log cannot be started.
    pub fn execute_plan(&self, args: ExecutePlanArgs) -> Result<ExecuteStart> {
        if cfg!(windows) {
            return Err(EngineError::Unsupported);
        }
        let mut state = self.state();
        if state.running.is_some() {
            return Err(EngineError::RunInProgress);
        }
        let token = &state
            .plans
            .get(&args.plan_id)
            .ok_or(EngineError::UnknownPlan)?
            .token;
        if *token != args.confirmation_token {
            return Err(EngineError::WrongToken);
        }
        let record = self.fresh_plan(&mut state, args.plan_id)?;
        let total = record.entries.len();
        let config = &self.0.config;
        let ctx = (config.context)().map_err(|e| EngineError::Context(e.to_string()))?;
        #[cfg(unix)]
        if crate::guard::should_refuse(ctx.euid()) {
            return Err(EngineError::RunningAsRoot);
        }
        let audit = match Audit::start(&config.audit_dir, total, args.keep_log_in_memory) {
            Ok(audit) => audit,
            Err(unavailable) => {
                return Ok(ExecuteStart::AuditLogUnavailable {
                    disk_full: unavailable.disk_full,
                });
            }
        };
        let record = state
            .plans
            .remove(&args.plan_id)
            .expect("checked above under the same lock");
        let stop = CancelToken::new();
        state.runs.insert(
            args.plan_id,
            RunSlot {
                stop: stop.clone(),
                done: 0,
                total,
                report: None,
            },
        );
        state.running = Some(args.plan_id);
        drop(state);
        let engine = self.clone();
        std::thread::spawn(move || engine.run_plan(&ctx, args.plan_id, &record, audit, &stop));
        Ok(ExecuteStart::Started)
    }

    pub fn get_run(&self, args: PlanRef) -> Result<RunView> {
        let state = self.state();
        let slot = state
            .runs
            .get(&args.plan_id)
            .ok_or(EngineError::UnknownPlan)?;
        Ok(RunView {
            plan_id: args.plan_id,
            status: if slot.report.is_some() {
                RunStatus::Finished
            } else {
                RunStatus::Running
            },
            items_done: slot.done,
            items_total: slot.total,
            report: slot.report.clone(),
        })
    }

    /// Finishes the current item, then halts (phase 8 item 8).
    pub fn stop_run(&self, args: PlanRef) -> Result<()> {
        let state = self.state();
        let slot = state
            .runs
            .get(&args.plan_id)
            .ok_or(EngineError::UnknownPlan)?;
        slot.stop.cancel();
        Ok(())
    }

    /// Blocks until the run has a report.
    #[doc(hidden)]
    pub fn wait_for_run(&self, args: PlanRef) -> Result<Report> {
        let mut state = self.state();
        loop {
            let slot = state
                .runs
                .get(&args.plan_id)
                .ok_or(EngineError::UnknownPlan)?;
            if let Some(report) = &slot.report {
                return Ok(report.clone());
            }
            state = self
                .0
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Shows a scanned candidate in the OS file manager. Not a delete.
    pub fn reveal(&self, args: RevealArgs) -> Result<()> {
        crate::platform::reveal(args.candidate_id)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn fresh_plan<'s>(&self, state: &'s mut State, plan_id: PlanId) -> Result<&'s PlanRecord> {
        let built = state
            .plans
            .get(&plan_id)
            .ok_or(EngineError::UnknownPlan)?
            .built;
        if !built.is_fresh_at(self.0.config.clock.stamp()) {
            state.plans.remove(&plan_id);
            return Err(EngineError::PlanExpired);
        }
        Ok(&state.plans[&plan_id])
    }

    fn run_scan(&self, scan_id: ScanId, cancel: &CancelToken) {
        let started = Instant::now();
        let result = (self.0.config.context)()
            .map_err(|e| EngineError::Context(e.to_string()).to_string())
            .and_then(|ctx| {
                discover::scan_normal(&ctx, cancel, |progress| {
                    self.update_scan(scan_id, |view| view.progress = progress);
                })
                .map_err(|e| match e {
                    ScanError::Cancelled => String::new(),
                    ScanError::RunningAsRoot => e.to_string(),
                })
            });
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.update_scan(scan_id, |view| match result {
            Ok(scan) => {
                view.duration_ms = duration_ms;
                view.status = ScanStatus::Finished;
                fill(view, scan);
            }
            Err(error) if error.is_empty() => view.status = ScanStatus::Cancelled,
            Err(error) => {
                view.status = ScanStatus::Failed;
                view.error = Some(error);
            }
        });
    }

    fn update_scan(&self, scan_id: ScanId, change: impl FnOnce(&mut ScanView)) {
        let mut state = self.state();
        if let Some(slot) = state.scan.as_mut().filter(|s| s.view.scan_id == scan_id) {
            change(&mut slot.view);
        }
        drop(state);
        self.0.changed.notify_all();
    }

    fn run_plan(
        &self,
        ctx: &Context,
        plan_id: PlanId,
        record: &PlanRecord,
        audit: Audit,
        stop: &CancelToken,
    ) {
        let config = &self.0.config;
        let report = execute::run(
            ctx,
            plan_id,
            &record.entries,
            audit,
            stop,
            Measure {
                free_space: &*config.free_space,
                settle: config.settle,
            },
            |done| {
                if let Some(slot) = self.state().runs.get_mut(&plan_id) {
                    slot.done = done;
                }
                self.0.changed.notify_all();
                if let Some(hook) = &config.after_item {
                    hook(done, stop);
                }
            },
        );
        let mut state = self.state();
        if let Some(slot) = state.runs.get_mut(&plan_id) {
            slot.report = Some(report);
        }
        state.running = None;
        drop(state);
        self.0.changed.notify_all();
    }
}

fn scan_slot(state: &State, scan_id: ScanId) -> Result<&ScanSlot> {
    state
        .scan
        .as_ref()
        .filter(|s| s.view.scan_id == scan_id)
        .ok_or(EngineError::UnknownScan)
}

fn fill(view: &mut ScanView, scan: NormalScan) {
    view.progress = scan.progress;
    let label = CatalogId::UserTemp.location_label();
    for root in scan.temp {
        match root {
            RootOutcome::Skipped { reason, .. } => view.roots.push(RootView {
                location_label: label,
                skipped: Some(reason.to_string()),
                frees_memory: false,
            }),
            RootOutcome::Scanned {
                frees_memory,
                candidates,
                skipped,
                ..
            } => {
                view.roots.push(RootView {
                    location_label: label,
                    skipped: None,
                    frees_memory,
                });
                view.candidates.extend(candidates);
                view.skipped
                    .extend(skipped.into_iter().map(|s| SkippedView {
                        detail: s.reason.to_string(),
                        display_name: s.display_name,
                        reason: s.reason,
                    }));
            }
        }
    }
    view.candidates.extend(scan.caches);
    view.skipped
        .extend(scan.cache_skipped.into_iter().map(|s| SkippedView {
            detail: s.reason.to_string(),
            display_name: s.display_name,
            reason: s.reason,
        }));
}
