//! Linux network namespace and `OpenVPN` lifecycle implementation.

use std::{
    collections::{HashMap, HashSet},
    io,
    net::{Ipv4Addr, SocketAddrV4},
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    net::{UnixListener, UnixStream},
    process::{Child, Command},
    sync::{RwLock, Semaphore},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    config::AppConfig,
    domain::{NodeId, UpstreamEndpoint, WorkerId, is_public_ipv4},
    openvpn::sanitize_openvpn,
};

use super::protocol::{NetdRequest, NetdResponse, read_frame, write_frame};

const MAX_DRAINING_WORKERS: usize = 8;
const MAX_MANAGED_WORKERS: usize = 64;
const CHILD_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Fatal helper setup or listener error.
#[derive(Debug, Error)]
pub enum NetdServerError {
    #[error("netd is supported only on Linux")]
    UnsupportedPlatform,
    #[error("failed to prepare netd runtime directory")]
    Runtime(#[source] io::Error),
    #[error("failed to bind netd Unix socket")]
    Bind(#[source] io::Error),
    #[error("netd listener failed")]
    Accept(#[source] io::Error),
    #[error("failed to install the root namespace leak guard: {0}")]
    Guard(String),
}

#[derive(Debug, Error)]
enum WorkerError {
    #[error("worker id already exists")]
    Duplicate,
    #[error("worker id was not found")]
    NotFound,
    #[error("worker capacity is exhausted")]
    Capacity,
    #[error("control plane supplied an invalid sanitized profile")]
    InvalidProfile,
    #[error("worker remote endpoint does not match its profile")]
    RemoteMismatch,
    #[error("failed to prepare worker files")]
    File(#[source] io::Error),
    #[error("network command failed: {0}")]
    Network(String),
    #[error("OpenVPN process could not be started")]
    OpenVpnSpawn(#[source] io::Error),
    #[error("OpenVPN exited before becoming ready")]
    OpenVpnExited,
    #[error("OpenVPN management interface failed")]
    Management(#[source] io::Error),
    #[error("OpenVPN connection timed out")]
    ConnectTimeout,
    #[error("worker SOCKS process could not be started")]
    WorkerSpawn(#[source] io::Error),
    #[error("worker SOCKS socket did not become ready")]
    WorkerNotReady,
    #[error("netd is shutting down")]
    ShuttingDown,
}

impl WorkerError {
    fn code(&self) -> &'static str {
        match self {
            Self::Duplicate => "duplicateWorker",
            Self::NotFound => "workerNotFound",
            Self::Capacity => "capacityExhausted",
            Self::InvalidProfile | Self::RemoteMismatch => "invalidProfile",
            Self::ConnectTimeout => "connectTimeout",
            Self::OpenVpnExited => "openvpnExited",
            Self::WorkerNotReady => "workerNotReady",
            Self::ShuttingDown => "shuttingDown",
            Self::File(_)
            | Self::Network(_)
            | Self::OpenVpnSpawn(_)
            | Self::Management(_)
            | Self::WorkerSpawn(_) => "workerSetupFailed",
        }
    }
}

struct WorkerProcess {
    network: NetworkAllocation,
    directory: PathBuf,
    socket_path: PathBuf,
    openvpn: Child,
    socks: Child,
    node_id: NodeId,
    ready: Arc<AtomicBool>,
    management_task: tokio::task::JoinHandle<()>,
}

struct WorkerManager {
    runtime_dir: PathBuf,
    upstream: UpstreamEndpoint,
    workers: RwLock<HashMap<WorkerId, WorkerProcess>>,
    starting: StdMutex<HashSet<WorkerId>>,
    free_networks: Arc<StdMutex<Vec<u16>>>,
    next_network: AtomicU16,
    unprivileged_uid: u32,
    unprivileged_gid: u32,
    openvpn_uid: u32,
    max_workers: usize,
    shutdown: CancellationToken,
}

struct WorkerReservation<'a> {
    starting: &'a StdMutex<HashSet<WorkerId>>,
    worker_id: WorkerId,
}

impl Drop for WorkerReservation<'_> {
    fn drop(&mut self) {
        let mut starting = self
            .starting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        starting.remove(&self.worker_id);
    }
}

impl WorkerManager {
    fn new(config: &AppConfig, shutdown: CancellationToken) -> Self {
        Self {
            runtime_dir: config.runtime_dir.join("workers"),
            upstream: config.upstream.clone(),
            workers: RwLock::new(HashMap::new()),
            starting: StdMutex::new(HashSet::new()),
            free_networks: Arc::new(StdMutex::new(Vec::new())),
            next_network: AtomicU16::new(1),
            unprivileged_uid: config.unprivileged_uid,
            unprivileged_gid: config.unprivileged_gid,
            openvpn_uid: config.openvpn_uid,
            max_workers: config
                .max_parallel_tests
                .saturating_add(MAX_DRAINING_WORKERS)
                .min(MAX_MANAGED_WORKERS),
            shutdown,
        }
    }

    async fn start(
        &self,
        worker_id: WorkerId,
        node_id: NodeId,
        expected_remote: SocketAddrV4,
        profile_text: String,
        connect_timeout: Duration,
    ) -> Result<PathBuf, WorkerError> {
        self.ensure_running()?;
        let _reservation = self.reserve(worker_id).await?;
        if !is_public_ipv4(*expected_remote.ip()) {
            return Err(WorkerError::RemoteMismatch);
        }
        let profile_text = Zeroizing::new(profile_text);
        let profile = sanitize_openvpn(profile_text.as_bytes(), *expected_remote.ip())
            .map_err(|_| WorkerError::InvalidProfile)?;
        if profile.remote() != expected_remote {
            return Err(WorkerError::RemoteMismatch);
        }
        let allocation = self.allocate_network()?;
        let directory = self.runtime_dir.join(worker_id.to_string());
        match tokio::fs::create_dir(&directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(WorkerError::Duplicate);
            }
            Err(error) => return Err(WorkerError::File(error)),
        }
        let file_setup = async {
            std::os::unix::fs::chown(&directory, Some(0), Some(self.unprivileged_gid))
                .map_err(WorkerError::File)?;
            tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o710))
                .await
                .map_err(WorkerError::File)?;
            let openvpn_directory = directory.join("openvpn");
            tokio::fs::create_dir(&openvpn_directory)
                .await
                .map_err(WorkerError::File)?;
            std::os::unix::fs::chown(
                &openvpn_directory,
                Some(self.openvpn_uid),
                Some(self.unprivileged_gid),
            )
            .map_err(WorkerError::File)?;
            tokio::fs::set_permissions(&openvpn_directory, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(WorkerError::File)?;
            let ipc_directory = directory.join("ipc");
            tokio::fs::create_dir(&ipc_directory)
                .await
                .map_err(WorkerError::File)?;
            std::os::unix::fs::chown(
                &ipc_directory,
                Some(self.unprivileged_uid),
                Some(self.unprivileged_gid),
            )
            .map_err(WorkerError::File)?;
            tokio::fs::set_permissions(&ipc_directory, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(WorkerError::File)?;
            let socket_path = ipc_directory.join("socks.sock");
            let management_path = openvpn_directory.join("management.sock");
            let profile_path = openvpn_directory.join("openvpn.conf");
            let auth_path = self
                .upstream
                .username()
                .map(|_| openvpn_directory.join("upstream.auth"));

            if let Some(auth_path) = &auth_path {
                let credentials = Zeroizing::new(format!(
                    "{}\n{}\n",
                    self.upstream
                        .username()
                        .ok_or(WorkerError::InvalidProfile)?,
                    self.upstream
                        .password()
                        .ok_or(WorkerError::InvalidProfile)?
                ));
                write_private(
                    auth_path,
                    credentials.as_bytes(),
                    self.openvpn_uid,
                    self.unprivileged_gid,
                )
                .await?;
            }
            let generated_profile = Zeroizing::new(build_runtime_profile(
                profile.as_str(),
                &self.upstream,
                auth_path.as_deref(),
                &management_path,
            ));
            write_private(
                &profile_path,
                generated_profile.as_bytes(),
                self.openvpn_uid,
                self.unprivileged_gid,
            )
            .await?;
            Ok::<_, WorkerError>((socket_path, management_path, profile_path))
        }
        .await;
        let (socket_path, management_path, profile_path) = match file_setup {
            Ok(paths) => paths,
            Err(error) => {
                cleanup_directory(&directory).await;
                return Err(error);
            }
        };

        if let Err(error) = self.ensure_running() {
            cleanup_directory(&directory).await;
            return Err(error);
        }

        if let Err(error) = prepare_worker_network(&allocation, &self.upstream).await {
            cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
            cleanup_directory(&directory).await;
            return Err(error);
        }
        if let Err(error) = self.ensure_running() {
            cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
            cleanup_directory(&directory).await;
            return Err(error);
        }
        let mut openvpn = match spawn_openvpn(
            &allocation.namespace,
            &profile_path,
            self.openvpn_uid,
            self.unprivileged_gid,
        ) {
            Ok(child) => child,
            Err(error) => {
                cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
                cleanup_directory(&directory).await;
                return Err(error);
            }
        };
        let management = match wait_for_openvpn(
            &management_path,
            &mut openvpn,
            connect_timeout,
            &self.shutdown,
        )
        .await
        {
            Ok(management) => management,
            Err(error) => {
                stop_child(&mut openvpn).await;
                cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
                cleanup_directory(&directory).await;
                return Err(error);
            }
        };
        if !wait_for_tunnel_routes(
            &allocation.namespace,
            &mut openvpn,
            Duration::from_secs(5),
            &self.shutdown,
        )
        .await
        {
            stop_child(&mut openvpn).await;
            cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
            cleanup_directory(&directory).await;
            return Err(WorkerError::WorkerNotReady);
        }
        let ready = Arc::new(AtomicBool::new(true));
        let management_task = tokio::spawn(monitor_management(management, Arc::clone(&ready)));
        let mut socks = match spawn_worker_socks(
            &allocation.namespace,
            &socket_path,
            self.unprivileged_uid,
            self.unprivileged_gid,
        ) {
            Ok(child) => child,
            Err(error) => {
                management_task.abort();
                stop_child(&mut openvpn).await;
                cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
                cleanup_directory(&directory).await;
                return Err(error);
            }
        };
        if !wait_for_socket(
            &socket_path,
            &mut socks,
            Duration::from_secs(5),
            &self.shutdown,
        )
        .await
        {
            management_task.abort();
            stop_child(&mut socks).await;
            stop_child(&mut openvpn).await;
            cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
            cleanup_directory(&directory).await;
            return Err(WorkerError::WorkerNotReady);
        }
        if let Err(error) = self.ensure_running() {
            management_task.abort();
            stop_child(&mut socks).await;
            stop_child(&mut openvpn).await;
            cleanup_worker_network(&allocation.namespace, &allocation.host_veth).await;
            cleanup_directory(&directory).await;
            return Err(error);
        }

        let process = WorkerProcess {
            network: allocation,
            directory,
            socket_path: socket_path.clone(),
            openvpn,
            socks,
            node_id,
            ready,
            management_task,
        };
        let mut workers = self.workers.write().await;
        if workers.contains_key(&worker_id) {
            drop(workers);
            stop_process(process).await;
            return Err(WorkerError::Duplicate);
        }
        workers.insert(worker_id, process);
        Ok(socket_path)
    }

    async fn reserve(&self, worker_id: WorkerId) -> Result<WorkerReservation<'_>, WorkerError> {
        {
            let mut starting = self
                .starting
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !starting.insert(worker_id) {
                return Err(WorkerError::Duplicate);
            }
        }
        let reservation = WorkerReservation {
            starting: &self.starting,
            worker_id,
        };
        let workers = self.workers.read().await;
        if workers.contains_key(&worker_id) {
            return Err(WorkerError::Duplicate);
        }
        let running = workers.len();
        drop(workers);
        let starting = self
            .starting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        if running.saturating_add(starting) > self.max_workers {
            return Err(WorkerError::Capacity);
        }
        Ok(reservation)
    }

    fn ensure_running(&self) -> Result<(), WorkerError> {
        if self.shutdown.is_cancelled() {
            Err(WorkerError::ShuttingDown)
        } else {
            Ok(())
        }
    }

    async fn stop(&self, worker_id: WorkerId) -> Result<(), WorkerError> {
        let process = self
            .workers
            .write()
            .await
            .remove(&worker_id)
            .ok_or(WorkerError::NotFound)?;
        stop_process(process).await;
        Ok(())
    }

    async fn is_ready(&self, worker_id: WorkerId) -> bool {
        let worker = {
            self.workers.read().await.get(&worker_id).map(|process| {
                (
                    process.network.namespace.clone(),
                    process.socket_path.clone(),
                    Arc::clone(&process.ready),
                )
            })
        };
        let Some((namespace, socket_path, ready)) = worker else {
            return false;
        };
        if !ready.load(Ordering::Acquire) {
            return false;
        }
        if !command_succeeds_in_namespace(&namespace, "ip", &["link", "show", "up", "dev", "tun0"])
            .await
            .unwrap_or(false)
        {
            return false;
        }
        if !namespace_routes_cover_public_ipv4(&namespace)
            .await
            .unwrap_or(false)
        {
            return false;
        }
        tokio::time::timeout(Duration::from_millis(500), UnixStream::connect(socket_path))
            .await
            .is_ok_and(|result| result.is_ok())
    }

    async fn stop_all(&self) {
        let processes = {
            let mut workers = self.workers.write().await;
            workers
                .drain()
                .map(|(_, process)| process)
                .collect::<Vec<_>>()
        };
        for process in processes {
            stop_process(process).await;
        }
    }

    fn allocate_network(&self) -> Result<NetworkAllocation, WorkerError> {
        let index = self
            .free_networks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop()
            .unwrap_or_else(|| self.next_network.fetch_add(1, Ordering::Relaxed));
        if index == 0 || index > 16_383 {
            return Err(WorkerError::Capacity);
        }
        let subnet = u32::from(index);
        let third = u8::try_from(subnet / 64).map_err(|_| WorkerError::Capacity)?;
        let base = u8::try_from((subnet % 64) * 4).map_err(|_| WorkerError::Capacity)?;
        let short = format!("{index:04x}");
        Ok(NetworkAllocation {
            _lease: NetworkLease {
                index,
                free_networks: Arc::clone(&self.free_networks),
            },
            namespace: format!("v2s-{short}"),
            host_veth: format!("v2h{short}"),
            worker_veth: format!("v2w{short}"),
            host_ip: Ipv4Addr::new(10, 231, third, base.saturating_add(1)),
            worker_ip: Ipv4Addr::new(10, 231, third, base.saturating_add(2)),
        })
    }
}

struct NetworkAllocation {
    _lease: NetworkLease,
    namespace: String,
    host_veth: String,
    worker_veth: String,
    host_ip: Ipv4Addr,
    worker_ip: Ipv4Addr,
}

struct NetworkLease {
    index: u16,
    free_networks: Arc<StdMutex<Vec<u16>>>,
}

impl Drop for NetworkLease {
    fn drop(&mut self) {
        self.free_networks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(self.index);
    }
}

/// Runs the privileged helper until shutdown, cleaning every owned worker.
pub async fn run_netd(
    config: AppConfig,
    shutdown: CancellationToken,
) -> Result<(), NetdServerError> {
    if !cfg!(target_os = "linux") {
        return Err(NetdServerError::UnsupportedPlatform);
    }
    tokio::fs::create_dir_all(&config.runtime_dir)
        .await
        .map_err(NetdServerError::Runtime)?;
    std::os::unix::fs::chown(&config.runtime_dir, Some(0), Some(config.unprivileged_gid))
        .map_err(NetdServerError::Runtime)?;
    tokio::fs::set_permissions(&config.runtime_dir, std::fs::Permissions::from_mode(0o750))
        .await
        .map_err(NetdServerError::Runtime)?;
    let workers_directory = config.runtime_dir.join("workers");
    tokio::fs::create_dir_all(&workers_directory)
        .await
        .map_err(NetdServerError::Runtime)?;
    std::os::unix::fs::chown(&workers_directory, Some(0), Some(config.unprivileged_gid))
        .map_err(NetdServerError::Runtime)?;
    tokio::fs::set_permissions(&workers_directory, std::fs::Permissions::from_mode(0o710))
        .await
        .map_err(NetdServerError::Runtime)?;
    prepare_root_network(
        &config.upstream,
        config.web_bind.port(),
        config.socks_bind.port(),
    )
    .await
    .map_err(|error| NetdServerError::Guard(error.to_string()))?;
    match tokio::fs::remove_file(&config.netd_socket).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(NetdServerError::Runtime(error)),
    }
    let listener = UnixListener::bind(&config.netd_socket).map_err(NetdServerError::Bind)?;
    std::os::unix::fs::chown(&config.netd_socket, Some(0), Some(config.unprivileged_gid))
        .map_err(NetdServerError::Runtime)?;
    tokio::fs::set_permissions(&config.netd_socket, std::fs::Permissions::from_mode(0o660))
        .await
        .map_err(NetdServerError::Runtime)?;
    let manager = Arc::new(WorkerManager::new(&config, shutdown.clone()));
    let clients = Arc::new(Semaphore::new(32));
    let mut client_tasks = JoinSet::new();

    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            joined = client_tasks.join_next(), if !client_tasks.is_empty() => {
                if let Some(Err(error)) = joined {
                    tracing::warn!(error = %error, "netd client task failed");
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(NetdServerError::Accept)?;
                let Ok(permit) = Arc::clone(&clients).try_acquire_owned() else {
                    continue;
                };
                let manager = Arc::clone(&manager);
                client_tasks.spawn(async move {
                    let _permit = permit;
                    if let Err(error) = serve_client(stream, &manager).await {
                        tracing::warn!(error = %error, "invalid netd client request");
                    }
                });
            }
        }
    }

