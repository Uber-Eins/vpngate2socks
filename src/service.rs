//! Application orchestration: refreshes, make-before-break switching, and test isolation.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore, broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    config::AppConfig,
    domain::{
        AppEvent, ConnectionState, NodeAvailability, NodeId, OperationId, ResolvedUpstreamEndpoint,
        TestRecord, TestState, UpstreamState, VpnNode, WorkerId,
    },
    netd::{NetdClient, NetdClientError},
    quality::fetch_ippure,
    socks::{UpstreamProbeError, probe_upstream},
    storage::{Store, StoreError},
    vpngate::{CsvLimits, ParseStats, VpnGateError, fetch_snapshot},
};

const TEST_QUEUE_CAPACITY: usize = 256;
const COMPLETED_OPERATION_HISTORY: usize = 1_024;
const OLD_WORKER_DRAIN: Duration = Duration::from_secs(30);

/// Cloneable application state shared by Axum handlers and background tasks.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    config: Arc<AppConfig>,
    upstream: ResolvedUpstreamEndpoint,
    store: Store,
    netd: NetdClient,
    nodes: RwLock<Arc<Vec<VpnNode>>>,
    last_refresh: RwLock<Option<RefreshInfo>>,
    refreshing: AtomicBool,
    connection_state: watch::Sender<ConnectionState>,
    active_worker: watch::Sender<Option<PathBuf>>,
    connection_commands: mpsc::Sender<ConnectionCommand>,
    operations: RwLock<HashMap<OperationId, TestState>>,
    test_queue: mpsc::Sender<TestJob>,
    queued_tests: AtomicUsize,
    running_tests: AtomicUsize,
    upstream_state: watch::Sender<UpstreamState>,
    events: broadcast::Sender<AppEvent>,
    shutdown: CancellationToken,
}

#[derive(Debug)]
enum ConnectionCommand {
    Connect {
        node_id: NodeId,
        response: oneshot::Sender<Result<ConnectionState, ServiceError>>,
    },
    Disconnect {
        response: oneshot::Sender<Result<ConnectionState, ServiceError>>,
    },
}

#[derive(Debug)]
struct TestJob {
    operation_id: OperationId,
    node_id: NodeId,
}

struct ActiveConnection {
    worker_id: WorkerId,
    node_id: NodeId,
    connected_at: DateTime<Utc>,
}

struct RefreshingGuard<'a>(&'a AtomicBool);

impl Drop for RefreshingGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Last successful node refresh.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshInfo {
    pub at: DateTime<Utc>,
    pub accepted: usize,
    pub rejected: usize,
    pub unsupported: usize,
}

/// Public service status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub connection: ConnectionState,
    pub proxy_ready: bool,
    pub queued_tests: usize,
    pub running_tests: usize,
    pub upstream_state: UpstreamState,
    pub last_refresh: Option<RefreshInfo>,
    pub lan_mode: bool,
    pub tls_configured: bool,
}

