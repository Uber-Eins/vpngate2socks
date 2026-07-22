//! Selection policy for automatic `IPPure` tests.

use std::collections::HashSet;

use crate::domain::{NodeAvailability, NodeId, VpnNode};

/// Minimal node data needed by the automatic-test policy.
#[derive(Debug, Clone)]
pub(crate) struct AutomaticTestCandidate {
    node_id: NodeId,
    score: u64,
    ping_ms: Option<u32>,
    eligible: bool,
}

impl From<&VpnNode> for AutomaticTestCandidate {
    fn from(node: &VpnNode) -> Self {
        Self {
            node_id: node.id.clone(),
            score: node.score,
            ping_ms: node.ping_ms,
            eligible: node.availability == NodeAvailability::Available && node.openvpn.is_some(),
        }
    }
}

/// Chooses untested nodes up to the queue's currently available capacity.
pub(crate) fn select_automatic_tests(
    candidates: impl IntoIterator<Item = AutomaticTestCandidate>,
    tested: &HashSet<NodeId>,
    known: &HashSet<NodeId>,
    limit: usize,
) -> Vec<NodeId> {
    let mut candidates = candidates
        .into_iter()
        .filter(|candidate| {
            candidate.eligible
                && !tested.contains(&candidate.node_id)
                && !known.contains(&candidate.node_id)
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| match (left.ping_ms, right.ping_ms) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.node_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_id(value: char) -> NodeId {
        value
            .to_string()
            .repeat(64)
            .parse()
            .expect("test node id is valid")
    }

    fn candidate(value: char, score: u64, ping_ms: Option<u32>) -> AutomaticTestCandidate {
        AutomaticTestCandidate {
            node_id: node_id(value),
            score,
            ping_ms,
            eligible: true,
        }
    }

    #[test]
    fn selects_only_missing_eligible_nodes_in_quality_order() {
        let tested = HashSet::from([node_id('a')]);
        let known = HashSet::from([node_id('b')]);
        let mut unavailable = candidate('c', 10_000, Some(1));
        unavailable.eligible = false;

        let selected = select_automatic_tests(
            [
                candidate('a', 9_000, Some(10)),
                candidate('b', 8_000, Some(12)),
                unavailable,
                candidate('d', 4_000, Some(30)),
                candidate('e', 5_000, None),
                candidate('f', 5_000, Some(40)),
            ],
            &tested,
            &known,
            2,
        );

        assert_eq!(selected, [node_id('f'), node_id('e')]);
    }

    #[test]
    fn respects_zero_queue_capacity() {
        let selected = select_automatic_tests(
            [candidate('a', 1, Some(1))],
            &HashSet::new(),
            &HashSet::new(),
            0,
        );

        assert!(selected.is_empty());
    }
}