    drop(listener);
    while let Some(joined) = client_tasks.join_next().await {
        if let Err(error) = joined {
            tracing::warn!(error = %error, "netd client task failed during shutdown");
        }
    }
    manager.stop_all().await;
    if command_succeeds("nft", &["list", "table", "inet", "vpngate2socks_root"])
        .await
        .unwrap_or(false)
    {
        if let Err(error) =
            run_command("nft", &["delete", "table", "inet", "vpngate2socks_root"]).await
        {
            tracing::warn!(error = %error, "failed to remove root nftables guard");
        }
    }
    if let Err(error) = tokio::fs::remove_file(&config.netd_socket).await {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(error = %error, "failed to remove netd socket");
        }
    }
    Ok(())
}

async fn serve_client(mut stream: UnixStream, manager: &WorkerManager) -> io::Result<()> {
    let request = tokio::time::timeout(
        Duration::from_secs(5),
        read_frame::<NetdRequest>(&mut stream),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "netd request timed out"))??;
    let response = match request {
        NetdRequest::Ping => NetdResponse::Pong,
        NetdRequest::StartWorker {
            worker_id,
            node_id,
            expected_remote,
            openvpn_config,
            connect_timeout_ms,
        } => {
            let timeout = Duration::from_millis(connect_timeout_ms.clamp(1_000, 300_000));
            match manager
                .start(worker_id, node_id, expected_remote, openvpn_config, timeout)
                .await
            {
                Ok(socks_socket) => NetdResponse::WorkerStarted { socks_socket },
                Err(error) => NetdResponse::Error {
                    code: error.code().to_owned(),
                    message: error.to_string(),
                },
            }
        }
        NetdRequest::StopWorker { worker_id } => match manager.stop(worker_id).await {
            Ok(()) => NetdResponse::Stopped,
            Err(error) => NetdResponse::Error {
                code: error.code().to_owned(),
                message: error.to_string(),
            },
        },
        NetdRequest::WorkerStatus { worker_id } => NetdResponse::WorkerStatus {
            ready: manager.is_ready(worker_id).await,
        },
    };
    tokio::time::timeout(Duration::from_secs(5), write_frame(&mut stream, &response))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "netd response timed out"))?
}

