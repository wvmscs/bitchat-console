# AGENTS.md

Guidance for coding agents (and humans) working in this crate.

## What this is

`bitchat-console`: a client of BitChat location channels (geohash) in the browser, served by
an internal web server. It joins a geohash, listens to its channel and its 8 neighbours on the
relays chosen by a strategy of [geohash-relays](https://github.com/wvmscs/geohash-relays)
(by default the iOS and Android apps' choice, merged), and lets the user send messages, direct
messages and group packets in the app's own formats. It can sit over the map of
[bitchat-geohash](https://github.com/wvmscs/bitchat-geohash) and follow its selection.

Essential rules: Tor for every relay connection; one key per geohash; relay selection only in
geohash-relays; wire formats identical to the app.

## Constraints

### Do NOT
- Modify another repository from here: `geohash-relays`, `geohash-relays-net`,
  `bitchat-geohash` and `bitchat-launcher` follow their own `AGENTS.md`. Rules that only
  concern the author's local checkout, if any, are in `AGENTS.local.md` (not committed).
- Connect to a relay without Tor. A direct connection is allowed only to a loopback
  address, and only in tests.
- Publish anything on public relays or real channels from tests or by agents. Closed-loop
  tests use only the relay given by `BITCHAT_TEST_RELAY` (the user's own relay), with a
  throwaway seed. Real publishing is
  for the user's own actions in the console.
- Write received message contents, nicknames or pubkeys to logs, or to disk anywhere but
  `history.sqlite` (`0600`).
- Add dependencies without discussion. Approved: those already in `Cargo.toml`.
- Commit seeds, keys, `.env` or database files.

### DO
- Read files before changing them; follow existing naming.
- Run the commands below before each commit.
- Keep wire formats identical to the BitChat app: kinds, tags, DM envelope, group
  packets. Check them against the Swift sources (a checkout of
  [permissionlesstech/bitchat](https://github.com/permissionlesstech/bitchat) at tag `v1.8.0`,
  `$BITCHAT_SRC` in `tests/vectors/`) with test vectors.

### Others
- Own git repository (`origin` = `git@github.com:wvmscs/bitchat-console.git`, branch `main`).
  The crate has its own `[workspace]`: it builds alone, with its own `Cargo.lock`.
- Relay selection lives only in `geohash-relays`; nothing else in the console knows how
  relays are chosen.
- Commits only when the user asks; pushing to `origin` is then authorized. Never force-push.

## Commands

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
reuse lint      # every file has its copyright and licence (also a GitHub Action)
```

<!-- REUSE-IgnoreStart -->
A new file starts with
`SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)` and
`SPDX-License-Identifier: GPL-3.0-or-later` in a comment, or is covered by `REUSE.toml`.
<!-- REUSE-IgnoreEnd -->

## Style

- Code, comments, rustdoc, logs and documentation in English.
- `thiserror` error type (`Error`); `tracing` for logs, never `println!` in library code.
