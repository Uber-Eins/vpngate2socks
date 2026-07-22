//! Core domain values and externally visible state.

use std::{
    fmt, io,
    net::{Ipv4Addr, SocketAddrV4},
    num::NonZeroU16,
    str::FromStr,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
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

/// Configured SOCKS5 endpoint and optional RFC 1929 credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct UpstreamEndpoint {
    host: String,
    port: NonZeroU16,
    username: Option<String>,
    password: Option<SecretString>,
}

impl UpstreamEndpoint {
    /// Constructs a validated endpoint from a numeric IPv4 address.
    pub fn new(
        host: Ipv4Addr,
        port: NonZeroU16,
        username: Option<String>,
        password: Option<SecretString>,
    ) -> Result<Self, &'static str> {
        Self::new_host(host.to_string(), port, username, password)
    }

    /// Constructs a validated endpoint from an IPv4 address or DNS/container hostname.
    pub fn new_host(
        host: impl Into<String>,
        port: NonZeroU16,
        username: Option<String>,
        password: Option<SecretString>,
    ) -> Result<Self, &'static str> {
        let host = host.into();
        if let Ok(address) = host.parse::<Ipv4Addr>() {
            if !is_valid_upstream_ipv4(address) {
                return Err("upstream host must be a unicast IPv4 address");
            }
        } else if host
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Err("upstream host contains an invalid IPv4 address");
        } else if !is_valid_upstream_hostname(&host) {
            return Err("upstream host must be an IPv4 address or valid ASCII hostname");
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
            host: host.to_ascii_lowercase(),
            port,
            username,
            password,
        })
    }

    /// Returns the configured hostname without credentials.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Resolves and pins one IPv4 address for firewall and transport consistency.
    pub async fn resolve_ipv4(&self) -> Result<ResolvedUpstreamEndpoint, UpstreamResolveError> {
        if let Ok(address) = self.host.parse::<Ipv4Addr>() {
            return self.resolve_to(SocketAddrV4::new(address, self.port.get()));
        }
        let addresses = tokio::net::lookup_host((self.host.as_str(), self.port.get()))
            .await
            .map_err(UpstreamResolveError::Lookup)?;
        let address = addresses
            .filter_map(|address| match address {
                std::net::SocketAddr::V4(address) if is_valid_upstream_ipv4(*address.ip()) => {
                    Some(address)
                }
                std::net::SocketAddr::V4(_) | std::net::SocketAddr::V6(_) => None,
            })
            .min_by_key(|address| u32::from(*address.ip()))
            .ok_or(UpstreamResolveError::NoIpv4)?;
        self.resolve_to(address)
    }

    /// Combines this configuration with the exact address selected by the helper.
    pub fn resolve_to(
        &self,
        address: SocketAddrV4,
    ) -> Result<ResolvedUpstreamEndpoint, UpstreamResolveError> {
        if address.port() != self.port.get() || !is_valid_upstream_ipv4(*address.ip()) {
            return Err(UpstreamResolveError::Mismatch);
        }
        if let Ok(configured) = self.host.parse::<Ipv4Addr>() {
            if configured != *address.ip() {
                return Err(UpstreamResolveError::Mismatch);
            }
        }
        Ok(ResolvedUpstreamEndpoint {
            address,
            username: self.username.clone(),
            password: self.password.clone(),
        })
    }
}

/// A configured upstream pinned to the same IPv4 address used by nftables.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedUpstreamEndpoint {
    address: SocketAddrV4,
    username: Option<String>,
    password: Option<SecretString>,
}

impl ResolvedUpstreamEndpoint {
    /// Returns the pinned network address without credentials.
    #[must_use]
    pub const fn socket_addr(&self) -> SocketAddrV4 {
        self.address
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
        let mut url = url::Url::parse(&format!("socks5h://{}", self.address))?;
        if let (Some(username), Some(password)) = (self.username(), self.password()) {
            let _ = url.set_username(username);
            let _ = url.set_password(Some(password));
        }
        Ok(url)
    }
}

