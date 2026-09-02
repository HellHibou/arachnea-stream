# Analyse — Application à chaud du port, du mode réseau et de la racine d'entrée (Option C)

## 1. Demande et objectif

Actuellement, les réglages serveur **Port du serveur**, **Mode réseau** (`local` / `private` / `public`)
et **Racine d'entrée** (`entrypoint_root`) sont persistés dans `data/config.json` par l'opération
admin `update-settings`, mais ne prennent effet qu'au **redémarrage complet du processus**
(`restart_required: true`). Le bouton « Recharger » ne reconstruit que le scraper.

L'option C vise à **appliquer ces trois réglages à chaud**, sans redémarrer le processus, en
reconstruisant et re-liant le serveur HTTP REST à la volée.

## 2. État actuel (constat de départ)

### 2.1 Cycle de vie du contrôleur REST

Tout est figé à la construction et au lancement de `RestControlerService`
(`server/crates/arachnea-core/src/controler/rest/service.rs`) :

| Élément | Où il est figé | Référence |
|---|---|---|
| Adresse/port de binding | `RestControlerService::new` calcule `socket_addr` depuis la config | `service.rs:63` |
| Filtre ACL réseau | `make_client_acl_filter` clone `allowed_networks` **par route** à l'enregistrement | `service.rs:189-218` |
| Préfixe de racine d'entrée | `make_base_filter` embarque `entrypoint_root: Vec<String>` dans chaque filtre warp ; la réécriture `<base href>` des assets HTML utilise aussi ce préfixe | `service.rs:245-273`, `service.rs:165` |
| Redirection `/` → `/{root}/` | `add_root_redirect`, ajouté une fois au `launch` | `service.rs:228-243`, `service.rs:681-687` |
| Binding du listener | `launch()` clone le routeur une fois, spawn un thread dédié avec son propre runtime tokio, `warp::serve(...).bind_with_graceful_shutdown(...)` | `service.rs:681-708`, `service.rs:415-445` |
| Tray serveur | Construit **une fois** au `launch` avec URL/mode réseau dérivés de `socket_addr` + `entrypoint_root` | `service.rs:714-753` |

L'enregistrement des routes se fait **avant** `launch()` et n'est pas rejouable :

- routes admin : `register_admin_service(&admin_state, controler.as_mut())` (`server/crates/arachnea-stream/src/main.rs:440`) ;
- routes stream/scraper : `reloadable.register_service(controler.as_mut())` (`main.rs:431`) ;
- montages web (`admin` + racine) : dans `create_application_controler_from_config`
  (`server/crates/arachnea-core/src/controler/mod.rs:989-994`).

### 2.2 Conséquences par réglage

| Réglage | Impact socket | Impact routeur | Impact client |
|---|---|---|---|
| `private` ↔ `public` | Aucun (bind `0.0.0.0` dans les deux cas) | Filtre ACL uniquement | Aucun |
| vers/depuis `local` | Oui (loopback ↔ `0.0.0.0`) | Filtre ACL | URL change potentiellement |
| Port | Oui (rebind obligatoire) | Aucun | L'UI admin parle à l'ancien port ; elle doit être redirigée |
| Racine d'entrée | Aucun | Tous les filtres de routes + `<base href>` + redirection racine | L'URL de l'UI admin change (`/{root}/admin/`) ; redirection nécessaire |

Le mode `local` exige de plus des privilèges élevés, validés **au moment de la requête**
(`op_update_settings`, `server/crates/arachnea-stream/src/admin/ops.rs:717-727`) via
`context.remote_addr()` + `application::is_running_elevated()`.

### 2.3 Ce qui existe déjà et peut être réutilisé

- `ShutdownSignal` : signal d'arrêt gracieux partagé (tray « close »), attendu par
  `bind_with_graceful_shutdown` (`service.rs:703-708`).
- Le serveur HTTP tourne sur **son propre thread** (`server_thread`), indépendant de la boucle
  main-thread et du tray : un redémarrage du serveur n'a pas à tuer le processus ni le tray.
- `ApplicationConfiguration` est déjà persistée et validée (`configuration.rs`) ; `AdminState`
  conserve déjà la configuration persistée à jour (`state.set_configuration`).


## 3. Approches envisagées

