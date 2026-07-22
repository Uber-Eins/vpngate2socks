CREATE TABLE IF NOT EXISTS auto_connect_config (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    region TEXT CHECK (region IS NULL OR (length(region) BETWEEN 1 AND 8)),
    ip_type TEXT NOT NULL CHECK (ip_type IN ('any', 'native', 'broadcast')),
    residential TEXT NOT NULL CHECK (
        residential IN ('any', 'residential', 'nonResidential')
    )
);

INSERT OR IGNORE INTO auto_connect_config (
    id, enabled, region, ip_type, residential
) VALUES (1, 0, NULL, 'any', 'any');