async fn prepare_root_network(
    upstream: &UpstreamEndpoint,
    web_port: u16,
    socks_port: u16,
) -> Result<(), WorkerError> {
    ensure_ipv4_forwarding().await?;
    if command_succeeds("nft", &["list", "table", "inet", "vpngate2socks_root"]).await? {
        run_command("nft", &["delete", "table", "inet", "vpngate2socks_root"]).await?;
    }
    let script = root_guard_script(upstream, web_port, socks_port);
    run_with_stdin("nft", &["-f", "-"], script.as_bytes()).await
}

async fn ensure_ipv4_forwarding() -> Result<(), WorkerError> {
    if tokio::fs::read_to_string("/proc/sys/net/ipv4/ip_forward")
        .await
        .is_ok_and(|value| value.trim() == "1")
    {
        return Ok(());
    }
    run_command("sysctl", &["-q", "-w", "net.ipv4.ip_forward=1"]).await
}

fn root_guard_script(upstream: &UpstreamEndpoint, web_port: u16, socks_port: u16) -> String {
    let address = upstream.socket_addr();
    format!(
        r#"
table inet vpngate2socks_root {{
    chain input {{
        type filter hook input priority filter; policy drop;
        iifname "lo" accept
        iifname "v2h*" drop
        ct state established,related accept
        tcp dport {{ {web_port}, {socks_port} }} accept
    }}
    chain output {{
        type filter hook output priority filter; policy drop;
        oifname "lo" accept
        ct state established,related accept
        ip daddr {host} tcp dport {port} accept
    }}
    chain forward {{
        type filter hook forward priority filter; policy drop;
        iifname "v2h*" ip daddr {host} tcp dport {port} accept
        oifname "v2h*" ip saddr {host} tcp sport {port} ct state established,related accept
    }}
    chain postrouting {{
        type nat hook postrouting priority srcnat; policy accept;
        ip saddr 10.231.0.0/16 masquerade
    }}
}}
"#,
        host = address.ip(),
        port = address.port()
    )
}