### Approche 1 — Supervisor interne : reconstruction + rebind complets

Extraire la boucle serveur de `launch()` dans un **supervisor** qui possède la liste des étapes
d'enregistrement (closures rejouables) et un fournisseur de configuration :

1. demande d'application des réglages (canal depuis `op_update_settings`) ;
2. arrêt gracieux du serveur courant (réutilise `ShutdownSignal`) ;
3. reconstruction d'un `RestControlerService` neuf en rejouant les enregistrements ;
4. rebind du listener avec la nouvelle config ;
5. mise à jour des réglages runtime (`AdminState`) et du tray.

- **Avantages** : couvre les trois réglages avec un seul mécanisme ; cohérence garantie
  (routeur, ACL, base href et redirection dérivés de la même config) ; réutilise le thread
  serveur isolé et le shutdown gracieux existants.
- **Inconvénients** : refonte du câblage (`main.rs` + `create_application_controler_from_config`)
  pour rendre les enregistrements rejouables ; fenêtre d'indisponibilité courte pendant le
  rebind ; cycle de vie du tray à traiter ; changement structurel dans `arachnea-core`
  (crate partagée).

### Approche 2 — Filtres dynamiques sans rebind (partielle)

Rendre `allowed_networks` et le préfixe de base **dynamiques** (état partagé `ArcSwap`/`RwLock`
lu par requête) au lieu de les figer dans chaque filtre warp.

- **Avantages** : `private` ↔ `public` instantané et sans coupure ; pas de rejeu des routes.
- **Inconvénients** : dans warp, les filtres composés capturent leur préfixe de chemin à
  l'enregistrement ; rendre le préfixe dynamique revient à implémenter une couche de
  réécriture de chemin manuelle (sémantique 404/redirections à réécrire) — risque élevé de
  régressions subtiles. **Ne couvre ni le port ni le mode `local`** (rebind inévitable).

### Approche 3 — Hybride (non retenue)

Combiner 1 et 2 : ACL dynamique pour `private` ↔ `public`, rebind pour le reste. Le gain se
limite au seul cas `private` ↔ `public`, face au coût de maintenir deux mécanismes. En
pratique, dès qu'un réglage socket ou la racine change, le rebind **et** le rejeu du routeur
sont de toute façon nécessaires (warp lie le filtre au `bind`) ; l'hybride se ramène donc à
l'Approche 1 pour l'essentiel.

**Décision : Approche 1 (supervisor) validée**, en acceptant une coupure de l'ordre de quelques
millisecondes à chaque application. La variante « ACL dynamique » reste à arbitrer — informations
détaillées en section 3.4.

### 3.4 Point 3 en détail — ACL dynamique vs rebind uniforme (tranché : rebind uniforme)

**Décision : ACL dynamique écartée.** L'Approche 1 (rebind uniforme) est le seul mécanisme
d'application. Le contenu ci-dessous est conservé pour traçabilité de l'arbitrage. Nuance
importante ajoutée : le cycle complet n'est déclenché **que si l'adresse de bind change**
(port, ou passage loopback ↔ `0.0.0.0`) ; les changements de mode réseau et de racine
rejouent les services **sur le même listener**, sans rebind ni coupure (voir 4.1 et 4.2).

**Ce que fait le rebind uniforme (Approche 1 seule).** Chaque application de réglages passe par le
cycle complet du supervisor (arrêt gracieux → rejeu des enregistrements → rebind), même quand seul
le filtre ACL change (`private` ↔ `public`). Un seul chemin de code, toutes les valeurs strictement
cohérentes après application ; coût : quelques centaines de millisecondes d'indisponibilité et
coupure des connexions en cours après le délai gracieux.

**Ce que permet l'ACL dynamique.** Le filtre ACL capture aujourd'hui une copie figée des réseaux
autorisés à chaque enregistrement de route (`make_client_acl_filter`, `service.rs:189-218` :
`let networks = self.allowed_networks.clone()`). Le rendre dynamique consiste à :

1. remplacer le champ `allowed_networks: Vec<IpNet>` par un état partagé
   `Arc<ArcSwap<Vec<IpNet>>>` (ou `RwLock`) dans `RestControlerService` ;
2. capturer l'`Arc` (et non la valeur) dans `make_client_acl_filter` et faire un `load()` par
   requête — coût par requête négligeable (un chargement atomique) ;
