-- bitchat-console local storage (lot 8, D16). Only what the user decides:
-- muted keys (no nickname, no content), saved messages, schedules.
CREATE TABLE mute (
    pubkey   TEXT PRIMARY KEY,
    muted_at INTEGER NOT NULL
);
CREATE TABLE preset (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    text     TEXT NOT NULL,
    -- The cell to write in; NULL: the centre at send time.
    cell     TEXT,
    nickname TEXT
);
CREATE TABLE schedule (
    id           INTEGER PRIMARY KEY,
    text         TEXT NOT NULL,
    cell         TEXT,
    nickname     TEXT,
    -- Next send (Unix time); NULL once a one-shot schedule is done.
    next_at      INTEGER,
    -- Seconds between sends; NULL: once.
    repeat_secs  INTEGER,
    last_result  TEXT
);
