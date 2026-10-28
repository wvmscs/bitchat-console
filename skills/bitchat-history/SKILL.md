---
name: bitchat-history
description: Read, search, moderate, summarise, back up or purge the message history of bitchat-console (BitChat location channels, private and group messages) kept in history.sqlite. Use when asked about what was said in a BitChat cell, to analyse or summarise conversations, to hide or redact messages, or to back up, rotate or wipe the history.
---

# bitchat-history

bitchat-console keeps every message it sees or sends in `history.sqlite`:
channel messages of the 9 cells it follows (kind 20000), private messages
(DMs) and group messages, received and sent, muted senders included. This
skill works on that file, locally, with the `bitchat-history` tool or the
console's HTTP API.

## Rules

- **Local only.** The file holds other people's messages. Never copy it,
  nor what you read in it, off this machine (no upload, no cloud service, no
  paste into a remote tool). Summaries stay local too unless the user says
  otherwise.
- **Hide or redact with `delete` / `update`, not by hand.** `delete` removes
  the content and keeps the row (`deleted_at` set): the console stops showing
  the message within 2 s, and relays sending it again cannot bring it back.
  `update` changes the content: the console shows the new text. Both are
  what the user sees in the page.
- **`remove` and `purge` are archive work.** They delete rows outright; the
  console's live feed keeps what it already shows. Use them to shrink the
  base, after a backup.
- **Do not touch `meta.since`** except through `rotate`: the console writes
  nothing older than it.
- **Do not change file permissions**: the base, its `-wal` and `-shm` are
  `0600` and must stay so.
- **Ask before** `wipe`, `remove`, `purge`, and before deleting or editing
  more than a handful of messages.
- Keys are pseudonyms: a sender's key is stable in one cell (it is derived
  from their device and the cell) but differs from cell to cell. Do not try
  to link keys across cells.

## Where

- Data directory: `~/.bitchat-console` (the console's `--data-dir`).
- Base: `<data-dir>/history.sqlite`. Backups: `<data-dir>/backups/`.
- The console may be running or not: both tools work either way (SQLite WAL,
  60 s busy timeout).

## The tool

```sh
bitchat-history [--data-dir <dir>] [--file <history.sqlite>] <command>
```

| Command | What it does |
|---|---|
| `query [filters]` | rows as JSON, one per line, newest first (200 by default, at most 5000) |
| `get <id>` | one row |
| `insert` | rows as JSON on stdin, one per line (puts back archived rows; once per message) |
| `update <id> <content>` | new content |
| `delete <id>...` | content removed, row kept (hidden in the console) |
| `remove <id>...` | rows removed outright |
| `purge --before <time>` | rows older than that, removed outright |
| `backup [<file>]` | consistent copy (`VACUUM INTO`), `0600` |
| `rotate [--keep 2h] [--dir <dir>]` | daily: backup, keep the last 2 h, start the base there, compact |
| `wipe` | delete the base (the console makes a new, empty one) |
| `cron [--at 04:17]` | prints the crontab line for the daily rotation |

Filters of `query`: `--kind channel|dm|group`, `--cell <geohash or prefix>`
(`u09t` matches every cell inside), `--group <id>`, `--key <hex>` (sender or
recipient), `--text <text>` (content or nickname, any case), `--from <time>`,
`--to <time>`, `--deleted`, `--order new|old|sender`, `--limit`, `--offset`.

`<time>`: Unix seconds, or an age: `90s`, `30m`, `2h`, `7d`.

Exit code 0 on success, 1 when the command fails, 2 for a wrong command line.

### Recipes

```sh
# What was said in u09tv in the last hour, oldest first
bitchat-history query --cell u09tv --from 1h --order old | jq -r '"\(.nickname // "anon")#\(.sender[-4:]): \(.content)"'

# Everything one key wrote today
bitchat-history query --key <hex> --from 1d

# Hide a message, redact another
bitchat-history delete 42
bitchat-history update 43 "[removed by moderation]"

# Daily rotation, from cron (prints the line to add with crontab -e)
bitchat-history cron
```

## The row (`message` table)

| Column | Meaning |
|---|---|
| `id` | row number (what `get`, `update`, `delete` take) |
| `kind` | `channel`, `dm`, `group` |
| `msg_id` | channel: Nostr event ID; dm: BitChat message ID (UUID); group: message ID |
| `own` | 1: sent by this console (the user) |
| `geohash` | channel: the cell; dm: the cell whose key the console uses with the other side |
| `group_id` | group messages |
| `sender`, `recipient` | hex keys (Nostr x-only; Ed25519 in groups); `recipient` for DMs only |
| `nickname` | as sent (`n` tag, group member); DMs carry none |
| `teleport` | channel: the sender says they are not in the cell |
| `content` | the text; NULL once deleted |
| `created_at`, `received_at` | Unix time of the event, of its arrival |
| `status` | DM sent: `sent`, `relayed`, `delivered`, `read`; DM received: `read` once read |
| `deleted_at` | set by `delete` |

Display names as the BitChat app shows them: `<nickname>#<last 4 hex digits of
the key>`, `anon#<4 digits>` without a nickname.

Other tables: `meta` (`since`: start of the base), `change` (filled by a
trigger when `content` or `deleted_at` changes; the console empties it).

## The console's HTTP API

When the console runs (default `http://127.0.0.1:8090`; through
bitchat-launcher, the page's address). Commands need the page's `Origin`
header.

- `GET /api/history?kind=&cell=&group=&key=&text=&from=&to=&deleted=true&order=&limit=&offset=`
  → `{"records": [...]}` (times in Unix seconds only)
- `GET /api/history/{id}`
- `POST /api/history/insert` (a row as JSON) → `{"id": n | null}`
- `POST /api/history/update {"id", "content"}`
- `POST /api/history/delete {"ids": [..]}` → `{"deleted": n}`
- `409 no_history` when the console runs with `--no-history`.

The page `history?cell=<geohash>` shows the same, with search, sort,
selection and deletion.
