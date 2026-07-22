//! VPN Gate API retrieval and defensive CSV parsing.

use std::{net::Ipv4Addr, num::NonZeroU16, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt as _;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::{
    domain::{NodeAvailability, NodeId, ResolvedUpstreamEndpoint, VpnNode, is_public_ipv4},
    openvpn::{OpenVpnConfigError, sanitize_openvpn},
};

const EXPECTED_HEADERS: [&str; 15] = [
    "HostName",
    "IP",
    "Score",
    "Ping",
    "Speed",
    "CountryLong",
    "CountryShort",
    "NumVpnSessions",
    "Uptime",
    "TotalUsers",
    "TotalTraffic",
    "LogType",
    "Operator",
    "Message",
    "OpenVPN_ConfigData_Base64",
];

/// Defensive input limits for the public CSV endpoint.
#[derive(Debug, Clone, Copy)]
pub struct CsvLimits {
    pub max_body_bytes: usize,
    pub max_rows: usize,
    pub max_profile_bytes: usize,
}

impl Default for CsvLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 16 * 1024 * 1024,
            max_rows: 20_000,
            max_profile_bytes: 256 * 1024,
        }
    }
}

/// Counts from a tolerant CSV refresh.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseStats {
    pub rows_seen: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub unsupported: usize,
}

/// A successfully parsed snapshot and its row-level statistics.
#[derive(Debug)]
pub struct ParsedSnapshot {
    pub nodes: Vec<VpnNode>,
    pub stats: ParseStats,
}