3. ajouter un mutateur (ex. `set_allowed_networks`) appelé par le supervisor.

**Gains.** `private` ↔ `public` s'applique instantanément, sans coupure, sans rejeu des routes et
sans fenêtre de rebind : les règles d'acceptation/rejet changent dès la requête suivante.

**Limites.** Ne couvre ni le port, ni le mode `local` (l'adresse de binding change :
loopback ↔ `0.0.0.0`) : dans ces cas le cycle complet reste requis.
Contrairement au préfixe de chemin (structurel dans warp), l'ACL est une simple donnée capturée :
la modification est propre, localisée à `service.rs`, sans risque de régression sur le routage.

**Coûts/risques supplémentaires.**

- deux mécanismes d'application à maintenir et tester (swap ACL vs cycle complet), avec une logique
  de choix : « si seul l'ACL change → swap ; sinon → cycle complet » ;
- risque de divergence transitoire entre l'état ACL courant et la configuration persistée si un
  cycle complet échoue à mi-parcours (à couvrir par la même logique de repli que le supervisor) ;
- la réponse `update-settings` doit refléter le mécanisme réellement utilisé, sinon le message
  « appliqué » masquerait l'existence d'une coupure.

**Effort estimé.** Modeste et bien borné : trois points de modification dans `service.rs` (champ,
filtre, mutateur), le branchement dans le supervisor et un cas de test dédié.

**Recommandation (historique).** Implémenter d'abord l'Approche 1 seule (un seul chemin
d'application), puis ajouter l'ACL dynamique en phase 2 optionnelle si nécessaire.
**→ Tranché : ACL dynamique écartée, rebind uniforme seul, avec application conditionnelle
(aucun rebind si aucun des trois réglages ne change).**

## 4. Conception détaillée de l'approche recommandée

### 4.1 `arachnea-core` — supervisor REST

- Nouveau type `RestServerSupervisor` (ou équivalent) dans
  `server/crates/arachnea-core/src/controler/rest/` qui détient :
  - la fabrique de configuration (`Arc<dyn Fn() -> RestControlerConfiguration>`),
  - les étapes d'enregistrement rejouables `Vec<Box<dyn Fn(&mut RestControlerService) + Send + Sync>>`,
   - le **listener TCP possédé par le supervisor** : pour pouvoir rejouer l'enregistrement des
     services sans coupure, le serveur tourne sur une boucle d'acceptation propriétaire qui
     dispatch chaque requête vers l'instantané courant du routeur
     (`warp::service(routeur_courant)` + accept loop hyper) ; les requêtes déjà exécutées
     terminent sur l'instantané sous lequel elles ont démarré ;
  - le handle du thread serveur courant + son `ShutdownSignal`.
- `launch()` délègue au supervisor : premier démarrage = enregistrement + bind, comportement
  inchangé.
- Nouvelle méthode, exposée via un handle `Arc` partagé (ex.
  `CoreApplicationOptions::rest_server_handle`) : `apply_configuration() -> Result<AppliedReport>` :
  1. construire la nouvelle config via la fabrique ;
  2. **pré-vérifier le bind** (`TcpListener::bind` immédiat puis drop) pour détecter tôt un port
     occupé — réduit fortement (sans l'annuler) la fenêtre d'échec post-arrêt ;
  3. arrêt gracieux du serveur courant (délai maximal borné, ex. 2 s) ;
  4. reconstruction + rejeu des enregistrements + rebind ;
  5. en cas d'échec de rebind : tenter de rebinder **l'ancienne** configuration ; si cela échoue
     aussi, état d'erreur explicite (le processus reste vivant, l'API admin expose l'échec —
     contrat à documenter).
- **Application conditionnelle** : `apply_configuration` compare d'abord la nouvelle configuration
  à la configuration courante.
   - Si l'adresse de bind est **inchangée** : aucun rebind, aucune coupure HTTP — l'enregistrement
     des services est **rejoué sur le même listener** et les requêtes suivantes utilisent le nouveau
     routeur ;
   - Si le port ou le mode `local` change l'adresse de bind : cycle complet (arrêt gracieux → rejeu
     → rebind).
