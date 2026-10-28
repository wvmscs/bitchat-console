# bitchat-console : plan de l'étape 1

Plan de réalisation de [SPECIFICATIONS.md](SPECIFICATIONS.md). Créé le 28/10/2026.

## Tableau de bord

| Lot | Contenu | État |
|---|---|---|
| 0 | Mise en place : dépôts, crates, dépendances, `AGENTS.md` | ✅ |
| 1 | Crate commune `geohash-relays` : listes, règles iOS et Android, voisins, interface de stratégie | ✅ |
| 2 | Connecteur Tor (SOCKS5, isolation par geohash), refus du clair | ✅ |
| 3 | Identités : graine, clé par geohash, identité Noise | ✅ |
| 4 | Lecture : 9 cellules, connexions, abonnements, fil, changement de geohash à chaud | ✅ |
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

### Lot 1 : crate `geohash-relays` ✅

- [x] Lecture des listes iOS et Android (règles de chaque app, corrections du lot 11 bis de
      bitchat-geohash), conflit d'hôte iOS ; URL des listes et relais par défaut des apps
      (`Platform::list_url`, `Platform::default_relays`). Le téléchargement reste chez
      l'appelant (crate sans réseau).
- [x] Stratégie `AS` (`AppStrategy`, apps : les deux, iOS seul, Android seul) derrière le
      trait `Strategy` (`read`, `write` ; place prévue pour A0 à A6).
- [x] Voisins d'un geohash comme iOS (`Geohash.neighbors(of:)`) : centre ± une cellule,
      longitude qui boucle, rien au-delà des pôles (5 voisins sur les bords) ;
      `with_neighbours` donne les 9 cellules. Module `geohash` sans dépendance (D19).
- [x] Tests sur les listes figées de bitchat-geohash : les 17 tests de sélection repris tels
      quels passent (mêmes choix sur `u09tv`, `u0`, `9q8yy`, `gcpvj`, `u1`, `gb`, `9q`) ;
      voisins contrôlés avec la crate `geohash` 0.13 (sauf aux pôles, où elle fait boucler la
      latitude et pas l'app).
- [x] `README.md` pensé pour une PR vers les apps : règle de chaque app, fichier et fonction
      sources.

### Lot 2 : connecteur Tor ✅

- [x] Trait `Connector` : ouvre un flux vers `host:port` pour une clé d'isolation (une
      cellule, D20).
- [x] Client SOCKS5 `Socks5` (RFC 1928 + 1929) : identifiant `bitchat-console/<cellule>`,
      donc un circuit par cellule (`IsolateSOCKSAuth`) ; `CONNECT` par nom de domaine, résolu
      par Tor, jamais localement ; délai de 60 s ; codes de refus du proxy traduits.
- [x] `Socks5::check` : au démarrage, un proxy SOCKS5 doit répondre, sinon
      `Error::TorUnavailable` (« start tor, or give its SOCKS address with --tor-socks »).
      `Loopback` : connexion directe vers le seul bouclage (`localhost`, `127/8`, `::1`), pour
      les tests ; tout autre hôte est refusé (`PermissionDenied`).
- [x] Tests : proxy SOCKS5 de test (`tests/common/socks.rs`) qui enregistre identifiant, hôte
      et port et relaie vers un serveur d'écho : identifiants distincts par cellule, nom non
      résolu, cible refusée expliquée, absence de proxy ou serveur non SOCKS → erreur,
      bouclage seul.

### Lot 3 : identités ✅

- [x] Graine de 32 octets (`seed.key`) et identité Noise (`noise.key` : clé statique X25519
      puis clé de signature Ed25519) dans le dossier de données, créé en `0700`, fichiers en
      `0600` ; un fichier lisible par d'autres (`Error::InsecureKeyFile`) ou de mauvaise
      taille (`Error::BadKeyFile`) est refusé. Rien n'est jamais journalisé.
- [x] Clé par geohash : `HMAC-SHA256(graine, geohash ‖ compteur big-endian)`, compteur 0 à 9
      jusqu'à une clé secp256k1 valide, clé publique x-only (`CellKey`, suffixe `#abcd`).
      **Vérifiée contre CryptoKit** (script `tests/vectors/identity.swift`, vecteurs sur
      `u09tv`, `u09tvw`, `9q8yy`).
- [x] Identité Noise : empreinte = SHA-256 de la clé statique (`getIdentityFingerprint`),
      identifiant de pair = 16 premiers caractères (`PeerID(publicKey:)`), clé de signature
      Ed25519 ; mêmes résultats que CryptoKit.

### Lot 4 : lecture ✅