/// Failure of the complete refresh; individual bad rows are represented in `ParseStats`.
#[derive(Debug, Error)]
pub enum VpnGateError {
    #[error("upstream proxy URL is invalid")]
    InvalidProxy(#[source] url::ParseError),
    #[error("failed to construct the VPN Gate HTTP client")]
    Client(#[source] reqwest::Error),
    #[error("VPN Gate request failed")]
    Request(#[source] reqwest::Error),
    #[error("VPN Gate returned HTTP {0}")]
    Http(reqwest::StatusCode),
    #[error("VPN Gate response exceeds the configured size limit")]
    BodyTooLarge,
    #[error("VPN Gate response is not valid UTF-8")]
    InvalidUtf8,
    #[error("VPN Gate CSV header was not found")]
    MissingHeader,
    #[error("VPN Gate CSV schema does not contain the expected 15 fields")]
    InvalidHeader,
    #[error("VPN Gate CSV exceeds the configured row limit")]
    TooManyRows,
}

/// Downloads the official node list only through the configured `SOCKS5h` proxy.
pub async fn fetch_snapshot(
    endpoint: &url::Url,
    upstream: &ResolvedUpstreamEndpoint,
    timeout: Duration,
    limits: CsvLimits,
) -> Result<ParsedSnapshot, VpnGateError> {
    let proxy_url = upstream.proxy_url().map_err(VpnGateError::InvalidProxy)?;
    let proxy = reqwest::Proxy::all(proxy_url.as_str()).map_err(VpnGateError::Client)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .tls_backend_native()
        .proxy(proxy)
        .timeout(timeout)
        .build()
        .map_err(VpnGateError::Client)?;
    let response = client
        .get(endpoint.clone())
        .send()
        .await
        .map_err(VpnGateError::Request)?;
    if !response.status().is_success() {
        return Err(VpnGateError::Http(response.status()));
    }

    let mut body = Vec::with_capacity(limits.max_body_bytes.min(256 * 1024));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(VpnGateError::Request)?;
        if body.len().saturating_add(chunk.len()) > limits.max_body_bytes {
            return Err(VpnGateError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    parse_snapshot(&body, limits)
}

/// Parses the marker-prefixed CSV returned by VPN Gate.
pub fn parse_snapshot(input: &[u8], limits: CsvLimits) -> Result<ParsedSnapshot, VpnGateError> {
    if input.len() > limits.max_body_bytes {
        return Err(VpnGateError::BodyTooLarge);
    }
    let input = std::str::from_utf8(input).map_err(|_| VpnGateError::InvalidUtf8)?;
    let header_offset = input
        .find("#HostName,")
        .ok_or(VpnGateError::MissingHeader)?;
    let csv = &input[header_offset + 1..];
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(csv.as_bytes());
    if reader
        .headers()
        .map_err(|_| VpnGateError::InvalidHeader)?
        .iter()
        .ne(EXPECTED_HEADERS)
    {
        return Err(VpnGateError::InvalidHeader);
    }

    let mut nodes = Vec::new();
    let mut stats = ParseStats::default();
    for record in reader.records() {
        let record = match record {
            Ok(record)
                if record.len() == 1
                    && record.get(0).is_some_and(|value| value.starts_with('*')) =>
            {
                break;
            }
            record => record,
        };
        if stats.rows_seen >= limits.max_rows {
            return Err(VpnGateError::TooManyRows);
        }
        stats.rows_seen += 1;
        let Ok(record) = record else {
            stats.rejected += 1;
            continue;
        };
        if record.len() != EXPECTED_HEADERS.len() {
            stats.rejected += 1;
            continue;
        }
        match parse_node(&record, limits.max_profile_bytes) {
            Ok(node) => {
                if node.availability != NodeAvailability::Available {
                    stats.unsupported += 1;
                }
                stats.accepted += 1;
                nodes.push(node);
            }
            Err(()) => stats.rejected += 1,
        }
    }
    Ok(ParsedSnapshot { nodes, stats })
}

fn parse_node(record: &csv::StringRecord, max_profile_bytes: usize) -> Result<VpnNode, ()> {
    let field = |index| record.get(index).ok_or(());
    let hostname = bounded_text(field(0)?, 255)?;
    let ip = field(1)?.parse::<Ipv4Addr>().map_err(|_| ())?;
    if !is_public_ipv4(ip) {
        return Err(());
    }
    let score = parse_bounded_u64(field(2)?, u64::MAX)?;
    let ping_ms = parse_optional_u32(field(3)?)?;
    let speed_bps = parse_bounded_u64(field(4)?, u64::MAX)?;
    let country_long = bounded_text(field(5)?, 128)?;
    let country_short = bounded_text(field(6)?, 8)?;
    let sessions =
        u32::try_from(parse_bounded_u64(field(7)?, u64::from(u32::MAX))?).map_err(|_| ())?;
    let uptime_ms = parse_bounded_u64(field(8)?, u64::MAX)?;
    let total_users = parse_bounded_u64(field(9)?, u64::MAX)?;
    let total_traffic_bytes = parse_bounded_u64(field(10)?, u64::MAX)?;
    let log_type = bounded_text(field(11)?, 64)?;
    let operator = bounded_text(field(12)?, 512)?;
    let message = bounded_text(field(13)?, 4096)?;
    let encoded_profile = field(14)?.trim();
    let max_encoded = max_profile_bytes.saturating_add(2) / 3 * 4 + 4;
    if encoded_profile.len() > max_encoded {
        return Err(());
    }
    let profile = Zeroizing::new(STANDARD.decode(encoded_profile).map_err(|_| ())?);
    if profile.len() > max_profile_bytes {
        return Err(());
    }

    let raw_digest = blake3::hash(&profile);
    let sanitized = sanitize_openvpn(&profile, ip);
    let (availability, openvpn, tcp_port, profile_digest) = match sanitized {
        Ok(profile) => (
            NodeAvailability::Available,
            Some(profile.clone()),
            NonZeroU16::new(profile.remote().port()),
            profile.digest(),
        ),
        Err(OpenVpnConfigError::UnsupportedProtocol) => (
            NodeAvailability::UnsupportedProtocol,
            None,
            None,
            raw_digest,
        ),
        Err(_) => (NodeAvailability::InvalidConfig, None, None, raw_digest),
    };
    let id = stable_node_id(&hostname, ip, tcp_port, profile_digest);

    Ok(VpnNode {
        id,
        hostname,
        ip,
        score,
        ping_ms,
        speed_bps,
        country_long,
        country_short,
        sessions,
        uptime_ms,
        total_users,
        total_traffic_bytes,
        log_type,
        operator,
        message,
        tcp_port,
        availability,
        openvpn,
    })
}

fn stable_node_id(
    hostname: &str,
    ip: Ipv4Addr,
    tcp_port: Option<NonZeroU16>,
    profile_digest: blake3::Hash,
) -> NodeId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(hostname.as_bytes());
    hasher.update(&[0]);
    hasher.update(&ip.octets());
    hasher.update(&tcp_port.map_or(0, NonZeroU16::get).to_be_bytes());
    hasher.update(profile_digest.as_bytes());
    NodeId::from_digest(hasher.finalize())
}

fn bounded_text(value: &str, maximum: usize) -> Result<String, ()> {
    let value = value.trim();
    if value.len() <= maximum && !value.as_bytes().contains(&0) {
        Ok(value.to_owned())
    } else {
        Err(())
    }
}

fn parse_bounded_u64(value: &str, maximum: u64) -> Result<u64, ()> {
    let parsed = value.trim().parse::<u64>().map_err(|_| ())?;
    if parsed <= maximum {
        Ok(parsed)
    } else {
        Err(())
    }
}

fn parse_optional_u32(value: &str) -> Result<Option<u32>, ()> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        Ok(None)
    } else {
        value.parse().map(Some).map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(protocol: &str) -> String {
        format!(
            "client\ndev tun\nproto {protocol}\nremote 1.2.3.4 443\n<ca>\nCA\n</ca>\n<cert>\nCERT\n</cert>\n<key>\nKEY\n</key>\n"
        )
    }

    fn row(hostname: &str, message: &str, profile: &str) -> String {
        let encoded = STANDARD.encode(profile);
        format!(
            "{hostname},1.2.3.4,100,20,1000000,Japan,JP,4,1000,30,4000,2weeks,operator,\"{message}\",{encoded}\n"
        )
    }

    fn document(rows: &str) -> String {
        format!("*vpn_servers\n#{}\n{rows}*\n", EXPECTED_HEADERS.join(","))
    }

    #[test]
    fn parses_markers_quotes_and_tcp_node() {
        let input = document(&row("vpn1", "hello, world", &profile("tcp")));
        let snapshot = parse_snapshot(input.as_bytes(), CsvLimits::default()).expect("valid CSV");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].message, "hello, world");
        assert_eq!(snapshot.nodes[0].availability, NodeAvailability::Available);
        assert_eq!(snapshot.stats.accepted, 1);
    }

    #[test]
    fn tolerates_bad_rows_and_marks_udp_unavailable() {
        let rows = format!("bad,row\n{}", row("vpn2", "udp", &profile("udp")));
        let snapshot = parse_snapshot(document(&rows).as_bytes(), CsvLimits::default())
            .expect("document remains parseable");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.stats.rejected, 1);
        assert_eq!(snapshot.stats.unsupported, 1);
    }

