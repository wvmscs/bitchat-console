# Changelog

## [Unreleased]

### Added
- Keys (lot 3): device seed and Noise identity in private files (`Keys::load_or_create`, `0600`, insecure files refused), one Nostr key per cell derived as the app does (`Seed::cell`, checked against CryptoKit vectors), Noise fingerprint and peer ID (`NoiseIdentity`). `geohash-relays` is a git dependency.
- Tor connector (lot 2): `Connector` trait, `Socks5` client that isolates each cell on its own circuit (SOCKS user name, decision D20) and lets Tor resolve names, `Socks5::check` (no Tor, no console), `Loopback` for tests only (refuses any other host).
- Common crate `geohash-relays` (decision D2, lot 1): relay lists, the apps' choice, neighbours, `Strategy`; in its own repository.
- Specification (`SPECIFICATIONS.md`, decisions D1–D14) and plan (`PLAN_ET1.md`, lots 0–12,
  decisions D15–D18), after reading BitChat iOS 1.8.0 and the relay alternatives document.
- Crate skeleton: library and `bitchat-console` binary (not usable yet), `Error` type with
  `TorUnavailable`, own `[workspace]`, GPL-3.0-only; `AGENTS.md` completed with the working
  rules (lot 0).
