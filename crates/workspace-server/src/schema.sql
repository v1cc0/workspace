CREATE TABLE users (
    github_user_id INTEGER PRIMARY KEY CHECK (github_user_id > 0),
    login TEXT NOT NULL
);

CREATE TABLE login_flows (
    id TEXT PRIMARY KEY NOT NULL,
    challenge TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    state_hash TEXT UNIQUE,
    browser_hash TEXT,
    gateway_verifier TEXT,
    github_user_id INTEGER REFERENCES users(github_user_id),
    failed INTEGER NOT NULL DEFAULT 0,
    consumed_at INTEGER
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    github_user_id INTEGER NOT NULL REFERENCES users(github_user_id),
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_expiry ON sessions (expires_at);

PRAGMA user_version = 1;
