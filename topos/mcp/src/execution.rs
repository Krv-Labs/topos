//! Bounded blocking execution, cooperative cancellation, and SEP-2663 task orchestration.
//!
//! Coordinates all CPU-heavy and subprocess-backed MCP tool invocations:
//! - Offloads synchronous work to `tokio::task::spawn_blocking` under a shared
//!   concurrency semaphore (`2..=4` permits) where the [`OwnedSemaphorePermit`]
//!   is owned by the blocking closure itself, never released early if an async
//!   waiter is dropped or aborted.
//! - Propagates cancellation via an operation-owned [`AtomicBool`] paired with
//!   an RAII [`CancelOnDrop`] guard on the awaiting async future so request
//!   cancellation, `tasks/cancel`, task TTL sweeps, and stdio EOF shutdown all
//!   signal the underlying blocking worker and kill child process groups.
//! - Bridges `rmcp::task_manager::TaskManager` for SEP-2663 task-capable clients
//!   on protocol version `2026-07-28+`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{CallToolResponse, CallToolResult, CreateTaskResult, ProtocolVersion};
use rmcp::service::RequestContext;
use rmcp::task_manager::{TaskExit, TaskManager, TaskOptions};
use rmcp::{ErrorData as McpError, RoleServer};
use tokio::sync::{mpsc, Notify, Semaphore};
use tokio_util::sync::CancellationToken;

/// Retention TTL for completed/failed/cancelled task records (`10` minutes).
///
/// Kept strictly separate from the active execution deadline so callers have a
/// window to poll `tasks/get` after a long-running operation completes or times out.
pub const TASK_RETENTION_TTL_MS: u64 = 600_000;

/// Suggested client polling interval (`1` second) advertised on created tasks.
pub const TASK_POLL_INTERVAL_MS: u64 = 1_000;

/// Grace period to await a blocking worker after signalling cancellation on an
/// inline request or task cancellation before returning to the caller.
const CANCEL_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);

/// What a tool was doing when its deadline passed, appended to the timeout
/// error so the caller learns where the time went. Read the moment the
/// deadline fires, before the worker is given its drain period to wind
/// down, so it reports where the time went and not where the worker ended.
pub type TimeoutNote = Arc<dyn Fn() -> String + Send + Sync>;

/// The error for an operation that ran past `timeout`: the tool's note on
/// what it was doing when there is one, else `where_`, which says which
/// wait it was in (empty when it was running).
fn timed_out(timeout: Duration, where_: &str, note: Option<&TimeoutNote>) -> McpError {
    let mut message = format!("Operation timed out after {}s", timeout.as_secs());
    match note {
        // The note says where the operation was, so every path reads alike.
        Some(note) => {
            message.push(' ');
            message.push_str(&note());
        }
        None => message.push_str(where_),
    }
    McpError::internal_error(message, None)
}

/// Marker error returned when a blocking evaluation or subprocess operation
/// exits early due to cooperative cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalCancelled;

impl std::fmt::Display for EvalCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation cancelled")
    }
}

impl std::error::Error for EvalCancelled {}

/// RAII guard held by the async future awaiting a `spawn_blocking` operation.
///
/// If the future is dropped or aborted for any reason (inline request cancellation,
/// `tasks/cancel`, opportunistic TTL sweep, or `TaskManager::shutdown()`), this
/// guard sets the shared `AtomicBool` so the blocking worker and any child
/// subprocesses exit at their next checkpoint.
pub struct CancelOnDrop(pub Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Lightweight progress reporter passed into synchronous `spawn_blocking` closures.
#[derive(Clone, Default)]
pub struct StatusReporter {
    tx: Option<mpsc::UnboundedSender<String>>,
}

impl StatusReporter {
    pub fn noop() -> Self {
        Self { tx: None }
    }

    fn channel() -> (Self, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx: Some(tx) }, rx)
    }

    /// Report a human-readable progress message to the owning SEP-2663 task (if any).
    pub fn report(&self, message: impl Into<String>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(message.into());
        }
    }
}

/// Check whether the caller negotiated MCP `2026-07-28+` and declared the
/// `io.modelcontextprotocol/tasks` client capability (either in per-request
/// `_meta` or at session `initialize` time).
pub fn client_supports_tasks(context: &RequestContext<RoleServer>) -> bool {
    let version_ok = context
        .protocol_version()
        .is_some_and(|v| v.as_str() >= ProtocolVersion::V_2026_07_28.as_str());
    if !version_ok {
        return false;
    }
    context
        .client_capabilities()
        .is_some_and(|caps| caps.supports_tasks())
}