- Montages web et `add_root_redirect` doivent être rejoués : les déplacer dans les étapes
  d'enregistrement (la racine est relue depuis la fabrique de config).
- Tray : la mise à jour du systray lors du rechargement fait l'objet de la section 4.5
  (dans le périmètre).

### 4.2 `arachnea-stream` — déclenchement et état

- `CoreApplicationOptions` (ou équivalent) reçoit un canal d'application des réglages
  (`watch`/`mpsc`) que `op_update_settings` alimente **après** avoir envoyé la réponse : spawn
  d'une tâche avec un délai fixe de **1 seconde**, défini comme constante nommée en tête du fichier
  `.rs` déclencheur (ex. `const APPLY_SETTINGS_DELAY: Duration = Duration::from_millis(1000);` dans
  `server/crates/arachnea-stream/src/admin/ops.rs`), pour laisser la réponse HTTP s'écouler sur le
  listener encore vivant.
- Après application réussie : mise à jour de `AdminRuntimeSettings` (`AdminState.settings`) avec
  les nouvelles valeurs effectives et leurs provenances (`Configuration`), afin que `settings`
  (GET) reflète le serveur courant.
- La réponse `update-settings` évolue : `restart_required` reste (compat) et s'accompagne de
  `applied: bool`, `apply_error: Option<String>` et `admin_url: Option<String>` (nouvelle URL de
  l'UI admin lorsque port/racine changent).
- Mode desktop (Tauri) : port/réseau/racine ne s'y appliquent pas ; `op_update_settings` ne
  déclenche l'application que si le contrôleur est le contrôleur REST (mode serveur).

### 4.3 Frontend (`front/admin-app`)

- `SettingsView.vue` : après `update-settings` :
  - si `applied` avec changement de port ou de racine : message de succès + **redirection** vers
    `admin_url` (nouvelle origine/chemin) ;
  - si erreur d'application : afficher `apply_error` ;
  - sinon comportement actuel. Utiliser enfin la clé i18n existante `settings.restartRequired`
    (`fr.json:88`) ou la remplacer par un libellé « appliqué à chaud ».
- `adminApi.ts` : étendre `UpdateSettingsResponse` (`applied`, `apply_error`, `admin_url`).

### 4.4 Contrats et documentation

- Mettre à jour `docs/specifications/arachnea-stream-fr.md` / `-en.md` (table des opérations
  admin, section `update-settings`).
- `CHANGELOG.md` (nouvelle entrée) et `docs/TODO.md` si des restes sont trackés
  (ex. extension de l'action tray « Reload configuration »).

### 4.5 Mise à jour du systray lors du rechargement (dans le périmètre)

**État actuel** (`server/crates/arachnea-core/src/controler/rest/tray.rs`) :

- `ServerTrayHandleImpl` stocke `server_url` et `admin_url` en simples `String` immuables, utilisés
  par `open_browser` / `open_admin` (`tray.rs:364-368`, `tray.rs:497-503`) ;
- les deux lignes d'information du menu (`Server: {url}`, `Network: {mode}`) sont construites une
  fois dans le `setup` Tauri comme items désactivés, et leurs handles `MenuItem` ne sont pas
  conservés (`tray.rs:581-589`) ;
- le tray est créé **au plus une fois** : `ServerTrayIconService::spawn_tray` consomme (take) le
  `tauri::Context`, non clonable (`tray.rs:769-777`) → **recréer le tray n'est pas possible** sans
  refonte du cycle de vie Tauri ;
- l'action « Reload configuration » du tray ne recharge que le scraper (callback
  `tray_reload_summary` fourni par `main.rs`).

**Mécanisme retenu — mise à jour sur place, sans recréation :**

1. conserver les handles `MenuItem<Wry>` des deux lignes d'information dans
   `ServerTrayHandleImpl` (remplis au `setup`) ;
2. protéger `server_url` / `admin_url` par `Mutex<String>` ;
3. étendre le trait `ServerTrayHandle` d'une méthode
   `update_configuration(update: ServerTrayUpdate)` avec
   `ServerTrayUpdate { server_url, admin_url, server_display_url, network_mode }` ;
4. l'implémentation Tauri met à jour les URL stockées puis applique `set_text(...)` sur les items ;
   les opérations de menu étant liées au main thread Tauri, passer par
   `AppHandle::run_on_main_thread` (impératif sur macOS où le tray tourne sur le main thread,
   prudent sur Windows/Linux).

**Effet.** Après chaque application à chaud, le menu tray affiche la nouvelle URL et le nouveau
mode réseau, et les actions « Open in browser » / « Open administration » pointent vers les
nouvelles URL.

**Points d'attention.**

- `set_text` sur un item désactivé est supporté par Tauri/muda ; vérifier le comportement sur
  chaque plateforme cible (Windows/Linux/macOS) lors de l'implémentation ;
- si le supervisor échoue et revient à l'ancienne configuration, le tray doit refléter la
  configuration réellement active : la mise à jour du tray est pilotée par le résultat de
  `apply_configuration` ;
- hors périmètre (inchangé) : étendre l'action « Reload configuration » du tray pour déclencher
  aussi l'application des réglages en attente — restes trackés éventuels dans `docs/TODO.md`.

## 5. Risques et points d'attention

1. **Fenêtre de rebind** : entre l'arrêt de l'ancien listener et le bind du nouveau, le port est
   libre → un autre processus pourrait le prendre. La pré-vérification (4.1 étape 2) réduit le
   risque sans l'éliminer ; échec = repli sur l'ancienne config, sinon état d'erreur critique.
2. **Requêtes en cours** : les routes de streaming sont longues ; l'arrêt gracieux doit être borné
   (timeout) et coupera les flux actifs. À documenter comme comportement attendu.
3. **Requête déclenchante** : `update-settings` est servie par le serveur qu'on redémarre ; la
   réponse doit être émise avant l'arrêt (délai post-réponse). Le frontend doit tolérer une
   connexion réinitialisée et retenter.
4. **Changement d'URL admin** : si la racine ou le port change, l'onglet courant pointe vers une
   URL qui va mourir ; la redirection portée par `admin_url` est indispensable, sinon l'utilisateur
   perd la main (accepter aussi le cas où le client ne peut plus joindre le serveur du tout).
5. **Mise à jour du tray** : traitée sur place via `ServerTrayHandle::update_configuration`
   (section 4.5) ; le marshalling main-thread est requis (impératif sur macOS). En cas d'échec
   d'application avec repli, le tray doit refléter la configuration réellement active.
6. **Crate partagée** : `arachnea-core` est commune aux backends REST et Tauri ; toute évolution
   du trait `ControlerService` doit rester neutre pour le backend desktop.
7. **macOS** : le tray tourne sur le main thread et bloque `launch()` (`service.rs:760-778`) ;
   le supervisor doit préserver cette répartition (serveur sur son thread, main thread intact).
8. **Double application** : protéger le supervisor contre les demandes concurrentes (verrou /
   état « restarting ») et les demandes pendant le premier démarrage.
9. **Dispatch par requête** : le rejeu sans rebind résout l'instantané courant du routeur pour
   chaque requête HTTP, y compris sur une connexion keep-alive ; les requêtes déjà en cours
   terminent sur l'ancien instantané.

## 6. Plan de mise en œuvre (statut après implémentation)

Toutes les étapes prévues sont réalisées ; la section 8 détaille ce qui a été fait
et les écarts techniques découverts en cours d'implémentation.

1. **Refactor neutre** — ✅ fait : supervisor extrait dans
   `server/crates/arachnea-core/src/controler/rest/supervisor.rs`, premier lancement
   identique, tests existants au vert (`cargo test -p arachnea-core --lib`).
2. **Rejeu des enregistrements** — ✅ fait : enregistrements rejouables via
   `RestControlerService::record_step` (routes sérialisées, JSON, stream, web dir,
   web assets) + montages web déplacés dans les étapes rejouables.
3. **Mécanisme d'application** — ✅ fait : `RestServerSupervisor::apply` avec
   application conditionnelle (rebind seulement si l'adresse de bind change),
   arrêt gracieux borné, délai post-réponse `APPLY_SETTINGS_DELAY` (1 s, constante
   nommée en tête d'`ops.rs`), pré-vérification de bind, repli sur erreur explicite.
4. **Intégration admin** — ✅ fait : déclenchement depuis `op_update_settings`,
   mise à jour des réglages runtime (`AdminState::update_effective_server_settings`),
   réponse étendue `{restart_required, applied, apply_error, admin_url}`.
5. **Systray** — ✅ fait : `ServerTrayHandle::update_configuration` +
   `ServerTrayUpdate`, mise à jour des items via `run_on_main_thread`, publication
   du handle côté macOS via un `handle_sink`.
6. **Frontend** — ✅ fait : réponses `applied`/`apply_error`/`admin_url` gérées dans
   `SettingsView.vue` (message « appliqué à chaud », erreur, redirection) + i18n fr/en.
7. **Documentation** — ✅ fait : spécifications admin (`arachnea-stream-fr/en.md`),
   `CHANGELOG.md`, `docs/TODO.md` (reste tracké : extension de l'action tray
   « Reload configuration »).

## 7. Décisions arrêtées

| Point | Décision |
|---|---|
| Approche | Approche 1 (supervisor interne : reconstruction + rebind) **validée** |
| Systray | Mise à jour sur place lors du rechargement, **dans le périmètre** (section 4.5) ; pas de recréation |
| Délai post-réponse | Fixe, **1 seconde**, en constante nommée en tête du fichier `.rs` déclencheur (`ops.rs`) |
| Échec définitif de rebind | État d'erreur explicite conservé et exposé via l'API admin (`apply_error`) ; le processus reste vivant |
| ACL dynamique (`private` ↔ `public`) | **Écartée** — rebind uniforme (Approche 1) uniquement |
| Application conditionnelle | Rebind seulement si l'**adresse de bind** change (port, ou loopback ↔ `0.0.0.0`) ; les changements ACL et racine rejouent les routes sur le même listener. Le routeur est résolu par requête HTTP, donc une connexion keep-alive utilise la racine courante à sa requête suivante. |

Mise en œuvre terminée — voir la section 8 pour le bilan détaillé et les écarts
techniques par rapport au plan initial.

## 8. Bilan d'implémentation (fait)

### 8.2 Écarts techniques par rapport au plan (découverts à l'implémentation)

1. **Signal de shutdown en deux niveaux.** Le plan supposait un `ShutdownSignal`
   partagé. En pratique le rebind s'appuyait sur le même signal que le tray/la boucle
   main-thread et arrêtait tout le processus. L'implémentation sépare donc :
   - le signal **d'instance** (boucle d'acceptation courante), recréé à chaque rebind ;
   - le signal **d'application** (`app_shutdown`), créé une fois au premier lancement et
     conservé, partagé avec le tray et la boucle main-thread. La boucle d'acceptation
     s'arrête sur l'un ou l'autre.
2. **Perte de l'adresse pair avec `warp::service`.** Le dispatch par connexion perd
   l'adresse TCP distante (l'implémentation `Service` de warp appelle `call_with_addr`
   avec `None`). Sans elle, le contrôle de réseau `local` (qui exige un client loopback)
   et l'ACL `private` se cassaient. Solution : la boucle d'acceptation propage l'adresse
   pair via l'en-tête contrôlé `x-arachnea-peer-addr` (strippé puis réinjecté à chaque
   requête, d'où une impossibilité de forger l'adresse de loin), et `peer_addr_filter`
   fusionne en-tête injecté + `warp::addr::remote()`.
