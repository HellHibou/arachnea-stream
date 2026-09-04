# Analyse - Généralisation de l'orchestration de rechargement

> Date : 2026-09-04
> Statut : phases 1 à 4 implémentées
> Périmètre : `arachnea-core`, `arachnea-scrapyfy`, `arachnea-stream` et le rechargement depuis l'administration ou le systray serveur.

## Objectif

L'administration est un service générique de `arachnea-scrapyfy`. Le rechargement déclenché depuis cette administration doit donc suivre la même propriété : Scrapyfy possède l'orchestration, la validation et les rapports génériques de rechargement.

`arachnea-stream` conserve uniquement les responsabilités liées à son runtime concret : construction d'un `StreamScraper`, routes Stream, endpoints proxy et DRM, résolveurs de lecteur et propagation du pays courant. Stream peut fournir des paramètres, des stores et des hooks étroits, mais Scrapyfy ne doit jamais dépendre de `StreamScraper` ni de `ReloadableStreamScraper`.

Les deux déclencheurs suivants doivent converger vers une même opération métier :

1. `POST admin/reload` depuis l'administration HTTP/Tauri ;
2. l'action systray serveur « Reload configuration ».

## État actuel

### Répartition déjà en place

`arachnea-scrapyfy::admin` possède déjà les éléments génériques suivants :

- routes `admin/*`, DTO, sessions, authentification et contrôle des écritures ;
- lecture des manifestes avec `load_service_catalog_detailed` ;
- politique d'activation persistante avec `PersistenceSourceEnabled` ;
- type de résultat générique `RuntimeReloadReport` ;
- `AdminRuntimeAdapter`, qui injecte les opérations applicatives ;
- `op_reload`, qui prépare chaque groupe puis appelle `AdminRuntimeAdapter::rebuild_validated_group` lorsqu'un runtime applicatif existe ;
- `admin::reload::prepare_group_reload`, qui valide un groupe sans runtime spécifique.

`arachnea-stream` possède actuellement :

- `ReloadableStreamScraper`, façade stable vers l'instance active ;
- l'enregistrement unique des routes Stream, lesquelles résolvent l'instance active à chaque requête ;
- `RegistrationEndpoints`, qui conserve les chemins publics proxy/DRM et le proxy core à réinjecter dans toute instance reconstruite ;
- `StreamScraperBuildOptions`, dont le cache et `current_country` doivent être conservés entre deux constructions ;
- `StreamAdminRuntimeAdapter`, qui persiste la configuration, chiffre les identifiants et délègue le rechargement du groupe Stream ;
- un callback systray dans `main.rs` qui appelle directement `ReloadableStreamScraper::reload_blocking`.

### Duplication supprimée pour le runtime Stream

Avant la phase 2, la validation générique était dupliquée entre Scrapyfy et Stream. La reconstruction appelée par l'administration est maintenant séparée :

| Étape | Scrapyfy : `prepare_group_reload` | Stream : `ReloadableStreamScraper::rebuild_validated` |
| --- | --- | --- |
| Chargement détaillé du manifeste | Oui | Non |
| Identification des sources invalides ou dupliquées | Oui | Non |
| Synchronisation des activations par défaut | Oui | Non |
| Lecture de l'état d'activation de chaque source | Oui | Non |
| Construction du runtime applicatif | Non | Oui |
| Publication atomique du runtime | Non | Oui |

Le chemin administratif ne duplique donc plus les règles de validation. Il reste une compatibilité temporaire :

- le systray contourne encore le coordinateur Scrapyfy à venir ;
- son wrapper Stream appelle toutefois déjà `prepare_group_reload`, donc il suit les mêmes règles de validation que l'API admin.

### Limite du rapport actuel

`op_reload` recharge tous les groupes, mais construit encore une réponse plate à partir du premier groupe déclaré. Les résultats des groupes suivants ne sont pas exposés au frontend ni au systray. Ce comportement est maintenu seulement pour la compatibilité du contrat actuel `{ applied, build_error }`.

## Répartition cible

