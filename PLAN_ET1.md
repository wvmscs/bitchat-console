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
| 5 | Écriture : messages, preuve de travail, présences, marque `teleport` | ✅ |
| 6 | Messages privés (enveloppe BitChat `v2:`, relais par défaut) | ✅ |
| 7 | Serveur web et page console | ✅ |
| 8 | Masquage, isolation d'un intervenant, messages préenregistrés et planifiés | ✅ |
| 9 | Superposition à la carte de bitchat-geohash | ⬜ |
| 10 | Groupes mixtes (transport Nostr, entre consoles) | ⬜ |
| 11 | Feature `arti` | ✅ |
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

### Lot 5 : écriture ✅

- [x] Message kind 20000 signé avec la clé de la cellule choisie parmi les 9 (centre par
      défaut), **exactement comme l'app** (`ephemeralGeohashTags`) : tags `g`, `n` (pseudo
      sans espaces autour, omis s'il est vide), `t=teleport` si la console s'annonce
      téléportée, puis `nonce` NIP-13 miné comme `NostrPoW.swift` (`["nonce", 16 hex,
      "8"]`, départ aléatoire, 2 s puis cible divisée par deux ; D8).
- [x] Envoi aux relais d'écriture de la cellule (`Strategy::write`, D4), connexion ouverte
      si besoin ; `OK` acceptés ajoutés aux relais du message, refus affichés ; message
      ajouté au fil (`own`).
- [x] Présences kind 20001 comme l'app (`createGeohashPresenceEvent` : tag `g` seul,
      contenu vide), chacune avec la clé de sa cellule, aux niveaux 2, 4 et 5 du centre,
      toutes les 40 à 80 s (première au bout de 3 s), par des connexions d'écriture seule
      quand la cellule n'est pas suivie. Choix « téléporté » à chaque changement de zone
      (`set_area(g, teleport)`) : messages marqués, **aucune présence** (D22).
- [x] Tests (lecteur indépendant branché sur le relais de test) : format et ordre des tags,
      clé de la cellule, preuve de travail ≥ 8 bits, `OK` pris en compte, voisin avec sa
      propre clé, refus hors zone et message vide, marque `teleport`, présences sur
      `u0`/`u09t`/`u09tv` puis de nouveau, plus rien une fois téléporté.
- [x] Boucle fermée sur `relay.famillevital.club`, par Tor (Arti, lot 11) : deux consoles à
      graines jetables, message de l'une vu par l'autre en 0,11 s.

### Lot 6 : messages privés ✅

- [x] Enveloppe BitChat (`envelope.rs`) comme `NostrProtocol.swift` : rumor kind 14 (id vide,
      sans signature, sans tag) → sceau kind 13 signé par la clé de la cellule → enveloppe
      kind 1059 signée par une clé à usage unique, tag `p` du destinataire ; dates du sceau et
      de l'enveloppe tirées à ±15 min. Chiffrement : ECDH secp256k1 (point partagé compressé,
      33 octets, comme `P256K`), HKDF-SHA256 (sel vide, info `"nip44-v2"`),
      XChaCha20-Poly1305, `v2:` + base64url sans remplissage. Clé du destinataire prise en Y
      pair, les deux parités essayées à l'ouverture. Tous les contrôles de l'app à
      l'ouverture (taille, tag `p`, signatures, forme du sceau et de la rumor, expéditeur =
      signataire du sceau ; forme Android acceptée).
- [x] **Vecteurs identiques à l'app** : clé HKDF et chiffré `v2:` calculés avec le
      `XChaCha20Poly1305Compat.swift` de l'app (`tests/vectors/envelope.swift`).
