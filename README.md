# bitchat-console

A BitChat client for a computer, for location channels (geohash): follow a cell and its 8
neighbours, write there, talk in private and in groups, from the browser, through Tor.

- **Reads and writes like the app.** For each cell, the relays the iOS and Android apps choose
  (both, from [geohash-relays](https://github.com/wvmscs/geohash-relays)); channel messages,
  presences, private messages and group messages in the app's formats, checked against the
  app's own Swift code.
- **Tor only.** Every relay, and the relay lists, are reached through Tor: Arti in the binary
  (`--features arti`) or a Tor daemon (`--tor-socks`). There is no clear-text fallback. Each
  cell has its own key, as in the app, and its own Tor circuit.
- **Teleported or on site.** At each change of area you say whether you are there: teleported
  messages are tagged as such and send no presence, like a teleported user of the app.
- **Private messages**, with delivery and read receipts, in the app's envelope.
- **Groups** in the format of BitChat 1.8, carried over Nostr between consoles (the app carries
  them over Bluetooth only).
- **Mute** someone, or isolate them in their own view; **saved** and **scheduled** messages.
- **Over the map**: with `--map`, a running [bitchat-geohash](https://github.com/wvmscs/bitchat-geohash)
  map shows with the console floating over it; a cell of the map can be followed in one click.

## Run

Tor comes from the binary (`--features arti`) or from a Tor daemon (`brew install tor`, then
`tor` or `brew services start tor`; its default SOCKS port, 127.0.0.1:9050, needs no setting).

```sh
cargo run --release --features arti -- u09tv --open
```

The page is served on `http://127.0.0.1:8090/`. Keys live in `~/.bitchat-console`
(`--data-dir`), readable by you only. Over the map:

```sh
bitchat-geohash u09t --listen 127.0.0.1:8080 &
cargo run --release --features arti -- u09tv --map http://127.0.0.1:8080 --open
```

Run the binary with a wrong option to list them all (`--teleport`, `--tor-socks`, `--apps`,
`--dm-relays`, `--no-presence`, `--pow-bits`, `--listen`, TLS…).

## Privacy

Received messages, nicknames and keys stay in memory; nothing about other people is written
to disk or to the logs, except the keys you mute. Images in messages are shown as links and
never loaded. With `--map`, the map tiles are loaded by the browser directly, not through Tor.

## Tests

```sh
cargo test                                   # offline: local relays, local SOCKS proxy
node --test tests/js/                        # page helpers
cargo test --release --features arti --test real_tor -- --ignored --nocapture   # real, through Tor
BITCHAT_TOR_SOCKS=127.0.0.1:9050 cargo test --release --test real_tor -- --ignored --nocapture  # same, Tor daemon
```

The real test publishes only on the relay named in it, with throwaway keys.

## Documents

[SPECIFICATIONS.md](SPECIFICATIONS.md) and [PLAN_ET1.md](PLAN_ET1.md) (in French): what it does,
the decisions, the measurements. [AGENTS.md](AGENTS.md): working rules. [CHANGELOG.md](CHANGELOG.md).

## Licence

GPL-3.0-only. The relay selection crate, geohash-relays, is public domain.