| Couche | Responsabilités de rechargement |
| --- | --- |
| `arachnea-core` | Exécution technique du systray, callback générique, `RestServerHandle`, application/rebinding REST. Aucune connaissance des manifestes ni des sources. |
| `arachnea-scrapyfy` | Modèles de rapport, inventaire des groupes, chargement et validation de catalogues, synchronisation d'activation, orchestration de groupe et globale, API admin, exécution bloquante utilisable par le systray. |
| `arachnea-stream` | Construction et échange atomique de `StreamScraper`, conservation des routes déjà enregistrées, injection proxy/DRM, réapplication de `current_country`, persistance concrète de configuration et d'identifiants. |

## Contrat proposé dans Scrapyfy

### Rapports partagés

`AdminGroupReload` et `StreamReloadReport` portaient les mêmes champs. Ils ont été remplacés par `RuntimeReloadReport`, défini hors de la couche HTTP dans `arachnea-scrapyfy::admin::reload`.

```rust
#[derive(Clone, Debug, Default, Serialize)]
pub struct RuntimeReloadReport {
    pub applied: bool,
    pub build_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReloadGroupReport {
    pub service_store_id: String,
    pub applied: bool,
    pub build_error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ReloadSummary {
    pub applied: bool,
    pub groups: Vec<ReloadGroupReport>,
}
```

`ReloadResponse` conserve `applied` et `build_error` pour le premier groupe, tout en exposant `groups` pour chaque groupe administrable. Le frontend consomme les résultats détaillés tout en restant compatible avec les deux champs historiques.

### Préparation générique d'un groupe

Scrapyfy doit extraire `generic_group_reload` de `admin/ops.rs` vers un module de coordination non lié aux routes. Cette préparation doit :

1. charger le manifeste détaillé ;
2. collecter les descripteurs de sources ;
3. créer les enregistrements d'activation manquants sans remplacer un choix administrateur existant ;
4. vérifier que l'état d'activation de chaque source est lisible ;
5. convertir les entrées invalides et dupliquées en erreur de validation ;
6. ne pas appeler le hook applicatif si la validation échoue.

Le résultat peut être représenté par une structure interne :

```rust
pub struct ValidatedReloadGroup {
    pub service_store_id: String,
    pub manifest_path: String,
}
```

Le premier jalon ne doit pas imposer le passage d'un catalogue déjà parsé à Stream. La reconstruction Stream peut relire son manifeste via `StreamScraper::from_options`; cela évite une refonte prématurée de son constructeur. Le contrat important est que Scrapyfy a validé le groupe avant l'appel du runtime.

### Hook de runtime applicatif

Le hook de `AdminRuntimeAdapter` doit exprimer que la validation a déjà eu lieu :

```rust
async fn rebuild_validated_group(
    &self,
    group: &ValidatedReloadGroup,
) -> anyhow::Result<Option<RuntimeReloadReport>>;
```

`Ok(None)` signifie qu'il n'existe aucun runtime applicatif pour ce groupe : la validation générique réussie constitue alors le résultat du groupe. `Ok(Some(_))` signifie que l'application a reconstruit son runtime. `Err(_)` reste réservé aux erreurs internes inattendues.

Cette API conserve l'inversion de dépendance correcte : Scrapyfy connaît un contrat limité, Stream implémente ce contrat, et Scrapyfy ne référence aucun type de Stream.

### Coordinateur unique

Scrapyfy doit exposer un coordinateur indépendant de HTTP/Tauri, par exemple :

```rust
pub struct ReloadCoordinator {
    // groupes, politique d'activation et adaptateur applicatif
}

impl ReloadCoordinator {
    pub async fn reload_all(&self) -> ReloadSummary;
    pub fn reload_all_and_apply_server_settings_blocking(&self) -> anyhow::Result<TrayReloadSummary>;
}
```

`op_reload` devient une fine adaptation HTTP : authentification, contrôle d'écriture, appel à `reload_all`, puis sérialisation de la réponse. Le callback systray appelle l'opération bloquante globale, qui recharge les groupes puis applique les réglages REST disponibles. Les deux chemins appliquent donc exactement les mêmes règles de validation.