3. **Condition de rebind affinée.** Le rebind est requis seulement si l'**adresse de
   bind** cible change (port ou passage loopback ↔ `0.0.0.0`). Les changements de
   racine et `private` ↔ `public` restent un rejeu sans coupure : la boucle
   d'acceptation conserve le listener et résout le routeur courant pour chaque requête
   HTTP, y compris sur une connexion keep-alive.
4. **Segments racine vides.** `warp::path("")` panique : une racine `/arach` (double
   barre ou espaces) produisait des segments vides. Les segments sont filtrés (constructeur
   et `root_segments()`).
5. **Normalisation racine vide.** La config persistée peut contenir
   `entrypoint_root: ""`. Elle est normalisée en `None` dès la résolution (résolveur
   applicatif, `normalize_target`, comparaison d'`ops.rs`) pour éviter des rebinds
   inutiles et de faux positifs.
6. **`admin_url` conditionné au réel changement effectif.** Le plan renvoyait une URL
   cible brute. L'implémentation résout la cible applicable (épinglages CLI inclus) et ne
   renvoie `admin_url` que si le port ou la racine bougeront réellement par rapport à
   l'état effectif — évite un faux renvoi quand `--network` est épinglé, par exemple.
7. **Fabrique d'instantané au lieu d'un fournisseur de config.** Le plan prévoyait
   `Arc<dyn Fn() -> RestControlerConfiguration>`. L'implémentation garde un
   `RestServiceSnapshot` (config initiale) + `RestServerSettings` (les trois réglages
   dynamiques) ; la seule fabrique applicative est le `RestSettingsSource`.
8. **macOS / tray.** Publication du handle via `handle_sink` pendant le `setup` Tauri
   (`run_on_main_thread` bloque le thread principal avant de pouvoir retourner le handle).

**Créé**

- `server/crates/arachnea-core/src/controler/rest/supervisor.rs` : `RestServerSupervisor`,
  `RestServerHandle`, `RestServerSettings`, `RestServerApplyReport`, `RestSettingsSource`,
  le listener/boucle d'acceptation propriétaires (`spawn_accept_thread` /
  `serve_connection`), la pré-vérification de bind, le repli sur erreur et la mise à jour
  du tray pilotée par le résultat de l'application. Contient aussi l'en-tête contrôlé
  `PEER_ADDR_HEADER` (`x-arachnea-peer-addr`) utilisé pour propager l'adresse pair.

