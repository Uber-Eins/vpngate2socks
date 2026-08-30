//! Application orchestration: refreshes, make-before-break switching, and test isolation.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    future::pending,
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
use tokio::sync::{Mutex, RwLock, Semaphore, broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    auto_connect::select_node as select_auto_connect_node,
    automatic_tests::{AutomaticTestCandidate, select_automatic_tests},
    config::AppConfig,
    domain::{
        AppEvent, AutoConnectConfig, ConnectionState, NodeAvailability, NodeId, NodeSummary,
        OperationId, ResolvedUpstreamEndpoint, TestRecord, TestState, UpstreamState, VpnNode,
        WorkerId,
    },
    netd::{NetdClient, NetdClientError},
    quality::fetch_ippure,
    socks::{UpstreamProbeError, probe_upstream},
    storage::{Store, StoreError},
    test_registry::{QueueRegistration, TestRegistry},
    vpngate::{CsvLimits, ParseStats, VpnGateError, fetch_snapshot},
};

const TEST_QUEUE_CAPACITY: usize = 256;
const COMPLETED_OPERATION_HISTORY: usize = 1_024;
const OLD_WORKER_DRAIN: Duration = Duration::from_secs(30);
const AUTO_RECONNECT_INITIAL: Duration = Duration::from_secs(1);
const AUTO_RECONNECT_MAX: Duration = Duration::from_secs(30);
const AUTO_NODE_COOLDOWN: Duration = Duration::from_secs(60);

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
    auto_connect: watch::Sender<AutoConnectConfig>,
    auto_connect_update: Mutex<()>,
    auto_connect_trigger: mpsc::Sender<()>,
    test_registry: RwLock<TestRegistry>,
    test_queue: mpsc::Sender<TestJob>,
    automatic_test_trigger: mpsc::Sender<()>,
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
    /// Full public view of the node named by `connection`, so the UI can show the
    /// active exit without it happening to be on the currently browsed node page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_node: Option<NodeSummary>,
    pub proxy_ready: bool,
    pub queued_tests: usize,
    pub running_tests: usize,
    pub upstream_state: UpstreamState,
    pub last_refresh: Option<RefreshInfo>,
    pub lan_mode: bool,
    pub tls_configured: bool,
}

