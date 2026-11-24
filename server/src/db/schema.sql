-- Paperloom Cloud. Run at every start; each statement is safe to repeat.
-- Times are milliseconds since 1970.

CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    sub TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS devices (
    id UUID PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS devices_user ON devices (user_id);

CREATE TABLE IF NOT EXISTS libraries (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    owner_id BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- The cursor of the newest change. Pushes lock this row, so cursors
    -- are handed out in the order pushes commit.
    head BIGINT NOT NULL DEFAULT 0,
    snapshot_cursor BIGINT,
    snapshot_blob TEXT,
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS libraries_owner ON libraries (owner_id);

CREATE TABLE IF NOT EXISTS members (
    library_id UUID NOT NULL REFERENCES libraries (id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    PRIMARY KEY (library_id, user_id)
);
CREATE INDEX IF NOT EXISTS members_user ON members (user_id);

CREATE TABLE IF NOT EXISTS changes (
    library_id UUID NOT NULL REFERENCES libraries (id) ON DELETE CASCADE,
    log_cursor BIGINT NOT NULL,
    device_id UUID NOT NULL,
    seq BIGINT NOT NULL,
    payload BYTEA NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (library_id, log_cursor),
    UNIQUE (library_id, device_id, seq)
);

-- The newest seq from each device, kept after a snapshot drops changes.
CREATE TABLE IF NOT EXISTS device_seqs (
    library_id UUID NOT NULL REFERENCES libraries (id) ON DELETE CASCADE,
    device_id UUID NOT NULL,
    last_seq BIGINT NOT NULL,
    PRIMARY KEY (library_id, device_id)
);

CREATE TABLE IF NOT EXISTS blobs (
    library_id UUID NOT NULL REFERENCES libraries (id) ON DELETE CASCADE,
    hash TEXT NOT NULL,
    size BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (library_id, hash)
);
