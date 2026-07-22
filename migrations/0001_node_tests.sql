CREATE TABLE IF NOT EXISTS node_tests (
    node_id TEXT PRIMARY KEY NOT NULL,
    fraud_score REAL CHECK (fraud_score IS NULL OR fraud_score BETWEEN 0 AND 100),
    is_residential INTEGER CHECK (is_residential IS NULL OR is_residential IN (0, 1)),
    is_broadcast INTEGER CHECK (is_broadcast IS NULL OR is_broadcast IN (0, 1)),
    exit_ip TEXT,
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    tested_at TEXT NOT NULL,
    error TEXT,
    CHECK (
        (fraud_score IS NOT NULL AND is_residential IS NOT NULL
            AND is_broadcast IS NOT NULL AND error IS NULL)
        OR
        (fraud_score IS NULL AND is_residential IS NULL
            AND is_broadcast IS NULL AND exit_ip IS NULL AND error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_node_tests_tested_at
    ON node_tests (tested_at);