async fn prepare_worker_network(
    allocation: &NetworkAllocation,
    upstream: &UpstreamEndpoint,
) -> Result<(), WorkerError> {
    run_command("ip", &["netns", "add", &allocation.namespace]).await?;
    run_command(
        "ip",
        &[
            "link",
            "add",
            &allocation.host_veth,
            "type",
            "veth",
            "peer",
            "name",
            &allocation.worker_veth,
        ],
    )
    .await?;
    run_command(
        "ip",
        &[
            "addr",
            "add",
            &format!("{}/30", allocation.host_ip),
            "dev",
            &allocation.host_veth,
        ],
    )
    .await?;
    run_command("ip", &["link", "set", &allocation.host_veth, "up"]).await?;
    run_command(
        "ip",
        &[
            "link",
            "set",
            &allocation.worker_veth,
            "netns",
            &allocation.namespace,
        ],
    )
    .await?;
    run_namespace_setup(&allocation.namespace).await?;
    run_command_in_namespace(&allocation.namespace, "ip", &["link", "set", "lo", "up"]).await?;
    run_command_in_namespace(
        &allocation.namespace,
        "ip",
        &[
            "addr",
            "add",
            &format!("{}/30", allocation.worker_ip),
            "dev",
            &allocation.worker_veth,
        ],
    )
    .await?;
    run_command_in_namespace(
        &allocation.namespace,
        "ip",
        &["link", "set", &allocation.worker_veth, "up"],
    )
    .await?;
    run_command_in_namespace(
        &allocation.namespace,
        "ip",
        &[
            "route",
            "add",
            "default",
            "via",
            &allocation.host_ip.to_string(),
        ],
    )
    .await?;
    let guard = worker_guard_script(&allocation.worker_veth, upstream);
    run_with_stdin_in_namespace(&allocation.namespace, "nft", &["-f", "-"], guard.as_bytes()).await
}

