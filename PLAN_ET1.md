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
| 9 | Superposition à la carte de bitchat-geohash | ✅ |
| 10 | Groupes mixtes (transport Nostr, entre consoles) | ✅ |
| 11 | Feature `arti` | ✅ |
| 12 | Validation réelle | ⏳ (avec l'utilisateur) |
| 13 | Zones de saisie (défilement), pseudo côté serveur, envoi du pseudo par cellule (D32) | ✅ |
| 14 | Base d'historique `history.sqlite` (D31) : schéma, écriture, droits, recréation | ✅ |
| 15 | Critère de départ (10 min) : messages grisés, DM cachés, effacement des DM | ✅ |
| 16 | Onglet d'historique : recherche, tri, sélection, effacement ; modifications externes en direct | ✅ |
| 17 | API CRUD, outil `bitchat-history` (sauvegarde, rotation, purge, effacement ; cron, D33), `SKILL.md` | ✅ |

Ordre : 0 → 1 → 2 → 3 ; 4 dépend de 1, 2 et 3 ; 5 et 6 dépendent de 4 ; 7 dépend de 4 ;
8 dépend de 5 et 7 ; 9 dépend de 7 ; 10 dépend de 6 ; 11 dépend de 2 ; 12 clôt l'étape.
Évolutions de l'interface ([EVOLUTION_01-UI.md](EVOLUTION_01-UI.md)) : 13 indépendant ; 15 et 16
dépendent de 14 ; 17 dépend de 14 et 16.

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
      abandon (l'identité d'une cellule ne peut pas s'authentifier). *Remplacé par D28 :
      authentification NIP-42 avec la clé de la connexion.*
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

### Lot 9 : superposition à la carte ✅

- [x] **Autrement que prévu en D10** (D26) : bitchat-geohash 0.1.0 (`rusqlite` 0.41) et Arti
      (`rusqlite` < 0.41) ne peuvent pas être liés dans un même binaire (une seule bibliothèque
      sqlite3), et bitchat-geohash reste intact. La console **relaie** donc une carte
      bitchat-geohash qui tourne à côté : `--map http://127.0.0.1:8080`.
- [x] Relais (`server/map.rs`, client HTTP/1 de `hyper`) : tout ce qui n'est pas la console
      va à la carte, avec son propre `Host` et, pour ses commandes, son propre `Origin` (la
      console contrôle d'abord l'origine réelle) ; flux SSE de la carte relayés au fil de
      l'eau ; carte absente → 502 `map_unreachable`, la console continue.
- [x] Page de la carte : la console y ajoute `overlay.js` et `overlay.css` et autorise son
      cadre (`frame-src 'self'` ajouté à la CSP de la carte, qui garde ses tuiles) ; panneau
      flottant (repliable, redimensionnable) avec la console (`/console/`) dans un cadre ;
      bouton « Follow in console » ajouté au menu d'une cellule de la carte : la console suit
      ce geohash (avec le choix « teleported » de la console).
- [x] Console servie aussi sous `/console/` (chemins relatifs) ; elle accepte d'être encadrée
      par elle-même seulement (`frame-ancestors 'self'`).
- [x] Tests (`tests/map.rs`, carte simulée qui contrôle `Host` et `Origin` comme
      bitchat-geohash) : injection, CSP, console sous `/console/`, commande relayée, refus d'une
      autre origine, flux SSE continu, carte absente. Essai réel : bitchat-geohash sur `u09t`
      et la console par-dessus (Arti, relais de l'utilisateur), clic sur `#u09ts` →
      « Follow in console » → la console suit `#u09ts`, cadre à jour, console propre.

### Lot 10 : groupes mixtes ✅

- [x] Format des groupes de BitChat 1.8 (`groups.rs`, depuis `GroupProtocol.swift`) : TLV à
      longueur sur 16 bits, liste des membres (empreinte, clé Ed25519, pseudo ≤ 64 octets
      coupé sur un caractère), état signé par le créateur sur `bitchat-group-v1 | id |
      époque | SHA256(clé) | SHA256(liste) | SHA256(nom)`, messages ChaCha20-Poly1305 (id et
      époque en données authentifiées) au contenu signé par l'auteur. **Vérifié avec le code
      Swift de l'app** (`tests/vectors/groups.swift` + `examples/group_vectors.rs`) : l'app
      décode l'état de la console, valide la signature du créateur (le TLV ajouté est ignoré)
      et ouvre le message (signature de l'auteur valide).
- [x] Transport Nostr (D27) : une adresse Nostr par groupe et par console (clé dérivée
      `group|<id>`, sans lien avec les clés de cellule) ; états et messages en `bitchat1:`
      dans l'enveloppe privée, un exemplaire par membre ; carnet empreinte → adresse dans un
      TLV `0x10` que l'app ignore ; invitation avec consentement (demande `0xC0` du créateur,
      acceptation `0xC1` de l'invité depuis la page, avec son identité Noise et son adresse).
- [x] Règles de l'app (`applyGroupState`) : état signé par le créateur qu'il nomme, créateur
      épinglé (empreinte et clé) et, sur Nostr, son adresse ; époque jamais en arrière ;
      liste sans moi = retrait ; époque + 1 et nouvelle clé à chaque changement de membres ;
      16 membres au plus ; membre retiré averti avec une clé nulle (comme l'app) ; départ
      local. **Destruction** (demandée, absente de l'app) : liste réduite au créateur envoyée
      à tous, puis groupe oublié.
- [x] Stockage : migration `0002_groups` (état encodé, clé comprise, et adresse épinglée du
      créateur) dans la base `0600`.
- [x] **Défauts trouvés par les tests** : boîte de l'adresse d'un groupe accepté mais pas
      encore reçu ignorée ; derniers envois d'un groupe détruit perdus (connexion fermée
      aussitôt) → toute connexion qui n'est plus voulue est désormais fermée 10 à 40 s plus
      tard (vaut aussi pour un message envoyé juste avant un changement de zone).
- [x] API (`POST /api/group`, `op` : create, invite, accept, decline, send, remove, leave,
      destroy ; événements `groups`, `group_message`) et page (invitations, groupes, fil,
      membres, retirer, quitter ou détruire, inviter depuis une conversation privée).
- [x] Tests : 3 consoles (création, invitation et consentement, messages dans les deux sens,
      3ᵉ membre et époque 3 pour tous, retrait et impossibilité d'écrire ensuite, refus pour
      un non-créateur, destruction), refus d'une invitation déclinée. **Essai réel par Tor**
      sur `relay.famillevital.club` : groupe rejoint en 0,77 s, message en 0,16 s.
- [ ] Crate Bluetooth (maillage BitChat) et PR vers l'app pour le transport Nostr des
      groupes : étapes suivantes (D6).

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

### Lot 12 : validation réelle ⏳

Fait par l'agent, seul :

- [x] Boucle fermée par Tor (Arti) sur `relay.famillevital.club`, graines jetables
      (`tests/real_tor.rs`) : liste iOS officielle par Tor, message vu par une autre console
      en 0,1 à 0,4 s, DM reçu et accusé « délivré », groupe rejoint et message de groupe.
- [x] Binaire réel avec Arti, page pilotée dans Chrome headless (lots 7 à 9), sur le relais
      de l'utilisateur ; superposition à une vraie carte bitchat-geohash.
- [x] **Lecture réelle à grande échelle par Tor** (28/10/2026, 3 h 37–3 h 48) : `u09t`
      téléporté (aucune présence, rien envoyé), listes officielles par Tor, 9 cellules, 120
      connexions (75 de canaux, 45 boîtes DM sur les relais par défaut) ; mémoire 26 à 36 Mo
      sans croissance ; 54 connexions de canaux sur 75 établies, les 21 autres vers les 3
      relais que bitchat-geohash classe aussi morts ou injoignables
      (`relay.cypherflow.ai`, `relay.sovereignresonance.org`, `treuzkas.branruz.com`) ;
      95 à 109 participants comptés ; aucun message à cette heure ; aucun avertissement.
- [x] **Démon `tor`** (Homebrew, installé par l'utilisateur, 28/10/2026 matin) :
      - boucle fermée par `--tor-socks 127.0.0.1:9050` (`BITCHAT_TOR_SOCKS`) : 18 connexions
        en 2,6 s, message vu en 0,37 s, DM reçu en 0,57 s et « délivré » à 0,94 s, groupe
        rejoint en 0,84 s, message de groupe en 0,11 s ;
      - isolation : deux noms d'utilisateur SOCKS sortent par deux IP différentes, le même
        nom garde la sienne (réglage par défaut `IsolateSOCKSAuth`, aucun `torrc`) ;
      - lecture seule de `u09t`, 90 s : 54 connexions de canaux sur 75, les mêmes 3 relais
        injoignables qu'avec Arti, 103 participants, 14 Mo, aucun avertissement.

À faire avec l'utilisateur :

- [x] DM entre la console et l'app iOS de l'utilisateur, dans les deux sens (28/10/2026,
      console #376d, app #e521, dans `u09tv`). Accusés « délivré » et « lu » vus de l'app : à
      confirmer.
- [x] Message dans un vrai canal (`u09tv`, autorisé par l'utilisateur), dans les deux sens.
      Le premier message, envoyé avant que l'app soit dans la cellule, n'a pas été reçu :
      normal, les messages de canal (20000) sont éphémères, les relais ne les gardent pas et
      l'app ne s'abonne qu'à la cellule où elle est. Présence vue par l'app : à confirmer.
- [ ] Safari.
- [ ] Revue des décisions prises par l'agent (D19 à D27).

### Lot 13 : saisie et pseudo ✅

Demandé dans [EVOLUTION_01-UI.md](EVOLUTION_01-UI.md) (28/10/2026).

- [x] Zones de saisie : au bout du champ, le texte tapé sort de la vue. Cause : la
      reconstruction d'une section par un événement en direct rend valeur, focus et curseur,
      pas `scrollLeft` (`app.js`, conservation des champs). Les champs gardent désormais leur
      défilement ; les champs de message (canal, DM, groupes, préenregistrés, planifiés) sont
      des `textarea` qui grandissent avec le texte jusqu'à 8 lignes environ, puis défilent
      (Entrée envoie, Maj+Entrée va à la ligne).
- [x] Pseudo gardé côté serveur (`console.sqlite`, migration `0004_nickname`, table
      `setting`), plus dans le `localStorage` (propre à l'origine : perdu quand le port change,
      ce que fait le lanceur si 8080 est pris, ou entre HTTP et HTTPS). Le même pour toutes les
      cellules (D32). Celui d'une page plus ancienne est repris une fois du navigateur.
      `POST /api/nickname`, `Local.nickname`.
- [x] Par cellule, choix « envoyer mon pseudo » ; sinon, pas de tag `n` (D32). L'app affiche
      `anon#xxxx` **seulement si elle n'a encore aucun pseudo pour cette clé** : elle garde en
      mémoire le dernier pseudo vu par clé (`LocationPresenceStore.setNickname`), jusqu'à son
      changement de canal ou son redémarrage, et la clé d'une cellule est stable. Un pseudo
      déjà envoyé dans une cellule y reste attaché à la clé (dans l'app et chez tout relais qui
      garde les événements) ; la page le dit (infobulle de la case). Case « nickname in
      #… » à côté du pseudo, pour la cellule choisie ; table `anon_cell`, `POST /api/anon`,
      `Local.anon_cells`. Une cellule sans pseudo l'emporte sur un pseudo donné.
- [x] Préenregistrés et planifiés : le pseudo pris à l'envoi (dernière version), plus celui de
      la création (un pseudo propre au message reste possible par l'API). Groupes : le pseudo
      de la console quand la page n'en donne pas.
- [x] DM : le `PrivateMessagePacket` de l'app ne porte qu'un ID et un contenu (TLV `0x00`,
      `0x01`) ; D17 interdit d'y ajouter le pseudo. L'app nomme l'expéditeur d'un DM avec le
      même cache (`displayNameForNostrPubkey` : pseudo connu de la clé, sinon `anon#xxxx`) :
      le pseudo vu dans les DM est celui d'un message de canal antérieur.
      Les groupes le portent déjà.

### Lot 14 : base d'historique ✅

- [x] `history.sqlite`, à part de `console.sqlite` (D31) : active par défaut
      (`--no-history` pour rien garder) ; messages de canal, DM et groupes, reçus et émis, y
      compris ceux des masqués. Module `history`, `ConsoleConfig::history_path`.
- [x] Schéma (`migrations/history/0001_history.sql`, table `message`) : type (`channel`,
      `dm`, `group`), sens (`own`), geohash, groupe, horodatages (événement, réception),
      émetteur, destinataire (DM), pseudo, téléporté, contenu, ID (événement Nostr, ID
      BitChat du DM ou du message de groupe : une fois par message, quel que soit le nombre
      de relais), état du DM (envoyé, relayé, délivré, lu), `deleted_at`. Table `meta` :
      `since`, début de la base.
- [x] Fichier, `-wal` et `-shm` en `0600` (créé en `0600` avant l'ouverture : SQLite donne
      ce mode aux deux autres), dossier de données déjà en `0700` ; mode WAL, attente de
      60 s sur un verrou d'un autre processus, écritures sur un fil à part (la console
      n'attend jamais).
- [x] Base détruite (effacement complet par le cron) : recréée vide en 2 s au plus, même en
      marche (fichier surveillé par son inode) ; restes `-wal` et `-shm` d'une base effacée
      supprimés avant (ils seraient rejoués dans la nouvelle).
- [x] Rien d'antérieur au début de la base (`since`) n'est écrit, et une ligne effacée
      (contenu `NULL`, `deleted_at`) reste : un relais qui renvoie un message après une
      purge, une rotation (D33) ou un effacement ne le fait pas revenir. Le masquage en
      mémoire des DM effacés par l'utilisateur va au lot 15.
- [x] Relecture : quand la console suit une zone, les messages des 24 dernières heures de
      ses 9 cellules reviennent de la base dans le fil (effacés exclus).
- [x] `AGENTS.md` : la règle « rien sur disque » renvoie à D31.

### Lot 15 : départ des intervenants ✅

- [x] Critère : ni présence (20001) ni message (20000) d'une clé dans la cellule depuis
      10 min (`ConsoleConfig::departure_secs`). Un téléporté n'envoie jamais de présence
      (D22) : seuls ses messages comptent. `GET /api/present` (`Console::presence`) : dernière
      activité de chaque clé dans chaque cellule, en mémoire ; la page la demande toutes les
      20 s et compte aussitôt un message reçu. Calcul dans `view.js` (`hasLeft`,
      `convoOver`), testé.
- [x] Messages de canal grisés (lisibles) quand leur auteur est parti ; un message restitué de
      l'historique dont l'auteur n'est plus vu l'est aussi.
- [x] DM : conversation rangée dans « Ended » (pli fermé) quand le correspondant est parti
      de sa cellule et n'a rien écrit depuis 10 min ; jamais tant qu'un message n'est pas lu,
      ni celle qui est ouverte ; un nouveau message la ramène ; toujours lisible dans
      l'historique.
- [x] Effacement des DM par l'utilisateur, un message ou la conversation (local : les relais
      gardent les enveloppes). `POST /api/dm/delete`, `Console::delete_dms`, événement `dms`.
      IDs gardés 30 jours dans `console.sqlite` (migration `0005_dm_deleted`), comme D29 :
      renvoyés par un relais après un redémarrage, ils restent effacés ; dans l'historique,
      contenu effacé, ligne gardée.
- [x] Décidé par l'utilisateur (28/10/2026) : la console qui suit une autre cellule ne quitte
      pas ses échanges en cours ; seul le départ du correspondant compte.

### Lot 16 : onglet d'historique ✅

- [x] Page `/history?cell=…` (`history.html`, `history.js`), ouverte dans un nouvel onglet
      (lien « history ↗ » de la barre, pour la cellule affichée ; « all private messages ↗ »
      sous les conversations terminées) : filtres cellule (ou préfixe), type, texte ou
      pseudo, clé, dates, effacés ; tri récent, ancien, par émetteur ; sélection et
      effacement ; filtres gardés dans l'adresse ; contenus en texte seulement. Recherche :
      pas de FTS5, une fonction SQL `contains_folded` (casse ignorée au-delà de l'ASCII, que
      `LIKE` ne replie pas ; feature `functions` de `rusqlite`, pas de nouvelle dépendance) :
      la base d'un jour reste petite (rotation, D33).
- [x] Modifications faites par un autre processus (IA, cron, client SQLite) visibles en 2 s
      au plus : trigger sur `content` et `deleted_at` → table `change`
      (`migrations/history/0002_changes.sql`), `PRAGMA data_version` vérifié à chaque tour du
      fil d'historique ; un message effacé quitte le fil, un message modifié y montre son
      nouveau contenu, un DM effacé ailleurs reste effacé après un redémarrage ; événement
      `history` et nouvel état pour les pages. Les lignes supprimées tout court (purge,
      rotation) restent dans le fil : c'est du travail d'archive. Au retour d'une zone, les
      messages effacés dans la base sont tenus pour vus (un relais ne les ramène pas).

### Lot 17 : API, outil batch, `SKILL.md` ✅

- [x] API CRUD HTTP de la console, gardée comme les autres par Host et Origin (faite avec
      le lot 16) : `GET /api/history` (filtres de la page), `GET /api/history/{id}`,
      `POST /api/history/insert` (ligne remise d'une archive), `/update` (contenu),
      `/delete` (contenu effacé, ligne gardée) ; `409 no_history` avec `--no-history`.
- [x] Outil `bitchat-history` (second binaire de la crate, `src/bin/bitchat-history.rs`,
      `default-run` pour la console) sur la même base, console arrêtée ou non : `query` (JSON
      par ligne, filtres de l'API, âges `30m`, `2h`, `7d`), `get`, `insert` (lignes JSON sur
      l'entrée), `update`, `delete` (contenu effacé, la console le cache), `remove` et `purge`
      (lignes supprimées tout court), `backup` (`VACUUM INTO`, `0600`), `rotate` (D33),
      `wipe`, `cron`. Codes de sortie 0, 1, 2.
- [x] Rotation (D33) : copie de toute la base dans `<data-dir>/backups/` (nom libre, même à
      la seconde près), suppression de ce qui a plus de 2 h, début de la base déplacé là
      **sans jamais reculer** (après un effacement complet, ce qui précédait reste dehors),
      `VACUUM`. Défauts trouvés en essai : deux copies dans la même seconde ; une rotation
      faisait reculer `since` (corrigé, testé).
- [x] Entrée cron : `bitchat-history cron [--at 04:17]` affiche la ligne (rotation
      quotidienne, journal `<data-dir>/rotate.log`) ; README.
- [x] `skills/bitchat-history/SKILL.md` pour l'outil, l'API et le schéma (IA locale) : règles
      (local seulement, `delete`/`update` plutôt qu'à la main, `remove`/`purge` = archives,
      ne pas toucher `since` ni les droits, demander avant les opérations destructrices).
- [x] bitchat-web : `bitchat-history` dans les paquets (signé comme les autres) dès que le
      commit de la console l'a.

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
| D26 | Superposition à la carte (remplace la réalisation prévue en D10) | Relais HTTP d'une carte bitchat-geohash lancée à côté (`--map`), panneau flottant injecté dans sa page, bouton « Follow in console » ; raison : conflit de versions SQLite entre bitchat-geohash 0.1.0 et Arti, et bitchat-geohash en lecture seule. Les tuiles de la carte restent chargées par le navigateur en direct (pas par Tor), comme dans bitchat-geohash — *prise par l'agent, à revoir* | ✅ |
| D27 | Groupes par Nostr | Adresse Nostr par groupe (`group|<id>`), états et messages au format de l'app dans l'enveloppe privée, carnet d'adresses en TLV `0x10` (ignoré par l'app), invitation avec consentement (`0xC0`/`0xC1`, valeurs hors de la plage de l'app), destruction = liste réduite au créateur ; révéler son identité Noise à un groupe relie ses clés de cellule pour ses membres (comme dans l'app) — *prise par l'agent, à revoir* | ✅ |
| D28 | Relais exigeant NIP-42 | **Décision de l'utilisateur (28/10/2026)** : la console répond au défi AUTH avec la clé de la connexion (celle de la cellule, ou du groupe), qui est déjà l'auteur de ce qu'elle publie et le destinataire de ce qu'elle lit sur cette connexion, sur son propre circuit Tor : l'authentification ne révèle rien de plus au relais. Après acceptation : abonnement renvoyé, publications refusées pour `auth-required:` renvoyées une fois. Un refus après authentification (liste blanche) est définitif, sans boucle ; un AUTH refusé abandonne le relais | ✅ |
| D29 | DM lus après redémarrage | **Accord de l'utilisateur (28/10/2026)** : les identifiants des messages privés lus (UUID aléatoires, sans contenu ni clé) sont gardés dans `console.sqlite` (table `dm_read`), 30 jours ; relus après un redémarrage, ils restent lus et ne reçoivent pas un second accusé | ✅ |
| D30 | La carte suit la console | **Choix de l'utilisateur (28/10/2026)** : nouvelle fonction dans bitchat-geohash (lot 19, 0.2.0, `POST /api/area`), zone de la carte = parent de la cellule de la console (`u09tv` → `u09t`, jamais moins de 2 caractères). C'est l'overlay, dans la page de la carte, qui la demande quand le centre de la console change ; la carte peut aussi changer de zone par son propre champ, la console ne la suit pas dans ce sens | ✅ |
| D31 | Historique sur disque (remplace D11 et la règle « messages en mémoire seulement » de D16) | **Approuvée par l'utilisateur (28/10/2026)** : tous les messages (canal, DM, groupes, reçus et émis, masqués compris) dans `history.sqlite`, fichier à part de `console.sqlite` (pas de clés de groupe à côté), actif par défaut, `0600` ; traitements par un outil externe et une IA locale | ✅ |
| D32 | Pseudo | **Choix de l'utilisateur (28/10/2026)** : un seul pseudo pour toutes les cellules, gardé par la console ; par cellule, possibilité de ne pas l'envoyer (l'app affiche alors `anon#xxxx`) | ✅ |
| D33 | Sauvegarde et rotation | **Demande de l'utilisateur (28/10/2026)** : par cron, chaque jour, sauvegarde et compactage, puis rotation sur une base neuve qui commence 2 h avant ; suppression d'enregistrements et effacement complet possibles, base alors recréée. Rotation sans échange de fichier sous la console (proposition de l'agent, **validée le 28/10/2026**) : `VACUUM INTO` vers l'archive datée, suppression de ce qui a plus de 2 h, `VACUUM` | ✅ |
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
| 28/10/2026 | 9 | Superposition par relais (D26). 52 tests Rust, 5 `node`. Essai réel avec bitchat-geohash et Chrome headless. Commit `84f60f3` poussé par erreur avec 4 avertissements clippy dans `tests/map.rs` (le commit n'attendait pas clippy) : corrigés au commit suivant ; désormais clippy est vérifié avant chaque commit. |
| 28/10/2026 | 10 | Groupes : format de l'app vérifié par son code Swift, transport Nostr, consentement, règles de l'app, destruction. Deux défauts trouvés et corrigés (boîte d'un groupe en attente, fermeture trop rapide des connexions). 57 tests Rust, 5 `node`, clippy propre avec et sans `arti`. Essai réel par Tor : groupe en 0,77 s, message en 0,16 s. |
| 28/10/2026 | 12 | Lecture réelle par Tor sur `u09t` (10 min, 120 connexions, mémoire stable, relais morts identiques à bitchat-geohash) ; README. Restent les essais avec l'app et la revue des décisions de l'agent. |
| 28/10/2026 | 12 | Démon `tor` : `tests/real_tor.rs` prend `BITCHAT_TOR_SOCKS` ; boucle fermée, isolation des circuits et lecture seule de `u09t` vérifiées par le démon. |
| 28/10/2026 | 9 | Correctif trouvé avec bitchat-launcher : un flux en direct de la carte relayé sous `--map` retenait l'arrêt du serveur (Ctrl-C sans effet tant qu'un navigateur restait sur la carte) ; il se termine maintenant à l'arrêt. Test `stopping_ends_the_map_streams`. |
| 28/10/2026 | 12 | Essai avec l'app signalé par l'utilisateur : la page reprenait le focus à chaque événement (saisie et sélection presque impossibles). Les champs gardent valeur, focus et curseur ; une section attend qu'aucun bouton ne soit tenu et qu'aucun texte n'y soit sélectionné. Vérifié dans Chrome headless (50 rendus pendant 20 s de saisie). |
| 28/10/2026 | 12 | Essai avec l'app dans `u09tv` : messages de canal et DM dans les deux sens. Évolutions demandées : identifiant de la console dans chaque cellule, liste des participants de la cellule courante (`Console::participants`, `GET /api/participants`, pseudo, téléporté, nombre de messages, dernière activité ; en mémoire seulement), avec DM et mise en sourdine depuis la liste. |
| 28/10/2026 | 12 | Signalé : « impossible de suivre #u0 ». Le serveur suit bien `u0` (217 participants, 28 messages en une minute) ; c'est la page : le champ perdait le focus à l'appui sur « Follow » et une mise à jour y remettait le centre avant le clic. Corrigé, vérifié dans Chrome headless. Messages des relais expliqués : damus exige AUTH (NIP-42) pour lire les DM et bannit par IP (sortie Tor partagée), relay.mmwaves.de n'accepte que des auteurs en liste blanche, nostr21.com refuse les DM (1059). |
| 28/10/2026 | 12 | D28 (décision de l'utilisateur) : authentification NIP-42 avec la clé de la connexion, renvoi de l'abonnement et des publications refusées, une fois. Relais de test avec NIP-42 (exigé à la connexion, pour l'écriture au premier refus, liste blanche) ; `tests/auth.rs`. |
| 28/10/2026 | 12 | Demandes de l'utilisateur : journal des relais regroupé (relais + texte, nombre, cellules), listes repliables et défilantes à hauteur bornée, son désactivable à l'arrivée d'un DM, non-lus (barre, titre, conversations) et « ✓✓ read » mis en évidence ; `DirectMessage::read`. Vérifié dans Chrome headless avec deux consoles sur le relais de l'utilisateur (DM, son, non-lus, accusé « lu », réponse). Remarque : les enveloppes privées (1059) sont gardées par les relais ; au redémarrage, la console les relit et les montre de nouveau non lues (tout est en mémoire, D16). |
| 28/10/2026 | 12 | Demandes de l'utilisateur : participants triés par les 4 derniers hexa (ordre stable), messages dans l'ordre d'arrivée (`Message::received`), liste figée sous le pointeur pour pouvoir ouvrir un DM ; D29 (lus gardés après redémarrage). Vérifié : test de redémarrage (`tests/dm.rs`), Chrome headless (tri, 0 changement pendant 15 s de survol, rattrapage à la sortie). |
| 28/10/2026 | 12 | Demande de l'utilisateur : pseudo du correspondant dans les messages privés, s'il est connu (dernier pseudo vu dans un message de canal ; les DM et présences n'en portent pas). Vérifié dans Chrome headless avec deux consoles sur le relais de l'utilisateur. |
| 28/10/2026 | 9 | Demande de l'utilisateur : la carte suit la cellule choisie dans la console (ou les 9), avec recadrage. La console envoie sa vue par `postMessage` à la page de la carte ; `hook.js` (script classique injecté avant le module de la carte) garde la carte Leaflet via `L.Map.addInitHook`, sans toucher à bitchat-geohash ; contour de la sélection, cadrage dans l'espace laissé par le panneau de la console et celui de la carte. Vérifié dans Chrome headless sur une vraie carte (zoom 12 → 13,5 pour une cellule, 12 pour les 9). |
| 28/10/2026 | 9 | Précision de l'utilisateur : la carte doit changer de zone, pas seulement de vue. D30 : bitchat-geohash 0.2.0 (lot 19, `804cd92`, tag `v0.2.0`) change sa zone en direct ; l'overlay la demande (parent de la cellule) quand le centre de la console change. Chrome headless : console sur `u09wj` → carte sur `u09w` (33 cellules) en 8,5 s. |
| 28/10/2026 | 9 | Signalé par l'utilisateur : le contour des cellules de la console gênait les survols de la carte (un second SVG posé dessus prenait le pointeur). Panneau Leaflet propre, sans événements de pointeur. Vérifié dans Chrome headless : sous le contour, le pointeur atteint la cellule de la carte et sa bulle s'ouvre. |
| 28/10/2026 | 9 | Demande de l'utilisateur : double-clic (ou ctrl-clic, cmd-clic ; sur Mac le ctrl-clic est un menu contextuel, capté aussi) sur une des 9 cellules de la console → la carte surveille la zone de cette cellule (son parent), la console garde son centre ; la carte ne suit plus que les changements de centre de la console, pas sa sélection. Vérifié dans Chrome headless (double-clic `u09wj` → `u09w`, clic simple sans effet sur la zone, ctrl-clic `u09ty` → `u09t`). |
| 28/10/2026 | 9 | Bug signalé : le sélecteur de niveau de la carte ne suivait pas la sélection. L'overlay règle le niveau de la carte sur celui de la cellule de la console (`#level=`), et la carte affiche les subdivisions après un changement de zone (bitchat-geohash 0.2.1, qui rend aussi le changement de zone 0,5 s au lieu de 8 à 10 s). Vérifié dans Chrome headless : niveau 4 mis à la main → 5 au clic d'une cellule ; double-clic `u09wj` → `u09w` en 0,5 s, niveau 5. |
| 28/10/2026 | 7 | Demande de l'utilisateur : mettre en sourdine les zones adjacentes. Option d'affichage « mute the 8 neighbours » (le fil ne montre que le centre, voisines estompées, la carte n'encadre que le centre) ; les voisines restent suivies (compteurs, DM, clic). Vérifié dans Chrome headless avec deux consoles sur le relais de l'utilisateur. |
| 28/10/2026 | 13–17 | Analyse d'[EVOLUTION_01-UI.md](EVOLUTION_01-UI.md) : lots 13 à 17. Décisions de l'utilisateur : D31 (historique `history.sqlite`, actif par défaut, masqués et groupes compris), D32 (même pseudo partout, envoi au choix par cellule), D33 (cron : sauvegarde, compactage, rotation à −2 h, purge, effacement). Vérifié dans l'app : une clé de cellule vaut `HMAC(graine, geohash)`, stable tant que l'utilisateur ne fait pas d'effacement d'urgence ; IA locale, donc pas de masquage des clés. |
| 28/10/2026 | 13, 15 | D33 validée ; pas de pseudo envoyé = pas de tag `n` (validé) ; la console qui change de cellule ne quitte pas ses échanges. Écart signalé par l'utilisateur sur `anon` et le pseudo des DM : l'app garde le dernier pseudo vu par clé (cache mémoire, vidé au changement de canal), d'où un pseudo encore affiché après qu'il n'est plus envoyé, et dans les DM. |
| 28/10/2026 | 13 | Lot 13 : champs qui gardent leur défilement, champs de message en `textarea` qui grandit (Entrée envoie, Maj+Entrée va à la ligne) ; pseudo dans `console.sqlite` (repris une fois du navigateur), case par cellule pour ne pas l'envoyer (D32), pseudo du moment pour les préenregistrés, planifiés et groupes. 2 tests Rust ajoutés (base, envoi et redémarrage), routes testées ; 14 tests `node`. Chrome headless sur le binaire réel (`u09tv` téléporté, démon Tor, graine jetable, rien envoyé) : 12 vérifications, dont un défaut trouvé et corrigé (le champ du pseudo gardait l'ancienne valeur à la reconstruction ; un champ qui montre l'état de la console ne garde que ce que l'on tape). |
| 28/10/2026 | 14 | Lot 14 : `history.sqlite` (D31), écriture sur un fil à part, WAL, `0600` pour la base, le WAL et la mémoire partagée, recréation en marche, `since` contre le retour de messages purgés, relecture des 24 h dans le fil, `--no-history`. 3 tests (`tests/history.rs` : tout est gardé, masqués compris, DM et états ; effacement en marche ; retour dans le fil, effacés et trop anciens) et groupes dans `tests/groups.rs` ; 68 tests Rust, 14 `node` ; clippy propre avec et sans `arti`. Binaire réel par le démon Tor (`u09tv` téléporté, rien envoyé) : fichiers en `0600`, WAL, base recréée 2 s après son effacement. |
| 28/10/2026 | 15 | Lot 15 : départ après 10 min sans présence ni message (`/api/present`, calcul dans la page), messages grisés, conversations terminées rangées, effacement des DM (un ou tous, gardés effacés après redémarrage, contenu effacé dans l'historique). Tests : effacement et redémarrage (`tests/dm.rs`), présence (`tests/local.rs`), routes, 1 test `node` ; 70 tests Rust, 15 `node` ; clippy propre. |
| 28/10/2026 | 16 | Lot 16 : onglet d'historique (filtres, tri, sélection, effacement, en direct), changements faits ailleurs repris dans le fil (trigger, `data_version`), API CRUD HTTP de l'historique. Module `history` restructuré autour de `HistoryFile` (partagé avec l'outil du lot 17). Défaut trouvé dans Chrome : la recherche « NUMÉRO » ne trouvait pas « numéro » (`LIKE` ne replie que l'ASCII) → fonction `contains_folded`. 74 tests Rust, 15 `node` ; clippy propre avec et sans `arti` ; Chrome headless sur le binaire réel : lien, filtres, recherche, effacement, effacés, modification externe visible en direct. |
| 28/10/2026 | 17 | Lot 17 : outil `bitchat-history` (requêtes, édition, effacement, sauvegarde, rotation D33, effacement complet, ligne cron), `SKILL.md`, README. Essai réel : rotation pendant que la console tourne, écriture juste après, aucune erreur. Deux défauts trouvés et corrigés (noms de copie à la seconde près, `since` qui reculait). 76 tests Rust, 15 `node` ; clippy propre avec et sans `arti`. |
| 29/10/2026 | — | Tag `v0.1.0` posé (version 0.1.0), à la demande de l'utilisateur. |