/// Region offered by the automatic connection configuration UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionOption {
    pub code: String,
    pub name: String,
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
    #[error("invalid automatic connection configuration: {0}")]
    InvalidAutoConnectConfig(&'static str),
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
        auto_connect_config: AutoConnectConfig,
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
        let (auto_connect, _) = watch::channel(auto_connect_config);
        let (auto_connect_trigger, auto_connect_rx) = mpsc::channel(1);
        let (test_queue, test_rx) = mpsc::channel(TEST_QUEUE_CAPACITY);
        let (automatic_test_trigger, automatic_test_rx) = mpsc::channel(1);
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
            auto_connect,
            auto_connect_update: Mutex::new(()),
            auto_connect_trigger,
            test_registry: RwLock::new(TestRegistry::default()),
            test_queue,
            automatic_test_trigger,
            queued_tests: AtomicUsize::new(0),
            running_tests: AtomicUsize::new(0),
            upstream_state,
            events,
            shutdown,
        }));
        tokio::spawn(connection_actor(
            state.clone(),
            connection_rx,
            auto_connect_rx,
        ));
        tokio::spawn(test_dispatcher(state.clone(), test_rx));
        tokio::spawn(automatic_test_scheduler(state.clone(), automatic_test_rx));
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

    /// Returns the currently active automatic connection policy.
    #[must_use]
    pub fn auto_connect_config(&self) -> AutoConnectConfig {
        self.0.auto_connect.borrow().clone()
    }

    /// Returns the distinct usable VPN Gate regions in code order.
    pub async fn auto_connect_regions(&self) -> Vec<RegionOption> {
        self.0
            .nodes
            .read()
            .await
            .iter()
            .filter(|node| node.availability == NodeAvailability::Available)
            .filter_map(|node| {
                let code = node.country_short.trim().to_uppercase();
                (!code.is_empty()).then(|| (code, node.country_long.clone()))
            })
            .fold(
                BTreeMap::<String, String>::new(),
                |mut regions, (code, name)| {
                    regions.entry(code).or_insert(name);
                    regions
                },
            )
            .into_iter()
            .map(|(code, name)| RegionOption { code, name })
            .collect()
    }

    /// Validates, persists, and activates a new automatic connection policy.
    pub async fn set_auto_connect_config(
        &self,
        config: AutoConnectConfig,
    ) -> Result<AutoConnectConfig, ServiceError> {
        let config = config
            .normalized()
            .map_err(ServiceError::InvalidAutoConnectConfig)?;
        let _update = self.0.auto_connect_update.lock().await;
        self.0.store.save_auto_connect_config(&config).await?;
        self.0.auto_connect.send_replace(config.clone());
        self.emit(AppEvent::AutoConnection(config.clone()));
        self.trigger_auto_connect();
        Ok(config)
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
        self.0
            .test_registry
            .write()
            .await
            .retain_observed(&current_ids);

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
        self.trigger_automatic_tests();
        self.trigger_auto_connect();
        Ok(info)
    }

    /// Requests a make-before-break switch and waits for its result.
    pub async fn connect(&self, node_id: NodeId) -> Result<ConnectionState, ServiceError> {
        let node = self
            .node(&node_id)
            .await
            .ok_or(ServiceError::NodeNotFound)?;
        if node.availability != NodeAvailability::Available || node.openvpn.is_none() {
            return Err(ServiceError::NodeUnavailable);
        }
        self.disable_auto_connect().await;
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
        self.disable_auto_connect().await;
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

    async fn disable_auto_connect(&self) {
        let _update = self.0.auto_connect_update.lock().await;
        let mut config = self.auto_connect_config();
        if !config.enabled {
            return;
        }
        config.enabled = false;
        self.0.auto_connect.send_replace(config.clone());
        self.emit(AppEvent::AutoConnection(config.clone()));
        if let Err(error) = self.0.store.save_auto_connect_config(&config).await {
            tracing::warn!(
                error = %error,
                "failed to persist disabled automatic connection policy"
            );
        }
    }

    /// Adds an isolated quality test to the bounded queue.
    pub async fn enqueue_test(&self, node_id: NodeId) -> Result<OperationId, ServiceError> {
        self.enqueue_test_job(node_id)
            .await
            .map(|(operation_id, _)| operation_id)
    }

    async fn enqueue_test_job(&self, node_id: NodeId) -> Result<(OperationId, bool), ServiceError> {
        let node = self
            .node(&node_id)
            .await
            .ok_or(ServiceError::NodeNotFound)?;
        if node.availability != NodeAvailability::Available || node.openvpn.is_none() {
            return Err(ServiceError::NodeUnavailable);
        }
        let mut registry = self.0.test_registry.write().await;
        let (operation_id, state) = match registry.queue(node_id.clone(), Utc::now()) {
            QueueRegistration::Existing(operation_id) => return Ok((operation_id, false)),
            QueueRegistration::New {
                operation_id,
                state,
            } => (operation_id, state),
        };
        self.0.queued_tests.fetch_add(1, Ordering::Relaxed);
        if self
            .0
            .test_queue
            .try_send(TestJob {
                operation_id,
                node_id: node_id.clone(),
            })
            .is_err()
        {
            self.0.queued_tests.fetch_sub(1, Ordering::Relaxed);
            registry.rollback_queued(&node_id, operation_id);
            return Err(ServiceError::QueueFull);
        }
        drop(registry);
        self.emit(AppEvent::Test {
            operation_id,
            state,
        });
        Ok((operation_id, true))
    }

    /// Returns a test operation snapshot.
    pub async fn test_state(&self, operation_id: OperationId) -> Result<TestState, ServiceError> {
        self.0
            .test_registry
            .read()
            .await
            .state(operation_id)
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
        let active_node = self.active_node(&connection).await;
        StatusSnapshot {
            connection,
            active_node,
            proxy_ready,
            queued_tests: self.0.queued_tests.load(Ordering::Relaxed),
            running_tests: self.0.running_tests.load(Ordering::Relaxed),
            upstream_state,
            last_refresh,
            lan_mode: self.0.config.lan_mode,
            tls_configured: self.0.config.tls.is_some(),
        }
    }

    /// Resolves the node a connection state points at into its public summary.
    ///
    /// A node can disappear from the snapshot between a refresh and this call, so a
    /// missing entry is reported as no active node rather than as an error.
    async fn active_node(&self, connection: &ConnectionState) -> Option<NodeSummary> {
        let node_id = connection.node_id()?;
        let node = self
            .0
            .nodes
            .read()
            .await
            .iter()
            .find(|node| &node.id == node_id)
            .cloned()?;
        let latest_test = self.0.store.latest_test(node_id).await.unwrap_or_default();
        Some(node.summary(latest_test))
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

    fn trigger_automatic_tests(&self) {
        let _triggered = self.0.automatic_test_trigger.try_send(());
    }

    fn trigger_auto_connect(&self) {
        let _triggered = self.0.auto_connect_trigger.try_send(());
    }

    async fn schedule_automatic_tests(&self) -> Result<usize, ServiceError> {
        let tested = self
            .0
            .store
            .latest_tests()
            .await?
            .into_keys()
            .collect::<HashSet<_>>();
        let nodes = self.nodes().await;
        let known = self.0.test_registry.read().await.known_nodes();
        let available_capacity = self.0.test_queue.capacity();
        let candidates = select_automatic_tests(
            nodes.iter().map(AutomaticTestCandidate::from),
            &tested,
            &known,
            available_capacity,
        );
        let mut scheduled = 0;
        for node_id in candidates {
            match self.enqueue_test_job(node_id).await {
                Ok((_, true)) => scheduled += 1,
                Ok((_, false))
                | Err(ServiceError::NodeNotFound | ServiceError::NodeUnavailable) => {}
                Err(ServiceError::QueueFull) => break,
                Err(error) => return Err(error),
            }
        }
        Ok(scheduled)
    }
}

async fn automatic_test_scheduler(state: AppState, mut triggers: mpsc::Receiver<()>) {
    loop {
        tokio::select! {
            () = state.0.shutdown.cancelled() => break,
            trigger = triggers.recv() => {
                if trigger.is_none() {
                    break;
                }
                match state.schedule_automatic_tests().await {
                    Ok(scheduled) if scheduled > 0 => {
                        tracing::info!(scheduled, "scheduled automatic IPPure tests");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(error = ?error, "failed to schedule automatic IPPure tests");
                    }
                }
            }
        }
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

async fn connection_actor(
    state: AppState,
    mut commands: mpsc::Receiver<ConnectionCommand>,
    mut auto_connect_triggers: mpsc::Receiver<()>,
) {
    let mut active: Option<ActiveConnection> = None;
    let mut auto_connect_config = state.0.auto_connect.subscribe();
    let mut auto_failures = HashMap::<NodeId, tokio::time::Instant>::new();
    let mut auto_retry_at = None;
    let mut auto_retry_delay = AUTO_RECONNECT_INITIAL;
    let mut health_check = tokio::time::interval(Duration::from_secs(1));
    health_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let scheduled_retry = auto_retry_at;
        let retry = async move {
            match scheduled_retry {
                Some(at) => tokio::time::sleep_until(at).await,
                None => pending::<()>().await,
            }
        };
        tokio::pin!(retry);
        tokio::select! {
            () = state.0.shutdown.cancelled() => break,
            () = &mut retry => {
                auto_retry_at = attempt_auto_connect(
                    &state,
                    &mut active,
                    &mut auto_failures,
                    &mut auto_retry_delay,
                ).await;
            }
            changed = auto_connect_config.changed() => {
                if changed.is_err() {
                    break;
                }
                auto_failures.clear();
                auto_retry_delay = AUTO_RECONNECT_INITIAL;
                auto_retry_at = attempt_auto_connect(
                    &state,
                    &mut active,
                    &mut auto_failures,
                    &mut auto_retry_delay,
                ).await;
            }
            trigger = auto_connect_triggers.recv() => {
                if trigger.is_none() {
                    break;
                }
                auto_retry_at = attempt_auto_connect(
                    &state,
                    &mut active,
                    &mut auto_failures,
                    &mut auto_retry_delay,
                ).await;
            }
            _ = health_check.tick(), if active.is_some() => {
                let worker_id = active.as_ref().map(|current| current.worker_id);
                if let Some(worker_id) = worker_id {
                    if !state.0.netd.worker_ready(worker_id).await.unwrap_or(false) {
                        if let Some(failed) = active.take() {
                            auto_failures.insert(
                                failed.node_id.clone(),
                                tokio::time::Instant::now() + AUTO_NODE_COOLDOWN,
                            );
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
                            if state.0.auto_connect.borrow().enabled {
                                auto_retry_at = Some(
                                    tokio::time::Instant::now() + auto_retry_delay,
                                );
                                auto_retry_delay = auto_retry_delay
                                    .saturating_mul(2)
                                    .min(AUTO_RECONNECT_MAX);
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
                        auto_retry_at = None;
                        auto_failures.clear();
                        auto_retry_delay = AUTO_RECONNECT_INITIAL;
                        let _sent = response.send(result);
                    }
                    ConnectionCommand::Disconnect { response } => {
                        auto_retry_at = None;
                        auto_failures.clear();
                        auto_retry_delay = AUTO_RECONNECT_INITIAL;
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

async fn attempt_auto_connect(
    state: &AppState,
    active: &mut Option<ActiveConnection>,
    failures: &mut HashMap<NodeId, tokio::time::Instant>,
    retry_delay: &mut Duration,
) -> Option<tokio::time::Instant> {
    let config = state.auto_connect_config();
    if !config.enabled {
        failures.clear();
        *retry_delay = AUTO_RECONNECT_INITIAL;
        return None;
    }

    let now = tokio::time::Instant::now();
    failures.retain(|_, retry_at| *retry_at > now);
    let tests = match state.latest_tests().await {
        Ok(tests) => tests,
        Err(error) => {
            tracing::warn!(error = %error, "failed to load IPPure results for automatic connection");
            let retry_at = now + *retry_delay;
            *retry_delay = retry_delay.saturating_mul(2).min(AUTO_RECONNECT_MAX);
            return Some(retry_at);
        }
    };
    let nodes = state.nodes().await;
    let excluded = failures.keys().cloned().collect::<HashSet<_>>();
    let Some(node_id) = select_auto_connect_node(&nodes, &tests, &config, &excluded) else {
        let has_cooled_down_candidate =
            select_auto_connect_node(&nodes, &tests, &config, &HashSet::new()).is_some();
        return has_cooled_down_candidate
            .then(|| failures.values().copied().min())
            .flatten();
    };

    if active
        .as_ref()
        .is_some_and(|connection| connection.node_id == node_id)
    {
        *retry_delay = AUTO_RECONNECT_INITIAL;
        return failures.values().copied().min();
    }

    match switch_connection(state, active, node_id.clone()).await {
        Ok(_) => {
            *retry_delay = AUTO_RECONNECT_INITIAL;
            tracing::info!(node.id = %node_id, "automatic connection selected node");
            failures.values().copied().min()
        }
        Err(error) => {
            let failed_at = tokio::time::Instant::now();
            failures.insert(node_id.clone(), failed_at + AUTO_NODE_COOLDOWN);
            let retry_at = failed_at + *retry_delay;
            *retry_delay = retry_delay.saturating_mul(2).min(AUTO_RECONNECT_MAX);
            tracing::warn!(
                error = %error,
                node.id = %node_id,
                "automatic connection attempt failed"
            );
            Some(retry_at)
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
    let running = state.0.test_registry.write().await.mark_running(
        job.operation_id,
        job.node_id.clone(),
        started_at,
    );
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
    state.0.test_registry.write().await.complete(
        job.operation_id,
        job.node_id,
        test_state.clone(),
        COMPLETED_OPERATION_HISTORY,
    );
    state.emit(AppEvent::Test {
        operation_id: job.operation_id,
        state: test_state,
    });
    state.trigger_auto_connect();
    state.trigger_automatic_tests();
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