async fn run_namespace_setup(namespace: &str) -> Result<(), WorkerError> {
    let executable =
        std::env::current_exe().map_err(|error| WorkerError::Network(error.to_string()))?;
    let output = Command::new("nsenter")
        .arg(namespace_argument(namespace))
        .args(["unshare", "--mount", "--propagation", "private"])
        .arg(executable)
        .arg("namespace-setup")
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorkerError::Network(format!(
            "namespace setup exited with {}",
            output.status
        )))
    }
}

/// Mounts a namespace-local procfs and disables IPv6 for current and future interfaces.
pub async fn configure_namespace() -> anyhow::Result<()> {
    let status = Command::new("mount")
        .args(["-t", "proc", "proc", "/proc"])
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .status()
        .await
        .context("failed to mount namespace-local procfs")?;
    anyhow::ensure!(status.success(), "mounting namespace-local procfs failed");
    for path in [
        "/proc/sys/net/ipv6/conf/all/disable_ipv6",
        "/proc/sys/net/ipv6/conf/default/disable_ipv6",
        "/proc/sys/net/ipv6/conf/lo/disable_ipv6",
    ] {
        tokio::fs::write(path, b"1\n")
            .await
            .with_context(|| format!("failed to harden {path}"))?;
    }
    Ok(())
}