- [x] Paquets BitChat (`packet.rs`, `BinaryProtocol.swift`) : `bitchat1:` + base64url d'un
      paquet v1 `noiseEncrypted`, sans destinataire, identifiant d'expéditeur aléatoire par
      enveloppe, TTL 7, remplissage PKCS#7 à 256/512/1024/2048 ; lecture v1 et v2, route,
      charge compressée (DEFLATE brut, bornée, D23). Message privé = TLV (identifiant UUID,
      contenu ≤ 255 octets comme l'app) ; accusés `delivered` et `read`.
- [x] Console : boîte de réception de chaque clé utilisée (9 cellules + cellules de
      présence) sur les relais par défaut des deux apps (`kinds:[1059]`, `#p`, `since:
      −24 h`, `limit: 100`), par le circuit de la cellule (D20) ; `send_dm(pair, cellule,
      texte)`, accusé « délivré » envoyé aussitôt à la réception (comme l'app),
      `mark_read(pair)` ; états `sent` → `relayed` → `delivered` → `read`, jamais en arrière.
- [x] Tests : 2 consoles sur un relais de test (aller-retour, accusés, réponse), refus (trop
      long, cellule non utilisée, clé invalide, vide) ; **essai réel par Tor** sur
      `relay.famillevital.club` : DM reçu en 0,26 s, « délivré » en 0,37 s.
- [ ] Essai avec l'app de l'utilisateur (DM de la console vers l'app et retour) : à faire
      avec lui (lot 12).

### Lot 7 : serveur web et page ✅

- [x] Serveur axum sur le modèle de bitchat-geohash (`guard.rs` et `tls.rs` repris) : `Host`
      contrôlé, commandes `POST` en JSON avec l'`Origin` du serveur, CSP stricte (aucune
      ressource extérieure, aucune image chargée), HTTPS optionnel (`--tls-cert`,
      `--tls-key`) ; adresse non locale refusée sans `--allow-remote` et TLS (la page montre
      des messages privés).
- [x] API : `GET /api/state` (zone, messages des 9 cellules, DM, clés publiques de la
      console par cellule), `GET /api/events` (SSE : `state`, puis `message`, `delivered`,
      `participants`, `status`, `dm`, `dm_status`, `relay_text` ; flux en retard fermé, le
      navigateur reprend), `POST /api/area`, `/api/send`, `/api/dm`, `/api/read` ; refus en
      JSON avec un code.
- [x] Page : barre (geohash, case « teleported » à chaque changement, état du flux, Tor),
      grille des 9 cellules (participants, messages, relais connectés), fil des 9 cellules
      ou d'une seule, saisie (cellule, pseudo, texte), messages privés (conversations par
      interlocuteur et cellule, états `✓`/`✓✓`/`read`, accusés de lecture à l'ouverture).
      Textes des relais par `textContent` uniquement ; liens cliquables, images jamais
      chargées (D9), marquées 🖼.
- [x] Ligne de commande (`main.rs`) : `<geohash> [--teleport] [--data-dir] [--tor-socks |
      --arti] [--relays-csv] [--android-csv | --no-android] [--apps] [--dm-relays]
      [--no-presence] [--pow-bits] [--listen] [--allow-remote] [--tls-cert --tls-key]
      [--open] [--verbose]` ; Tor par défaut : Arti si le binaire l'a, sinon le démon (D24) ;
      port occupé et Tor absent signalés avant tout ; codes de sortie 2 et 1.
- [x] Tests : `tests/server.rs` (fichiers, état, `Host` 421, `Origin` 403, JSON 415, refus
      motivés, flux SSE en direct, envoi depuis l'API, nouvel état au changement de zone),
      `tests/js/view.test.mjs` (4 tests `node`). Essai réel : binaire avec Arti sur le seul
      relais de l'utilisateur, page pilotée dans Chrome headless (flux en direct, 9 cellules
      connectées, message envoyé depuis la page et accepté par le relais, console propre).

### Lot 8 : masquage et messages préenregistrés ✅

- [x] Stockage local SQLite (`db.rs`, `console.sqlite` dans le dossier de données, fichier en
      `0600`) : migrations numérotées, version refusée si plus récente (comme
      bitchat-geohash). `rusqlite` 0.40, la plage qu'accepte Arti (une seule bibliothèque
      sqlite3 liée).
- [x] Masquage (D25) : `set_muted(clé)` ; tous les messages de cette clé cachés dans la page,
      démasquage, ou **fenêtre d'isolement** (le fil ne montre plus que cette clé) ; liste
      conservée au redémarrage (clé et date seulement). Une même personne a une clé par
      cellule : le masquage vaut pour la clé.
- [x] Messages préenregistrés : enregistrement (nom, texte, cellule et pseudo facultatifs),
      envoi sur demande (dans la cellule choisie, la sienne ou le centre), suppression.
- [x] Messages planifiés : date, une fois ou à intervalle (au plus une fois par minute) ;
      envoyés par la console à l'échéance (vérification chaque seconde), construits comme un
      envoi normal ; une cellule non suivie est ignorée avec le motif ; une échéance manquée
      n'est pas rattrapée (prochaine occurrence future) ; dernier résultat affiché.
- [x] API (`/api/mute`, `/api/preset`, `/api/preset/delete`, `/api/preset/send`,
      `/api/schedule`, `/api/schedule/delete`, événement SSE `local`) et page (🔇 par message,
      liste « Muted » avec « isolate » et « unmute », sections « Saved messages » et
      « Scheduled »).
- [x] Tests : masqués conservés après redémarrage et fichier privé, préenregistré envoyé dans
      un voisin, planifié envoyé à l'échéance, répétition reportée au lendemain, cellule hors
      zone ignorée, refus ; 5 tests `node`. Essai du binaire réel (Arti, relais de
      l'utilisateur) : sections présentes, message enregistré puis envoyé depuis la page.

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

### Lot 11 : feature `arti` ✅

Fait avant le lot 6 (dépend du seul lot 2) pour permettre les essais réels par Tor sans rien
installer.

- [x] Connecteur `Arti` (`arti-client` 0.47, `rustls`, feature `arti` hors défaut) :
      `Arti::bootstrap(dossier)` (état et cache de Tor dans `tor-state`, `tor-cache`), un
      `IsolationToken` par clé d'isolation (cellule, D20), délai de 60 s.
- [x] **Défauts trouvés par l'essai réel** dans le téléchargement des listes (lot 4) :
      requête jamais partie (écriture TLS sans `flush`) ; lecture jusqu'à la fin du flux alors
      que le CDN garde la connexion (lecture bornée à `Content-Length`, `Connection: close`).
      Test du cas « connexion gardée » ajouté.
- [x] Essai réel (`tests/real_tor.rs`, `--ignored`) : Tor démarré en 0,9 s (cache), liste iOS
      officielle par Tor (326 relais) en 0,8 s, 18 connexions en 2,8 s, boucle fermée sur
      `relay.famillevital.club` en 0,11 s. `tests/real_tor_diag.rs` : chaque étape d'un GET
      HTTPS par Tor, chronométrée.

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
| D22 | Présence et téléportation | Comme l'app : en mode « sur place », présences (tag `g` seul, jamais `teleport`, comme iOS) ; en mode « téléporté », messages `t=teleport` et aucune présence (un utilisateur téléporté de l'app n'en envoie pas) — *prise par l'agent, à revoir* | ✅ |
| D23 | Paquets compressés reçus | Lecture des charges compressées (DEFLATE brut, comme `CompressionUtil`) avec la petite crate `miniz_oxide` (hors D15), bornée en taille et en taux ; à l'envoi, pas de compression (l'app lit les deux, et rien n'en paraît hors de l'enveloppe) — *prise par l'agent, à revoir* | ✅ |
| D24 | Tor par défaut | Arti (dans le binaire) quand le binaire est construit avec la feature `arti`, sinon le démon `tor` en SOCKS `127.0.0.1:9050` ; `--tor-socks` ou `--arti` pour choisir — *prise par l'agent, à revoir* | ✅ |
| D25 | Masqués sur disque | La liste des masqués est enregistrée (clé publique et date seulement, jamais pseudo ni contenu) : c'est un choix volontaire de l'utilisateur, utile après un redémarrage ; la base est en `0600` — *prise par l'agent, à revoir* | ✅ |
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
| 28/10/2026 | 5 | Écriture : `pow.rs` (comme `NostrPoW.swift`), `Console::send`, `set_area(g, teleport)`, présences, `OK`. 28 tests, stables sur 3 lancements ; clippy propre. Boucle fermée réelle reportée (Tor). |
| 28/10/2026 | 11 | Feature `arti` (avancée avant le lot 6). Premier essai bloqué : le GET des listes ne partait pas (pas de `flush` après l'écriture TLS) et attendait une fin de flux que le CDN ne donne pas ; diagnostic pas à pas par Tor, deux corrections, test ajouté. Essai réel complet par Tor sur le relais de l'utilisateur : boucle fermée en 0,11 s. 29 tests (+ 2 réels `--ignored`), clippy propre avec et sans `arti`. |
| 28/10/2026 | 6 | DM : `packet.rs`, `envelope.rs` (vecteurs identiques au code Swift de l'app), boîtes de réception, accusés. Les tests et l'essai réel n'utilisent jamais les relais par défaut réels (relais de test, ou celui de l'utilisateur). 39 tests (+ réels), clippy propre avec et sans `arti`. Essai réel par Tor : DM en 0,26 s, accusé en 0,37 s. Reste l'essai avec l'app (lot 12). |
| 28/10/2026 | 7 | Serveur, API, page, ligne de commande. 48 tests Rust, 4 `node` ; clippy propre avec et sans `arti`. Essai réel du binaire par Tor sur le relais de l'utilisateur, page dans Chrome headless. |
| 28/10/2026 | 8 | Stockage local, masquage et isolement, préenregistrés, planifiés. Conflit `rusqlite` 0.41 / Arti résolu en 0.40. Base créée en `0644` repérée à l'essai réel → `0600`, test ajouté. 49 tests, 5 `node`, clippy propre avec et sans `arti`. |
