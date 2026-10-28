# bitchat-console : spécifications (projet)

Version de travail du 28/10/2026, à valider. Les décisions ouvertes sont dans la dernière
section (D1 à D14).

## Objectifs

Un client BitChat « console » pour ordinateur, dans la continuité de
[bitchat-geohash](../bitchat-geohash/SPECIFICATIONS.md) :

1. Se placer sur un geohash et suivre ce canal **et ses 8 voisins** (même précision).
2. Pour chacune des 9 cellules, prendre les relais que choisissent **iOS et Android** (union
   des deux sélections, comme bitchat-geohash), et y écouter les kinds 20000 et 20001.
3. **Écrire** dans ces canaux : messages texte, liens (dont liens d'images).
4. **Tor obligatoire** vers les relais ; **une clé par geohash**, comme l'app.
5. Masquer un intervenant (mute, démasquer, ou l'isoler dans une fenêtre dédiée).
6. Messages privés (DM) à un intervenant, compatibles avec l'app.
7. Messages préenregistrés, envoyés sur demande ou à une heure programmée.
8. Groupes : voir D6 (les groupes de BitChat 1.8 ne passent pas par Nostr).
9. Même approche CLI que bitchat-geohash ; serveur web interne, affichage dans le navigateur ;
   fenêtre console superposable à la carte de bitchat-geohash et qui suit le geohash choisi.
10. Plus tard : stratégies de sélection des relais interchangeables (AS, puis A0 à A6 de
    `bitchat-envelope/Documents/geohash-relays-requirements-and-alternatives.md`), dans une
    **crate commune** isolée, à proposer ensuite aux mainteneurs de BitChat iOS et Android.

## Ce que fait l'app (rétro-ingénierie, iOS 1.8.0)

Sources : dépôt local `~/Developments_local/bitchat` (v1.8.0) et le document d'alternatives
ci-dessus (§1), qui couvre aussi Android.

| Sujet | Comportement de l'app | Conséquence pour la console |
|---|---|---|
| Messages de canal | kind 20000, tags `g` (geohash), `n` (pseudo, optionnel), `t`=`teleport` (optionnel), `nonce` (NIP-13) ; vers les 5 relais geohash de la cellule | même format ; écrire vers l'union iOS + Android (D4) |
| Présences | kind 20001, tag `g` seul, toutes les 40 à 80 s, **seulement** aux canaux de la position GPS réelle (niveaux 2, 4, 5) | la console envoie aussi des présences (D5) |
| Abonnement | `kinds:[20000,20001]`, `#g:[gh]`, `since: −1 h`, `limit: 200` | idem, pour les 9 cellules |
| Identité | une clé par geohash : `HMAC-SHA256(graine, geohash ‖ compteur)` (`NostrIdentityBridge.deriveIdentity(forGeohash:)`) | même dérivation, graine propre à la console (D3) |
| Preuve de travail | iOS calcule 8 bits par message (abandon après 2 s) ; Android peut exiger 12 bits et rejeter en dessous | 8 bits par défaut, réglable (D8) |
| Tor | actif par défaut dans les deux apps ; iOS embarque Arti (`localPackages/Arti`, `arti-client` 0.38) et n'envoie rien avant que Tor soit prêt | `arti-client` dans le processus (D1) |
| Messages privés | enveloppe BitChat : kinds 1059 → 13 → 14 (numéros NIP-17/59, **format propre**, préfixe `v2:`), destinataire = clé geohash du correspondant, **sur les relais par défaut** (`relay.damus.io`, `nos.lol`, `relay.primal.net`, `offchain.pub`…), pas sur les relais geohash | reproduire l'enveloppe à l'octet près (D7) |
| Groupes (1.8) | `ChatGroupCoordinator` : invitations et clés signées Noise (paquets `groupInvite` 0x06, `groupKeyUpdate` 0x07), transport `MeshGroupMessaging` = **Bluetooth (BLEService) uniquement** | groupes mixtes Bluetooth + Nostr, mêmes paquets (D6) |
| Paquets BitChat dans Nostr | l'app transporte déjà des paquets BitChat binaires dans ses DM Nostr : `bitchat1:` + base64url d'un `BitchatPacket` (`NostrEmbeddedBitChat`) | support des paquets de groupe par Nostr (D6) |
| Images | transferts de fichiers par le maillage Bluetooth uniquement ; un canal geohash ne transporte que du texte | images = liens (D9) |
| AUTH (NIP-42), CLOSED | ignorés | lus et affichés (la console, elle, sait dire pourquoi un relais refuse) ; AUTH accepté avec la clé de la connexion (D28) |

## Principes

### 1. Zone suivie

* Entrée : un geohash *g* (précision 2 à 7). Les 9 cellules sont *g* et ses 8 voisins de
  même précision. Au bord d'une zone (pôles, antiméridien), moins de 8 voisins.
* Changer de geohash (dans la page, ou depuis la carte de bitchat-geohash) recalcule les 9
  cellules, les relais et les abonnements, sans redémarrer.

### 2. Relais

* Pour chaque cellule : sélection iOS (liste revue) et Android (liste georelays), règles de
  chaque app (celles corrigées au lot 11 bis de bitchat-geohash), union par serveur.
* Le calcul vit dans la **crate commune** (D2), derrière une interface « stratégie » :
  `AS` (simulation des apps) d'abord, A0 à A6 ensuite. Rien d'autre dans la console ne sait
  comment les relais sont choisis.
* Relais par défaut (DM) : ceux de l'app, configurables.

### 3. Lecture

* Une connexion par relais (via Tor), abonnements groupés comme le plan d'écoute de
  bitchat-geohash. Déduplication par id, signature vérifiée.
* Fil par cellule et fil combiné des 9 cellules ; chaque message porte sa cellule, son
  pseudo (`n`) et le suffixe de clé (`#abcd`, comme l'app).

### 4. Écriture

* Message dans la cellule choisie parmi les 9 (centre par défaut), signé avec la clé de cette
  cellule, avec preuve de travail, vers ses relais (D4). Accusé `OK` de chaque relais affiché.
* Pseudo par défaut, modifiable par cellule.
* Présence (D5) : battements kind 20001 toutes les 40 à 80 s, comme l'app, aux niveaux 2, 4
  et 5 de la cellule choisie, tant que la console est ouverte sur ce geohash.
* À chaque changement de geohash, l'utilisateur choisit si la console s'y annonce téléportée
  (`t=teleport` sur messages et présences) ou non.

### 4 bis. Groupes mixtes Bluetooth + Nostr (D6)

* Même modèle que les groupes de BitChat 1.8 : groupe créé et signé par son créateur, état du
  groupe (membres, clé) diffusé par invitation et mise à jour de clé, messages chiffrés avec
  la clé du groupe.
* Même format binaire que l'app (paquets `groupInvite`, `groupKeyUpdate`, messages de
  groupe), transporté par Nostr dans l'enveloppe privée BitChat, sous forme `bitchat1:`
  comme les DM : une enveloppe par membre.
* La console a donc sa propre identité Noise (clé statique et clé de signature Ed25519), comme
  un pair BitChat.
* Fonctionne entre consoles dès la première version. Ensuite : crate Bluetooth (maillage
  BitChat) pour joindre les membres à portée, puis PR vers BitChat pour que l'app accepte
  aussi le transport Nostr des groupes.

### 5. Masquage

* Mute par clé publique (donc par cellule : une même personne a une clé par geohash).
  Messages cachés, démasquage possible, ou fenêtre dédiée qui ne montre que cet intervenant.
* Liste locale, jamais publiée.

### 6. Messages préenregistrés

* Textes enregistrés, avec cellule cible ; envoi sur demande, ou planifié (date et heure, ou
  répétition). La planification ne vaut que tant que la console tourne.

### 7. Affichage

* Serveur web interne (axum), page servie localement, mises à jour en direct (SSE), commandes
  en `POST` protégées comme dans bitchat-geohash (Host, Origin, CSP), HTTPS optionnel.
* Fenêtre console **superposable** à la carte de bitchat-geohash, qui suit le geohash choisi
  sur la carte (D10).

### 8. Vie privée et sécurité

* Tor obligatoire : aucune connexion à un relais en clair (D1).
* La graine des clés ne quitte jamais la machine ; fichier en `0600` (ou trousseau macOS).
* Le contenu des messages reçus reste en mémoire par défaut (D11).
* Aucun envoi automatique sans action de l'utilisateur, hormis les messages planifiés qu'il a
  lui-même créés.

## Tests

* Hors ligne : relais de test local (comme `TestRelay` de bitchat-geohash) ; Tor contourné
  **seulement** pour une adresse de bouclage, et seulement dans les tests.
* Réseau réel : publication uniquement sur `wss://relay.famillevital.club` avec des clés
  jetables, sauf demande explicite de l'utilisateur.

## Décisions à prendre

| # | Question | Proposition |
|---|---|---|
| D1 | Tor ✅ | Interface « connecteur » unique. D'abord un démon `tor` externe en SOCKS5 (`--tor-socks 127.0.0.1:9050`, Tor Browser : 9150), un identifiant SOCKS par geohash pour isoler les circuits (`IsolateSOCKSAuth`) ; arrêt avec message clair si `tor` est absent, jamais de repli en clair. Ensuite Arti dans le binaire, en feature `arti` |
| D2 | Crate commune ✅ | `geohash-relays` (dépôt propre, `wvmscs/geohash-relays`) : listes, règles iOS et Android, voisins, interface de stratégie ; bitchat-console en dépend par git ; bitchat-geohash y migrera plus tard (avec accord). Liaisons Swift/Kotlin (`uniffi`) pour les PR vers les apps, à étudier |
| D3 | Graine des clés | Graine propre à la console, fichier `0600` dans le dossier de données ; option trousseau macOS ensuite. La graine du téléphone n'est pas importable |
| D4 | Relais d'écriture | Union iOS + Android de la cellule (atteint les deux populations) ; option « comme iOS » ou « comme Android » |
| D5 | Présence ✅ | Présences envoyées comme l'app (40 à 80 s, niveaux 2, 4, 5 de la cellule choisie). Marque `t=teleport` (messages et présences) : choisie à chaque changement de geohash, avec ou sans |
| D6 | Groupes ✅ | Groupes mixtes Bluetooth + Nostr, format des groupes 1.8, transport Nostr `bitchat1:` dans l'enveloppe privée ; entre consoles d'abord ; crate Bluetooth et PR vers BitChat ensuite |
| D7 | DM | Enveloppe BitChat `v2:` (1059/13/14) vers la clé geohash du correspondant, sur les relais par défaut, via Tor ; compatibilité vérifiée avec l'app de l'utilisateur |
| D8 | Preuve de travail | 8 bits comme iOS (12 en option pour les utilisateurs Android qui l'exigent) |
| D9 | Images | Liens seulement à la première version ; dépôt d'images (Blossom ou autre, via Tor) plus tard |
| D10 | Superposition à la carte ✅ (réalisée autrement : D26 de `PLAN_ET1.md`, relais d'une carte lancée à côté) | Le binaire console embarque le moniteur et la carte de bitchat-geohash (dépendance git sur `v0.1.0`) et affiche la console en panneau flottant au-dessus ; un clic sur la carte choisit le geohash |
| D11 | Conservation | Messages reçus en mémoire seulement ; historique local en option |
| D12 | Dépôt ✅ | `wvmscs/bitchat-console`, privé, GPL-3.0-only, autonome (`[workspace]` propre : le `Cargo.toml` du workspace `Nostr/` reste intact) |
| D13 | Notes de lieu (kind 1) | Hors périmètre de la première version |
| D14 | Pont maillage (`r`) et dépôts courrier (1401) | Hors périmètre |
