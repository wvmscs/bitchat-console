# bitchat-console : plan de l'étape 1

Plan de réalisation de [SPECIFICATIONS.md](SPECIFICATIONS.md). Créé le 28/10/2026.

## Tableau de bord

| Lot | Contenu | État |
|---|---|---|
| 0 | Mise en place : dépôts, crates, dépendances, `AGENTS.md` | ✅ |
| 1 | Crate commune `geohash-relays` : listes, règles iOS et Android, voisins, interface de stratégie | ⬜ |
| 2 | Connecteur Tor (SOCKS5, isolation par geohash), refus du clair | ⬜ |
| 3 | Identités : graine, clé par geohash, identité Noise | ⬜ |
| 4 | Lecture : 9 cellules, connexions, abonnements, fil, changement de geohash à chaud | ⬜ |
| 5 | Écriture : messages, preuve de travail, présences, marque `teleport` | ⬜ |
| 6 | Messages privés (enveloppe BitChat `v2:`, relais par défaut) | ⬜ |
| 7 | Serveur web et page console | ⬜ |
| 8 | Masquage, isolation d'un intervenant, messages préenregistrés et planifiés | ⬜ |
| 9 | Superposition à la carte de bitchat-geohash | ⬜ |
| 10 | Groupes mixtes (transport Nostr, entre consoles) | ⬜ |
| 11 | Feature `arti` | ⬜ |
| 12 | Validation réelle | ⬜ |

Ordre : 0 → 1 → 2 → 3 ; 4 dépend de 1, 2 et 3 ; 5 et 6 dépendent de 4 ; 7 dépend de 4 ;
8 dépend de 5 et 7 ; 9 dépend de 7 ; 10 dépend de 6 ; 11 dépend de 2 ; 12 clôt l'étape.

## Lots

### Lot 0 : mise en place ✅

- [x] Dépôts git `bitchat-console` (GPL-3.0-only) et `geohash-relays` (**Unlicense**, D18)
      sur GitHub `wvmscs`, privés, chacun avec son propre `[workspace]` : le `Cargo.toml` de
      `Nostr/` reste intact.
- [x] Squelettes des deux crates, type d'erreur, journalisation (`tracing`), CI locale
      (`fmt`, `clippy -D warnings`, tests). Les dépendances de D15 arrivent avec les lots qui
      s'en servent.