- [x] 9 cellules (`with_neighbours`) → relais de la stratégie (lot 1) → **une connexion par
      couple (relais, cellule)** (D20) via le connecteur ; filtre `kinds:[20000,20001]`,
      `#g:[cellule]`, `since: −1 h`, `limit: 200` (comme les apps). Client de relais
      (`relay.rs`) : WebSocket sur le flux Tor, TLS (racines du web), reconnexion de 1 s à
      5 min (×2), file des publications pendant les coupures, `CLOSED auth-required:` →
      abandon (l'identité d'une cellule ne peut pas s'authentifier).
- [x] Contrôles : kind, cellule du `g` = cellule de la connexion, limites des tags, puis
      déduplication par id, puis id et signature ; un événement n'est marqué « vu » qu'une
      fois valide (**défaut trouvé par les tests** : sinon un relais qui l'envoie d'abord sur
      la mauvaise connexion le cache). `NOTICE` et `CLOSED` remontés comme texte à afficher.
- [x] Fil (`MessageStore`, D16) : tampon mémoire par cellule (500 messages, 24 h), trié par
      date, relais qui ont livré chaque message ; pseudo (`n`), marque `teleport` ; date
      future ramenée à maintenant (comme iOS). Participants par cellule (clés vues en 15 min,
      messages et présences).
- [x] Changement de zone à chaud (`Console::set_area`) : seules les connexions qui changent
      bougent ; un message d'une cellule quittée n'est plus montré ; niveau non canal
      (`NotAChannel`) et geohash invalide refusés.
- [x] Listes de relais (D21) : téléchargées par le connecteur (Tor, isolation
      `relay-lists`), HTTP/1.0 + TLS ; dernière copie valide en cache dans le dossier de
      données, utilisée si le téléchargement échoue ou si iOS refuserait la liste (hôte à deux
      endroits) ; fichier local accepté ; sinon `Error::NoRelayList`.
- [x] Tests (relais de test local, relais scripté, proxy SOCKS5 de test) : fil des 9
      cellules, déduplication entre deux relais, présence comptée sans message, événements
      invalides écartés (signature, autre cellule, autre kind), changement de zone,
      9 circuits pour 9 cellules, listes (téléchargement, cache, repli, refus, Tor).

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
| D19 | Calcul des geohash dans `geohash-relays` | Module maison sans dépendance (décodage, encodage, voisins), aligné sur `Geohash.swift` d'iOS plutôt que sur la crate `geohash` (qui fait boucler la latitude aux pôles) : crate facile à transposer dans les apps — *prise par l'agent, à revoir* | ✅ |
| D20 | Isolation Tor | Une connexion par couple (relais, cellule), chacune avec son identifiant SOCKS, donc son circuit Tor : un relais ne voit sur une connexion qu'une cellule et sa clé, et ne peut pas relier les clés de deux cellules par la connexion ni par l'adresse de sortie (l'app iOS partage une connexion par relais et n'isole pas ses flux). Coût : plus de connexions — *prise par l'agent, à revoir* | ✅ |
| D21 | Listes de relais | Téléchargées par Tor comme les relais (rien ne sort en clair), dernière copie valide en cache et utilisée en repli ; règle iOS : liste avec un hôte à deux endroits refusée — *prise par l'agent, à revoir* | ✅ |
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
| 28/10/2026 | 0 | Lot 0 commité dans les deux dépôts (`977e4cd` console, `4861316` geohash-relays) et poussé. L'utilisateur laisse l'agent poursuivre seul (un commit par lot, CHANGELOG, décisions tracées) ; relais `relay.famillevital.club` autorisé pour tous les tests. |
| 28/10/2026 | 1 | `geohash-relays` : listes, sélection (portées de bitchat-geohash sans réseau), module `geohash` maison (D19), `Strategy`/`AppStrategy`, README. 24 tests (17 de sélection repris tels quels), clippy et rustdoc propres. Commité `55f63cd`. |
| 28/10/2026 | 2 | Connecteur : trait `Connector`, `Socks5` (isolation par identifiant, D20), `Loopback` (tests seulement), `Error::TorUnavailable`. 7 tests (proxy SOCKS5 de test). `tor` n'est pas installé sur la machine : essai réel avec Tor reporté au lot 11 (Arti, sans rien installer) ou à l'utilisateur. |
| 28/10/2026 | 3 | Module `identity` (graine, `CellKey`, `NoiseIdentity`, `Keys`), dépendances `k256`, `sha2`, `hmac`, `rand_core`, `x25519-dalek`, `ed25519-dalek` (D15). Vecteurs CryptoKit identiques. `geohash-relays` en dépendance git (SSH, `git-fetch-with-cli`), verrouillée sur `55f63cd`. 13 tests. |
| 28/10/2026 | 4 | Lecture : `nostr.rs` (repris de bitchat-geohash, signature ajoutée), `relay.rs`, `console.rs` (tâche, zone, fil, participants), `store.rs` (D16), `lists.rs` (D21). Défaut trouvé par les tests et corrigé (événement marqué vu avant validation). 23 tests, stables sur 3 lancements. Essai réel par Tor reporté (pas de `tor` installé : lot 11, Arti). |