/// Expected service failure mapped to a stable API error code.
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("node was not found")]
    NodeNotFound,
    #[error("node is not usable by this version")]
    NodeUnavailable,
    #[error("node refresh is already running")]
    RefreshBusy,
    #[error("test queue is full")]
    QueueFull,
    #[error("test operation was not found")]
    OperationNotFound,
    #[error("background service is shutting down")]
    ShuttingDown,
    #[error("VPN Gate refresh failed")]
    Refresh(#[from] VpnGateError),
    #[error("worker operation failed")]
    Worker(#[from] NetdClientError),
    #[error("persistence failed")]
    Store(#[from] StoreError),
}

impl AppState {
    /// Creates state and starts bounded connection and test actors.
    #[must_use]
    pub fn new(
        config: AppConfig,
        upstream: ResolvedUpstreamEndpoint,
        store: Store,
        shutdown: CancellationToken,
    ) -> Self {
        let config = Arc::new(config);
        let netd = NetdClient::new(
            config.netd_socket.clone(),
            config.connect_timeout + Duration::from_secs(10),
        );
        let (connection_state, _) = watch::channel(ConnectionState::Disconnected);
        let (active_worker, _) = watch::channel(None);
        let (upstream_state, _) = watch::channel(UpstreamState::Checking);
        let (connection_commands, connection_rx) = mpsc::channel(8);
        let (test_queue, test_rx) = mpsc::channel(TEST_QUEUE_CAPACITY);
        let (events, _) = broadcast::channel(256);
        let state = Self(Arc::new(Inner {
            config,
            upstream,
            store,
            netd,
            nodes: RwLock::new(Arc::new(Vec::new())),
            last_refresh: RwLock::new(None),
            refreshing: AtomicBool::new(false),
            connection_state,
            active_worker,
            connection_commands,
            operations: RwLock::new(HashMap::new()),
            test_queue,
            queued_tests: AtomicUsize::new(0),
            running_tests: AtomicUsize::new(0),
            upstream_state,
            events,
            shutdown,
        }));
        tokio::spawn(connection_actor(state.clone(), connection_rx));
        tokio::spawn(test_dispatcher(state.clone(), test_rx));
        tokio::spawn(upstream_monitor(state.clone()));
        state
    }

    /// Returns immutable configuration.
    #[must_use]
    pub fn config(&self) -> &AppConfig {
        &self.0.config
    }

    /// Returns a receiver that always contains the currently selected worker socket.
    #[must_use]
    pub fn active_worker(&self) -> watch::Receiver<Option<PathBuf>> {
        self.0.active_worker.subscribe()
    }

    /// Subscribes to connection, test, and refresh events.
    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<AppEvent> {
        self.0.events.subscribe()
    }

    /// Returns an immutable node snapshot without holding a lock during later awaits.
    pub async fn nodes(&self) -> Arc<Vec<VpnNode>> {
        self.0.nodes.read().await.clone()
    }

    /// Loads the persisted latest-test map used to decorate node pages.
    pub async fn latest_tests(&self) -> Result<HashMap<NodeId, TestRecord>, StoreError> {
        self.0.store.latest_tests().await
    }

    /// Returns one internal node from the current snapshot.
    pub async fn node(&self, node_id: &NodeId) -> Option<VpnNode> {
        self.0
            .nodes
            .read()
            .await
            .iter()
            .find(|node| &node.id == node_id)
            .cloned()
    }

    /// Refreshes nodes atomically; a failed download leaves the previous snapshot intact.
    pub async fn refresh_nodes(&self) -> Result<RefreshInfo, ServiceError> {
        if self
            .0
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(ServiceError::RefreshBusy);
        }
        let _refreshing = RefreshingGuard(&self.0.refreshing);
        self.refresh_nodes_inner().await
    }

    async fn refresh_nodes_inner(&self) -> Result<RefreshInfo, ServiceError> {
        let snapshot = fetch_snapshot(
            &self.0.config.vpngate_url,
            &self.0.upstream,
            Duration::from_secs(30),
            CsvLimits::default(),
        )
        .await?;
        let stats = snapshot.stats;
        let current_ids = snapshot
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<HashSet<_>>();
        *self.0.nodes.write().await = Arc::new(snapshot.nodes);

        if let Some(cutoff) = Utc::now().checked_sub_signed(TimeDelta::days(30)) {
            if let Err(error) = self.0.store.cleanup_stale(&current_ids, cutoff).await {
                tracing::warn!(error = %error, "failed to clean stale test results");
            }
        }
        let info = refresh_info(stats);
        *self.0.last_refresh.write().await = Some(info.clone());
        self.emit(AppEvent::NodesRefreshed {
            accepted: info.accepted,
            rejected: info.rejected,
            at: info.at,
        });
        Ok(info)
    }

    /// Requests a make-before-break switch and waits for its result.
    pub async fn connect(&self, node_id: NodeId) -> Result<ConnectionState, ServiceError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .connection_commands
            .send(ConnectionCommand::Connect {
                node_id,
                response: response_tx,
            })
            .await
            .map_err(|_| ServiceError::ShuttingDown)?;
        response_rx.await.map_err(|_| ServiceError::ShuttingDown)?
    }

    /// Disconnects the active relay and stops its worker.
    pub async fn disconnect(&self) -> Result<ConnectionState, ServiceError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .connection_commands
            .send(ConnectionCommand::Disconnect {
                response: response_tx,
            })
            .await
            .map_err(|_| ServiceError::ShuttingDown)?;
        response_rx.await.map_err(|_| ServiceError::ShuttingDown)?
    }

    /// Adds an isolated quality test to the bounded queue.
    pub async fn enqueue_test(&self, node_id: NodeId) -> Result<OperationId, ServiceError> {
        let node = self
            .node(&node_id)
            .await
            .ok_or(ServiceError::NodeNotFound)?;
        if node.availability != NodeAvailability::Available || node.openvpn.is_none() {
            return Err(ServiceError::NodeUnavailable);
        }
        let operation_id = OperationId::new();
        let state = TestState::Queued {
            node_id: node_id.clone(),
            queued_at: Utc::now(),
        };
        self.0
            .operations
            .write()
            .await
            .insert(operation_id, state.clone());
        let job = TestJob {
            operation_id,
            node_id,
        };
        self.0.queued_tests.fetch_add(1, Ordering::Relaxed);
        if self.0.test_queue.try_send(job).is_err() {
            self.0.queued_tests.fetch_sub(1, Ordering::Relaxed);
            self.0.operations.write().await.remove(&operation_id);
            return Err(ServiceError::QueueFull);
        }
        self.emit(AppEvent::Test {
            operation_id,
            state,
        });
        Ok(operation_id)
    }

    /// Returns a test operation snapshot.
    pub async fn test_state(&self, operation_id: OperationId) -> Result<TestState, ServiceError> {
        self.0
            .operations
            .read()
            .await
            .get(&operation_id)
            .cloned()
            .ok_or(ServiceError::OperationNotFound)
    }

    /// Returns readiness, queue counts, refresh data, and helper reachability.
    pub async fn status(&self) -> StatusSnapshot {
        let upstream_state = if self.0.netd.ping().await.is_ok() {
            *self.0.upstream_state.borrow()
        } else {
            UpstreamState::NetdUnavailable
        };
        let last_refresh = self.0.last_refresh.read().await.clone();
        let connection = self.0.connection_state.borrow().clone();
        let proxy_ready = self.0.active_worker.borrow().is_some();
        StatusSnapshot {
            connection,
            proxy_ready,
            queued_tests: self.0.queued_tests.load(Ordering::Relaxed),
            running_tests: self.0.running_tests.load(Ordering::Relaxed),
            upstream_state,
            last_refresh,
            lan_mode: self.0.config.lan_mode,
            tls_configured: self.0.config.tls.is_some(),
        }
    }

    /// Verifies that the helper and persistent store can serve requests.
    pub async fn is_ready(&self) -> bool {
        let (netd, store) = tokio::join!(self.0.netd.ping(), self.0.store.health());
        netd.is_ok() && store.is_ok() && *self.0.upstream_state.borrow() == UpstreamState::Ready
    }

    /// Starts automatic refresh with bounded exponential retry after transient failures.
    pub fn start_refresh_loop(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut retry = Duration::from_secs(5);
            loop {
                let delay = match state.refresh_nodes().await {
                    Ok(_) => {
                        retry = Duration::from_secs(5);
                        state.0.config.refresh_interval
                    }
                    Err(ServiceError::RefreshBusy) => Duration::from_secs(1),
                    Err(error) => {
                        tracing::warn!(error = ?error, "VPN Gate refresh failed");
                        state.emit(AppEvent::RefreshFailed {
                            message: error.to_string(),
                            at: Utc::now(),
                        });
                        let current = retry;
                        retry = retry.saturating_mul(2).min(state.0.config.refresh_interval);
                        current
                    }
                };
                tokio::select! {
                    () = state.0.shutdown.cancelled() => break,
                    () = tokio::time::sleep(delay) => {}
                }
            }
        });
    }

    fn emit(&self, event: AppEvent) {
        let _receiver_count = self.0.events.send(event);
    }
}