- [x] Dépendances approuvées (D15).
- [x] `AGENTS.md` de la console complété (texte de l'utilisateur gardé, règles ajoutées à la
      suite) sur le modèle de bitchat-geohash : contraintes, invariants (Tor, pas de données
      personnelles hors mémoire, publication réelle seulement sur action de l'utilisateur),
      commandes ; `AGENTS.md` de `geohash-relays`.

### Lot 1 : crate `geohash-relays` ⬜

- [ ] Lecture des listes iOS et Android (règles de chaque app, corrections du lot 11 bis de
      bitchat-geohash), conflit d'hôte iOS (D18 de bitchat-geohash).
- [ ] Stratégie `AS` : sélection de chaque app et union par serveur, derrière un trait
      `Strategy` (place prévue pour A0 à A6).
- [ ] Voisins d'un geohash (8, moins aux bords).
- [ ] Tests sur les listes figées de bitchat-geohash (mêmes résultats sur `u09tv`, `u0`,
      `9q8yy`, `gcpvj`, `u1`, `gb`, `9q`).
- [ ] Documentation pensée pour une PR future vers les apps : règle de chaque app, sources.

### Lot 2 : connecteur Tor ⬜

- [ ] Trait `Connector` : ouvre un flux vers `host:port` pour une cellule donnée.
- [ ] Client SOCKS5 (méthode « utilisateur/mot de passe » pour l'isolation : un identifiant
      par geohash, `IsolateSOCKSAuth`) ; résolution DNS par Tor (`ATYP` domaine).
- [ ] Au démarrage : `tor` absent ou injoignable → arrêt avec message clair, jamais de
      connexion directe. Connexion directe seulement vers une adresse de bouclage, et
      seulement dans les tests.
- [ ] Tests : serveur SOCKS5 de test local (isolation vérifiée par les identifiants reçus).

### Lot 3 : identités ⬜

- [ ] Graine de 32 octets, fichier `0600` dans le dossier de données (refus d'un fichier
      lisible par d'autres).
- [ ] Clé par geohash : `HMAC-SHA256(graine, geohash ‖ compteur big-endian)`, compteur 0 à 9
      jusqu'à une clé secp256k1 valide (`NostrIdentityBridge.deriveIdentity`). Vecteurs de
      test ; vérification croisée avec le code Swift.
- [ ] Identité Noise de la console (clé statique X25519, clé de signature Ed25519, empreinte)
      pour les groupes (lot 10).

### Lot 4 : lecture ⬜

- [ ] 9 cellules → relais (lot 1) → plan d'écoute (une connexion par relais via le connecteur,
      abonnements groupés), filtre `kinds:[20000,20001]`, `#g`, `since: −1 h`, `limit: 200`.
- [ ] Vérification des signatures, déduplication par id, `AUTH` et `CLOSED` lus et affichés.
- [ ] Fil : cellule, pseudo (`n`), suffixe de clé, marque `teleport` ; participants par
      cellule.
- [ ] Changement de geohash à chaud : seuls les relais et abonnements qui changent bougent.
- [ ] Tests avec un relais de test local.

### Lot 5 : écriture ⬜

- [ ] Message kind 20000 signé avec la clé de la cellule choisie : tags `g`, `n`, `t=teleport`
      selon le choix fait au changement de geohash (D5), `nonce` NIP-13 (8 bits, D8).
- [ ] Envoi aux relais de la cellule (union, D4), `OK` de chaque relais affiché.
- [ ] Présences kind 20001 toutes les 40 à 80 s aux niveaux 2, 4 et 5 de la cellule (D5).
- [ ] Boucle fermée sur `relay.famillevital.club` avec une graine jetable.

### Lot 6 : messages privés ⬜

- [ ] Enveloppe BitChat : ECDH secp256k1, HKDF-SHA256, XChaCha20-Poly1305,
      `v2:` + base64url(nonce ‖ chiffré ‖ tag), kinds 14 → 13 → 1059, clé jetable pour
      l'enveloppe externe ; charge `bitchat1:` (`NostrEmbeddedBitChat`) avec accusés de
      réception et de lecture.
- [ ] Envoi depuis la clé de la cellule vers la clé du correspondant, sur les relais par
      défaut ; abonnement `kinds:[1059]`, `#p:[clés de la console]`.
- [ ] Vecteurs de test tirés du code Swift ; essai avec l'app de l'utilisateur.

### Lot 7 : serveur web et page ⬜

- [ ] Serveur axum sur le modèle de bitchat-geohash (Host, Origin, CSP, SSE, HTTPS optionnel).
- [ ] Page : fil des 9 cellules (ou d'une seule), saisie, choix de la cellule d'envoi,
      pseudo, état des relais et de Tor, DM.
- [ ] Données personnelles (pseudos, contenus) affichées mais jamais écrites sur disque ni
      dans les journaux (D11).

### Lot 8 : masquage et messages préenregistrés ⬜

- [ ] Mute, démasquage, fenêtre dédiée à un intervenant ; liste locale.
- [ ] Messages préenregistrés : envoi sur demande ; planification (date, répétition) tant que
      la console tourne. Stockage local (D16).

### Lot 9 : superposition à la carte ⬜

- [ ] Dépendance git sur bitchat-geohash `v0.1.0` : moniteur et carte dans le même binaire.
- [ ] Console en panneau flottant au-dessus de la carte ; un clic sur une cellule choisit le
      geohash et met la console à jour (D10).

### Lot 10 : groupes mixtes ⬜

- [ ] Format des groupes de BitChat 1.8 (`GroupProtocol.swift`, `GroupStore.swift`) : état
      signé par le créateur (Ed25519), clé de groupe, messages ChaCha20-Poly1305.
- [ ] Transport Nostr : paquets de groupe en `bitchat1:` dans l'enveloppe privée, une par
      membre.
- [ ] Création, invitation, départ, exclusion et rotation de clé, destruction, entre deux
      consoles.

### Lot 11 : feature `arti` ⬜

- [ ] Connecteur Arti (`arti-client`) en feature de compilation, isolation par geohash,
      cache des annuaires dans le dossier de données.

### Lot 12 : validation réelle ⬜

- [ ] Lecture et écriture réelles via Tor, DM avec l'app de l'utilisateur, groupes entre
      deux consoles, superposition à la carte, mesures dans le journal.

## Décisions

Décisions de la spec : D1, D2, D5, D6, D10, D12 ✅ ; les autres, ci-dessous et dans la spec.

| # | Question | Proposition | État |
|---|---|---|---|
| D15 | Dépendances | Comme bitchat-geohash (`tokio`, `tokio-tungstenite`, `k256`, `sha2`, `serde`, `axum`, `rusqlite`…) plus `hmac`, `hkdf`, `chacha20poly1305` (XChaCha20 et ChaCha20), `ed25519-dalek`, `x25519-dalek`, `rand` ; `arti-client` en feature (lot 11) | ✅ |
| D16 | Stockage local | Messages reçus et émis : tampon en mémoire par cellule, borné (nombre et durée), perdu à l'arrêt ; historique local en option (SQLite, conservation limitée, purge) ; mutes, préenregistrés, groupes : SQLite. Interface `MessageStore` isolée (Redis branchable plus tard si plusieurs processus doivent partager le fil). Redis écarté pour l'étape 1 : démon de plus, persistance disque par défaut contraire à D11, lisible par tout processus local | ✅ |
| D18 | Licence de `geohash-relays` | Unlicense (domaine public), comme l'app iOS ; compatible avec la GPL-3.0 de l'app Android, donc proposable aux deux | ✅ |
| D17 | Neutralité de la console | Hors D5 (`teleport` au choix) et des groupes mixtes, la console ne se distingue pas de l'app sur le réseau (mêmes kinds, tags et formats) | ✅ |

## Risques

- **Compatibilité binaire** des DM et des groupes avec l'app : vecteurs de test tirés du
  code Swift et essais réels obligatoires avant de déclarer un lot fini.
- **Tor** : relais qui bloquent les sorties Tor (comme pour l'app).
- **Arti** : API en version 0.x, mises à jour fréquentes.
- **Publication réelle** : tout envoi automatique (présences, planifiés) est visible des
  autres utilisateurs ; ne jamais l'activer pendant les tests hors de
  `relay.famillevital.club`.

## Journal

| Date | Lot | Avancement |
|---|---|---|
| 28/10/2026 | — | Spec rédigée après lecture de BitChat iOS 1.8.0 et du document d'alternatives. D1, D2, D5, D6, D10, D12 validées. Plan initial. |
| 28/10/2026 | — | D15 (dépendances) et D17 (neutralité) validées ; D16 (stockage) en discussion : question Redis. |
| 28/10/2026 | — | D16 validée (mémoire bornée + SQLite, pas de Redis à l'étape 1). Lot 0 lancé. |
| 28/10/2026 | 0 | D18 : `geohash-relays` sous Unlicense (iOS : Unlicense, Android : GPL-3.0). Crates `bitchat-console` et `geohash-relays` créées (squelettes, `[workspace]` propre, fmt, clippy et tests verts), dépôts git locaux, dépôts GitHub privés créés et reliés (`origin`), rien de poussé. |
