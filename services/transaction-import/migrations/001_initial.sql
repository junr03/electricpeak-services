CREATE SCHEMA IF NOT EXISTS transaction_import;

CREATE TABLE IF NOT EXISTS transaction_import.imports (
    sha256 BYTEA PRIMARY KEY CHECK (octet_length(sha256) = 32),
    source_file TEXT NOT NULL,
    row_count BIGINT NOT NULL CHECK (row_count >= 0),
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS transaction_import.transactions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    import_sha256 BYTEA NOT NULL REFERENCES transaction_import.imports (sha256),
    row_number BIGINT NOT NULL CHECK (row_number > 0),
    date DATE NOT NULL,
    name TEXT NOT NULL,
    amount NUMERIC NOT NULL,
    status TEXT NOT NULL,
    category TEXT,
    parent_category TEXT,
    excluded BOOLEAN NOT NULL,
    tags TEXT,
    transaction_type TEXT NOT NULL,
    account TEXT NOT NULL,
    account_mask TEXT,
    note TEXT,
    recurring TEXT,
    UNIQUE (import_sha256, row_number)
);

CREATE INDEX IF NOT EXISTS transactions_date_idx
    ON transaction_import.transactions (date);