/// Failure while resolving or reconciling the configured upstream endpoint.
#[derive(Debug, Error)]
pub enum UpstreamResolveError {
    #[error("failed to resolve the upstream SOCKS5 hostname")]
    Lookup(#[source] io::Error),
    #[error("upstream SOCKS5 hostname has no usable IPv4 address")]
    NoIpv4,
    #[error("resolved upstream SOCKS5 address does not match its configuration")]
    Mismatch,
}

fn is_valid_upstream_ipv4(host: Ipv4Addr) -> bool {
    host.octets()[0] != 0 && host != Ipv4Addr::BROADCAST && !host.is_multicast()
}

fn is_valid_upstream_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
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

impl fmt::Debug for ResolvedUpstreamEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedUpstreamEndpoint")
            .field("address", &self.address)
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

/// `IPPure` network classification used by automatic connection selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IpTypeFilter {
    /// Do not filter on the `IPPure` broadcast classification.
    #[default]
    Any,
    /// Require a directly assigned (non-broadcast) exit IP.
    Native,
    /// Require an exit IP classified as broadcast.
    Broadcast,
}

impl IpTypeFilter {
    /// Returns the stable persistence representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Native => "native",
            Self::Broadcast => "broadcast",
        }
    }

    /// Parses the stable persistence representation.
    #[must_use]
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "any" => Some(Self::Any),
            "native" => Some(Self::Native),
            "broadcast" => Some(Self::Broadcast),
            _ => None,
        }
    }
}

/// `IPPure` residential classification used by automatic connection selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResidentialFilter {
    /// Do not filter on the residential classification.
    #[default]
    Any,
    /// Require a residential exit IP.
    Residential,
    /// Require a non-residential exit IP.
    NonResidential,
}

impl ResidentialFilter {
    /// Returns the stable persistence representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Residential => "residential",
            Self::NonResidential => "nonResidential",
        }
    }

    /// Parses the stable persistence representation.
    #[must_use]
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "any" => Some(Self::Any),
            "residential" => Some(Self::Residential),
            "nonResidential" => Some(Self::NonResidential),
            _ => None,
        }
    }
}

/// Persisted policy for automatically selecting and reconnecting a VPN node.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoConnectConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub ip_type: IpTypeFilter,
    pub residential: ResidentialFilter,
}

impl AutoConnectConfig {
    /// Normalizes a user-provided region and rejects values VPN Gate cannot emit.
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        self.region = self
            .region
            .take()
            .map(|region| region.trim().to_uppercase())
            .filter(|region| !region.is_empty());
        if self
            .region
            .as_ref()
            .is_some_and(|region| region.len() > 8 || region.chars().any(char::is_control))
        {
            return Err("region must be at most 8 bytes and contain no control characters");
        }
        Ok(self)
    }
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
    AutoConnection(AutoConnectConfig),
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
    fn upstream_container_hostname_is_validated_and_pinned() {
        let endpoint = UpstreamEndpoint::new_host(
            "HOST.containers.internal",
            NonZeroU16::new(1080).expect("non-zero port"),
            Some("user".to_owned()),
            Some(SecretString::new("secret")),
        )
        .expect("container hostname is valid");
        assert_eq!(endpoint.host(), "host.containers.internal");

        let resolved = endpoint
            .resolve_to("10.0.2.2:1080".parse().expect("test address"))
            .expect("hostname can be pinned to IPv4");
        assert_eq!(
            resolved.socket_addr(),
            "10.0.2.2:1080".parse().expect("test address")
        );
        let proxy_url = resolved.proxy_url().expect("proxy URL is valid");
        assert_eq!(proxy_url.host_str(), Some("10.0.2.2"));
        assert!(!format!("{resolved:?}").contains("secret"));
    }

    #[test]
    fn upstream_hostname_rejects_ambiguous_or_unsafe_values() {
        let port = NonZeroU16::new(1080).expect("non-zero port");
        for host in [
            "host..internal",
            "-host.internal",
            "host_.internal",
            "[::1]",
            "127.1",
            "999.0.0.1",
        ] {
            assert!(
                UpstreamEndpoint::new_host(host, port, None, None).is_err(),
                "{host} must be rejected"
            );
        }
    }

    #[test]
    fn numeric_upstream_must_match_the_pinned_address() {
        let endpoint = UpstreamEndpoint::new(
            Ipv4Addr::new(10, 0, 2, 2),
            NonZeroU16::new(1080).expect("non-zero port"),
            None,
            None,
        )
        .expect("valid endpoint");
        assert!(
            endpoint
                .resolve_to("10.0.2.3:1080".parse().expect("test address"))
                .is_err()
        );
        assert!(
            endpoint
                .resolve_to("10.0.2.2:1081".parse().expect("test address"))
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
