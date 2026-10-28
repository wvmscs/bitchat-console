-- SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Changes of content after writing (lot 16): by the page, the API,
-- bitchat-history or any SQLite client. The console takes them into its feed
-- within seconds, then empties this table. Rows removed outright (purge,
-- rotation) are not noted: the feed keeps them.
CREATE TABLE change (
    id     INTEGER PRIMARY KEY,
    kind   TEXT NOT NULL,
    msg_id TEXT NOT NULL
);
CREATE TRIGGER message_changed AFTER UPDATE OF content, deleted_at ON message
BEGIN
    INSERT INTO change (kind, msg_id) VALUES (NEW.kind, NEW.msg_id);
END;