async fn upstream_monitor(state: AppState) {
    loop {
        let next = match probe_upstream(&state.0.upstream, Duration::from_secs(3)).await {
            Ok(()) => UpstreamState::Ready,
            Err(UpstreamProbeError::Authentication) => UpstreamState::AuthenticationFailed,
            Err(
                UpstreamProbeError::Unreachable(_)
                | UpstreamProbeError::Timeout
                | UpstreamProbeError::Protocol,
            ) => UpstreamState::Unreachable,
        };
        if *state.0.upstream_state.borrow() != next {
            state.0.upstream_state.send_replace(next);
            state.emit(AppEvent::Upstream {
                state: next,
                at: Utc::now(),
            });
        }
        tokio::select! {
            () = state.0.shutdown.cancelled() => break,
            () = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

fn refresh_info(stats: ParseStats) -> RefreshInfo {
    RefreshInfo {
        at: Utc::now(),
        accepted: stats.accepted,
        rejected: stats.rejected,
        unsupported: stats.unsupported,
    }
}

async fn connection_actor(state: AppState, mut commands: mpsc::Receiver<ConnectionCommand>) {
    let mut active: Option<ActiveConnection> = None;
    let mut health_check = tokio::time::interval(Duration::from_secs(1));
    health_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = state.0.shutdown.cancelled() => break,
            _ = health_check.tick(), if active.is_some() => {
                let worker_id = active.as_ref().map(|current| current.worker_id);
                if let Some(worker_id) = worker_id {
                    if !state.0.netd.worker_ready(worker_id).await.unwrap_or(false) {
                        if let Some(failed) = active.take() {
                            state.0.active_worker.send_replace(None);
                            let connection = ConnectionState::Failed {
                                node_id: failed.node_id,
                                message: "VPN 隧道已断开；代理已闭锁".to_owned(),
                                at: Utc::now(),
                            };
                            state.0.connection_state.send_replace(connection.clone());
                            state.emit(AppEvent::Connection(connection));
                            if let Err(error) = state.0.netd.stop_worker(worker_id).await {
                                tracing::warn!(error = %error, worker.id = %worker_id, "failed to stop disconnected worker");
                            }
                        }
                    }
                }
            }
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    ConnectionCommand::Connect { node_id, response } => {
                        let result = switch_connection(&state, &mut active, node_id).await;
                        let _sent = response.send(result);
                    }
                    ConnectionCommand::Disconnect { response } => {
                        state.0.active_worker.send_replace(None);
                        let connection = ConnectionState::Disconnected;
                        state.0.connection_state.send_replace(connection.clone());
                        state.emit(AppEvent::Connection(connection.clone()));
                        if let Some(old) = active.take() {
                            if let Err(error) = state.0.netd.stop_worker(old.worker_id).await {
                                tracing::warn!(error = %error, worker.id = %old.worker_id, "failed to stop worker");
                            }
                        }
                        let _sent = response.send(Ok(connection));
                    }
                }
            }
        }
    }
    state.0.active_worker.send_replace(None);
    if let Some(old) = active {
        if let Err(error) = state.0.netd.stop_worker(old.worker_id).await {
            tracing::warn!(error = %error, worker.id = %old.worker_id, "failed to stop worker during shutdown");
        }
    }
}

