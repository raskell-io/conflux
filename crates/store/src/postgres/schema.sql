-- PostgreSQL schema for Conflux store
-- Run this to initialize the database or use PostgresStore::create_tables()

CREATE TABLE IF NOT EXISTS operations (
    id UUID PRIMARY KEY,
    document_id TEXT NOT NULL,
    hlc_timestamp TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    actor_class TEXT NOT NULL,
    op_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    payload JSONB NOT NULL,
    intent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_operations_doc_hlc
    ON operations (document_id, hlc_timestamp);
CREATE INDEX IF NOT EXISTS idx_operations_entity
    ON operations (entity_id);
CREATE INDEX IF NOT EXISTS idx_operations_actor
    ON operations (actor_id);
CREATE INDEX IF NOT EXISTS idx_operations_doc_created
    ON operations (document_id, created_at DESC);

CREATE TABLE IF NOT EXISTS snapshots (
    id UUID PRIMARY KEY,
    document_id TEXT NOT NULL,
    hlc_timestamp TEXT NOT NULL,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_snapshots_doc_hlc
    ON snapshots (document_id, hlc_timestamp DESC);

CREATE TABLE IF NOT EXISTS milestones (
    id UUID PRIMARY KEY,
    document_id TEXT NOT NULL,
    git_commit TEXT,
    hlc_range_start TEXT NOT NULL,
    hlc_range_end TEXT NOT NULL,
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_milestones_doc_created
    ON milestones (document_id, created_at DESC);

CREATE TABLE IF NOT EXISTS version_vectors (
    document_id TEXT PRIMARY KEY,
    data JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
