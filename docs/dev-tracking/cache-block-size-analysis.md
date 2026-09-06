# Paramètre `cache-block-size` et passage des tailles de cache en KiB — analyse

## Contexte et problème initial

Le cache serveur foyer (`ScraperServerCache`, `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_cache.rs`) utilisait le block engine avec sa taille de bloc par défaut (16 MiB dans foyer 0.22). Avec une configuration `--cache-max-disk-bytes = 10485760` (10 MiB), le découpage `capacité / block_size` donnait **0 bloc stable**, d'où les logs :

```
WARN foyer_storage::engine::block::engine: block-based object disk cache stable blocks count is too small ...
INFO foyer_storage::engine::block::recover: Recovers 0 blocks with data, 0 clean blocks, 0 total entries ...
```

Le cache disque est alors inutilisable (aucune entrée persistée). Le `--cache-max-memory-bytes 2048` (2 KiB) est également inopérant : les entrées sont pondérées par leur taille sérialisée et toute entrée > 2 KiB est rejetée à l'insertion.

Il n'existait aucun moyen de régler la taille de bloc : `build_hybrid_cache()` construisait `BlockEngineConfig::new(device)` sans `with_block_size`.

## Périmètre demandé

1. **Nouveau paramètre `--cache-block-size` en ligne de commande** (crate `arachnea-scrapyfy`, même modèle que `--cache-max-disk-bytes` / `--cache-max-memory-bytes` : CLI > surcharge persistée > défaut).
2. **Exposition front admin** du nouveau paramètre (carte « Application » de `SettingsView.vue`, y compris la variante popup `settings-popup`).
3. **Valeurs par défaut envoyées au front** : quand un paramètre n'est pas surchargé, l'API `settings` doit renvoyer la valeur effective par défaut (et plus « rien »), avec la provenance (`command_line` / `configuration` / `default`).
4. **Unité en kilo-octets (KiB) partout, backend y compris** ; la conversion en octets se fait au dernier moment (à la construction du `ScraperCacheConfig` runtime). Rupture de contrat assumée : les anciennes clés/octets (`cache-max-disk-bytes`, etc.) sont remplacées par des clés KiB.
5. **Front** :
   - validation : block size multiple de 4 KiB ; `cache-max-disk` et `cache-max-memory` strictement supérieurs à `cache-block-size` ;
   - boutons d'incrémentation/décrémentation : ± 4 KiB pour le block size, ± (valeur du block size) pour les max ;
   - à côté de chaque number field, une dropdown d'unité **K / M / G** ; le front convertit en KiB avant l'envoi au backend.
## Décisions de conception

1. **Drapeaux CLI** : les noms existants sont conservés et le nouveau paramètre suit le même schéma — `--cache-max-disk-bytes`, `--cache-max-memory-bytes`, `--cache-block-size`. **Le suffixe `-kib` n'est pas ajouté** : la valeur attendue est en kilo-octets (K), précisé dans la doc Rust (doc comments) et le texte d'aide CLI (ex. « Override the maximum disk cache size in K (kilobytes). »).
2. **Persistance** (`data/config.json`, `additional_options`) : les clés `cache-max-disk-bytes`, `cache-max-memory-bytes` conservent leur nom et accueillent désormais des valeurs en kilo-octets ; nouvelle clé `cache-block-size`. Attention : les anciennes valeurs persistées en octets (ex. `"10485760"`) doivent être re-saisies en K à la main.
3. **Défauts** (constantes dans `scraper_cache.rs`, dérivées des défauts KiB) :
   - `DEFAULT_CACHE_MAX_DISK_KIB = 102 400` (100 MiB) ;
   - `DEFAULT_CACHE_MAX_MEMORY_KIB = 32 768` (32 MiB) ;
   - `DEFAULT_CACHE_BLOCK_SIZE_KIB = 16 384` (16 MiB, défaut foyer 0.22).
4. **`ScraperCacheConfig`** gagne `block_size_bytes: u64` (unité interne = octets) ; `build_hybrid_cache()` applique `.with_block_size(...)`. La conversion K → octets n'existe que dans `ScraperAdminSettings::scraper_cache_config()`.
5. **Validation croisée** dans `ScraperAdminSettings::validate()` : block > 0, multiple de 4 KiB, block < max disque effectif et block < max mémoire effective (les bornes retombent sur les défauts quand elles ne sont pas surchargées). Appelée aussi bien sur les valeurs persistées que sur la configuration **fusionnée** (CLI > persisté) côté API admin.
6. **`SettingsResponse`** : `cache_max_disk_bytes`, `cache_max_memory_bytes`, `cache_block_size_bytes` (noms de champs conservés, valeurs exprimées en K) sont désormais **toujours renseignés** (défaut inclus) avec leurs champs `_source` ; `UpdateSettingsRequest` garde des champs optionnels (`0` = efface la surcharge → retour au défaut).
7. **Front** : les trois champs partagent un motif identique — number field + `v-select` d'unité (K=1, M=1024, G=1 048 576 en facteurs K) + boutons ± ; la valeur de référence interne est le K (kilo-octet) ; l'affichage est converti dans l'unité choisie ; à l'enregistrement le front envoie des K.