async fn switch_connection(
    state: &AppState,
    active: &mut Option<ActiveConnection>,
    node_id: NodeId,
) -> Result<ConnectionState, ServiceError> {
    if let Some(current) = active.as_ref() {
        if current.node_id == node_id {
            return Ok(ConnectionState::Connected {
                node_id: current.node_id.clone(),
                worker_id: current.worker_id,
                since: current.connected_at,
            });
        }
    }
    let node = state
        .node(&node_id)
        .await
        .ok_or(ServiceError::NodeNotFound)?;
    if node.availability != NodeAvailability::Available {
        return Err(ServiceError::NodeUnavailable);
    }
    let profile = node.openvpn.ok_or(ServiceError::NodeUnavailable)?;
    let worker_id = WorkerId::new();
    let connecting = ConnectionState::Connecting {
        node_id: node_id.clone(),
        worker_id,
        since: Utc::now(),
    };
    state.0.connection_state.send_replace(connecting.clone());
    state.emit(AppEvent::Connection(connecting));
    let socket = match state
        .0
        .netd
        .start_worker(
            worker_id,
            node_id.clone(),
            profile.remote(),
            profile.as_str().to_owned(),
            state.0.config.connect_timeout,
        )
        .await
    {
        Ok(socket) => socket,
        Err(error) => {
            let fallback = active.as_ref().map_or_else(
                || ConnectionState::Failed {
                    node_id: node_id.clone(),
                    message: error.to_string(),
                    at: Utc::now(),
                },
                |current| ConnectionState::Connected {
                    node_id: current.node_id.clone(),
                    worker_id: current.worker_id,
                    since: current.connected_at,
                },
            );
            state.0.connection_state.send_replace(fallback.clone());
            state.emit(AppEvent::Connection(fallback));
            return Err(ServiceError::Worker(error));
        }
    };

    state.0.active_worker.send_replace(Some(socket.clone()));
    let connected_at = Utc::now();
    let connection = ConnectionState::Connected {
        node_id: node_id.clone(),
        worker_id,
        since: connected_at,
    };
    state.0.connection_state.send_replace(connection.clone());
    state.emit(AppEvent::Connection(connection.clone()));
    let replacement = ActiveConnection {
        worker_id,
        node_id,
        connected_at,
    };
    if let Some(old) = active.replace(replacement) {
        let netd = state.0.netd.clone();
        tokio::spawn(async move {
            tokio::time::sleep(OLD_WORKER_DRAIN).await;
            if let Err(error) = netd.stop_worker(old.worker_id).await {
                tracing::warn!(error = %error, worker.id = %old.worker_id, "failed to stop drained worker");
            }
        });
    }
    Ok(connection)
}