**Modifiés (backend)**

- `server/crates/arachnea-core/src/controler/rest/service.rs` : `RestServiceSnapshot`
  (config initiale reconstruisible) + `ReplayStep` (enregistrement rejouable) ;
  `record_step` branché sur toutes les méthodes `ControlerService`
  (sérialisé, JSON, stream, `register_web_directory`, `register_embedded_web_assets`) ;
  `from_snapshot`/`finalize_router` pour le rebuild ; `peer_addr_filter` remplace
  `warp::addr::remote()` (filtre admin et ACL) ; `run_server`/`warp::serve` supprimés —
  `launch()` délègue le listener au supervisor.
- `server/crates/arachnea-core/src/controler/rest/mod.rs` : export du module
  `supervisor` et de ses types publics.
- `server/crates/arachnea-core/src/controler/mod.rs` : `rest_server_handle()` (défaut
  `None` sur le trait `ControlerService` — neutre pour le backend desktop),
  re-exports, accesseur `RequestControlerContext::header()`.
- `server/crates/arachnea-core/src/controler/rest/tray.rs` : `ServerTrayUpdate`,
  `ServerTrayHandle::update_configuration`, `ServerTrayConfiguration::handle_sink`
  (publication du handle Tauri vers le supervisor, indispensable sur macOS où
  `run_on_main_thread` bloque), items d'information du menu conservés pour `set_text`.