fn worker_guard_script(worker_veth: &str, upstream: &UpstreamEndpoint) -> String {
    let address = upstream.socket_addr();
    format!(
        r#"
table inet vpngate2socks_guard {{
    chain input {{
        type filter hook input priority filter; policy drop;
        iifname "lo" accept
        iifname "{worker_veth}" ip saddr {host} tcp sport {port} ct state established,related accept
        iifname "tun0" ct state established,related accept
    }}
    chain output {{
        type filter hook output priority filter; policy drop;
        oifname "lo" accept
        oifname "{worker_veth}" ip daddr {host} tcp dport {port} accept
        oifname "tun0" accept
    }}
}}
"#,
        host = address.ip(),
        port = address.port()
    )
}

fn build_runtime_profile(
    sanitized: &str,
    upstream: &UpstreamEndpoint,
    auth_path: Option<&Path>,
    management_path: &Path,
) -> String {
    let address = upstream.socket_addr();
    let auth = auth_path.map_or_else(String::new, |path| format!(" {}", path.display()));
    format!(
        "{sanitized}socks-proxy {} {}{auth}\nmanagement {} unix\nmanagement-hold\nmanagement-query-passwords\n",
        address.ip(),
        address.port(),
        management_path.display()
    )
}

async fn write_private(
    path: &Path,
    bytes: &[u8],
    owner_uid: u32,
    owner_gid: u32,
) -> Result<(), WorkerError> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(path)
        .await
        .map_err(WorkerError::File)?;
    file.write_all(bytes).await.map_err(WorkerError::File)?;
    std::os::unix::fs::chown(path, Some(owner_uid), Some(owner_gid)).map_err(WorkerError::File)
}

fn spawn_openvpn(
    namespace: &str,
    profile: &Path,
    openvpn_uid: u32,
    openvpn_gid: u32,
) -> Result<Child, WorkerError> {
    Command::new("nsenter")
        .arg(namespace_argument(namespace))
        .arg("setpriv")
        .arg(format!("--reuid={openvpn_uid}"))
        .arg(format!("--regid={openvpn_gid}"))
        .args([
            "--clear-groups",
            "--bounding-set=-all,+net_admin",
            "--inh-caps=-all,+net_admin",
            "--ambient-caps=+net_admin",
            "--no-new-privs",
            "openvpn",
            "--config",
        ])
        .arg(profile)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("HOME", "/nonexistent")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(WorkerError::OpenVpnSpawn)
}

fn spawn_worker_socks(
    namespace: &str,
    socket: &Path,
    unprivileged_uid: u32,
    unprivileged_gid: u32,
) -> Result<Child, WorkerError> {
    let executable = std::env::current_exe().map_err(WorkerError::WorkerSpawn)?;
    Command::new("nsenter")
        .arg(namespace_argument(namespace))
        .arg("setpriv")
        .arg(format!("--reuid={unprivileged_uid}"))
        .arg(format!("--regid={unprivileged_gid}"))
        .args([
            "--clear-groups",
            "--inh-caps=-all",
            "--ambient-caps=-all",
            "--bounding-set=-all",
            "--no-new-privs",
        ])
        .arg(executable)
        .arg("worker")
        .arg("--socket")
        .arg(socket)
        .arg("--require-tun")
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("HOME", "/nonexistent")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(WorkerError::WorkerSpawn)
}