    #[test]
    fn node_id_is_stable() {
        let input = document(&row("vpn1", "same", &profile("tcp")));
        let first = parse_snapshot(input.as_bytes(), CsvLimits::default()).expect("valid CSV");
        let second = parse_snapshot(input.as_bytes(), CsvLimits::default()).expect("valid CSV");
        assert_eq!(first.nodes[0].id, second.nodes[0].id);
    }

    #[test]
    fn rejects_profiles_above_limit_before_decode() {
        let limits = CsvLimits {
            max_profile_bytes: 8,
            ..CsvLimits::default()
        };
        let input = document(&row("vpn1", "large", &profile("tcp")));
        let snapshot = parse_snapshot(input.as_bytes(), limits).expect("document is parseable");
        assert!(snapshot.nodes.is_empty());
        assert!(snapshot.stats.rejected >= 1);
    }

    #[test]
    fn enforces_the_row_limit() {
        let rows = format!(
            "{}{}",
            row("vpn1", "first", &profile("tcp")),
            row("vpn2", "second", &profile("tcp"))
        );
        let limits = CsvLimits {
            max_rows: 1,
            ..CsvLimits::default()
        };
        assert!(matches!(
            parse_snapshot(document(&rows).as_bytes(), limits),
            Err(VpnGateError::TooManyRows)
        ));
    }
}
