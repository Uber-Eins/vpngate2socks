//! `SQLite` persistence for the most recent test of each node.

use std::{collections::HashMap, str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use sqlx::{ConnectOptions as _, Row as _, sqlite::SqlitePoolOptions};
use thiserror::Error;

use crate::domain::{
    AutoConnectConfig, IpPureResult, IpTypeFilter, NodeId, ResidentialFilter, TestRecord,
};

/// Cloneable asynchronous `SQLite` repository.
#[derive(Debug, Clone)]
pub struct Store {
    pool: sqlx::SqlitePool,
}

/// Persistence failure, including validation of values loaded from disk.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("database contains an invalid node id")]
    InvalidNodeId,
    #[error("database contains an invalid IP address")]
    InvalidIp,
    #[error("duration does not fit in SQLite INTEGER")]
    DurationOverflow,
    #[error("test record violates persistence invariants")]
    InvalidRecord,
    #[error("automatic connection configuration violates persistence invariants")]
    InvalidAutoConnectConfig,
}

/// Shared projection for both latest-test queries.
const LATEST_TEST_COLUMNS: &str = r"
    SELECT node_id, fraud_score, is_residential, is_broadcast, exit_ip,
           duration_ms, tested_at, error
    FROM node_tests
    ";

/// Rebuilds one record, validating every value loaded from disk.
fn record_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<TestRecord, StoreError> {
    let node_id = row
        .try_get::<String, _>("node_id")?
        .parse::<NodeId>()
        .map_err(|_| StoreError::InvalidNodeId)?;
    let fraud_score = row.try_get::<Option<f64>, _>("fraud_score")?;
    let is_residential = row.try_get::<Option<bool>, _>("is_residential")?;
    let is_broadcast = row.try_get::<Option<bool>, _>("is_broadcast")?;
    let exit_ip = row
        .try_get::<Option<String>, _>("exit_ip")?
        .map(|value| value.parse().map_err(|_| StoreError::InvalidIp))
        .transpose()?;
    let result = match (fraud_score, is_residential, is_broadcast) {
        (Some(fraud_score), Some(is_residential), Some(is_broadcast)) => {
            if !fraud_score.is_finite() || !(0.0..=100.0).contains(&fraud_score) {
                return Err(StoreError::InvalidRecord);
            }
            Some(IpPureResult {
                fraud_score,
                is_residential,
                is_broadcast,
                exit_ip,
            })
        }
        (None, None, None) if exit_ip.is_none() => None,
        _ => return Err(StoreError::InvalidRecord),
    };
    let duration_ms = u64::try_from(row.try_get::<i64, _>("duration_ms")?)
        .map_err(|_| StoreError::DurationOverflow)?;
    let error: Option<String> = row.try_get("error")?;
    if result.is_some() == error.is_some() {
        return Err(StoreError::InvalidRecord);
    }
    Ok(TestRecord {
        node_id,
        result,
        duration_ms,
        tested_at: row.try_get::<DateTime<Utc>, _>("tested_at")?,
        error,
    })
}