async fn wait_for_openvpn(
    management_path: &Path,
    child: &mut Child,
    timeout: Duration,
    shutdown: &CancellationToken,
) -> Result<BufReader<UnixStream>, WorkerError> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut stream = loop {
        if shutdown.is_cancelled() {
            return Err(WorkerError::ShuttingDown);
        }
        if child.try_wait().map_err(WorkerError::Management)?.is_some() {
            return Err(WorkerError::OpenVpnExited);
        }
        match UnixStream::connect(management_path).await {
            Ok(stream) => break stream,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::select! {
                    () = shutdown.cancelled() => return Err(WorkerError::ShuttingDown),
                    () = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }
            Err(_) => return Err(WorkerError::ConnectTimeout),
        }
    };
    stream
        .write_all(b"state on\nhold release\nstate\n")
        .await
        .map_err(WorkerError::Management)?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(WorkerError::ConnectTimeout);
        }
        let read = tokio::time::timeout(remaining, reader.read_line(&mut line));
        let bytes = tokio::select! {
            () = shutdown.cancelled() => return Err(WorkerError::ShuttingDown),
            result = read => result
                .map_err(|_| WorkerError::ConnectTimeout)?
                .map_err(WorkerError::Management)?,
        };
        if bytes == 0 {
            return Err(WorkerError::OpenVpnExited);
        }
        if line.contains(",CONNECTED,SUCCESS,") {
            return Ok(reader);
        }
        if line.starts_with(">FATAL:") || line.contains(",EXITING,") {
            return Err(WorkerError::OpenVpnExited);
        }
    }
}

async fn monitor_management(mut management: BufReader<UnixStream>, ready: Arc<AtomicBool>) {
    let mut line = String::new();
    loop {
        line.clear();
        match management.read_line(&mut line).await {
            Ok(0) | Err(_) => {
                ready.store(false, Ordering::Release);
                break;
            }
            Ok(_) if line.starts_with(">STATE:") => {
                ready.store(line.contains(",CONNECTED,SUCCESS,"), Ordering::Release);
            }
            Ok(_) if line.starts_with(">FATAL:") => {
                ready.store(false, Ordering::Release);
                break;
            }
            Ok(_) => {}
        }
    }
}

async fn wait_for_socket(
    socket: &Path,
    child: &mut Child,
    timeout: Duration,
    shutdown: &CancellationToken,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        if UnixStream::connect(socket).await.is_ok() {
            return true;
        }
        if shutdown.is_cancelled() || tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::select! {
            () = shutdown.cancelled() => return false,
            () = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }
}

async fn wait_for_tunnel_routes(
    namespace: &str,
    child: &mut Child,
    timeout: Duration,
    shutdown: &CancellationToken,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        if namespace_routes_cover_public_ipv4(namespace)
            .await
            .unwrap_or(false)
        {
            return true;
        }
        if shutdown.is_cancelled() || tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::select! {
            () = shutdown.cancelled() => return false,
            () = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
}

async fn namespace_routes_cover_public_ipv4(namespace: &str) -> Result<bool, WorkerError> {
    let output = Command::new("nsenter")
        .arg(namespace_argument(namespace))
        .args(["ip", "-4", "route", "show", "table", "main"])
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    if !output.status.success() {
        return Ok(false);
    }
    let routes = std::str::from_utf8(&output.stdout)
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    Ok(route_output_covers_public_ipv4(routes))
}

fn route_output_covers_public_ipv4(routes: &str) -> bool {
    let mut default = false;
    let mut lower_half = false;
    let mut upper_half = false;
    for line in routes.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if !fields.windows(2).any(|pair| pair == ["dev", "tun0"]) {
            continue;
        }
        match fields.first().copied() {
            Some("default") => default = true,
            Some("0.0.0.0/1") => lower_half = true,
            Some("128.0.0.0/1") => upper_half = true,
            _ => {}
        }
    }
    default || (lower_half && upper_half)
}

async fn run_command(program: &str, arguments: &[&str]) -> Result<(), WorkerError> {
    let output = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorkerError::Network(format!(
            "{program} exited with {}",
            output.status
        )))
    }
}

async fn command_succeeds(program: &str, arguments: &[&str]) -> Result<bool, WorkerError> {
    Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .map_err(|error| WorkerError::Network(error.to_string()))
}

async fn run_command_in_namespace(
    namespace: &str,
    program: &str,
    arguments: &[&str],
) -> Result<(), WorkerError> {
    let namespace = namespace_argument(namespace);
    let mut command_arguments = vec![namespace.as_str(), program];
    command_arguments.extend_from_slice(arguments);
    run_command("nsenter", &command_arguments).await
}

async fn command_succeeds_in_namespace(
    namespace: &str,
    program: &str,
    arguments: &[&str],
) -> Result<bool, WorkerError> {
    let namespace = namespace_argument(namespace);
    let mut command_arguments = vec![namespace.as_str(), program];
    command_arguments.extend_from_slice(arguments);
    command_succeeds("nsenter", &command_arguments).await
}

fn namespace_argument(namespace: &str) -> String {
    format!("--net=/run/netns/{namespace}")
}