## Runtime conservé dans Stream

Après extraction, `ReloadableStreamScraper` conserve :

- `RwLock<Arc<StreamScraper>>` et `current()` ;
- l'enregistrement stable de toutes les routes Stream ;
- la construction hors du chemin des requêtes ;
- la restauration de `RegistrationEndpoints` ;
- la réapplication de `current_country` ;
- l'échange atomique de l'instance seulement après une construction réussie.

Sa méthode de reconstruction devient conceptuellement :

```rust
pub async fn rebuild_validated(&self) -> anyhow::Result<RuntimeReloadReport>;
```

Elle ne charge plus `load_service_catalog_detailed`, ne crée plus `PersistenceSourceEnabled`, ne synchronise plus les activations et ne décide plus quelles erreurs de catalogue empêchent la publication.

L'invariant à préserver est le suivant : une erreur de construction Stream ne remplace jamais l'instance active, et les requêtes en cours continuent à utiliser leur `Arc<StreamScraper>` précédent.

## Systray et paramètres serveur

L'action actuelle du systray recharge uniquement le runtime Stream. Elle ne fait pas appliquer les paramètres serveur persistés (port, réseau et racine), alors que `update-settings` les applique déjà via `RestServerHandle`.

Après introduction du coordinateur, le callback peut produire une opération d'application globale en deux sous-étapes indépendantes :

1. `ReloadCoordinator::reload_all_and_apply_server_settings_blocking()` ;

Les deux résultats doivent être rapportés séparément. Une erreur de rebind REST ne doit pas annuler un runtime scraper déjà reconstruit, et un manifeste invalide ne doit pas empêcher l'application d'une correction de port, réseau ou racine.

La décision d'orchestrer ces deux sous-étapes appartient à Scrapyfy, car il est propriétaire du service admin et de son opération de reload. L'exécution réelle de l'application REST reste un hook Stream vers `RestServerHandle`, donc dans la bonne couche applicative.

## Plan de migration

### Phase 1 — Types et validation partagés — réalisée le 2026-09-04

- [x] Créer `arachnea-scrapyfy/src/admin/reload.rs`, un module non lié aux routes HTTP.
- [x] Déplacer le rapport générique et la validation de `generic_group_reload` dans `prepare_group_reload`.
- [x] Remplacer `AdminGroupReload` et `StreamReloadReport` par `RuntimeReloadReport`.
- [x] Conserver le contrat frontend actuel en projetant le premier résultat de groupe vers les champs historiques de `ReloadResponse`.

#### Réalisation

La phase introduit `RuntimeReloadReport` dans Scrapyfy et l'emploie à la fois dans l'adaptateur admin et dans le runtime Stream. `op_reload` délègue désormais la validation générique au module `admin::reload`; le comportement de l'API `reload`, y compris sa réponse plate historique, reste inchangé.

À la clôture de la phase 1, `ReloadableStreamScraper` conservait encore sa validation interne afin de préserver le séquencement existant. La phase 2 a ensuite extrait cette validation et appelle désormais le runtime uniquement après préparation par Scrapyfy.

### Phase 2 — Runtime Stream validé — réalisée le 2026-09-04

- [x] Retirer de `ReloadableStreamScraper::rebuild_validated` le chargement/contrôle de catalogue et la synchronisation des activations.
- [x] Introduire `ValidatedReloadGroup` et faire valider chaque groupe par Scrapyfy avant son hook de runtime.
- [x] Renommer le hook de l'adaptateur en `rebuild_validated_group`.
- [x] Conserver dans Stream la construction hors requête, la restauration proxy/DRM, la réapplication de `current_country` et le swap atomique.

#### Réalisation

`prepare_group_reload` valide le manifeste et les activations dans Scrapyfy, puis retourne `ValidatedReloadGroup`. `op_reload` ne déclenche `rebuild_validated_group` qu'après cette étape. Le runtime Stream ne contient plus de lecture détaillée de manifeste, de politique `PersistenceSourceEnabled` ni de décision de validation : `rebuild_validated` reconstruit via `StreamScraper::from_validated_options`, qui ne relit ni ne synchronise les activations, puis publie uniquement une instance déjà autorisée par Scrapyfy.

