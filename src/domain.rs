//! Core domain values and externally visible state.

use std::{
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
    num::NonZeroU16,
    str::FromStr,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::openvpn::SanitizedOpenVpn;

/// Stable identifier derived from a VPN Gate node and its sanitized profile.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Creates an identifier from a BLAKE3 digest.
    #[must_use]
    pub fn from_digest(digest: blake3::Hash) -> Self {
        Self(digest.to_hex().to_string())
    }

    /// Returns the wire representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("NodeId").field(&self.0).finish()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for NodeId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value.to_owned()))
        } else {
            Err("node id must be 64 lowercase hexadecimal characters")
        }
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// Unique identifier for one isolated network worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerId(Uuid);

impl WorkerId {
    /// Creates a collision-resistant worker identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns a short value suitable for Linux interface names.
    #[must_use]
    pub fn short(self) -> String {
        self.0.simple().to_string()[..8].to_owned()
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Unique identifier for a queued or running node test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(Uuid);

impl OperationId {
    /// Creates a new operation identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for OperationId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// A secret that is redacted from diagnostics and zeroed when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Wraps a secret value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Explicitly exposes the secret at the point where it is required.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Validated IPv4 SOCKS5 endpoint and optional RFC 1929 credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct UpstreamEndpoint {
    host: Ipv4Addr,
    port: NonZeroU16,
    username: Option<String>,
    password: Option<SecretString>,
}

impl UpstreamEndpoint {
    /// Constructs a validated endpoint.
    pub fn new(
        host: Ipv4Addr,
        port: NonZeroU16,
        username: Option<String>,
        password: Option<SecretString>,
    ) -> Result<Self, &'static str> {
        if host.octets()[0] == 0 || host == Ipv4Addr::BROADCAST || host.is_multicast() {
            return Err("upstream host must be a unicast IPv4 address");
        }
        if username.is_some() != password.is_some() {
            return Err("upstream username and password must be configured together");
        }
        if username.as_deref().is_some_and(str::is_empty) {
            return Err("upstream username must not be empty");
        }
        if username.as_deref().is_some_and(invalid_socks_credential)
            || password
                .as_ref()
                .is_some_and(|value| invalid_socks_credential(value.expose()))
        {
            return Err(
                "upstream credentials must be at most 255 bytes and contain no line breaks",
            );
        }
        Ok(Self {
            host,
            port,
            username,
            password,
        })
    }

    /// Returns the network address without credentials.
    #[must_use]
    pub const fn socket_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.host, self.port.get())
    }

    /// Returns the optional username.
    #[must_use]
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    /// Returns the optional password at an explicit exposure point.
    #[must_use]
    pub fn password(&self) -> Option<&str> {
        self.password.as_ref().map(SecretString::expose)
    }

    /// Builds a remote-DNS SOCKS URL for HTTP clients.
    pub fn proxy_url(&self) -> Result<url::Url, url::ParseError> {
        let mut url = url::Url::parse(&format!("socks5h://{}:{}", self.host, self.port))?;
        if let (Some(username), Some(password)) = (self.username(), self.password()) {
            let _ = url.set_username(username);
            let _ = url.set_password(Some(password));
        }
        Ok(url)
    }
}

fn invalid_socks_credential(value: &str) -> bool {
    value.is_empty()
        || u8::try_from(value.len()).is_err()
        || value
            .bytes()
            .any(|byte| matches!(byte, b'\0' | b'\r' | b'\n'))
}

impl fmt::Debug for UpstreamEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpstreamEndpoint")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("has_credentials", &self.username.is_some())
            .finish_non_exhaustive()
    }
}

/// Why a downloaded node can or cannot be used in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeAvailability {
    Available,
    UnsupportedProtocol,
    InvalidConfig,
}

/// Internal representation of a VPN Gate node. The profile is deliberately not serializable.
#[derive(Debug, Clone)]
pub struct VpnNode {
    pub id: NodeId,
    pub hostname: String,
    pub ip: Ipv4Addr,
    pub score: u64,
    pub ping_ms: Option<u32>,
    pub speed_bps: u64,
    pub country_long: String,
    pub country_short: String,
    pub sessions: u32,
    pub uptime_ms: u64,
    pub total_users: u64,
    pub total_traffic_bytes: u64,
    pub log_type: String,
    pub operator: String,
    pub message: String,
    pub tcp_port: Option<NonZeroU16>,
    pub availability: NodeAvailability,
    pub openvpn: Option<SanitizedOpenVpn>,
}

/// Public node fields returned by the Web API.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub id: NodeId,
    pub hostname: String,
    pub ip: Ipv4Addr,
    pub score: u64,
    pub ping_ms: Option<u32>,
    pub speed_bps: u64,
    pub country_long: String,
    pub country_short: String,
    pub sessions: u32,
    pub uptime_ms: u64,
    pub total_users: u64,
    pub total_traffic_bytes: u64,
    pub log_type: String,
    pub operator: String,
    pub message: String,
    pub tcp_port: Option<NonZeroU16>,
    pub availability: NodeAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_test: Option<TestRecord>,
}

impl VpnNode {
    /// Produces the public view without exposing the `OpenVPN` profile.
    #[must_use]
    pub fn summary(&self, latest_test: Option<TestRecord>) -> NodeSummary {
        NodeSummary {
            id: self.id.clone(),
            hostname: self.hostname.clone(),
            ip: self.ip,
            score: self.score,
            ping_ms: self.ping_ms,
            speed_bps: self.speed_bps,
            country_long: self.country_long.clone(),
            country_short: self.country_short.clone(),
            sessions: self.sessions,
            uptime_ms: self.uptime_ms,
            total_users: self.total_users,
            total_traffic_bytes: self.total_traffic_bytes,
            log_type: self.log_type.clone(),
            operator: self.operator.clone(),
            message: self.message.clone(),
            tcp_port: self.tcp_port,
            availability: self.availability,
            latest_test,
        }
    }

