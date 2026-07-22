//! In-memory lifecycle and deduplication for `IPPure` test operations.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::domain::{NodeId, OperationId, TestState};

/// Result of atomically registering one queued node test.
pub(crate) enum QueueRegistration {
    Existing(OperationId),
    New {
        operation_id: OperationId,
        state: TestState,
    },
}

/// Keeps API-visible operations and the one-in-flight-test-per-node invariant together.
#[derive(Default)]
pub(crate) struct TestRegistry {
    operations: HashMap<OperationId, TestState>,
    pending_by_node: HashMap<NodeId, OperationId>,
    observed_nodes: HashSet<NodeId>,
}

impl TestRegistry {
    pub(crate) fn queue(&mut self, node_id: NodeId, queued_at: DateTime<Utc>) -> QueueRegistration {
        if let Some(operation_id) = self.pending_by_node.get(&node_id) {
            return QueueRegistration::Existing(*operation_id);
        }
        let operation_id = OperationId::new();
        let state = TestState::Queued {
            node_id: node_id.clone(),
            queued_at,
        };
        self.pending_by_node.insert(node_id, operation_id);
        self.operations.insert(operation_id, state.clone());
        QueueRegistration::New {
            operation_id,
            state,
        }
    }

    pub(crate) fn rollback_queued(&mut self, node_id: &NodeId, operation_id: OperationId) {
        if self.pending_by_node.get(node_id) == Some(&operation_id) {
            self.pending_by_node.remove(node_id);
        }
        self.operations.remove(&operation_id);
    }

    pub(crate) fn state(&self, operation_id: OperationId) -> Option<TestState> {
        self.operations.get(&operation_id).cloned()
    }

    pub(crate) fn mark_running(
        &mut self,
        operation_id: OperationId,
        node_id: NodeId,
        started_at: DateTime<Utc>,
    ) -> TestState {
        let state = TestState::Running {
            node_id,
            started_at,
        };
        self.operations.insert(operation_id, state.clone());
        state
    }

    pub(crate) fn complete(
        &mut self,
        operation_id: OperationId,
        node_id: NodeId,
        state: TestState,
        history_limit: usize,
    ) {
        if self.pending_by_node.get(&node_id) == Some(&operation_id) {
            self.pending_by_node.remove(&node_id);
        }
        self.observed_nodes.insert(node_id);
        self.operations.insert(operation_id, state);
        self.prune_completed(history_limit);
    }

    pub(crate) fn known_nodes(&self) -> HashSet<NodeId> {
        self.pending_by_node
            .keys()
            .chain(self.observed_nodes.iter())
            .cloned()
            .collect()
    }

    pub(crate) fn retain_observed(&mut self, current_nodes: &HashSet<NodeId>) {
        self.observed_nodes
            .retain(|node_id| current_nodes.contains(node_id));
    }

    fn prune_completed(&mut self, history_limit: usize) {
        let mut completed = self
            .operations
            .iter()
            .filter_map(|(operation_id, state)| match state {
                TestState::Succeeded { record, .. } | TestState::Failed { record, .. } => {
                    Some((*operation_id, record.tested_at))
                }
                TestState::Queued { .. } | TestState::Running { .. } => None,
            })
            .collect::<Vec<_>>();
        let excess = completed.len().saturating_sub(history_limit);
        if excess == 0 {
            return;
        }
        completed.sort_unstable_by_key(|(_, tested_at)| *tested_at);
        for (operation_id, _) in completed.into_iter().take(excess) {
            self.operations.remove(&operation_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use crate::domain::{TestRecord, TestState};

    use super::*;

    fn node_id(value: char) -> NodeId {
        value
            .to_string()
            .repeat(64)
            .parse()
            .expect("test node id is valid")
    }

    #[test]
    fn returns_the_existing_operation_for_duplicate_in_flight_tests() {
        let mut registry = TestRegistry::default();
        let node_id = node_id('a');
        let first = registry.queue(node_id.clone(), Utc::now());
        let QueueRegistration::New { operation_id, .. } = first else {
            panic!("first registration must be new");
        };

        let duplicate = registry.queue(node_id, Utc::now());

        assert!(matches!(duplicate, QueueRegistration::Existing(id) if id == operation_id));
    }

    #[test]
    fn completed_nodes_are_known_but_can_be_manually_queued_again() {
        let mut registry = TestRegistry::default();
        let node_id = node_id('b');
        let QueueRegistration::New { operation_id, .. } =
            registry.queue(node_id.clone(), Utc::now())
        else {
            panic!("first registration must be new");
        };
        let record = TestRecord {
            node_id: node_id.clone(),
            result: None,
            duration_ms: 12,
            tested_at: Utc
                .timestamp_opt(1_700_000_000, 0)
                .single()
                .expect("timestamp is valid"),
            error: Some("test failure".to_owned()),
        };
        registry.complete(
            operation_id,
            node_id.clone(),
            TestState::Failed {
                node_id: node_id.clone(),
                record,
            },
            10,
        );

        assert!(registry.known_nodes().contains(&node_id));
        assert!(matches!(
            registry.queue(node_id, Utc::now()),
            QueueRegistration::New { .. }
        ));
    }
}
