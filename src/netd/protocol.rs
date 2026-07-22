//! Length-delimited protocol shared by the control plane and privileged helper.

use std::{io, net::SocketAddrV4, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::UnixStream,
};

use crate::domain::{NodeId, WorkerId};

const MAX_FRAME_BYTES: usize = 512 * 1024;
const CONTROL_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Commands accepted by the narrowly scoped privileged helper.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "command",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NetdRequest {
    Ping,
    StartWorker {
        worker_id: WorkerId,
        node_id: NodeId,
        expected_remote: SocketAddrV4,
        openvpn_config: String,
        connect_timeout_ms: u64,
    },
    StopWorker {
        worker_id: WorkerId,
    },
    WorkerStatus {
        worker_id: WorkerId,
    },
}

/// Responses from the privileged helper.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NetdResponse {
    Pong,
    WorkerStarted { socks_socket: PathBuf },
    Stopped,
    WorkerStatus { ready: bool },
    Error { code: String, message: String },
}

/// Control-plane client for `netd`.
#[derive(Debug, Clone)]
pub struct NetdClient {
    socket_path: PathBuf,
    request_timeout: Duration,
}

/// Communication failure or a typed helper rejection.
#[derive(Debug, Error)]
pub enum NetdClientError {
    #[error("netd is unavailable")]
    Unavailable(#[source] io::Error),
    #[error("netd request timed out")]
    Timeout,
    #[error("netd protocol failed")]
    Protocol(#[source] io::Error),
    #[error("netd rejected the request ({code}): {message}")]
    Rejected { code: String, message: String },
    #[error("netd returned an unexpected response")]
    UnexpectedResponse,
}

impl NetdClient {
    /// Creates a client. Each request uses a fresh Unix connection.
    #[must_use]
    pub fn new(socket_path: PathBuf, request_timeout: Duration) -> Self {
        Self {
            socket_path,
            request_timeout,
        }
    }

    /// Checks whether the helper accepts a request.
    pub async fn ping(&self) -> Result<(), NetdClientError> {
        match self
            .request(NetdRequest::Ping, CONTROL_REQUEST_TIMEOUT)
            .await?
        {
            NetdResponse::Pong => Ok(()),
            _ => Err(NetdClientError::UnexpectedResponse),
        }
    }

    /// Starts a worker and waits until `OpenVPN` reports `CONNECTED,SUCCESS`.
    pub async fn start_worker(
        &self,
        worker_id: WorkerId,
        node_id: NodeId,
        expected_remote: SocketAddrV4,
        openvpn_config: String,
        connect_timeout: Duration,
    ) -> Result<PathBuf, NetdClientError> {
        let connect_timeout_ms = u64::try_from(connect_timeout.as_millis()).unwrap_or(u64::MAX);
        match self
            .request(
                NetdRequest::StartWorker {
                    worker_id,
                    node_id,
                    expected_remote,
                    openvpn_config,
                    connect_timeout_ms,
                },
                self.request_timeout,
            )
            .await?
        {
            NetdResponse::WorkerStarted { socks_socket } => Ok(socks_socket),
            NetdResponse::Error { code, message } => {
                Err(NetdClientError::Rejected { code, message })
            }
            _ => Err(NetdClientError::UnexpectedResponse),
        }
    }

    /// Stops and removes an isolated worker.
    pub async fn stop_worker(&self, worker_id: WorkerId) -> Result<(), NetdClientError> {
        match self
            .request(NetdRequest::StopWorker { worker_id }, STOP_REQUEST_TIMEOUT)
            .await?
        {
            NetdResponse::Stopped => Ok(()),
            NetdResponse::Error { code, message } => {
                Err(NetdClientError::Rejected { code, message })
            }
            _ => Err(NetdClientError::UnexpectedResponse),
        }
    }

    /// Checks whether the worker still has a usable `tun0` interface.
    pub async fn worker_ready(&self, worker_id: WorkerId) -> Result<bool, NetdClientError> {
        match self
            .request(
                NetdRequest::WorkerStatus { worker_id },
                CONTROL_REQUEST_TIMEOUT,
            )
            .await?
        {
            NetdResponse::WorkerStatus { ready } => Ok(ready),
            NetdResponse::Error { code, message } => {
                Err(NetdClientError::Rejected { code, message })
            }
            _ => Err(NetdClientError::UnexpectedResponse),
        }
    }

    async fn request(
        &self,
        request: NetdRequest,
        timeout: Duration,
    ) -> Result<NetdResponse, NetdClientError> {
        let request = async {
            let mut stream = UnixStream::connect(&self.socket_path)
                .await
                .map_err(NetdClientError::Unavailable)?;
            write_frame(&mut stream, &request)
                .await
                .map_err(NetdClientError::Protocol)?;
            read_frame(&mut stream)
                .await
                .map_err(NetdClientError::Protocol)
        };
        tokio::time::timeout(timeout, request)
            .await
            .map_err(|_| NetdClientError::Timeout)?
    }
}

pub(super) async fn read_frame<T>(stream: &mut UnixStream) -> io::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let length = usize::try_from(stream.read_u32().await?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid frame length"))?;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds protocol limit",
        ));
    }
    let mut bytes = vec![0_u8; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(super) async fn write_frame<T>(stream: &mut UnixStream, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds protocol limit",
        ));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid frame length"))?;
    stream.write_u32(length).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tokio::net::UnixListener;

    use super::*;

    #[test]
    fn protocol_never_serializes_an_untyped_command() {
        let request = NetdRequest::StopWorker {
            worker_id: WorkerId::new(),
        };
        let value = serde_json::to_value(request).expect("request serializes");
        assert_eq!(value["command"], "stopWorker");
    }

    #[tokio::test]
    async fn client_and_mock_helper_exchange_bounded_typed_frames() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("netd.sock");
        let listener = UnixListener::bind(&path).expect("mock listener");
        let helper = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("mock connection");
            assert!(matches!(
                read_frame::<NetdRequest>(&mut stream)
                    .await
                    .expect("request frame"),
                NetdRequest::Ping
            ));
            write_frame(&mut stream, &NetdResponse::Pong)
                .await
                .expect("response frame");
        });
        let client = NetdClient::new(path, Duration::from_secs(1));
        client.ping().await.expect("typed ping succeeds");
        helper.await.expect("mock helper exits");
    }
}