## État actuel (implémentation terminée)

- **Backend — implémenté et validé** (`cargo check --workspace` OK ; `cargo test -p arachnea-scrapyfy admin_settings` : 9 tests) :
  - `scraper_cache.rs` : constantes `DEFAULT_CACHE_*_KIB` (102 400 / 32 768 / 16 384 K) + défauts octets dérivés ; `ScraperCacheConfig.block_size_bytes` ; `build_hybrid_cache` applique `with_block_size` ;
  - `admin_settings.rs` : nouvelle clé `cache-block-size`, champ `cache_block_size_bytes` (K), validation croisée (multiple de 4 K, block < bornes effectives), conversion K → octets uniquement dans `scraper_cache_config()`, 9 tests ;
  - `application_opts.rs` : drapeaux `--cache-max-disk-bytes` / `--cache-max-memory-bytes` / `--cache-block-size` (valeurs en K, aide CLI à jour), `scraper_cache_config()` retourne `Result<Option<...>>` (validation CLI) ;
  - `admin/dto.rs` + `admin/ops.rs` : `SettingsResponse` toujours renseigné (défauts inclus) avec `_source` ; `UpdateSettingsRequest.cache_block_size_bytes` ; `op_update_settings` valide les valeurs persistées **et** la configuration fusionnée ;
  - `arachnea-stream` : `main.rs` propage le `Result` ; `reloadable_stream_scraper.rs` compare aussi `block_size_bytes` pour décider du rebuild à chaud.
- **Front — implémenté et validé** (`npm run build` = type-check + build OK) :
  - `adminApi.ts` : DTOs miroir (`cache_max_disk_bytes`, `cache_max_memory_bytes`, `cache_block_size_bytes` + `_source`, valeurs en K ; champ `cache_block_size_bytes` en requête) ;
  - `SettingsView.vue` : champs cache avec sélecteur d'unité K/M/G (facteurs K : 1 / 1024 / 1 048 576), boutons ± (block ± 4 K ; bornes ± block size), validations locales (entiers, block > 0 multiple de 4, block < min(disk, memory) effectifs) ; envoi en K, champ non modifié = absent de la requête, vide/0 = efface la surcharge ;
  - locales `en.json` / `fr.json` : libellés « (K) », hint et messages d'erreur nouveaux.
- **Documentation** : CHANGELOG et TODO mis à jour.

> **Note de revue** : l'état backend ci-dessus a été écrit avant la décision de conserver les noms de drapeaux/champs sans suffixe `-kib`. À l'implémentation, réaligner : drapeaux `--cache-max-disk-bytes` / `--cache-max-memory-bytes` / `--cache-block-size`, clés de persistance `*-bytes` + `cache-block-size`, champs DTO/API `cache_max_disk_bytes` / `cache_max_memory_bytes` / `cache_block_size_bytes` — seules les **valeurs** passent en kilo-octets, documentées dans la doc Rust et l'aide CLI.

## Implémentation front restante (plan)

1. `adminApi.ts` : miroir des DTOs (`cache_max_disk_bytes`, `cache_max_memory_bytes`, `cache_block_size_bytes` + `_source` en réponse, valeurs en K ; les trois champs optionnels en requête, en K).
2. `SettingsView.vue` (carte « Application », plein écran et popup) :
   - état par champ : valeur en K (nombre) + unité (`'K' | 'M' | 'G'`) ; affichage = `valeur_K / facteur(unité)` ; saisie = `valeur_K = affichage × facteur` ;
   - boutons ± : block ± 4 K ; max ± (block size K effectif, défaut 16 384) ;
   - validations locales : entiers ; block > 0 et multiple de 4 ; block < min(disk, memory) effectifs ; max ≥ 0 (0/ vide = retour au défaut) ;
   - requête envoyée en K (`0` pour effacer).
3. Locales `en.json` / `fr.json` : libellés « (K) », nouveaux messages d'erreur (multiple de 4, block < bornes), hint des unités.

## Validation prévue

- `cargo check --workspace` puis `cargo test -p arachnea-scrapyfy admin_settings` (8 tests attendus).
- `npm run build` dans `front/admin-app` (type-check + build).
- Manuel : page settings — valeurs par défaut affichées (102400 / 32768 / 16384 KiB), unités K/M/G fonctionnelles, ± aux bons paliers, rejet des valeurs invalides, application à chaud (rebuild du cache) observable dans les logs foyer.

