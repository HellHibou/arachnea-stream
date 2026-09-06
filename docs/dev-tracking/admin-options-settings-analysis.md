# Paramètres admin : exposer `--current-country` et les tailles de cache — analyse (v2)

## Périmètre

Ajouter la gestion des trois options applicatives aujourd'hui limitées à la ligne de commande :

- `--current-country` — surcharge le pays courant de l'application.
- `--cache-max-disk-bytes` — surcharge la taille maximale du cache disque en octets.
- `--cache-max-memory-bytes` — surcharge la taille maximale du cache mémoire en octets.

Plus deux exigences côté front :

1. Réorganiser la page des paramètres admin : le groupe existant (port / root / mode réseau) devient **« Serveur »**, un **nouveau groupe** accueille les trois nouvelles options, et le **bouton Enregistrer est déplacé hors du groupe** (il a un fonctionnement global, pas spécifique à « Serveur »).
2. Corriger et améliorer la fenêtre popup d'administration (desktop uniquement) : elle affiche actuellement une page blanche (régression récente) et doit montrer la coquille admin complète (barre de gauche visible) **sauf** les sections « Mot de passe » et « Serveur », en n'affichant à la place que le nouveau groupe d'options.

## Décisions validées

1. Les trois options restent **dans le crate `arachnea-scrapyfy`** (porteuses par `ScraperRuntimeOptions`), pas dans `CoreApplicationOptions` : elles sont spécifiques à Scrapyfy et leur code (parsing, validation, persistance, application) vit dans ce crate de préférence.
2. Validation du pays : forme seule, 2 lettres A–Z majuscules (la validation sémantique reste au runtime).
3. Tailles de cache : **rechargement à chaud** de la façade avec les nouvelles options (pas de `restart_required`).

## État actuel

### Backend

**Modèle d'options**

- `CoreApplicationOptions` (`server/crates/arachnea-core/src/controler/options.rs`) porte les paramètres persistés « serveur » : `server_port`, `network_mode`, `entrypoint_root`, `password_hash`, chacun avec un champ de provenance `SettingSource`. Il gère le chargement/sauvegarde JSON (`data/config.json`), le parsing CLI + configuration, la validation et l'export.
- `SrcapyfyApplicationOptions` (`server/crates/arachnea-scrapyfy/src/scrapyfy/application_opts.rs`) enveloppe `CoreApplicationOptions` et ajoute les trois options cibles comme champs **CLI uniquement, non persistés** : `current_country: Option<String>`, `cache_max_disk_bytes: Option<u64>`, `cache_max_memory_bytes: Option<u64>`.
- `ScraperRuntimeOptions` (`server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs`, ligne 105) porte aujourd'hui `cache_config: Option<ScraperCacheConfig>` et s'applique via `apply_to(&mut ScraperAgregator)`.
- Application effective au démarrage (`server/crates/arachnea-stream/src/main.rs` vers la ligne 201) : `ScraperRuntimeOptions { cache_config: options.scraper_cache_config() }` → `ReloadableStreamScraper::new(...)` ; puis `options.current_country` → `reloadable.set_current_country(...)`.
- La façade rechargable (`reloadable_stream_scraper.rs`) conserve `runtime_options` et le **ré-applique à chaque rebuild** (ligne 223) — le support du rechargement à chaud avec de nouvelles options existe donc déjà via `rebuild_validated`.

**Pipeline des paramètres admin**

- DTOs (`server/crates/arachnea-scrapyfy/src/admin/dto.rs`) : `SettingsResponse` et `UpdateSettingsRequest` ne couvrent que `server_port`, `entrypoint_root`, `network_mode` (+ champs en lecture seule `password_configured` / `public_http_warning`).
- `op_settings` / `op_update_settings` (`server/crates/arachnea-scrapyfy/src/admin/ops.rs`) : lisent l'instantané des paramètres effectifs, fusionnent la requête dans les options persistées, puis appliquent à chaud les paramètres serveur quand c'est possible.
- `StreamAdminRuntimeAdapter` (`server/crates/arachnea-stream/src/admin_composition.rs`) : implémente `AdminRuntimeAdapter` (settings serveur, credentials, rebuilds). Les surcharges de services sont persistées via le namespace `arachnea-services` du `PersistenceStore` générique — modèle à suivre pour les nouvelles options.

### Front (admin-app)

- `front/admin-app/src/views/SettingsView.vue` : deux cartes « outlined » — « Paramètres » (port, mode réseau, entrypoint root + bouton Enregistrer dans `v-card-actions`) et « Mot de passe administrateur ». L'enregistrement déclenche `updateSettings` et gère la redirection vers la nouvelle URL admin en cas de changement de port/root.
- `front/admin-app/src/services/adminApi.ts` reflète les DTOs backend (`SettingsResponse`, `UpdateSettingsRequest`).