- `server/crates/arachnea-stream/src/admin/ops.rs` : constante
  `APPLY_SETTINGS_DELAY: Duration = Duration::from_millis(1000)` en tête de fichier,
  déclenchement applicatif différé, `hot_apply_admin_url` (host depuis l'en-tête
  `Host`, port et racine cibles), `admin_url` conditionné au changement réel.
- `server/crates/arachnea-stream/src/admin/dto.rs` : `UpdateSettingsResponse` étendu
  (`applied`, `apply_error`, `admin_url`).
- `server/crates/arachnea-stream/src/admin/mod.rs` : `AdminState.rest_server`,
  `update_effective_server_settings`, `configuration()` rendue publique.
- `server/crates/arachnea-stream/src/main.rs` : épinglages CLI capturés avant le
  constructeur du contrôleur, résolveur `RestSettingsSource` (CLI épinglé prioritaire,
  sinon config persistée), normalisation de la racine vide.

**Modifiés (frontend et docs)**

- `front/admin-app/src/services/adminApi.ts` : interface `UpdateSettingsResponse`.
- `front/admin-app/src/views/SettingsView.vue` : message « appliqué à chaud »,
  affichage de `apply_error`, redirection différée vers `admin_url` (port/racine
  changent), sinon rechargement local.
- `front/admin-app/public/locales/{fr,en}.json` : libellés `appliedHot`,
  `redirectNotice`, description mise à jour.
- `docs/specifications/arachnea-stream-{fr,en}.md`, `CHANGELOG.md`, `docs/TODO.md`.

