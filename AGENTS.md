# AGENTS.md

Guidance for coding agents (and humans) working in this crate.

## What this is
bitchat-console s'inscrit dans la continuité de bitchat-geohash (voir ../bitchat-geohash/AGENTS.md).

le but est ici de se connecter à un geohash, de selectionner tous les relais (fusion ios et android) associés au geohash et de se mettre à l'écoute des messages concernant ce geohash et ses 8 voisins.

On devra également pouvoir envoyer des messages, images (ou liens) sur les relais associés aux 9 geohash (kind 20000/20001).

## sécurité

- utilisation obligatoire de Tor pour anonymiser les IP (voir dans le repo BitChat les modules correspondants) lors de la connexion aux relais

- clé pub différente dans chaque geohash.

## autres fonctions

- possibilité de mettre en mute un intervenant non désiré (tous les messages de cet intervenant sont alors cachés).demute possible ou isolation de cet intervenant dans une fenètre d́edicacée.

- reprise des fonctions de gestion de groupe (création, invitation, destruction....) de bitchat v1.8

- possibilités d'envoie de DM (direct message) à un intervenant.

- Messages préenregistré à envoi sur demande ou planifié

## Technique

- adopter la meme approche CLI que bitchat-geohash.
- serveur web interne, affichage sur un navigateur.
- il devra etre possible de superposer l'affichage de console dans une fenetre au dessus de l'affichage geohash
- idem, la selection d'un geohash devra provoquer l'actualisation de la fenetre affichée

## Future
Sur la base du travail effectué pour Console,

- on devra pouvoir modulairement mettre en ouvre les différentes stratégies alternatives décrites dans le document ../bitchat-envelope/Documents/geohash-relays-requirements-and-alternatives.md

- a cette fin toutes les fonctions de sélection des relais devront être soigneusement isolées.

- on implémentra, outre la simulation  des comportements actuels ios/android (AS), les stratégies A0 à A6.
---

The sections below were added in lot 0 (28/10/2026) and follow `../bitchat-geohash/AGENTS.md`.
The spec is [SPECIFICATIONS.md](SPECIFICATIONS.md) (decisions D1–D14); progress and the
other decisions are in [PLAN_ET1.md](PLAN_ET1.md).

## Constraints

### Do NOT
- Modify `bitchat-envelope`, `nip90-dvm`, the workspace `Nostr/Cargo.toml` or `src/`
  (read-only); ask first if a change there looks necessary. `geohash-relays`,
  `bitchat-geohash`, `bitchat-launcher` and `bitchat-web` follow their own `AGENTS.md`.
- Connect to a relay without Tor (D1). A direct connection is allowed only to a loopback
  address, and only in tests.
- Publish anything on public relays or real channels from tests or by agents. Closed-loop
  tests use only the relay given by `BITCHAT_TEST_RELAY` (the user's own relay), with a
  throwaway seed. Real publishing is
  for the user's own actions in the console.
- Write received message contents, nicknames or pubkeys to logs, or to disk anywhere but
  `history.sqlite` (D31, `0600`), which replaces D11 and the memory-only rule of D16.
- Add dependencies outside D15 (`PLAN_ET1.md`) without discussion.
- Commit seeds, keys, `.env` or database files.

### DO
- Read files before changing them; follow existing naming.
- Keep `PLAN_ET1.md` up to date: boxes, dashboard, dated journal line, decisions.
- Run the commands below before each commit.
- Keep wire formats identical to the BitChat app (D17): kinds, tags, DM envelope, group
  packets. Check them against the Swift sources (`~/Developments_local/bitchat`, v1.8.0) with
  test vectors.

### Others
- Own git repository (`origin` = `git@github.com:wvmscs/bitchat-console.git`, branch `main`).
  The crate has its own `[workspace]`: it builds alone, with its own `Cargo.lock`.
- Relay selection lives only in `geohash-relays` (D2); nothing else in the console knows how
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

- Code, comments, rustdoc and logs in English; `SPECIFICATIONS.md` and `PLAN_ET1.md` in French.
- `thiserror` error type (`Error`); `tracing` for logs, never `println!` in library code.