/// Check whether `cancel_flag` has been signalled.
#[inline]
pub fn is_cancelled(cancel_flag: Option<&AtomicBool>) -> bool {
    cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed))
}

fn default_concurrency_limit() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(2, 4)
}

/// Server-scoped coordinator for bounded blocking execution and clean shutdown.
#[derive(Clone)]
pub struct ExecutionCoordinator {
    semaphore: Arc<Semaphore>,
    active_ops: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    next_op_id: Arc<AtomicU64>,
    idle_notify: Arc<Notify>,
    task_manager: TaskManager,
}

struct ActiveOpGuard {
    id: u64,
    active_ops: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    idle_notify: Arc<Notify>,
}

impl Drop for ActiveOpGuard {
    fn drop(&mut self) {
        let is_empty = {
            let mut map = self
                .active_ops
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.remove(&self.id);
            map.is_empty()
        };
        if is_empty {
            self.idle_notify.notify_waiters();
        }
    }
}

impl Default for ExecutionCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionCoordinator {
    pub fn new() -> Self {
        Self::with_max_concurrency(default_concurrency_limit())
    }

    pub fn with_task_manager(task_manager: TaskManager) -> Self {
        Self {
            task_manager,
            ..Self::new()
        }
    }

    pub fn with_max_concurrency(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency.max(1))),
            active_ops: Arc::new(Mutex::new(HashMap::new())),
            next_op_id: Arc::new(AtomicU64::new(1)),
            idle_notify: Arc::new(Notify::new()),
            task_manager: TaskManager::new(),
        }
    }

    fn register_op(&self, cancel_flag: Arc<AtomicBool>) -> ActiveOpGuard {
        let id = self.next_op_id.fetch_add(1, Ordering::Relaxed);
        let mut map = self
            .active_ops
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.insert(id, cancel_flag);
        ActiveOpGuard {
            id,
            active_ops: Arc::clone(&self.active_ops),
            idle_notify: Arc::clone(&self.idle_notify),
        }
    }

    /// Number of currently running blocking operations (used by tests and shutdown).
    pub fn active_operation_count(&self) -> usize {
        self.active_ops
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Signal all in-flight operations to cancel and wait up to `grace` for
    /// their `spawn_blocking` closures to finish.
    pub async fn shutdown_and_wait(&self, grace: Duration) {
        loop {
            let notified = self.idle_notify.notified();
            let is_empty = {
                let map = self
                    .active_ops
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for flag in map.values() {
                    flag.store(true, Ordering::SeqCst);
                }
                map.is_empty()
            };
            if is_empty {
                return;
            }
            if tokio::time::timeout(grace, notified).await.is_err() {
                return;
            }
        }
    }

    /// Run a synchronous tool body on `spawn_blocking` under the shared
    /// concurrency semaphore, bound to the inline request's cancellation token.
    pub async fn run_inline<F>(
        &self,
        context: &RequestContext<RoleServer>,
        work: F,
    ) -> Result<CallToolResult, McpError>
    where
        F: FnOnce(&AtomicBool) -> Result<CallToolResult, EvalCancelled> + Send + 'static,
    {
        if context.ct.is_cancelled() {
            return Err(McpError::internal_error("Request cancelled", None));
        }

        let permit = tokio::select! {
            biased;
            _ = context.ct.cancelled() => {
                return Err(McpError::internal_error("Request cancelled", None));
            }
            res = Arc::clone(&self.semaphore).acquire_owned() => {
                res.map_err(|e| McpError::internal_error(format!("Semaphore closed: {e}"), None))?
            }
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(Arc::clone(&cancel_flag));
        let active_guard = self.register_op(Arc::clone(&cancel_flag));
        let worker_flag = Arc::clone(&cancel_flag);

        let mut handle = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _active = active_guard;
            if worker_flag.load(Ordering::Relaxed) {
                return Err(EvalCancelled);
            }
            work(&worker_flag)
        });

        tokio::select! {
            biased;
            _ = context.ct.cancelled() => {
                cancel_flag.store(true, Ordering::SeqCst);
                let _ = tokio::time::timeout(CANCEL_DRAIN_TIMEOUT, &mut handle).await;
                Err(McpError::internal_error("Request cancelled", None))
            }
            join_res = &mut handle => {
                match join_res {
                    Ok(Ok(result)) => Ok(result),
                    Ok(Err(EvalCancelled)) => Err(McpError::internal_error("Request cancelled", None)),
                    Err(join_err) => Err(McpError::internal_error(
                        format!("Blocking worker panicked: {join_err}"),
                        None,
                    )),
                }
            }
        }
    }

    /// Same as [`Self::run_task_or_inline`], using the coordinator's embedded
    /// server-scoped [`TaskManager`].
    pub async fn run_task<F>(
        &self,
        context: &RequestContext<RoleServer>,
        timeout: Duration,
        initial_status: &'static str,
        work: F,
    ) -> Result<CallToolResponse, McpError>
    where
        F: FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
            + Send
            + 'static,
    {
        self.run_task_or_inline(&self.task_manager, context, timeout, initial_status, work)
            .await
    }

    /// [`Self::run_task`] for a tool that can say what it was doing when the
    /// deadline passed: `note` is appended to the timeout error, whether the
    /// operation was queued or running, inline or as a task.
    pub async fn run_task_described<F>(
        &self,
        context: &RequestContext<RoleServer>,
        timeout: Duration,
        initial_status: &'static str,
        note: TimeoutNote,
        work: F,
    ) -> Result<CallToolResponse, McpError>
    where
        F: FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
            + Send
            + 'static,
    {
        self.run_with_note(
            &self.task_manager,
            context,
            timeout,
            initial_status,
            Some(note),
            work,
        )
        .await
    }

    /// Run a heavy tool body either as a background SEP-2663 task (when the
    /// client negotiated `2026-07-28+` and declared task support) or inline on
    /// `spawn_blocking` with an active execution deadline.
    pub async fn run_task_or_inline<F>(
        &self,
        task_manager: &TaskManager,
        context: &RequestContext<RoleServer>,
        timeout: Duration,
        initial_status: &'static str,
        work: F,
    ) -> Result<CallToolResponse, McpError>
    where
        F: FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
            + Send
            + 'static,
    {
        self.run_with_note(task_manager, context, timeout, initial_status, None, work)
            .await
    }

    async fn run_with_note<F>(
        &self,
        task_manager: &TaskManager,
        context: &RequestContext<RoleServer>,
        timeout: Duration,
        initial_status: &'static str,
        note: Option<TimeoutNote>,
        work: F,
    ) -> Result<CallToolResponse, McpError>
    where
        F: FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
            + Send
            + 'static,
    {
        if !client_supports_tasks(context) {
            return self
                .run_inline_with_deadline(&context.ct, timeout, note.as_ref(), work)
                .await;
        }

        let task = self.spawn_task(task_manager, timeout, initial_status, note, work);
        Ok(CallToolResponse::Task(CreateTaskResult::new(task)))
    }

    /// Start the task that runs `work` under the shared semaphore, with the
    /// deadline `timeout` covering the wait for capacity and the run alike.
    fn spawn_task<F>(
        &self,
        task_manager: &TaskManager,
        timeout: Duration,
        initial_status: &'static str,
        note: Option<TimeoutNote>,
        work: F,
    ) -> rmcp::model::Task
    where
        F: FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
            + Send
            + 'static,
    {
        let coordinator = self.clone();
        let options = TaskOptions::new()
            .with_ttl_ms(Some(TASK_RETENTION_TTL_MS))
            .with_poll_interval_ms(TASK_POLL_INTERVAL_MS);

        let task = task_manager.spawn(options, move |task_ctx| {
            Box::pin(async move {
                task_ctx.set_status_message(initial_status);

                let cancel_flag = Arc::new(AtomicBool::new(false));
                let _cancel_on_drop = CancelOnDrop(Arc::clone(&cancel_flag));
                let deadline = tokio::time::sleep(timeout);
                tokio::pin!(deadline);

                let permit = tokio::select! {
                    biased;
                    _ = task_ctx.cancelled() => {
                        cancel_flag.store(true, Ordering::SeqCst);
                        return Err(TaskExit::Cancelled);
                    }
                    _ = &mut deadline => {
                        cancel_flag.store(true, Ordering::SeqCst);
                        return Err(TaskExit::Error(timed_out(
                            timeout,
                            " while waiting for execution capacity",
                            note.as_ref(),
                        )));
                    }
                    res = Arc::clone(&coordinator.semaphore).acquire_owned() => {
                        res.map_err(|e| {
                            TaskExit::Error(McpError::internal_error(
                                format!("Semaphore closed: {e}"),
                                None,
                            ))
                        })?
                    }
                };

                let active_guard = coordinator.register_op(Arc::clone(&cancel_flag));
                let worker_flag = Arc::clone(&cancel_flag);
                let (reporter, mut status_rx) = StatusReporter::channel();

                let mut handle = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let _active = active_guard;
                    work(&worker_flag, &reporter)
                });

                let mut status_closed = false;
                loop {
                    tokio::select! {
                        biased;
                        _ = task_ctx.cancelled() => {
                            cancel_flag.store(true, Ordering::SeqCst);
                            let _ = tokio::time::timeout(CANCEL_DRAIN_TIMEOUT, &mut handle).await;
                            return Err(TaskExit::Cancelled);
                        }
                        _ = &mut deadline => {
                            let error = timed_out(timeout, "", note.as_ref());
                            cancel_flag.store(true, Ordering::SeqCst);
                            let _ = tokio::time::timeout(CANCEL_DRAIN_TIMEOUT, &mut handle).await;
                            return Err(TaskExit::Error(error));
                        }
                        join_res = &mut handle => {
                            while let Ok(msg) = status_rx.try_recv() {
                                task_ctx.set_status_message(msg);
                            }
                            return match join_res {
                                Ok(Ok(result)) => {
                                    if cancel_flag.load(Ordering::SeqCst)
                                        || task_ctx.is_cancel_requested()
                                    {
                                        Err(TaskExit::Cancelled)
                                    } else {
                                        Ok(result)
                                    }
                                }
                                Ok(Err(EvalCancelled)) => Err(TaskExit::Cancelled),
                                Err(join_err) => Err(TaskExit::Error(McpError::internal_error(
                                    format!("Blocking worker panicked: {join_err}"),
                                    None,
                                ))),
                            };
                        }
                        maybe_msg = status_rx.recv(), if !status_closed => {
                            match maybe_msg {
                                Some(msg) => task_ctx.set_status_message(msg),
                                None => status_closed = true,
                            }
                        }
                    }
                }
            })
        });

        task
    }

    async fn run_inline_with_deadline<F>(
        &self,
        ct: &CancellationToken,
        timeout: Duration,
        note: Option<&TimeoutNote>,
        work: F,
    ) -> Result<CallToolResponse, McpError>
    where
        F: FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
            + Send
            + 'static,
    {
        if ct.is_cancelled() {
            return Err(McpError::internal_error("Request cancelled", None));
        }

        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        let permit = tokio::select! {
            biased;
            _ = ct.cancelled() => {
                return Err(McpError::internal_error("Request cancelled", None));
            }
            _ = &mut deadline => {
                return Err(timed_out(timeout, "", note));
            }
            res = Arc::clone(&self.semaphore).acquire_owned() => {
                res.map_err(|e| McpError::internal_error(format!("Semaphore closed: {e}"), None))?
            }
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(Arc::clone(&cancel_flag));
        let active_guard = self.register_op(Arc::clone(&cancel_flag));
        let worker_flag = Arc::clone(&cancel_flag);
        let reporter = StatusReporter::noop();

        let mut handle = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _active = active_guard;
            work(&worker_flag, &reporter)
        });

        tokio::select! {
            biased;
            _ = ct.cancelled() => {
                cancel_flag.store(true, Ordering::SeqCst);
                let _ = tokio::time::timeout(CANCEL_DRAIN_TIMEOUT, &mut handle).await;
                Err(McpError::internal_error("Request cancelled", None))
            }
            _ = &mut deadline => {
                let error = timed_out(timeout, "", note);
                cancel_flag.store(true, Ordering::SeqCst);
                let _ = tokio::time::timeout(CANCEL_DRAIN_TIMEOUT, &mut handle).await;
                Err(error)
            }
            join_res = &mut handle => {
                match join_res {
                    Ok(Ok(result)) => Ok(CallToolResponse::Complete(result)),
                    Ok(Err(EvalCancelled)) => Err(McpError::internal_error("Request cancelled", None)),
                    Err(join_err) => Err(McpError::internal_error(
                        format!("Blocking worker panicked: {join_err}"),
                        None,
                    )),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeout_error_carries_the_tools_note_when_it_has_one() {
        let timeout = Duration::from_secs(300);
        let plain = timed_out(timeout, "", None);
        assert_eq!(plain.message, "Operation timed out after 300s");
        let note: TimeoutNote = Arc::new(|| "while scoring 4 file(s).".to_string());
        let waiting = timed_out(timeout, " while waiting for execution capacity", None);
        assert_eq!(
            waiting.message,
            "Operation timed out after 300s while waiting for execution capacity"
        );
        let described = timed_out(timeout, "", Some(&note));
        assert_eq!(
            described.message,
            "Operation timed out after 300s while scoring 4 file(s)."
        );
    }

    /// A note that reads `phase` when the deadline fires.
    fn phase_note(phase: &Arc<Mutex<&'static str>>) -> TimeoutNote {
        let phase = Arc::clone(phase);
        Arc::new(move || format!("while {}.", phase.lock().unwrap()))
    }

    /// A worker that sets `phase` to `scoring`, waits to be cancelled, then
    /// moves it on to `finishing` and returns, as a worker does when it
    /// finishes during the drain.
    fn worker(
        phase: &Arc<Mutex<&'static str>>,
    ) -> impl FnOnce(&AtomicBool, &StatusReporter) -> Result<CallToolResult, EvalCancelled>
           + Send
           + 'static {
        let phase = Arc::clone(phase);
        move |cancel, _| {
            *phase.lock().unwrap() = "scoring";
            while !cancel.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            *phase.lock().unwrap() = "finishing";
            Err(EvalCancelled)
        }
    }

    #[tokio::test]
    async fn an_inline_timeout_carries_the_phase_at_the_deadline() {
        let coordinator = ExecutionCoordinator::with_max_concurrency(2);
        let phase = Arc::new(Mutex::new("queued"));
        let note = phase_note(&phase);
        let error = coordinator
            .run_inline_with_deadline(
                &CancellationToken::new(),
                Duration::from_millis(100),
                Some(&note),
                worker(&phase),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        // The worker moved on while it was being drained; the note was read
        // before that.
        assert_eq!(error.message, "Operation timed out after 0s while scoring.");
        assert_eq!(*phase.lock().unwrap(), "finishing");
    }

    #[tokio::test]
    async fn a_task_timeout_carries_the_phase_at_the_deadline() {
        let coordinator = ExecutionCoordinator::with_max_concurrency(2);
        let phase = Arc::new(Mutex::new("queued"));
        let manager = TaskManager::new();
        let task = coordinator.spawn_task(
            &manager,
            Duration::from_millis(100),
            "working",
            Some(phase_note(&phase)),
            worker(&phase),
        );
        let message = failed_message(&manager, &task.task_id).await;
        assert_eq!(message, "Operation timed out after 0s while scoring.");
    }

    #[tokio::test]
    async fn a_task_that_never_got_capacity_says_so_and_carries_the_note() {
        let coordinator = ExecutionCoordinator::with_max_concurrency(1);
        let _held = Arc::clone(&coordinator.semaphore)
            .acquire_owned()
            .await
            .unwrap();
        let phase = Arc::new(Mutex::new("queued"));
        let manager = TaskManager::new();
        let task = coordinator.spawn_task(
            &manager,
            Duration::from_millis(50),
            "working",
            Some(phase_note(&phase)),
            worker(&phase),
        );
        let message = failed_message(&manager, &task.task_id).await;
        assert_eq!(message, "Operation timed out after 0s while queued.");
    }

    /// The error message of the task once it has failed.
    async fn failed_message(manager: &TaskManager, id: &str) -> String {
        for _ in 0..200 {
            let payload = manager.get_task(id).unwrap().payload;
            if let rmcp::model::TaskPayload::Failed { error } = payload {
                return error["message"].as_str().unwrap().to_string();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the task did not fail");
    }

    #[tokio::test]
    async fn cancel_on_drop_sets_flag_when_future_is_aborted() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);
        let handle = tokio::spawn(async move {
            let _guard = CancelOnDrop(flag_clone);
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        tokio::task::yield_now().await;
        handle.abort();
        let _ = handle.await;
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn shutdown_and_wait_signals_active_operations_and_waits_for_completion() {
        let coord = ExecutionCoordinator::with_max_concurrency(2);
        let flag = Arc::new(AtomicBool::new(false));
        let active_guard = coord.register_op(Arc::clone(&flag));
        let worker_flag = Arc::clone(&flag);

        tokio::task::spawn_blocking(move || {
            let _guard = active_guard;
            while !worker_flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        assert_eq!(coord.active_operation_count(), 1);
        coord.shutdown_and_wait(Duration::from_secs(2)).await;
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(coord.active_operation_count(), 0);
    }
}