### Fenêtre popup desktop (régression)

- `open_admin_window` (`server/crates/arachnea-core/src/controler/tauri/mod.rs`, ligne 777) charge `{web_scheme}://localhost/admin/` via `WebviewUrl::CustomProtocol` ; les assets sont servis par le handler de protocole custom avec le mount scopé `admin/` (`web_assets.rs`).
- Symptôme rapporté : la popup affiche une page blanche en mode desktop (régression récente). Causes candidates à vérifier pendant l'implémentation :
  - la substitution du marqueur `<base href="{base}">` : le serveur injecte une balise base à l'exécution ; via le schéma custom Tauri, si le marqueur n'est pas remplacé, `getAppBaseDir()` (baseUrl.ts) retombe sur `import.meta.env.BASE_URL` (`./`) résolu contre `.../admin/` — cela *devrait* fonctionner, donc la régression est plutôt à chercher du côté de la résolution des assets sous le mount scopé, la sélection du mount dans `tauri/mod.rs` (vers la ligne 660), ou un changement récent du scopage dans `web_assets.rs`.
  - un échec JS au démarrage (erreur de config, `useAppConfig` en échec via le schéma custom) laissant `isLoading`/un `v-main` vide sans contenu visible.
  - Plan de diagnostic : reproduire en local sur un build desktop, inspecter la console/logs de la webview, et comparer les chemins d'assets servis pour `index.html` sous le mount scopé.

## Impact attendu

- Une nouvelle structure d'options Scrapyfy persistée dans un **namespace dédié du `PersistenceStore`** (ex. `arachnea-scrapyfy`), hors de `data/config.json` : le fichier de configuration core reste limité aux paramètres « serveur ».
- Les DTOs de l'API settings et leur miroir front évoluent (additif : nouveaux champs optionnels uniquement).
- Application à chaud : pays via `set_current_country` ; tailles de cache via mise à jour des `ScraperRuntimeOptions` de la façade puis rebuild (qui les ré-applique).
- La page des paramètres front est réorganisée ; la popup obtient une variante restreinte.

## Implémentation proposée

### Backend (crate `arachnea-scrapyfy`)

1. **Nouvelle structure `ScraperAdminSettings`** (dans `arachnea-scrapyfy`, ex. `scrapyfy/admin_settings.rs`) :
   - Champs : `current_country: Option<String>` (2 lettres A–Z majuscules) et les tailles de cache ; sérialisable (serde) pour le stockage persistant.
   - Règle de résolution de la valeur effective : **CLI > surcharge persistée > défaut**. En pratique, la valeur persistée ne s'applique que si l'option CLI équivalente est absente (les champs CLI de `SrcapyfyApplicationOptions` restent la source de plus haute priorité).
   - Validation : pays = 2 caractères `[A-Z]` (ou vide/absent) ; octets > 0 ; `ScraperCacheConfig::default()` (100 Mio disque / 32 Mio mémoire) quand aucune surcharge.
2. **Persistance** : via le `PersistenceStore` générique sous un namespace dédié (ex. `arachnea-scrapyfy`), sur le modèle des surcharges d'activation `arachnea-services`. Le crate Stream fournit le store ; Scrapyfy fournit le codec/clé et la logique — aucun champ ajouté à `CoreApplicationOptions`.
3. **`ScraperRuntimeOptions`** gagne `current_country: Option<String>` (ou le pays reste porté par la façade via `set_current_country` et seul le cache passe par `ScraperRuntimeOptions` — variante retenue à l'implémentation ; l'important est que la structure vive dans `arachnea-scrapyfy`).
4. **Façade rechargable / Stream** :
   - Nouvelle méthode `set_runtime_options(ScraperRuntimeOptions)` sur `ReloadableStreamScraper` : met à jour les options conservées puis déclenche un rebuild (qui applique les nouvelles options, ligne 223) — c'est le « rechargement du cache avec les nouvelles options » validé.
   - Le pays s'applique via `set_current_country` (déjà propagé à l'instance courante et aux rebuilds via `options.current_country`).