async fn run_with_stdin(
    program: &str,
    arguments: &[&str],
    input: &[u8],
) -> Result<(), WorkerError> {
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LANG", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    child
        .stdin
        .take()
        .ok_or_else(|| WorkerError::Network("command stdin was unavailable".to_owned()))?
        .write_all(input)
        .await
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    let status = child
        .wait()
        .await
        .map_err(|error| WorkerError::Network(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(WorkerError::Network(format!(
            "{program} exited with {status}"
        )))
    }
}

async fn run_with_stdin_in_namespace(
    namespace: &str,
    program: &str,
    arguments: &[&str],
    input: &[u8],
) -> Result<(), WorkerError> {
    let namespace = namespace_argument(namespace);
    let mut command_arguments = vec![namespace.as_str(), program];
    command_arguments.extend_from_slice(arguments);
    run_with_stdin("nsenter", &command_arguments, input).await
}

async fn stop_process(mut process: WorkerProcess) {
    tracing::info!(node.id = %process.node_id, namespace = %process.network.namespace, "stopping worker");
    process.ready.store(false, Ordering::Release);
    process.management_task.abort();
    stop_child(&mut process.socks).await;
    stop_child(&mut process.openvpn).await;
    cleanup_worker_network(&process.network.namespace, &process.network.host_veth).await;
    cleanup_directory(&process.directory).await;
}

async fn stop_child(child: &mut Child) {
    if child.start_kill().is_ok() {
        let _status = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    }
}

async fn cleanup_namespace(namespace: &str) {
    if let Err(error) = run_command("ip", &["netns", "delete", namespace]).await {
        tracing::warn!(error = %error, namespace, "failed to clean network namespace");
    }
}

async fn cleanup_worker_network(namespace: &str, host_veth: &str) {
    if command_succeeds("ip", &["link", "show", "dev", host_veth])
        .await
        .unwrap_or(false)
    {
        if let Err(error) = run_command("ip", &["link", "delete", host_veth]).await {
            tracing::warn!(error = %error, interface = host_veth, "failed to clean worker veth");
        }
    }
    let namespace_path = PathBuf::from("/run/netns").join(namespace);
    if tokio::fs::metadata(namespace_path).await.is_ok() {
        cleanup_namespace(namespace).await;
    }
}

async fn cleanup_directory(directory: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(directory).await {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(error = %error, path = %directory.display(), "failed to clean worker directory");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use crate::domain::SecretString;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn guard_allows_only_upstream_on_veth_and_tun_output() {
        let upstream = UpstreamEndpoint::new(
            Ipv4Addr::new(203, 0, 113, 10),
            NonZeroU16::new(1080).expect("non-zero"),
            Some("user".to_owned()),
            Some(SecretString::new("password")),
        )
        .expect("valid endpoint");
        let script = worker_guard_script("v2wdeadbeef", &upstream);
        assert!(script.contains("policy drop"));
        assert!(script.contains("203.0.113.10 tcp dport 1080 accept"));
        assert!(script.contains("oifname \"tun0\" accept"));
        assert!(script.contains("iifname \"v2wdeadbeef\" ip saddr 203.0.113.10 tcp sport 1080"));
        assert!(!script.contains("\n        ct state established,related accept"));
        assert!(!script.contains("password"));
    }

    #[test]
    fn root_guard_never_forwards_arbitrary_worker_traffic() {
        let upstream = UpstreamEndpoint::new(
            Ipv4Addr::new(203, 0, 113, 10),
            NonZeroU16::new(1080).expect("non-zero"),
            None,
            None,
        )
        .expect("valid endpoint");
        let script = root_guard_script(&upstream, 8080, 1080);
        assert!(script.contains("iifname \"v2h*\" ip daddr 203.0.113.10 tcp dport 1080 accept"));
        assert!(script.contains("iifname \"v2h*\" drop"));
        assert!(!script.contains("iifname \"v2h*\" accept"));
    }

    #[test]
    fn network_indices_are_reused_only_after_the_lease_drops() {
        let directory = tempdir().expect("temporary directory");
        let config = AppConfig::test_config(directory.path().to_path_buf());
        let manager = WorkerManager::new(&config, CancellationToken::new());
        let first = manager.allocate_network().expect("first allocation");
        let first_namespace = first.namespace.clone();
        let second = manager.allocate_network().expect("second allocation");
        assert_ne!(first.namespace, second.namespace);
        drop(first);
        let reused = manager.allocate_network().expect("reused allocation");
        assert_eq!(reused.namespace, first_namespace);
    }

    #[test]
    fn tunnel_routes_must_cover_both_ipv4_halves() {
        assert!(route_output_covers_public_ipv4(
            "default via 10.0.0.1 dev tun0\n"
        ));
        assert!(route_output_covers_public_ipv4(
            "default via 10.231.0.1 dev v2w0001\n0.0.0.0/1 via 10.8.0.1 dev tun0\n128.0.0.0/1 via 10.8.0.1 dev tun0\n"
        ));
        assert!(!route_output_covers_public_ipv4(
            "default via 10.231.0.1 dev v2w0001\n0.0.0.0/1 via 10.8.0.1 dev tun0\n"
        ));
    }
}
