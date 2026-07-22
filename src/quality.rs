//! `IPPure` parsing and requests routed through an isolated worker.

use std::{net::Ipv4Addr, path::PathBuf, time::Duration};

use futures_util::StreamExt as _;
use serde_json::Value;
use thiserror::Error;

use crate::{domain::IpPureResult, socks::one_shot_bridge};

const MAX_IPPURE_BODY_BYTES: usize = 64 * 1024;

/// Failure while obtaining or validating `IPPure` risk data.
#[derive(Debug, Error)]
pub enum IpPureError {
    #[error("failed to create worker bridge")]
    Bridge(#[source] std::io::Error),
    #[error("failed to construct IPPure client")]
    Client(#[source] reqwest::Error),
    #[error("IPPure request failed")]
    Request(#[source] reqwest::Error),
    #[error("IPPure returned HTTP {0}")]
    Http(reqwest::StatusCode),
    #[error("IPPure response exceeds the configured limit")]
    BodyTooLarge,
    #[error("IPPure returned invalid JSON")]
    Json(#[source] serde_json::Error),
    #[error("IPPure response is missing or has invalid field {0}")]
    InvalidField(&'static str),
}

/// Calls `IPPure` through a temporary loopback-to-worker SOCKS bridge.
pub async fn fetch_ippure(
    endpoint: &url::Url,
    worker_socket: PathBuf,
    timeout: Duration,
) -> Result<IpPureResult, IpPureError> {
    let (bridge_address, bridge_task) = one_shot_bridge(worker_socket)
        .await
        .map_err(IpPureError::Bridge)?;
    let result = async {
        let proxy = reqwest::Proxy::all(format!("socks5h://{bridge_address}"))
            .map_err(IpPureError::Client)?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .proxy(proxy)
            .timeout(timeout)
            .build()
            .map_err(IpPureError::Client)?;
        let response = client
            .get(endpoint.clone())
            .send()
            .await
            .map_err(IpPureError::Request)?;
        if !response.status().is_success() {
            return Err(IpPureError::Http(response.status()));
        }
        let mut body = Vec::with_capacity(4096);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(IpPureError::Request)?;
            if body.len().saturating_add(chunk.len()) > MAX_IPPURE_BODY_BYTES {
                return Err(IpPureError::BodyTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        parse_ippure(&body)
    }
    .await;
    bridge_task.abort();
    result
}

/// Extracts required risk fields from either the root object or a `data` object.
pub fn parse_ippure(input: &[u8]) -> Result<IpPureResult, IpPureError> {
    let root: Value = serde_json::from_slice(input).map_err(IpPureError::Json)?;
    let object = root
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(&root);
    let fraud_score = field(object, &["fraudScore", "fraud_score"])
        .and_then(Value::as_f64)
        .filter(|score| score.is_finite() && (0.0..=100.0).contains(score))
        .ok_or(IpPureError::InvalidField("fraudScore"))?;
    let is_residential = field(object, &["isResidential", "is_residential"])
        .and_then(Value::as_bool)
        .ok_or(IpPureError::InvalidField("isResidential"))?;
    let is_broadcast = field(object, &["isBroadcast", "is_broadcast"])
        .and_then(Value::as_bool)
        .ok_or(IpPureError::InvalidField("isBroadcast"))?;
    let exit_ip = field(object, &["ip", "query", "address"])
        .and_then(Value::as_str)
        .map(|value| {
            value
                .parse::<Ipv4Addr>()
                .map_err(|_| IpPureError::InvalidField("ip"))
        })
        .transpose()?;

    Ok(IpPureResult {
        fraud_score,
        is_residential,
        is_broadcast,
        exit_ip,
    })
}

fn field<'a>(object: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| object.get(*name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_fields() {
        let parsed = parse_ippure(
            br#"{"fraudScore":12,"isResidential":true,"isBroadcast":false,"ip":"1.1.1.1"}"#,
        )
        .expect("valid payload");
        assert!((parsed.fraud_score - 12.0).abs() < f64::EPSILON);
        assert!(parsed.is_residential);
        assert!(!parsed.is_broadcast);
        assert_eq!(parsed.exit_ip, Some(Ipv4Addr::new(1, 1, 1, 1)));
    }

    #[test]
    fn rejects_each_missing_required_field() {
        for payload in [
            br#"{"isResidential":true,"isBroadcast":false}"#.as_slice(),
            br#"{"fraudScore":12,"isBroadcast":false}"#.as_slice(),
            br#"{"fraudScore":12,"isResidential":true}"#.as_slice(),
        ] {
            assert!(parse_ippure(payload).is_err());
        }
    }

    #[test]
    fn accepts_nested_data_and_snake_case() {
        let parsed = parse_ippure(
            br#"{"data":{"fraud_score":1.5,"is_residential":false,"is_broadcast":true}}"#,
        )
        .expect("valid nested payload");
        assert!((parsed.fraud_score - 1.5).abs() < f64::EPSILON);
        assert!(parsed.is_broadcast);
    }
}