impl Store {
    /// Opens the database, enables WAL, and runs built-in migrations.
    pub async fn open(database_url: &str) -> Result<Self, StoreError> {
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .disable_statement_logging();
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Upserts the latest success or failure for a node.
    pub async fn save_test(&self, record: &TestRecord) -> Result<(), StoreError> {
        match (&record.result, &record.error) {
            (Some(result), None)
                if result.fraud_score.is_finite()
                    && (0.0..=100.0).contains(&result.fraud_score) => {}
            (None, Some(_)) => {}
            _ => return Err(StoreError::InvalidRecord),
        }
        let duration_ms =
            i64::try_from(record.duration_ms).map_err(|_| StoreError::DurationOverflow)?;
        let (fraud_score, is_residential, is_broadcast, exit_ip) =
            record
                .result
                .as_ref()
                .map_or((None, None, None, None), |result| {
                    (
                        Some(result.fraud_score),
                        Some(result.is_residential),
                        Some(result.is_broadcast),
                        result.exit_ip.map(|ip| ip.to_string()),
                    )
                });
        sqlx::query(
            r"
            INSERT INTO node_tests (
                node_id, fraud_score, is_residential, is_broadcast, exit_ip,
                duration_ms, tested_at, error
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(node_id) DO UPDATE SET
                fraud_score = excluded.fraud_score,
                is_residential = excluded.is_residential,
                is_broadcast = excluded.is_broadcast,
                exit_ip = excluded.exit_ip,
                duration_ms = excluded.duration_ms,
                tested_at = excluded.tested_at,
                error = excluded.error
            ",
        )
        .bind(record.node_id.as_str())
        .bind(fraud_score)
        .bind(is_residential)
        .bind(is_broadcast)
        .bind(exit_ip)
        .bind(duration_ms)
        .bind(record.tested_at)
        .bind(record.error.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Loads all latest results, keyed by validated node identifiers.
    pub async fn latest_tests(&self) -> Result<HashMap<NodeId, TestRecord>, StoreError> {
        let rows = sqlx::query(LATEST_TEST_COLUMNS).fetch_all(&self.pool).await?;

        let mut records = HashMap::with_capacity(rows.len());
        for row in rows {
            let record = record_from_row(&row)?;
            records.insert(record.node_id.clone(), record);
        }
        Ok(records)
    }

    /// Loads the latest result for a single node, if one was ever recorded.
    pub async fn latest_test(&self, node_id: &NodeId) -> Result<Option<TestRecord>, StoreError> {
        let row = sqlx::query(&format!("{LATEST_TEST_COLUMNS} WHERE node_id = ?"))
            .bind(node_id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(record_from_row).transpose()
    }

    /// Loads the singleton automatic connection policy.
    pub async fn auto_connect_config(&self) -> Result<AutoConnectConfig, StoreError> {
        let row = sqlx::query(
            r"
            SELECT enabled, region, ip_type, residential
            FROM auto_connect_config
            WHERE id = 1
            ",
        )
        .fetch_one(&self.pool)
        .await?;
        let ip_type = IpTypeFilter::from_stored(&row.try_get::<String, _>("ip_type")?)
            .ok_or(StoreError::InvalidAutoConnectConfig)?;
        let residential = ResidentialFilter::from_stored(&row.try_get::<String, _>("residential")?)
            .ok_or(StoreError::InvalidAutoConnectConfig)?;
        AutoConnectConfig {
            enabled: row.try_get("enabled")?,
            region: row.try_get("region")?,
            ip_type,
            residential,
        }
        .normalized()
        .map_err(|_| StoreError::InvalidAutoConnectConfig)
    }

    /// Atomically persists the singleton automatic connection policy.
    pub async fn save_auto_connect_config(
        &self,
        config: &AutoConnectConfig,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r"
            INSERT INTO auto_connect_config (
                id, enabled, region, ip_type, residential
            ) VALUES (1, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                enabled = excluded.enabled,
                region = excluded.region,
                ip_type = excluded.ip_type,
                residential = excluded.residential
            ",
        )
        .bind(config.enabled)
        .bind(config.region.as_deref())
        .bind(config.ip_type.as_str())
        .bind(config.residential.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Removes old results only when their nodes are absent from the current snapshot.
    pub async fn cleanup_stale(
        &self,
        current_nodes: &std::collections::HashSet<NodeId>,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let candidates = sqlx::query("SELECT node_id FROM node_tests WHERE tested_at < ?")
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await?;
        let stale = candidates
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("node_id").ok())
            .filter(|node_id| {
                node_id
                    .parse::<NodeId>()
                    .map_or(true, |node_id| !current_nodes.contains(&node_id))
            })
            .collect::<Vec<_>>();

        let mut transaction = self.pool.begin().await?;
        let mut removed = 0_u64;
        for node_id in stale {
            removed = removed.saturating_add(
                sqlx::query("DELETE FROM node_tests WHERE node_id = ?")
                    .bind(node_id)
                    .execute(&mut *transaction)
                    .await?
                    .rows_affected(),
            );
        }
        transaction.commit().await?;
        Ok(removed)
    }

    /// Verifies that `SQLite` can service a query.
    pub async fn health(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn node_id(value: u8) -> NodeId {
        format!("{value:064x}")
            .parse()
            .expect("generated node id is valid")
    }

    #[tokio::test]
    async fn upsert_keeps_only_latest_test() {
        let store = Store::open("sqlite::memory:")
            .await
            .expect("in-memory database opens");
        let first = TestRecord {
            node_id: node_id(1),
            result: None,
            duration_ms: 10,
            tested_at: Utc
                .timestamp_opt(1_700_000_000, 0)
                .single()
                .expect("valid time"),
            error: Some("timeout".to_owned()),
        };
        let second = TestRecord {
            node_id: node_id(1),
            result: Some(IpPureResult {
                fraud_score: 7.0,
                is_residential: true,
                is_broadcast: false,
                exit_ip: Some("1.1.1.1".parse().expect("valid IP")),
            }),
            duration_ms: 20,
            tested_at: Utc
                .timestamp_opt(1_700_000_001, 0)
                .single()
                .expect("valid time"),
            error: None,
        };
        store.save_test(&first).await.expect("first write succeeds");
        store
            .save_test(&second)
            .await
            .expect("second write succeeds");

        let records = store.latest_tests().await.expect("read succeeds");
        assert_eq!(records.len(), 1);
        assert_eq!(records.get(&node_id(1)), Some(&second));
    }

    #[tokio::test]
    async fn rejects_ambiguous_or_out_of_range_records() {
        let store = Store::open("sqlite::memory:")
            .await
            .expect("in-memory database opens");
        let record = TestRecord {
            node_id: node_id(2),
            result: Some(IpPureResult {
                fraud_score: 101.0,
                is_residential: false,
                is_broadcast: false,
                exit_ip: None,
            }),
            duration_ms: 1,
            tested_at: Utc::now(),
            error: None,
        };
        assert!(matches!(
            store.save_test(&record).await,
            Err(StoreError::InvalidRecord)
        ));
    }

    #[tokio::test]
    async fn automatic_connection_config_defaults_and_round_trips() {
        let store = Store::open("sqlite::memory:")
            .await
            .expect("in-memory database opens");
        assert_eq!(
            store
                .auto_connect_config()
                .await
                .expect("default configuration loads"),
            AutoConnectConfig::default()
        );

        let config = AutoConnectConfig {
            enabled: true,
            region: Some("JP".to_owned()),
            ip_type: IpTypeFilter::Native,
            residential: ResidentialFilter::Residential,
        };
        store
            .save_auto_connect_config(&config)
            .await
            .expect("configuration write succeeds");
        assert_eq!(
            store
                .auto_connect_config()
                .await
                .expect("saved configuration loads"),
            config
        );
    }
}