À ce stade de la migration, le callback systray utilisait encore un wrapper transitoire vers le runtime Stream. La phase 3 l'a ensuite remplacé par le coordinateur partagé.

### Phase 3 — Coordinateur partagé entre admin et tray — réalisée le 2026-09-04

- [x] Construire `ReloadCoordinator` lors de l'enregistrement de l'administration Scrapyfy.
- [x] Faire déléguer `op_reload` au coordinateur.
- [x] Donner au systray une référence vers le coordinateur, plutôt qu'une référence directe vers `ReloadableStreamScraper`.
- [x] Supprimer `tray_reload_summary` spécifique à Stream et `ReloadableStreamScraper::reload_blocking`.

#### Réalisation

`ReloadCoordinator` possède les groupes administrables, le store d'activation et l'adaptateur applicatif. Il produit un `ReloadSummary` interne contenant les résultats de chaque groupe, tout en permettant à `op_reload` de conserver la projection historique sur le premier groupe. Le callback systray utilise le coordinateur plutôt qu'une façade Stream directe et journalise tous les résultats de groupe.

Le systray et `POST admin/reload` partagent donc le même enchaînement de validation et de reconstruction. L'application des paramètres REST depuis le systray reste hors de cette phase et appartient à la phase 4.

### Phase 4 — Rapport multi-groupes et application REST du tray — réalisée le 2026-09-04

- [x] Exposer les résultats par groupe dans `ReloadResponse.groups` et l'interface d'administration.
- [x] Ajouter au résumé du systray le résultat d'application des paramètres REST.
- [x] Mettre à jour les spécifications, `docs/TODO.md` et `CHANGELOG.md`.

#### Réalisation

`ReloadGroupReport` est maintenant sérialisé et `ReloadResponse` expose tous les groupes dans leur ordre d'administration, tout en conservant les champs historiques du premier groupe. L'interface admin affiche chaque résultat de groupe et son erreur éventuelle après une action de reload.

Le callback systray exécute `reload_all_and_apply_server_settings_blocking`. Après le reload des groupes, il appelle `apply_server_settings` uniquement lorsqu'un serveur REST cible existe. Le rapport de port, réseau, racine et toute erreur de bind est ajouté séparément au résumé de log ; cet échec ne modifie pas les résultats déjà publiés des groupes.

## Critères d'acceptation

- Scrapyfy ne dépend d'aucun type ni crate Stream.
- Les appels admin et systray exécutent la même validation de groupe.
- Une validation de manifeste échouée ne déclenche pas de reconstruction Stream.
- Une construction Stream échouée laisse l'instance active et les routes enregistrées intactes.
- Les endpoints proxy/DRM et `current_country` sont préservés après une reconstruction réussie.
- Les résultats de tous les groupes sont disponibles dans le modèle interne et dans `ReloadResponse.groups`, tandis que les champs plats historiques restent disponibles pour compatibilité.
- L'application REST depuis le systray, si activée, est indépendante du résultat de reconstruction des groupes.

## Approches écartées

### Déplacer `ReloadableStreamScraper` dans Scrapyfy

Écarté : cette façade dépend directement des routes Stream, des résolveurs de lecteur, du proxy HTTP, des chemins DRM et du pays courant. La déplacer ferait connaître à Scrapyfy une sémantique d'application qui ne lui appartient pas.

### Faire dépendre Scrapyfy de `ReloadableStreamScraper`

Écarté : cela inverse les dépendances. Scrapyfy doit recevoir un hook de runtime limité par injection, jamais importer une façade d'application concrète.

### Conserver un chemin direct systray vers Stream

Écarté : le tray et l'API finiraient avec des règles de validation, des rapports et des effets de bord distincts. Le systray doit appeler le même coordinateur Scrapyfy que `POST admin/reload`.