use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};

use crate::navigation_model::{PreviewTarget, SourceFreshness};
use crate::runtime_discovery::{
    CommandRunner, PaneSnapshot, RuntimeDiscovery, RuntimeDiscoveryError,
};

const LIVE_UPDATE_CHANNEL_CAPACITY: usize = 1;
const MAX_UPDATES_PER_DRAIN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneSnapshotRequest {
    pub(crate) session_name: String,
    pub(crate) tab_id: u64,
    pub(crate) tab_position: u64,
    pub(crate) pane_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotPreviewState {
    Unavailable {
        reason: String,
    },
    Stale {
        request: PaneSnapshotRequest,
    },
    Loading {
        request: PaneSnapshotRequest,
    },
    Ready {
        request: PaneSnapshotRequest,
        body: String,
    },
    Empty {
        request: PaneSnapshotRequest,
    },
    Error {
        request: PaneSnapshotRequest,
        message: String,
    },
}

pub(crate) trait SnapshotLoader: Send + Sync {
    fn load_snapshot(
        &self,
        request: &PaneSnapshotRequest,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError>;

    fn load_snapshot_with_cancel(
        &self,
        request: &PaneSnapshotRequest,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
        if is_cancelled() {
            return Err(RuntimeDiscoveryError::Cancelled {
                command: "snapshot loader invocation".to_owned(),
            });
        }

        self.load_snapshot(request)
    }

    fn subscribe_live_snapshots_with_cancel(
        &self,
        request: &PaneSnapshotRequest,
        on_snapshot: &mut dyn FnMut(PaneSnapshot),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<(), RuntimeDiscoveryError> {
        if is_cancelled() {
            return Err(RuntimeDiscoveryError::Cancelled {
                command: "snapshot live subscription".to_owned(),
            });
        }

        let snapshot = self.load_snapshot_with_cancel(request, is_cancelled)?;
        if is_cancelled() {
            return Err(RuntimeDiscoveryError::Cancelled {
                command: "snapshot live subscription".to_owned(),
            });
        }

        on_snapshot(snapshot);
        Ok(())
    }
}

impl<T: SnapshotLoader + ?Sized> SnapshotLoader for Arc<T> {
    fn load_snapshot(
        &self,
        request: &PaneSnapshotRequest,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
        (**self).load_snapshot(request)
    }

    fn load_snapshot_with_cancel(
        &self,
        request: &PaneSnapshotRequest,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
        (**self).load_snapshot_with_cancel(request, is_cancelled)
    }

    fn subscribe_live_snapshots_with_cancel(
        &self,
        request: &PaneSnapshotRequest,
        on_snapshot: &mut dyn FnMut(PaneSnapshot),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<(), RuntimeDiscoveryError> {
        (**self).subscribe_live_snapshots_with_cancel(request, on_snapshot, is_cancelled)
    }
}

impl<R: CommandRunner + Send + Sync> SnapshotLoader for RuntimeDiscovery<R> {
    fn load_snapshot(
        &self,
        request: &PaneSnapshotRequest,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
        self.dump_screen_for_pane(&request.session_name, request.pane_id)
    }

    fn load_snapshot_with_cancel(
        &self,
        request: &PaneSnapshotRequest,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
        self.dump_screen_for_pane_with_cancel(&request.session_name, request.pane_id, is_cancelled)
    }

    fn subscribe_live_snapshots_with_cancel(
        &self,
        request: &PaneSnapshotRequest,
        on_snapshot: &mut dyn FnMut(PaneSnapshot),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<(), RuntimeDiscoveryError> {
        self.subscribe_pane_updates_with_cancel(
            &request.session_name,
            request.pane_id,
            on_snapshot,
            is_cancelled,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingSnapshotRequest {
    request_id: u64,
    request: PaneSnapshotRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LivePreviewPayload {
    Snapshot(PaneSnapshot),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePreviewUpdate {
    request_id: u64,
    request: PaneSnapshotRequest,
    payload: LivePreviewPayload,
}

struct LivePreviewWorker {
    request_id: u64,
    request: PaneSnapshotRequest,
    updates_rx: Receiver<()>,
    latest_update: Arc<Mutex<Option<LivePreviewUpdate>>>,
    cancel_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

pub(crate) struct SnapshotPreviewController {
    loader: Option<Arc<dyn SnapshotLoader>>,
    state: SnapshotPreviewState,
    active_request_id: Option<u64>,
    active_request: Option<PaneSnapshotRequest>,
    request_counter: u64,
    live_worker: Option<LivePreviewWorker>,
}

impl SnapshotPreviewController {
    pub(crate) fn disabled() -> Self {
        Self {
            loader: None,
            state: SnapshotPreviewState::Unavailable {
                reason: "selection has no pane snapshot target".to_owned(),
            },
            active_request_id: None,
            active_request: None,
            request_counter: 0,
            live_worker: None,
        }
    }

    pub(crate) fn with_loader(loader: Box<dyn SnapshotLoader>) -> Self {
        Self {
            loader: Some(Arc::from(loader)),
            state: SnapshotPreviewState::Unavailable {
                reason: "selection has no pane snapshot target".to_owned(),
            },
            active_request_id: None,
            active_request: None,
            request_counter: 0,
            live_worker: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_loader_for_tests(loader: Box<dyn SnapshotLoader>) -> Self {
        Self::with_loader(loader)
    }

    pub(crate) fn state(&self) -> &SnapshotPreviewState {
        &self.state
    }

    pub(crate) fn poll_live_updates(&mut self) -> bool {
        let mut changed = false;

        for _ in 0..MAX_UPDATES_PER_DRAIN {
            let receive_result = match self.live_worker.as_ref() {
                Some(worker) => worker.updates_rx.try_recv(),
                None => break,
            };

            match receive_result {
                Ok(()) => {
                    if let Some(update) = self.take_latest_live_update() {
                        changed |= self.apply_live_update(update);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if let Some(update) = self.take_latest_live_update() {
                        changed |= self.apply_live_update(update);
                    }
                    self.stop_live_worker();
                    break;
                }
            }
        }

        changed
    }

    pub(crate) fn refresh_for_target(&mut self, preview_target: &PreviewTarget) {
        let Some((request, source_freshness)) = request_from_preview_target(preview_target) else {
            self.clear_active_request();
            self.stop_live_worker();
            self.state = SnapshotPreviewState::Unavailable {
                reason: "selection has no pane snapshot target".to_owned(),
            };
            return;
        };

        if source_freshness == SourceFreshness::Stale {
            self.clear_active_request();
            self.stop_live_worker();
            self.state = SnapshotPreviewState::Stale { request };
            return;
        }

        if self.loader.is_none() {
            self.clear_active_request();
            self.stop_live_worker();
            self.state = SnapshotPreviewState::Unavailable {
                reason: "snapshot loader is disabled".to_owned(),
            };
            return;
        }

        if self.active_request.as_ref() == Some(&request)
            && self.active_request_id.is_some()
            && source_supports_live_updates(source_freshness)
        {
            if self.live_worker.is_none() {
                let request_id = self
                    .active_request_id
                    .expect("active request id checked before restart");
                self.start_live_worker(request_id, request.clone());
            }
            return;
        }

        let pending = self.begin_request(request);
        let result = self
            .loader
            .as_ref()
            .map(|loader| loader.load_snapshot(&pending.request))
            .expect("snapshot loader checked before request start");
        let _ = self.apply_request_result(pending, result);

        if source_supports_live_updates(source_freshness) {
            let request_id = self
                .active_request_id
                .expect("request id is active after request application");
            let request = self
                .active_request
                .as_ref()
                .expect("request is active after request application")
                .clone();
            self.start_live_worker(request_id, request);
        } else {
            self.stop_live_worker();
        }
    }

    fn begin_request(&mut self, request: PaneSnapshotRequest) -> PendingSnapshotRequest {
        self.stop_live_worker();
        let request_id = self.request_counter.wrapping_add(1);
        self.request_counter = request_id;
        self.active_request_id = Some(request_id);
        self.active_request = Some(request.clone());
        self.state = SnapshotPreviewState::Loading {
            request: request.clone(),
        };

        PendingSnapshotRequest {
            request_id,
            request,
        }
    }

    fn apply_request_result(
        &mut self,
        pending: PendingSnapshotRequest,
        result: Result<PaneSnapshot, RuntimeDiscoveryError>,
    ) -> bool {
        let is_active_request = self.active_request_id == Some(pending.request_id)
            && self.active_request.as_ref() == Some(&pending.request);
        if !is_active_request {
            return false;
        }

        self.state = match result {
            Ok(PaneSnapshot::Ready(body)) => SnapshotPreviewState::Ready {
                request: pending.request,
                body,
            },
            Ok(PaneSnapshot::Empty) => SnapshotPreviewState::Empty {
                request: pending.request,
            },
            Err(error) => SnapshotPreviewState::Error {
                request: pending.request,
                message: error.to_string(),
            },
        };

        true
    }

    fn apply_live_update(&mut self, update: LivePreviewUpdate) -> bool {
        let is_active_request = self.active_request_id == Some(update.request_id)
            && self.active_request.as_ref() == Some(&update.request)
            && self.live_worker.as_ref().is_some_and(|worker| {
                worker.request_id == update.request_id && worker.request == update.request
            });

        if !is_active_request {
            return false;
        }

        let next_state = match update.payload {
            LivePreviewPayload::Snapshot(PaneSnapshot::Ready(body)) => {
                SnapshotPreviewState::Ready {
                    request: update.request,
                    body,
                }
            }
            LivePreviewPayload::Snapshot(PaneSnapshot::Empty) => SnapshotPreviewState::Empty {
                request: update.request,
            },
            LivePreviewPayload::Error(message) => SnapshotPreviewState::Error {
                request: update.request,
                message,
            },
        };

        if self.state == next_state {
            return false;
        }

        self.state = next_state;
        true
    }

    fn clear_active_request(&mut self) {
        self.active_request_id = None;
        self.active_request = None;
    }

    fn start_live_worker(&mut self, request_id: u64, request: PaneSnapshotRequest) {
        self.stop_live_worker();

        let Some(loader) = self.loader.as_ref() else {
            return;
        };

        let (updates_tx, updates_rx) = mpsc::sync_channel(LIVE_UPDATE_CHANNEL_CAPACITY);
        let latest_update = Arc::new(Mutex::new(None));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let worker_cancel_flag = Arc::clone(&cancel_flag);
        let worker_loader = Arc::clone(loader);
        let worker_request = request.clone();
        let worker_latest_update = Arc::clone(&latest_update);

        let join_handle = thread::spawn(move || {
            run_subscribe_snapshot_worker(
                worker_loader,
                request_id,
                worker_request,
                updates_tx,
                worker_latest_update,
                worker_cancel_flag,
            );
        });

        self.live_worker = Some(LivePreviewWorker {
            request_id,
            request,
            updates_rx,
            latest_update,
            cancel_flag,
            join_handle: Some(join_handle),
        });
    }

    pub(crate) fn shutdown(&mut self) {
        self.stop_live_worker();
    }

    fn stop_live_worker(&mut self) {
        let Some(mut worker) = self.live_worker.take() else {
            return;
        };

        worker.cancel_flag.store(true, Ordering::Relaxed);
        if let Some(join_handle) = worker.join_handle.take() {
            let _ = join_handle.join();
        }
    }

    fn take_latest_live_update(&self) -> Option<LivePreviewUpdate> {
        let worker = self.live_worker.as_ref()?;

        take_latest_update(worker.latest_update.as_ref())
    }
}

impl Drop for SnapshotPreviewController {
    fn drop(&mut self) {
        self.stop_live_worker();
    }
}

fn run_subscribe_snapshot_worker(
    loader: Arc<dyn SnapshotLoader>,
    request_id: u64,
    request: PaneSnapshotRequest,
    updates_tx: SyncSender<()>,
    latest_update: Arc<Mutex<Option<LivePreviewUpdate>>>,
    cancel_flag: Arc<AtomicBool>,
) {
    let mut emit_snapshot = |snapshot: PaneSnapshot| {
        if cancel_flag.load(Ordering::Relaxed) {
            return;
        }

        let update = LivePreviewUpdate {
            request_id,
            request: request.clone(),
            payload: LivePreviewPayload::Snapshot(snapshot),
        };

        store_latest_update(latest_update.as_ref(), update);
        if let Err(TrySendError::Disconnected(_)) = updates_tx.try_send(()) {
            cancel_flag.store(true, Ordering::Relaxed);
        }
    };

    let subscribe_result =
        loader.subscribe_live_snapshots_with_cancel(&request, &mut emit_snapshot, &|| {
            cancel_flag.load(Ordering::Relaxed)
        });

    match subscribe_result {
        Ok(()) | Err(RuntimeDiscoveryError::Cancelled { .. }) => {}
        Err(error) => {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }

            let payload = match loader
                .load_snapshot_with_cancel(&request, &|| cancel_flag.load(Ordering::Relaxed))
            {
                Ok(snapshot) => LivePreviewPayload::Snapshot(snapshot),
                Err(RuntimeDiscoveryError::Cancelled { .. }) => return,
                Err(fallback_error) => LivePreviewPayload::Error(format!(
                    "{error}; fallback snapshot failed: {fallback_error}"
                )),
            };

            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }

            let update = LivePreviewUpdate {
                request_id,
                request,
                payload,
            };

            store_latest_update(latest_update.as_ref(), update);
            let _ = updates_tx.try_send(());
        }
    }
}

fn store_latest_update(slot: &Mutex<Option<LivePreviewUpdate>>, update: LivePreviewUpdate) {
    match slot.lock() {
        Ok(mut guard) => {
            *guard = Some(update);
        }
        Err(poisoned) => {
            *poisoned.into_inner() = Some(update);
        }
    }
}

fn take_latest_update(slot: &Mutex<Option<LivePreviewUpdate>>) -> Option<LivePreviewUpdate> {
    match slot.lock() {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    }
}

fn source_supports_live_updates(source_freshness: SourceFreshness) -> bool {
    matches!(
        source_freshness,
        SourceFreshness::Runtime | SourceFreshness::Subscription
    )
}

fn request_from_preview_target(
    preview_target: &PreviewTarget,
) -> Option<(PaneSnapshotRequest, SourceFreshness)> {
    match preview_target {
        PreviewTarget::PaneSnapshotCandidate {
            session_name,
            tab_id,
            tab_position,
            pane_id,
            source_freshness,
        } => Some((
            PaneSnapshotRequest {
                session_name: session_name.clone(),
                tab_id: *tab_id,
                tab_position: *tab_position,
                pane_id: *pane_id,
            },
            *source_freshness,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use super::{
        LivePreviewPayload, LivePreviewUpdate, PaneSnapshotRequest, PendingSnapshotRequest,
        SnapshotLoader, SnapshotPreviewController, SnapshotPreviewState, store_latest_update,
        take_latest_update,
    };
    use crate::navigation_model::{PreviewTarget, SourceFreshness};
    use crate::runtime_discovery::{PaneSnapshot, RuntimeDiscoveryError};

    struct FakeLoader {
        queue: Mutex<VecDeque<Result<PaneSnapshot, RuntimeDiscoveryError>>>,
    }

    impl FakeLoader {
        fn new(results: Vec<Result<PaneSnapshot, RuntimeDiscoveryError>>) -> Self {
            Self {
                queue: Mutex::new(results.into()),
            }
        }
    }

    impl SnapshotLoader for FakeLoader {
        fn load_snapshot(
            &self,
            _request: &PaneSnapshotRequest,
        ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
            self.queue
                .lock()
                .expect("fake snapshot queue lock should succeed")
                .pop_front()
                .expect("fake snapshot queue should have next result")
        }

        fn subscribe_live_snapshots_with_cancel(
            &self,
            _request: &PaneSnapshotRequest,
            _on_snapshot: &mut dyn FnMut(PaneSnapshot),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<(), RuntimeDiscoveryError> {
            Err(RuntimeDiscoveryError::Cancelled {
                command: "fake-subscribe".to_owned(),
            })
        }
    }

    fn pane_target(source_freshness: SourceFreshness) -> PreviewTarget {
        pane_target_for_pane_id(source_freshness, 7)
    }

    fn pane_target_for_pane_id(source_freshness: SourceFreshness, pane_id: u64) -> PreviewTarget {
        PreviewTarget::PaneSnapshotCandidate {
            session_name: "work".to_owned(),
            tab_id: 3,
            tab_position: 2,
            pane_id,
            source_freshness,
        }
    }

    fn pane_request(session_name: &str, pane_id: u64) -> PaneSnapshotRequest {
        PaneSnapshotRequest {
            session_name: session_name.to_owned(),
            tab_id: 1,
            tab_position: 0,
            pane_id,
        }
    }

    #[test]
    fn refresh_maps_loader_success_empty_and_error_results() {
        let mut ready_controller = SnapshotPreviewController::with_loader(Box::new(
            FakeLoader::new(vec![Ok(PaneSnapshot::Ready("hello".to_owned()))]),
        ));
        ready_controller.refresh_for_target(&pane_target(SourceFreshness::Runtime));
        assert!(matches!(
            ready_controller.state(),
            SnapshotPreviewState::Ready { body, .. } if body == "hello"
        ));

        let mut empty_controller = SnapshotPreviewController::with_loader(Box::new(
            FakeLoader::new(vec![Ok(PaneSnapshot::Empty)]),
        ));
        empty_controller.refresh_for_target(&pane_target(SourceFreshness::Runtime));
        assert!(matches!(
            empty_controller.state(),
            SnapshotPreviewState::Empty { .. }
        ));

        let mut error_controller = SnapshotPreviewController::with_loader(Box::new(
            FakeLoader::new(vec![Err(RuntimeDiscoveryError::CommandFailed {
                command: "zellij --session work action dump-screen --pane-id 7 --ansi".to_owned(),
                exit_code: Some(1),
                stderr: "boom".to_owned(),
            })]),
        ));
        error_controller.refresh_for_target(&pane_target(SourceFreshness::Runtime));
        assert!(matches!(
            error_controller.state(),
            SnapshotPreviewState::Error { message, .. } if message.contains("dump-screen")
        ));
    }

    #[test]
    fn stale_result_is_rejected_and_does_not_override_newer_preview() {
        let mut controller = SnapshotPreviewController::disabled();
        let old_pending = controller.begin_request(pane_request("session-a", 3));
        let new_pending = controller.begin_request(pane_request("session-b", 4));

        assert!(controller.apply_request_result(
            clone_pending(&new_pending),
            Ok(PaneSnapshot::Ready("new".to_owned()))
        ));
        assert!(!controller.apply_request_result(
            clone_pending(&old_pending),
            Ok(PaneSnapshot::Ready("old".to_owned()))
        ));

        assert!(matches!(
            controller.state(),
            SnapshotPreviewState::Ready { request, body }
                if request.session_name == "session-b" && request.pane_id == 4 && body == "new"
        ));
    }

    #[test]
    fn stale_source_maps_to_stale_preview_state() {
        let mut controller = SnapshotPreviewController::disabled();
        controller.refresh_for_target(&pane_target(SourceFreshness::Stale));

        assert!(matches!(
            controller.state(),
            SnapshotPreviewState::Stale { request }
                if request.session_name == "work" && request.pane_id == 7
        ));
    }

    #[derive(Default)]
    struct CountingLoader {
        snapshot_calls: AtomicUsize,
        subscribe_calls: AtomicUsize,
        active_subscriptions: AtomicUsize,
        max_active_subscriptions: AtomicUsize,
    }

    impl CountingLoader {
        fn snapshot_calls(&self) -> usize {
            self.snapshot_calls.load(Ordering::SeqCst)
        }

        fn subscribe_calls(&self) -> usize {
            self.subscribe_calls.load(Ordering::SeqCst)
        }

        fn active_subscriptions(&self) -> usize {
            self.active_subscriptions.load(Ordering::SeqCst)
        }

        fn max_active_subscriptions(&self) -> usize {
            self.max_active_subscriptions.load(Ordering::SeqCst)
        }
    }

    impl SnapshotLoader for CountingLoader {
        fn load_snapshot(
            &self,
            _request: &PaneSnapshotRequest,
        ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
            let sequence = self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            Ok(PaneSnapshot::Ready(format!("frame-{sequence}")))
        }

        fn subscribe_live_snapshots_with_cancel(
            &self,
            _request: &PaneSnapshotRequest,
            _on_snapshot: &mut dyn FnMut(PaneSnapshot),
            is_cancelled: &dyn Fn() -> bool,
        ) -> Result<(), RuntimeDiscoveryError> {
            self.subscribe_calls.fetch_add(1, Ordering::SeqCst);

            let active = self.active_subscriptions.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active_subscriptions
                .fetch_max(active, Ordering::SeqCst);

            while !is_cancelled() {
                thread::sleep(Duration::from_millis(10));
            }

            self.active_subscriptions.fetch_sub(1, Ordering::SeqCst);
            Err(RuntimeDiscoveryError::Cancelled {
                command: "test-subscribe".to_owned(),
            })
        }
    }

    #[derive(Default)]
    struct StableLoader {
        snapshot_calls: AtomicUsize,
        subscribe_calls: AtomicUsize,
    }

    impl StableLoader {
        fn snapshot_calls(&self) -> usize {
            self.snapshot_calls.load(Ordering::SeqCst)
        }

        fn subscribe_calls(&self) -> usize {
            self.subscribe_calls.load(Ordering::SeqCst)
        }
    }

    impl SnapshotLoader for StableLoader {
        fn load_snapshot(
            &self,
            _request: &PaneSnapshotRequest,
        ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
            self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            Ok(PaneSnapshot::Ready("steady".to_owned()))
        }

        fn subscribe_live_snapshots_with_cancel(
            &self,
            _request: &PaneSnapshotRequest,
            on_snapshot: &mut dyn FnMut(PaneSnapshot),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<(), RuntimeDiscoveryError> {
            self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
            on_snapshot(PaneSnapshot::Ready("steady".to_owned()));
            Ok(())
        }
    }

    #[test]
    fn live_worker_lifecycle_cancels_previous_worker_and_avoids_duplicates() {
        let loader = std::sync::Arc::new(CountingLoader::default());
        let mut controller =
            SnapshotPreviewController::with_loader_for_tests(Box::new(loader.clone()));

        controller.refresh_for_target(&pane_target_for_pane_id(SourceFreshness::Runtime, 7));
        thread::sleep(Duration::from_millis(30));
        let first_request_id = controller
            .live_worker
            .as_ref()
            .expect("first worker should start")
            .request_id;

        controller.refresh_for_target(&pane_target_for_pane_id(SourceFreshness::Runtime, 7));
        let repeated_request_id = controller
            .live_worker
            .as_ref()
            .expect("worker should remain active for same pane")
            .request_id;
        assert_eq!(first_request_id, repeated_request_id);
        assert_eq!(loader.subscribe_calls(), 1);

        controller.refresh_for_target(&pane_target_for_pane_id(SourceFreshness::Runtime, 9));
        thread::sleep(Duration::from_millis(30));
        let second_request_id = controller
            .live_worker
            .as_ref()
            .expect("worker should restart for new pane")
            .request_id;
        assert_ne!(first_request_id, second_request_id);
        assert_eq!(loader.subscribe_calls(), 2);

        controller.refresh_for_target(&PreviewTarget::Unavailable {
            reason: "no pane selected",
        });
        assert!(controller.live_worker.is_none());
        thread::sleep(Duration::from_millis(30));
        assert_eq!(loader.active_subscriptions(), 0);

        let before_drop = loader.snapshot_calls();
        drop(controller);
        thread::sleep(Duration::from_millis(30));
        assert_eq!(loader.snapshot_calls(), before_drop);
        assert_eq!(loader.max_active_subscriptions(), 1);
    }

    #[test]
    fn stale_live_updates_are_rejected_by_request_identity() {
        let mut controller = SnapshotPreviewController::disabled();
        let old_pending = controller.begin_request(pane_request("session-a", 3));
        let new_pending = controller.begin_request(pane_request("session-b", 4));

        let stale_update = LivePreviewUpdate {
            request_id: old_pending.request_id,
            request: old_pending.request.clone(),
            payload: LivePreviewPayload::Snapshot(PaneSnapshot::Ready("old".to_owned())),
        };

        assert!(!controller.apply_live_update(stale_update));
        assert!(controller.apply_request_result(
            clone_pending(&new_pending),
            Ok(PaneSnapshot::Ready("new".to_owned()))
        ));
        assert!(matches!(
            controller.state(),
            SnapshotPreviewState::Ready { request, body }
                if request.session_name == "session-b" && request.pane_id == 4 && body == "new"
        ));
    }

    #[test]
    fn duplicate_live_frames_are_coalesced() {
        let loader = std::sync::Arc::new(StableLoader::default());
        let mut controller =
            SnapshotPreviewController::with_loader_for_tests(Box::new(loader.clone()));

        controller.refresh_for_target(&pane_target(SourceFreshness::Runtime));
        assert!(matches!(
            controller.state(),
            SnapshotPreviewState::Ready { body, .. } if body == "steady"
        ));

        thread::sleep(Duration::from_millis(30));
        assert!(!controller.poll_live_updates());
        assert_eq!(loader.snapshot_calls(), 1);
        assert_eq!(loader.subscribe_calls(), 1);
    }

    #[test]
    fn latest_update_slot_replaces_older_frame() {
        let slot = Mutex::new(None);

        store_latest_update(
            &slot,
            LivePreviewUpdate {
                request_id: 1,
                request: pane_request("work", 7),
                payload: LivePreviewPayload::Snapshot(PaneSnapshot::Ready("frame-1".to_owned())),
            },
        );
        store_latest_update(
            &slot,
            LivePreviewUpdate {
                request_id: 1,
                request: pane_request("work", 7),
                payload: LivePreviewPayload::Snapshot(PaneSnapshot::Ready("frame-2".to_owned())),
            },
        );

        let latest = take_latest_update(&slot).expect("latest update should be available");
        assert!(matches!(
            latest.payload,
            LivePreviewPayload::Snapshot(PaneSnapshot::Ready(body)) if body == "frame-2"
        ));
        assert!(take_latest_update(&slot).is_none());
    }

    #[test]
    fn subscribe_unavailable_keeps_one_shot_snapshot_fallback() {
        struct UnavailableSubscribeLoader;

        impl SnapshotLoader for UnavailableSubscribeLoader {
            fn load_snapshot(
                &self,
                _request: &PaneSnapshotRequest,
            ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
                Ok(PaneSnapshot::Ready("baseline".to_owned()))
            }

            fn subscribe_live_snapshots_with_cancel(
                &self,
                _request: &PaneSnapshotRequest,
                _on_snapshot: &mut dyn FnMut(PaneSnapshot),
                _is_cancelled: &dyn Fn() -> bool,
            ) -> Result<(), RuntimeDiscoveryError> {
                Err(RuntimeDiscoveryError::CommandFailed {
                    command: "zellij --session work subscribe --pane-id 7 --ansi --format json"
                        .to_owned(),
                    exit_code: Some(1),
                    stderr: "subscribe unavailable".to_owned(),
                })
            }
        }

        let mut controller =
            SnapshotPreviewController::with_loader(Box::new(UnavailableSubscribeLoader));

        controller.refresh_for_target(&pane_target(SourceFreshness::Runtime));
        thread::sleep(Duration::from_millis(30));
        let _ = controller.poll_live_updates();
        assert!(matches!(
            controller.state(),
            SnapshotPreviewState::Ready { body, .. } if body == "baseline"
        ));
    }

    fn clone_pending(pending: &PendingSnapshotRequest) -> PendingSnapshotRequest {
        PendingSnapshotRequest {
            request_id: pending.request_id,
            request: pending.request.clone(),
        }
    }
}