    /// Matches the documented region and free-text search fields.
    #[must_use]
    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        query.is_empty()
            || self.country_long.to_lowercase().contains(&query)
            || self.country_short.to_lowercase().contains(&query)
            || self.hostname.to_lowercase().contains(&query)
            || self.ip.to_string().contains(&query)
            || self.operator.to_lowercase().contains(&query)
            || self.message.to_lowercase().contains(&query)
    }
}

/// Risk information returned by `IPPure`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpPureResult {
    pub fraud_score: f64,
    pub is_residential: bool,
    pub is_broadcast: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_ip: Option<Ipv4Addr>,
}

/// Last persisted result for a node, including failures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRecord {
    pub node_id: NodeId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<IpPureResult>,
    pub duration_ms: u64,
    pub tested_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// State of the active make-before-break connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConnectionState {
    Disconnected,
    Connecting {
        node_id: NodeId,
        worker_id: WorkerId,
        since: DateTime<Utc>,
    },
    Connected {
        node_id: NodeId,
        worker_id: WorkerId,
        since: DateTime<Utc>,
    },
    Failed {
        node_id: NodeId,
        message: String,
        at: DateTime<Utc>,
    },
}

impl ConnectionState {
    /// Whether new local SOCKS connections may be accepted.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// Lifecycle of an isolated node quality test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TestState {
    Queued {
        node_id: NodeId,
        queued_at: DateTime<Utc>,
    },
    Running {
        node_id: NodeId,
        started_at: DateTime<Utc>,
    },
    Succeeded {
        node_id: NodeId,
        record: TestRecord,
    },
    Failed {
        node_id: NodeId,
        record: TestRecord,
    },
}

/// Reachability of the configured upstream SOCKS5 server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpstreamState {
    Checking,
    Ready,
    Unreachable,
    AuthenticationFailed,
    NetdUnavailable,
}

/// Event sent to `WebUI` clients over SSE.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppEvent {
    Connection(ConnectionState),
    Test {
        operation_id: OperationId,
        state: TestState,
    },
    NodesRefreshed {
        accepted: usize,
        rejected: usize,
        at: DateTime<Utc>,
    },
    RefreshFailed {
        message: String,
        at: DateTime<Utc>,
    },
    Upstream {
        state: UpstreamState,
        at: DateTime<Utc>,
    },
}

/// Returns true only for globally routable IPv4 targets.
#[must_use]
pub fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let value = u32::from(ip);
    let in_cidr = |network: Ipv4Addr, prefix: u32| {
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };
        value & mask == u32::from(network) & mask
    };

    !(in_cidr(Ipv4Addr::UNSPECIFIED, 8)
        || in_cidr(Ipv4Addr::new(10, 0, 0, 0), 8)
        || in_cidr(Ipv4Addr::new(100, 64, 0, 0), 10)
        || in_cidr(Ipv4Addr::new(127, 0, 0, 0), 8)
        || in_cidr(Ipv4Addr::new(169, 254, 0, 0), 16)
        || in_cidr(Ipv4Addr::new(172, 16, 0, 0), 12)
        || in_cidr(Ipv4Addr::new(192, 0, 0, 0), 24)
        || in_cidr(Ipv4Addr::new(192, 0, 2, 0), 24)
        || in_cidr(Ipv4Addr::new(192, 88, 99, 0), 24)
        || in_cidr(Ipv4Addr::new(192, 168, 0, 0), 16)
        || in_cidr(Ipv4Addr::new(198, 18, 0, 0), 15)
        || in_cidr(Ipv4Addr::new(198, 51, 100, 0), 24)
        || in_cidr(Ipv4Addr::new(203, 0, 113, 0), 24)
        || in_cidr(Ipv4Addr::new(224, 0, 0, 0), 4)
        || in_cidr(Ipv4Addr::new(240, 0, 0, 0), 4))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use super::*;

    #[test]
    fn node_id_rejects_noncanonical_values() {
        assert!("a".repeat(64).parse::<NodeId>().is_ok());
        assert!("A".repeat(64).parse::<NodeId>().is_err());
        assert!("a".repeat(63).parse::<NodeId>().is_err());
    }

    #[test]
    fn public_ipv4_rejects_non_public_ranges() {
        assert!(!is_public_ipv4(Ipv4Addr::LOCALHOST));
        assert!(!is_public_ipv4(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_public_ipv4(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(is_public_ipv4(Ipv4Addr::new(1, 1, 1, 1)));
    }

    #[test]
    fn upstream_credentials_are_bounded_and_redacted() {
        let endpoint = UpstreamEndpoint::new(
            Ipv4Addr::LOCALHOST,
            NonZeroU16::new(1080).expect("non-zero port"),
            Some("user".to_owned()),
            Some(SecretString::new("secret")),
        )
        .expect("valid endpoint");
        assert!(!format!("{endpoint:?}").contains("secret"));
        assert!(
            UpstreamEndpoint::new(
                Ipv4Addr::LOCALHOST,
                NonZeroU16::new(1080).expect("non-zero port"),
                Some("line\nbreak".to_owned()),
                Some(SecretString::new("secret")),
            )
            .is_err()
        );
    }

    #[test]
    fn upstream_state_uses_the_frontend_wire_name() {
        assert_eq!(
            serde_json::to_string(&UpstreamState::AuthenticationFailed).expect("state serializes"),
            "\"authenticationFailed\""
        );
    }
}
