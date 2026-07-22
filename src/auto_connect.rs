//! Automatic node filtering and deterministic best-bandwidth selection.

use std::collections::{HashMap, HashSet};

use crate::domain::{
    AutoConnectConfig, IpTypeFilter, NodeAvailability, NodeId, ResidentialFilter, TestRecord,
    VpnNode,
};

/// Selects the highest-bandwidth usable node matching the configured region and IP traits.
#[must_use]
pub fn select_node(
    nodes: &[VpnNode],
    tests: &HashMap<NodeId, TestRecord>,
    config: &AutoConnectConfig,
    excluded: &HashSet<NodeId>,
) -> Option<NodeId> {
    nodes
        .iter()
        .filter(|node| node.availability == NodeAvailability::Available)
        .filter(|node| !excluded.contains(&node.id))
        .filter(|node| matches_region(node, config))
        .filter(|node| matches_ip_traits(tests.get(&node.id), config))
        .max_by(|left, right| {
            left.speed_bps
                .cmp(&right.speed_bps)
                .then_with(|| left.score.cmp(&right.score))
                .then_with(|| {
                    right
                        .ping_ms
                        .unwrap_or(u32::MAX)
                        .cmp(&left.ping_ms.unwrap_or(u32::MAX))
                })
                .then_with(|| right.id.cmp(&left.id))
        })
        .map(|node| node.id.clone())
}

fn matches_region(node: &VpnNode, config: &AutoConnectConfig) -> bool {
    config
        .region
        .as_deref()
        .is_none_or(|region| node.country_short.eq_ignore_ascii_case(region))
}

fn matches_ip_traits(record: Option<&TestRecord>, config: &AutoConnectConfig) -> bool {
    if config.ip_type == IpTypeFilter::Any && config.residential == ResidentialFilter::Any {
        return true;
    }
    let Some(result) = record.and_then(|record| record.result.as_ref()) else {
        return false;
    };
    let ip_type_matches = match config.ip_type {
        IpTypeFilter::Any => true,
        IpTypeFilter::Native => !result.is_broadcast,
        IpTypeFilter::Broadcast => result.is_broadcast,
    };
    let residential_matches = match config.residential {
        ResidentialFilter::Any => true,
        ResidentialFilter::Residential => result.is_residential,
        ResidentialFilter::NonResidential => !result.is_residential,
    };
    ip_type_matches && residential_matches
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, num::NonZeroU16};

    use chrono::Utc;

    use super::*;
    use crate::domain::IpPureResult;

    fn node(value: u8, region: &str, speed_bps: u64) -> VpnNode {
        VpnNode {
            id: format!("{value:064x}")
                .parse()
                .expect("generated node id is valid"),
            hostname: format!("vpn-{value}"),
            ip: Ipv4Addr::new(1, 1, 1, value),
            score: u64::from(value),
            ping_ms: Some(u32::from(value)),
            speed_bps,
            country_long: region.to_owned(),
            country_short: region.to_owned(),
            sessions: 1,
            uptime_ms: 1,
            total_users: 1,
            total_traffic_bytes: 1,
            log_type: String::new(),
            operator: String::new(),
            message: String::new(),
            tcp_port: NonZeroU16::new(443),
            availability: NodeAvailability::Available,
            openvpn: None,
        }
    }

    fn test_record(node_id: NodeId, is_broadcast: bool, is_residential: bool) -> TestRecord {
        TestRecord {
            node_id,
            result: Some(IpPureResult {
                fraud_score: 1.0,
                is_residential,
                is_broadcast,
                exit_ip: None,
            }),
            duration_ms: 1,
            tested_at: Utc::now(),
            error: None,
        }
    }

    #[test]
    fn chooses_the_highest_bandwidth_matching_node() {
        let nodes = vec![node(1, "JP", 10), node(2, "JP", 30), node(3, "US", 50)];
        let config = AutoConnectConfig {
            enabled: true,
            region: Some("JP".to_owned()),
            ..AutoConnectConfig::default()
        };

        assert_eq!(
            select_node(&nodes, &HashMap::new(), &config, &HashSet::new()),
            Some(nodes[1].id.clone())
        );
    }

    #[test]
    fn requires_successful_ippure_data_for_ip_filters() {
        let nodes = vec![node(1, "JP", 30), node(2, "JP", 20), node(3, "JP", 10)];
        let tests = HashMap::from([
            (
                nodes[0].id.clone(),
                test_record(nodes[0].id.clone(), true, false),
            ),
            (
                nodes[1].id.clone(),
                test_record(nodes[1].id.clone(), false, false),
            ),
        ]);
        let config = AutoConnectConfig {
            enabled: true,
            ip_type: IpTypeFilter::Native,
            residential: ResidentialFilter::NonResidential,
            ..AutoConnectConfig::default()
        };

        assert_eq!(
            select_node(&nodes, &tests, &config, &HashSet::new()),
            Some(nodes[1].id.clone())
        );
    }

    #[test]
    fn skips_temporarily_excluded_nodes() {
        let nodes = vec![node(1, "JP", 30), node(2, "JP", 20)];
        let excluded = HashSet::from([nodes[0].id.clone()]);

        assert_eq!(
            select_node(
                &nodes,
                &HashMap::new(),
                &AutoConnectConfig::default(),
                &excluded,
            ),
            Some(nodes[1].id.clone())
        );
    }
}