5. **API admin (DTOs + ops, dans `arachnea-scrapyfy::admin`)** :
   - `SettingsResponse` : `current_country: Option<String>`, `cache_max_disk_bytes: Option<u64>`, `cache_max_memory_bytes: Option<u64>` (valeurs effectives/persistées selon disponibilité).
   - `UpdateSettingsRequest` : les trois champs optionnels ; absent = inchangé, chaîne vide du pays ou valeurs nulles = suppression de la surcharge.
   - `op_update_settings` : validation dans Scrapyfy, écriture de `ScraperAdminSettings` via le hook adaptateur, puis application à chaud (pays + rebuild cache). Un nouveau hook `AdminRuntimeAdapter` (ex. `update_scraper_settings(settings)`) est implémenté par `StreamAdminRuntimeAdapter` en appelant la façade rechargable.
   - Les valeurs épinglées CLI gardent la précédence : le hook lit la valeur *effective* (CLI or persisté) et refuse de retirer une surcharge épinglée par CLI (retour `bad_request` ou no-op documenté).

### Front (admin-app)

1. **Groupes et bouton Enregistrer** (`SettingsView.vue`) :
   - Carte 1 renommée « Serveur » (nouvelle clé i18n, ex. `settings.serverGroup`) : port, mode réseau, entrypoint root. Supprimer son bouton Enregistrer dans `v-card-actions`.
   - Nouvelle carte « Application » (titre de travail ; les trois nouvelles options) : champ texte pays courant (2 lettres majuscules, vide = détection automatique), champs numériques cache disque et cache mémoire (vide/0 = défaut 100 Mio / 32 Mio).
   - Un unique bouton global Enregistrer est déplacé au niveau de la page (rangée d'actions sous les alertes) et soumet l'intégralité de `UpdateSettingsRequest` comme aujourd'hui (flux de redirection inchangé).
2. **Variante popup** :
   - Ajouter une route restreinte (ex. `settings-desktop` ou un flag `meta.popup`) affichant la vue des paramètres sans les cartes « Serveur » et « Mot de passe » — uniquement la nouvelle carte « Application ».
   - La coquille (`App.vue`) affiche toujours la barre de navigation gauche ; la garder visible en mode popup (exigence : navigation complète, seules ces deux sections sont exclues).
   - L'URL de la fenêtre Tauri (`ADMIN_WINDOW_OPEN_COMMAND`, `tauri/mod.rs`) pointe vers cette route restreinte au lieu de la racine admin ; le flux de connexion reste intact (la popup conserve la gestion `status`/auth).
3. **Correction de la page blanche** : diagnostiquer selon le plan ci-dessus avant de toucher au code ; le travail sur la route restreinte revalide le service des assets sous le schéma custom.

## Questions ouvertes

1. **Stratégie d'URL de la popup** : route dédiée vs paramètre de requête. Recommandé : route dédiée, sans changement de mount backend.
2. ✅ **Emplacement exact du pays** : dans `ScraperRuntimeOptions` (appliqué via `apply_to`) ou porté par la façade (comme aujourd'hui via `set_current_country`) ? Recommandé : garder `set_current_country` sur la façade pour l'application, et sérialiser le pays uniquement dans `ScraperAdminSettings` (persisté). **Décision retenue à l'implémentation.**

## Compromis

- Persister dans le `PersistenceStore` (namespace Scrapyfy) plutôt que dans `data/config.json` respecte le cloisonnement des crates ; contrepartie : deux fichiers de configuration à documenter pour l'administrateur.
- Le rechargement à chaud du cache reconstruit l'instance (coût d'un rebuild des sources) mais garantit l'application effective des nouvelles bornes foyer ; c'est la décision validée.
- `SrcapyfyApplicationOptions` conserve ses champs CLI (source de plus haute priorité) ; seule la résolution effective consulte désormais la surcharge persistée.

## Plan de validation

- `cargo build` / `cargo check` sur le workspace ; exécuter les tests admin existants.
- Front : `npm run build` dans `admin-app` ; test manuel de la page des paramètres (enregistrement avec les nouveaux champs, redirection inchangée).
- Desktop : exécution manuelle — la popup affiche la coquille complète + les paramètres restreints, plus de page blanche ; le namespace `arachnea-scrapyfy` du store contient les nouvelles clés ; les flags CLI gardent la précédence ; le rebuild à chaud applique bien les nouvelles tailles de cache.
## Point d'étape — implémentation backend ✅ (terminée)

Le backend est implémenté et compile. Décisions finales intégrées :

- ✅ **Persistance dans `data/config.json`** (même fichier que les paramètres serveur), conformément au retour utilisateur. Mécanisme de cloisonnement Core :
  - `CoreApplicationOptions` gagne `additional_options: BTreeMap<String, Option<String>>` (`serde(skip)`), accesseurs `additional_option(name)` / `set_additional_option(name, value)`. Au chargement (`parse_vect`, source `Configuration`), toute clé inconnue du core est préservée telle quelle ; `export`/`save` la réécrit. Core n'interprète jamais ces clés.
  - Scrapyfy lit/écrit ses trois clés via `ScraperAdminSettings::from_configuration` / `apply_to_configuration`.
- ✅ `SrcapyfyApplicationOptions` garde ses champs CLI (source la plus haute priorité) et expose en plus `cli_scraper` (valeurs CLI uniquement) pour l'API admin.
- ✅ `AdminRuntimeAdapter` gagne `apply_scraper_settings(&ScraperAdminSettings)` ; `StreamAdminRuntimeAdapter` la déploie sur la façade rechargable (`ReloadableStreamScraper::apply_scraper_settings`, `runtime_options` passé en `RwLock`).
- ✅ `StreamAdminRuntimeAdapter::save_persisted_settings` fusionne `settings.additional_options` dans la config avant `save`.
- ✅ `ScraperAdminSettings` créée dans `server/crates/arachnea-scrapyfy/src/scrapyfy/admin_settings.rs` avec validation (pays 2 lettres A–Z majuscules, octets > 0), résolution CLI > persisté > défaut et 5 tests unitaires.
- ✅ DTOs admin étendus (`SettingsResponse` avec provenance ; `UpdateSettingsRequest` avec les trois champs, pays vide ou octets 0 = suppression) et `op_settings` / `op_update_settings` mis à jour (garde anti-écrasement des champs épinglés CLI, application à chaud).
- ✅ `register_admin_service` reçoit les surcharges CLI ; `main.rs` les capture avant le déplacement des options.

### Validation

- ✅ `cargo build` / `cargo check` sur le workspace : OK. Tests `arachnea-scrapyfy::admin_settings` : 5/5 passent. Une seule défaillance est **préexistante sur Windows** (séparateur de chemin `\` vs `/` dans `resolve_manifest_sources_expands_recursive_imports_in_depth_first_order`, fichier non modifié).
- ✅ Front : `npm run build` dans `admin-app` (type-check + build) : OK — voir « Reste à faire » ci-dessous pour le détail.
- ⬜ Desktop : exécution manuelle — la popup affiche la coquille complète + les paramètres restreints, plus de page blanche ; `data/config.json` contient les nouvelles clés ; les flags CLI gardent la précédence ; le rebuild à chaud applique bien les nouvelles tailles de cache.

## Reste à faire — front ⬜ → ✅ (implémenté)

- ✅ **Groupes et bouton Enregistrer** (`SettingsView.vue`) : carte renommée « Serveur » (`settings.serverGroup`), bouton Enregistrer retiré de la carte et placé au niveau de la page (rangée d'actions globale, fonctionnement inchangé).
- ✅ **Nouvelle carte « Application »** : pays courant (2 lettres majuscules, vide = détection automatique, hint de provenance), cache disque et cache mémoire (vide/0 = suppression de la surcharge, défaut 100 Mio / 32 Mio), validation locale avant envoi.
- ✅ **Miroir DTOs** (`adminApi.ts`) : `SettingsResponse` avec `current_country`, `current_country_source`, `cache_max_disk_bytes`, `cache_max_memory_bytes` ; `UpdateSettingsRequest` avec les trois champs ; `useAdminApi.updateSettings` réutilise désormais `UpdateSettingsRequest`.
- ✅ **Variante popup desktop** : nouvelle route `settings-popup` (`/settings/app`, même composant) affichant la coquille complète sans les cartes « Serveur » et « Mot de passe » ; en mode popup, seuls les trois champs « Application » sont soumis. `ADMIN_WINDOW_DEFAULT_ROUTE` (`tauri/mod.rs`) pointe la fenêtre Tauri vers `/admin/settings/app/`.
- ✅ **Page blanche — cause identifiée et corrigée** : le handler de protocole Tauri injectait `<base href="./">` (`TAURI_WEB_BASE`) dans les HTML des bundles scopés ; toute route profonde du bundle admin résolvait alors la base contre l'URL de la page (`/admin/settings/app/`), cassant assets, locales et API. La base est désormais calculée depuis le mount (`/admin/` pour les bundles scopés, `./` conservé pour le mount racine).

### Validation

- ✅ `cargo check -p arachnea-core` : OK (un seul warning préexistant non lié, `EntityQuery::predicates`).
- ✅ `npm run build` dans `admin-app` (type-check + build) : OK.
- ⬜ Desktop : exécution manuelle — la popup affiche la coquille complète + la carte « Application », plus de page blanche ; `data/config.json` contient les nouvelles clés ; les flags CLI gardent la précédence ; le rebuild à chaud applique bien les nouvelles tailles de cache.