async fn test_dispatcher(state: AppState, mut jobs: mpsc::Receiver<TestJob>) {
    let semaphore = Arc::new(Semaphore::new(state.0.config.max_parallel_tests));
    loop {
        tokio::select! {
            () = state.0.shutdown.cancelled() => break,
            job = jobs.recv() => {
                let Some(job) = job else { break };
                let permit = tokio::select! {
                    () = state.0.shutdown.cancelled() => break,
                    permit = Arc::clone(&semaphore).acquire_owned() => permit,
                };
                let Ok(permit) = permit else { break };
                state.0.queued_tests.fetch_sub(1, Ordering::Relaxed);
                state.0.running_tests.fetch_add(1, Ordering::Relaxed);
                let state = state.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    run_test(&state, job).await;
                    state.0.running_tests.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }
}

async fn run_test(state: &AppState, job: TestJob) {
    let started_at = Utc::now();
    let running = TestState::Running {
        node_id: job.node_id.clone(),
        started_at,
    };
    state
        .0
        .operations
        .write()
        .await
        .insert(job.operation_id, running.clone());
    state.emit(AppEvent::Test {
        operation_id: job.operation_id,
        state: running,
    });
    let start = tokio::time::Instant::now();
    let worker_id = WorkerId::new();
    let result = async {
        let node = state
            .node(&job.node_id)
            .await
            .ok_or(ServiceError::NodeNotFound)?;
        let profile = node.openvpn.ok_or(ServiceError::NodeUnavailable)?;
        let socket = state
            .0
            .netd
            .start_worker(
                worker_id,
                job.node_id.clone(),
                profile.remote(),
                profile.as_str().to_owned(),
                state.0.config.connect_timeout,
            )
            .await?;
        let result = fetch_ippure(
            &state.0.config.ippure_url,
            socket,
            state.0.config.ippure_timeout,
        )
        .await
        .map_err(|error| error.to_string());
        if let Err(error) = state.0.netd.stop_worker(worker_id).await {
            tracing::warn!(error = %error, worker.id = %worker_id, "failed to stop test worker");
        }
        result.map_err(ServiceTestError::Quality)
    }
    .await;
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (test_state, record) = match result {
        Ok(result) => {
            let record = TestRecord {
                node_id: job.node_id.clone(),
                result: Some(result),
                duration_ms,
                tested_at: Utc::now(),
                error: None,
            };
            (
                TestState::Succeeded {
                    node_id: job.node_id.clone(),
                    record: record.clone(),
                },
                record,
            )
        }
        Err(error) => {
            let record = TestRecord {
                node_id: job.node_id.clone(),
                result: None,
                duration_ms,
                tested_at: Utc::now(),
                error: Some(error.to_string()),
            };
            (
                TestState::Failed {
                    node_id: job.node_id.clone(),
                    record: record.clone(),
                },
                record,
            )
        }
    };
    if let Err(error) = state.0.store.save_test(&record).await {
        tracing::warn!(error = %error, node.id = %job.node_id, "failed to persist test result");
    }
    let mut operations = state.0.operations.write().await;
    operations.insert(job.operation_id, test_state.clone());
    prune_completed_operations(&mut operations);
    drop(operations);
    state.emit(AppEvent::Test {
        operation_id: job.operation_id,
        state: test_state,
    });
}

fn prune_completed_operations(operations: &mut HashMap<OperationId, TestState>) {
    let mut completed = operations
        .iter()
        .filter_map(|(operation_id, state)| match state {
            TestState::Succeeded { record, .. } | TestState::Failed { record, .. } => {
                Some((*operation_id, record.tested_at))
            }
            TestState::Queued { .. } | TestState::Running { .. } => None,
        })
        .collect::<Vec<_>>();
    let excess = completed.len().saturating_sub(COMPLETED_OPERATION_HISTORY);
    if excess == 0 {
        return;
    }
    completed.sort_unstable_by(|left, right| left.1.cmp(&right.1));
    for (operation_id, _) in completed.into_iter().take(excess) {
        operations.remove(&operation_id);
    }
}

#[derive(Debug, Error)]
enum ServiceTestError {
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error("IPPure check failed: {0}")]
    Quality(String),
}

impl From<NetdClientError> for ServiceTestError {
    fn from(error: NetdClientError) -> Self {
        Self::Service(ServiceError::Worker(error))
    }
}
