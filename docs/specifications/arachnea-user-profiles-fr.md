# Comptes et profils utilisateurs serveur — contrat d’implémentation

Statut (18 novembre 2026) : lots 1 à 9 livrés, validation locale et qualifications manuelles restantes détaillées en fin de document. Contrat cible du [plan](../dev-tracking/server-user-profiles-analysis.md) ; stores, comptes/sessions, administration des utilisateurs et protection du contenu sont implémentés ; API de profils/préférences, avatars embarqués et quota admin à chaud sont implémentés ; les collections streaming et adaptateurs front sont livrés ; le sélecteur, la reprise et le guide de profils publics sont livrés ; le bilan du lot 9 est consigné en fin de document. [Version anglaise](arachnea-user-profiles-en.md).

## 1. Périmètre et responsabilités

Les comptes publics sont administrés, distincts de l’identité administrateur et des identifiants des fournisseurs. L’authentification précède tout accès au contenu. Ce périmètre ne comprend ni inscription publique, ni récupération par e-mail, ni import local, ni file d’écritures hors ligne, ni autorisation de lecteurs externes.

`arachnea-stream/src/profiles/` porte la fonctionnalité applicative, sans nouvelle crate :

| Module | Responsabilité |
| --- | --- |
| `common/models`, `common/repository` | Identités compte/profil, préférences communes, guide et transactions locales dans `profiles` |
| `accounts/service`, `accounts/sessions`, `accounts/credentials` | Identifiants publics, sessions persistantes, mutations admin ; les repositories communs conservent l’autorité identité/quota |
| `streaming/models`, `streaming/repository`, `streaming/operations`, `streaming/validation`, `streaming/state` | Préférences streaming, homes, sections et signets |
| `coordination` | Opérations durables, sérialisation par profil, reprise et suppression des comptes |
| `avatars` | Validation du manifeste embarqué, catalogue, images et avatar de repli |
| `accounts/dto`, `accounts/routes`, `accounts/guard`, `accounts/native` | DTO comptes, adaptateurs REST/Tauri, autorisation du contenu, gestionnaire de secrets natifs ; adaptateurs de profils aux étapes suivantes |
| Composition `main.rs` | Ouverture des stores, injection des repositories/configuration/authentification, reprise et routes |

Core ne porte que les extensions génériques de persistance/contrôleur. Scrapyfy conserve son authentification administrateur, sans métier des comptes publics. YAML et façades de scraping ne façonnent pas les réponses des profils.

Une instance `SqliteDatabase` par store physique, avec injection des vues typées :

| Store | Tables et clés |
| --- | --- |
| `data/profiles.sqlite3` | `users(id)` avec login normalisé unique ; `user_sessions(id)` indexé par `user_id` ; `profiles(id)` indexé par `user_id`, unicité `(user_id, normalized_name)` ; `profile_settings(profile_id)` ; `profile_onboarding(profile_id, scope)` ; `profile_operations(operation_id)` |
| `data/arachnea-stream-profiles.sqlite3` | `stream_profile_settings(profile_id)` ; `profile_homes(profile_id, home_id)` ; `profile_home_sections(profile_id, section_key)` ; `profile_bookmarks(profile_id, source_id, entry)`; `stream_profile_collections(profile_id, kind)`; `stream_profile_operations(operation_id)` |

`user_id`, `profile_id` et `home_id` sont des UUID générés par le serveur, jamais réattribués. Le client génère les UUID stables de répétition `operation_id`. Seul le socle commun crée `profile_id`, réutilisé tel quel en streaming sans recopier propriétaire/nom/avatar. Aucune clé étrangère interbase ni transaction globale atomique. Identités, propriétaire, état, noms normalisés, révisions et champs de recherche restent explicites ; les documents imbriqués sont du JSON typé avec `schema_version: 1`. Déclarer la version de chaque schéma via le mécanisme Core existant. Les stores services/Cloudflare/proxy restent inchangés. Préserver les constructeurs en mémoire.

Core fournit désormais des transactions locales synchrones IMMEDIATE via `SqliteDatabase::transaction`, des opérations typées get/find/put/delete_where et des index uniques composés vérifiés appartenant à l’appelant via `SqliteTransaction::ensure_unique`. Le put transactionnel utilise un upsert sur clé primaire et refuse les collisions secondaires sans supprimer d’autre ligne. Une vue d’une autre base physique est refusée. Ces appels s’exécutent sur un thread bloquant et ne doivent ni s’imbriquer ni invoquer les méthodes async de la base verrouillée. L’enregistrement REST accepte actuellement GET et POST pour les commandes JSON ; l’étape 3 doit fournir un enregistrement contrôlant les méthodes et un contexte d’authentification fiable avant de l’utiliser pour les mutations.

Un profil neuf ne possède aucune home personnalisée, section ni signet. Préférences communes et guide existent à sa création ; les données streaming sont initialisées au premier usage seulement. Aucune donnée du navigateur n’initialise un profil. Sauvegarder/restaurer les deux stores à une même frontière de quiescence de l’application ; vérifier les rattachements et reprendre les opérations avant d’accepter les écritures.

## 2. Inventaire exhaustif du stockage navigateur et destination

Cet inventaire décrit le stockage local initial avant implémentation. Le lot 5 remplace la persistance homes/sections/signets ; les lots 7/8 retirent les autres préférences personnelles locales.

Inventaire des appels de stockage des sources `front/public-app` et `front/admin-app`. Les paramètres ci-dessous sont les suffixes littéraux de `arachnea.param.` dans `services/storage/keys.ts` (`PARAMETERS_DEF`) ; les autres clés sont complètes. Les valeurs par défaut et enums actuels proviennent de `keys.ts`, `sanitizers.ts`, `types/media.ts`, `services/languages.ts` et sont conservés sauf précision ici.

| Clé(s) actuelle(s) | Propriétaire / champ cible |
| --- | --- |
| `language`, `theme` | Profil commun, `settings.values.language`, `settings.values.theme` ; langue null = langue du navigateur |
| `defaultHomeId` | Profil streaming ; null = accueil agrégé ; sinon home de ce profil |
| `thumbnailOrientation`, `thumbnailImageFit`, `thumbnailSizeMultiplier`, `collectionMode` | Affichage streaming |
| `useTrailerAsBackground`, `useCatalogBannersAsBackground`, `isBackgroundAnimated`, `backgroundImageFit` | Arrière-plan streaming |
| `isEpisodeAutoplayEnabled` | Lecteur streaming |
| `videoPlayer.autoskip.previously`, `videoPlayer.autoskip.intro`, `videoPlayer.autoskip.coming_next`, `videoPlayer.autoskip.outro`, `videoPlayer.autoskip.ads` | Saut des chapitres streaming |
| `videoPlayer.trackProfiles` | Combinaisons audio/sous-titres ordonnées, distinctes des profils utilisateurs |
| `videoPlayer.subtitleLanguagePriority`, `videoPlayer.subtitleTranslationByLanguage`, `videoPlayer.subtitleLanguages` | Sous-titres streaming ; conserver l’alias historique dans le DTO typé, exiger l’accord des deux listes ordonnées si fournies ensemble, retourner la priorité canonique dans les deux ; aucun repli sur le navigateur |
| `videoPlayer.allowAudioDescription`, `securityMode` | Accessibilité/sécurité du lecteur streaming |
| `arachnea.home.showSectionEditingButtons` | Affichage streaming |
| `arachnea.videoPlayer.preferences.quality` | Lecteur streaming, chaîne ou null (Auto) ; disponibilité résolue pour chaque média |
| `arachnea.videoPlayer.preferences.audioTrack`, `arachnea.videoPlayer.preferences.textTrack` | Derniers choix manuels streaming ; conserver les champs/origin typés actuels, sans considérer un ID fournisseur comme universel ; choix indisponible = repli vers les combinaisons configurées |
| `arachnea.videoPlayer.preferences.textTrackSettings` | Apparence des sous-titres streaming ; clés existantes autorisées uniquement |
| `arachnea.videoPlayer.preferences.volume`, `arachnea.videoPlayer.preferences.muted` | Appareil ; conserver les clés, volume fini entre 0 et 1 et muet booléen |
| `arachnea.navigation.visible`, `arachnea.navigation.sourcesExpanded` | Appareil ; clés conservées |
| `admin.theme` | Interface admin/appareil ; indépendant du thème public |
| `arachnea.theme.key` | Constante `THEME_KEY` déclarée sans consommateur actif ; ne pas lire en repli |
| `arachnea.test` | Sonde transitoire de disponibilité de `storage/persistence.ts`, pas une préférence |

Le thème public n’utilise plus `arachnea.param.theme` ; `useTheme` expose la valeur du profil en mémoire. Initialiser i18n/thème depuis les défauts puis l’instantané serveur commun. Dimensions des fenêtres, navigation temporaire, pages chargées, pagination et notifications restent sur l’appareil/en mémoire. Aucun usage de `sessionStorage` trouvé. Le helper générique contient `localStorage.clear()` ; cette fonctionnalité ne doit pas l’utiliser.

La base IndexedDB `arachnea-db` contient ces stores personnels :

| Store / clé actuelle | Document / identité cible |
| --- | --- |
| `custom_homes` / `id`, index `navigationOrder` | `profile_homes` ; préserver `CustomHomeDefinition` schemaVersion 1, nom, ordre, sources de bannières, raccourcis, blocs avec IDs distincts/options d’affichage, fusion, visibilité des signets et dates |
| `home_sections` / `name`, index `order` | `profile_home_sections` ; ordre, mode de collection, orientation/ajustement ; `name` devient `section_key`, y compris `local:bookmarks` réservé |
| `entry_bookmarks` / `key` | `profile_bookmarks` ; identité source/entry et tous les champs `EntryBookmark` : groupId, selectedItemId, playbackTime, title, alternativeTitleLabel, imagePosterUrl, imageLandscapeUrl, imageUrl, description, mediaTypeLabel, seasonLabel, episodeNumber, isFullyWatched ; modifiedAt devient une date serveur |

La clé base64url du signet reste un jeton de route/présentation, jamais une autorisation. Conserver le trim source/entry, sans autre réécriture d’URL ni invention d’identité de source. Les anciens stores restent physiquement présents ; la persistance homes/sections/signets est inactive dès l’étape 5, les autres préférences personnelles étant retirées à l’étape 8.

Chemins à remplacer, consommateurs indirects inclus :

- `services/indexDB.ts` ouvre la base et fournit le CRUD ; `customHomes.ts` ouvre aussi des transactions directes via `withDatabaseRetry`.
- `services/storage/index.ts` initialise les paramètres locaux, écrit via watchers, charge sections/signets et les recharge au retour visibilité/focus ; `storage/sectionRecords.ts` et `storage/bookmarks.ts` écrivent.
- Les homes contournent ce store via `router/index.ts`, `services/defaultHome.ts`, `composables/home/useCustomHomeManager.ts`, `useCustomHomeDefinition.ts`.
- `composables/home/useHomeCatalog.ts` consomme les identités de signets et le cache réactif ; les écritures depuis les vues détail/lecteur passent par le bridge du stockage. Remplacer uniquement Pinia ne couvre pas ces chemins.
- `main.ts`, `App.vue`, `i18n/index.ts`, `composables/useTheme.ts` consomment les préférences au démarrage ; `Parameters.vue` et `parameters/ParametersDialog.vue` modifient actuellement directement les valeurs partagées.
- `services/rustify/apiClient.ts` indexe les ETags REST uniquement par commande/paramètres. Désactiver le cache personnel et vider les caches authentifiés en mémoire au changement de compte/profil/serveur. Toute réutilisation du cache catalogue reste précédée de l’autorisation, y compris une réponse 304.

Nouvelles clés de reprise sans secret : `arachnea.resume.<encoded-server-identity>.lastUserId` et `arachnea.resume.<encoded-server-identity>.<user_id>.lastProfileId`. Encoder en base64url l’origine canonique et la racine applicative du serveur ; le mode local natif utilise une identité d’installation stable. Revalider les deux IDs côté serveur. Aucun mot de passe/jeton de session dans le stockage JavaScript.

## 3. DTO de transport et validation

Les nouvelles API utilisent snake_case pour l’enveloppe et les métadonnées. L’objet `values` conserve exactement les clés/champs frontend ci-dessus (points/camelCase inclus) ; les adaptateurs isolent le nommage Rust. Dates UTC en millisecondes Unix entières. Révisions entières positives représentables exactement en JSON, incrémentées par le serveur ; `expected_revision: 0` = création seulement. Refuser champs de mutation inconnus, enums invalides, nombres non finis et versions inconnues avec erreurs de champ ; ne pas corriger silencieusement les écritures distantes.

| DTO | Champs / invariants |
| --- | --- |
| `AccountView` | `user_id`, `login`, `must_change_password` ; aucun hash, secret ni permission admin |
| `SessionView` | `authenticated`, `account: AccountView|null`, `expires_at: integer|null`, `persistent: boolean` ; compte/expiration null si anonyme |
| `ProfileView` | `profile_id`, `user_id`, `name`, `avatar_id`, `order`, `state: creating|active|deleting`, `revision`, `created_at`, `updated_at` |
| `VersionedSettings` | `schema_version: 1`, `revision`, `values` ; commun = language/theme seulement, streaming = lignes streaming de la section 2 seulement |
| `OnboardingView` | `scope: common|arachnea-stream`, `state: pending|completed`, `revision` ; absence streaming = non initialisé |
| `ProfileSnapshot` | `profile: ProfileView`, `common: VersionedSettings`, `streaming: VersionedSettings|null`, `onboarding: OnboardingView[]`, `pending_operation: OperationView|null` ; sans toutes les homes/signets |
| `ProfileList` | `profiles: ProfileView[]`, `max_profiles_per_user`, `used_slots` ; identités creating/deleting comptées jusqu’au nettoyage, seules les actives sont sélectionnables |
| `OperationView` | `operation_id`, `kind`, `profile_id: UUID|null`, `state`, `step`, `retryable`, `result: object|null` ; jamais de payload durable/hash exposé |
| `AvatarCatalogue` | `schema_version: 1`, `fallback_avatar_id`, `avatars: [{avatar_id, url, mime_type}]` ; URLs résolues via le préfixe du contrôleur |
| `HomeRecord` | `profile_id`, `home_id`, `revision`, `definition: CustomHomeDefinition` ; definition.id = home_id, dates serveur |
| `SectionRecord` | `profile_id`, `section_key`, `revision`, `order`, `collectionMode`, `thumbnailOrientation`, `thumbnailImageFit` |
| `BookmarkRecord` | `profile_id`, `source_id`, `entry`, `revision`, `modified_at`, `bookmark: EntryBookmark` ; remplacement complet, pas de maximum implicite de position |

Login : trim, minuscules ASCII, 3–64 caractères `[a-z0-9._-]`, unique entre comptes. Mot de passe : 4–128 valeurs scalaires Unicode, sans trim/normalisation/troncature ; Argon2id avec sel aléatoire neuf. La création impose par défaut le remplacement du mot de passe temporaire à la connexion suivante ; l’admin peut décocher cette option à la création ou plus tard depuis le tableau des utilisateurs. Le reset admin impose toujours ce remplacement. Un remplacement identique au mot de passe actuel est accepté après un avertissement et une confirmation explicites du client. Même erreur de connexion pour compte absent, mot de passe erroné ou compte inactif.

Nom de profil : trim, normalisation NFC, 1–64 valeurs scalaires Unicode, aucun caractère de contrôle. Clé d’unicité = case folding Unicode par défaut du nom normalisé, puis NFC ; contrainte `(user_id, normalized_name)` dans la transaction commune, pas seulement dans l’UI. Un renommage conserve l’UUID. `avatar_id` doit appartenir au catalogue embarqué. Manifeste : `{schema_version:1, fallback_avatar_id, avatars:[{id,file}]}` ; IDs uniques, fichiers relatifs sans traversée/chemin absolu, MIME PNG/JPEG/WebP vérifiés. Manifeste invalide/fichier absent fait échouer le build. Un avatar retiré utilise le repli sans réécrire l’identité stockée ; toute nouvelle sélection doit être valide.

Quota : entier positif représentable exactement en JSON, défaut 3 ; une baisse sous l’usage actuel ne supprime ni ne bloque les profils existants, mais interdit les créations jusqu’au retour sous le plafond. Stocker `max_profiles_per_user` uniquement dans les options additionnelles Core de `config.json`. Sérialiser toutes les mutations de configuration, persister avant publication ; un échec conserve la valeur effective précédente. Pas de reconstruction du scraper pour ce seul changement.

Langue UI null ou locale supportée, thème existant. Langues de lecture selon normalisation/limites existantes (`MAX_TRACK_PROFILES`, limites sous-titres 10) ; clés de traduction incluses dans la priorité canonique. Préserver enums et défauts de la section 2. Enums d’affichage : `portrait|landscape`, `contain|cover`, `grid|single-row|list` ; thumbnailSizeMultiplier fini entre 0.5 et 1.5. Préférences booléennes = booléens JSON ; securityMode = `safe|confirmation|unsafe`. Apparence limitée aux `TEXT_TRACK_SETTINGS_KEYS` actuelles. Position null ou secondes finies positives/zéro ; épisode null ou entier positif ; ordres entiers positifs/zéro représentables exactement en JSON. Résoudre les références home/défaut/sections dans le profil autorisé. Supprimer une home par défaut remet defaultHomeId à null dans la même transaction streaming. Un réordonnancement fournit la séquence complète concernée sans doublon, oubli ni ID étranger.

## 4. Opérations et erreurs

`<api>` désigne le préfixe du contrôleur sous la racine configurée, normalement `/api`. Les noms ci-dessous sont les commandes cibles, également Tauri. GET pour les lectures avec paramètres de requête ; POST JSON pour les mutations, refusées en GET (405). Réponses `{data: T, error: null}` ou `{data: null, error: {code, message, field_errors, operation_id}}`. Messages anglais, codes stables traduits par l’UI, field_errors associant chemins de champ et messages anglais. Ces API n’utilisent pas l’enveloppe d’agrégation du scraper.

Chaque requête profil porte `profile_id` ; le contexte session/admin est injecté par le transport, jamais déduit d’un `user_id` client. Chaque mutation durable porte un UUID client `operation_id`, rattaché à l’acteur et au type d’opération. Persister l’empreinte de la requête normalisée et le résultat : même ID/corps = reprise/résultat d’origine, corps différent = conflit. Garder les reçus durant la vie du compte, ou du profil pour les opérations propres au profil ; empêcher une répétition en vol de recréer un profil supprimé. Revalider authentification/propriété même pour relire un reçu.

| Méthode / commande | Requête → données |
| --- | --- |
| POST `accounts/login` | `{login,password}` → SessionView ; session transmise uniquement par transport protégé |
| GET `accounts/session` | `{}` → SessionView ; état anonyme minimal autorisé |
| POST `accounts/change_password` | `{operation_id,current_password,new_password}` → SessionView ; révocation des autres sessions, rotation du secret courant |
| POST `accounts/logout` | `{operation_id}` → `{revoked:true}` ; effacement du secret de transport, répétition sûre |
| GET `profiles/list` | `{}` → ProfileList |
| POST `profiles/create` | `{operation_id,name,avatar_id,common_values}` → OperationView ; profil commun seulement |
| GET `profiles/get` | `{profile_id}` → ProfileSnapshot |
| POST `profiles/update` | `{operation_id,profile_id,expected_revision,name,avatar_id}` → ProfileView |
| POST `profiles/delete` | `{operation_id,profile_id,expected_revision}` → OperationView ; confirmation UI obligatoire |
| POST `profiles/initialize_streaming` | `{operation_id,profile_id,values}` → OperationView ; initialisation unique |
| POST `profiles/save_preferences` | `{operation_id,profile_id,common?:{expected_revision,values},streaming?:{expected_revision,values}}` → OperationView ; au moins un domaine, aucune initialisation implicite |
| POST `profiles/complete_onboarding` | `{operation_id,profile_id,scope,expected_revision}` → OnboardingView ; identité/préférences requises déjà confirmées |
| GET `profiles/operation` | `{operation_id}` → OperationView, propriétaire/admin selon l’opération |
| GET `profiles/avatars`, `profiles/avatar/<avatar_id>` | `{}` → AvatarCatalogue / octets image |
| GET `profiles/homes`, `profiles/sections`, `profiles/bookmarks` | `{profile_id}` → `{items: Record[], revision}` ; révision de collection pour ordonner/réconcilier les changements |
| POST `profiles/save_home`, `profiles/save_section`, `profiles/save_bookmark` | `{operation_id,profile_id,expected_revision,value}` → Record correspondant ; value contient identité/document |
| POST `profiles/delete_home`, `profiles/delete_section`, `profiles/delete_bookmark` | `{operation_id,profile_id,expected_revision,identity}` → `{deleted:true}` |
| POST `profiles/reorder_homes`, `profiles/reorder_sections` | `{operation_id,profile_id,expected_revision,ids}` → `{items,revision}` ; révision attendue de collection |
| GET `admin/users`, `admin/profile_settings` | admin → `{users:[AccountView + state,created_at]}` / `{max_profiles_per_user}` |
| POST `admin/create_user` | `{operation_id,login,temporary_password,must_change_password?}` → AccountView ; l’option omise impose le remplacement |
| POST `admin/delete_user` | `{operation_id,user_id}` → OperationView ; confirmation UI obligatoire |
| POST `admin/reset_user_password` | `{operation_id,user_id,temporary_password}` → `{user_id,must_change_password:true}` |
| POST `admin/set_user_password_change` | `{operation_id,user_id,must_change_password}` → `{user_id,must_change_password}` ; ne change pas le mot de passe ni ne révoque les sessions |
| POST `admin/save_profile_settings` | `{operation_id,max_profiles_per_user}` → valeur effective `{max_profiles_per_user}` |

`identity` vaut `{home_id}`, `{section_key}` ou `{source_id,entry}`. `value` vaut `{home_id,definition}`, le contenu de SectionRecord ou `{source_id,entry,bookmark}`, sans métadonnées serveur. Une création de home omet home_id et reçoit un UUID serveur ; un éventuel definition.id temporaire est remplacé. Une modification exige le home_id existant et un definition.id identique ; les retries réutilisent le UUID généré. Révisions de ligne et collection distinctes ; chaque mutation incrémente transactionnellement celle de collection. Collection vide initialement à 1.

| Erreur stable | HTTP | Réaction client |
| --- | --- | --- |
| `invalid_credentials`, `session_required`, `session_expired`, `session_revoked` | 401 | Connexion ; invalider les caches personnels si session invalide |
| `password_change_required` | 403 | Écran de remplacement obligatoire, sans profils/contenu |
| `admin_required`, `origin_rejected` | 403 | Refus |
| `profile_inaccessible`, `resource_not_found` | 404 | Ne pas révéler l’existence d’une ressource étrangère ; revalider la sélection |
| `invalid_data`, `unsupported_schema_version` | 422 | Brouillon conservé, erreurs de champ |
| `login_taken`, `profile_name_taken`, `profile_quota_reached` | 409 | Rafraîchir liste/quota, conserver les saisies |
| `revision_conflict`, `operation_id_conflict`, `operation_in_progress` | 409 | Recharger/comparer explicitement, aucun écrasement silencieux |
| `storage_unavailable` | 503 | Brouillon conservé, répéter avec le même ID |
| `method_not_allowed` | 405 | Corriger le transport |

Travail coordonné en cours : 202 avec OperationView ; achevé : 200. Échec transitoire après commit durable : inclure operation_id pour consultation/reprise. L’instantané expose l’opération et les révisions par domaine ; l’UI ne doit pas annoncer le brouillon intégralement enregistré. Aucun mot de passe dans les reçus/logs : seulement les hashes/intention interne nécessaires aux opérations atomiques ; répétitions authentifiées liées au reçu, sans clair ni empreinte de requête dérivée du mot de passe.

## 5. Cohérence et cycle de vie

Compte `active|deleting`, profil `creating|active|deleting`, guide séparé. Identité non active = aucun accès streaming. Types coordonnés : `create_profile`, `initialize_streaming`, `save_preferences`, `delete_profile`, `delete_user` ; états durables `pending|running|retry_pending|completed` avec étape, révisions attendues, payload versionné, acteur et dernière erreur contextualisée. Les mutations locales/reçus d’un même store sont atomiques. Validation/conflit définitif avant premier commit ne crée aucune opération métier en attente.

| Parcours | Points de reprise ordonnés |
| --- | --- |
| Création commune | Réserver quota/nom, UUID creating et opération → défauts/guide communs → active et terminé ; aucun streaming |
| Initialisation streaming | Verrouiller/revalider → intention durable → préférences au même UUID → guide du service pending et opération terminée |
| Préférences mixtes | Verrouiller/revalider → valider les deux révisions avant écriture et persister l’intention → commit commun → commit streaming → terminer ; reprise selon marqueur d’opération, jamais égalité supposée des valeurs |
| Suppression profil | Verrouiller → deleting et opération en commun → supprimer toutes les lignes streaming → supprimer préférences/guide/identité communs et terminer le reçu |
| Suppression compte | deleting et révocation atomique → terminer/coordonner les opérations et suppressions de tous les profils → supprimer données communes du compte et terminer le reçu admin |

Un même verrou par profil couvre validation, écritures streaming et suppression pour toutes les fenêtres/requêtes. Verrou du compte avant ceux des profils, plusieurs profils acquis par UUID triés. Revalider compte/session/profil sous verrou avant chaque écriture. La suppression attend les commits en vol, bloque les nouveaux et termine/rend caduques les sauvegardes inachevées avant nettoyage. La reprise ne recrée aucune ligne streaming pour une identité absente/deleting. Quota/unicité exigent aussi une transaction commune entre créations concurrentes ; le verrou UI ne suffit pas. Un seul processus applicatif écrit par répertoire de données, sauf coordination équivalente au niveau base.

Les sauvegardes mixtes utilisent la reprise vers l’avant, sans prétendre à un rollback global. Elles bloquent les autres mutations du même profil tant qu’incomplètes, mais exposent les états confirmés par domaine et l’opération en attente. Réconcilier les marqueurs au redémarrage avant les écritures de profils. Suppression répétable ; reçu de suppression disponible après nettoyage pour son appelant autorisé. Supprimer le dernier profil ramène au guide. Ordre/home par défaut restent atomiques dans le streaming. Conflit = rechargement/comparaison/réapplication explicite, sans dernier écrivain gagnant ni maximum de position.

Progression : au plus une écriture toutes les 10 secondes pendant la lecture, plus pause/fin/changement de média. Sérialiser par signet, fusionner les mises à jour non envoyées et attendre la révision confirmée avant l’envoi suivant. Avec connexion fonctionnelle, fermeture brutale = perte possible des 10 dernières secondes ; panne réseau = perte potentiellement supérieure, erreur visible et attente en mémoire. Aucune garantie d’écriture à la fermeture ni persistance hors ligne. Le changement de profil tente de terminer les écritures, ignore les callbacks obsolètes et n’envoie jamais l’ancienne progression sous le nouvel UUID.

## 6. Sessions et routes protégées

Secret de session navigateur aléatoire cryptographique, cookie HttpOnly, SameSite=Lax, limité au préfixe API sous la racine applicative, Secure en HTTPS ; un déploiement local HTTP explicite ne peut pas utiliser Secure. `user_sessions` ne stocke que l’empreinte cryptographique, user_id, dernière activité, expiration/révocation. Chaque usage authentifié valide compte, remplacement obligatoire et session persistée ; expiration glissante à 30 jours. Déconnexion/reset/suppression invalident la requête suivante, segments et catalogue en cache inclus. Les octets déjà livrés ne peuvent être rappelés.

Limiter les origines d’écriture à l’application (connexion/déconnexion incluses) ; vérifier Origin ou Referer de même origine, refuser les écritures navigateur invérifiables. Pas de CORS permissif avec credentials. Réponses personnelles/session `Cache-Control: no-store`. Cookies compatibles avec la racine configurée. Session publique sans permission admin, cookie admin sans autorisation de contenu public.

| Surface existante/cible | Accès requis |
| --- | --- |
| Shell de connexion, JS/CSS/polices embarqués et ressources par défaut nécessaires | Anonyme pour affichage/connexion uniquement, aucun préchargement personnel/catalogue |
| `accounts/login`, `accounts/session` minimal | Anonyme autorisé, sans profils/contenu |
| `accounts/change_password`, `accounts/logout` | Session valide, autorisé pendant le remplacement obligatoire |
| `search`, `load_home`, `get_service`, `list_lives`, `get_category`, `get_section`, `get_recommendations`, `get_banners`, `get_players`, `get_entry`, `get_season`, `get_live`, `get_stream` | Compte public valide sans restriction ; contrôle avant scraping ou ETag/304 |
| `proxy` et **tous ses chemins descendants** | Même autorisation pour images/média/manifestes réécrits, initialisations/segments HLS/DASH, audio, sous-titres, vignettes ; requêtes range incluses |
| `get_drm_license` (POST binaire) | Même autorisation avant lecture/transmission du corps de licence |
| `translation/get_services`, `translation/translate_url` | Même autorisation, sous-titres traduits en streaming inclus |
| Tous les `profiles/*`, catalogue/images avatars inclus | Session sans restriction ; propriété active pour chaque opération liée au profil |
| `admin/*`, opérations d’administration | Authentification admin indépendante existante, également pour comptes/quota |
| IPC Tauri et routes binaires du protocole personnalisé | Mêmes contrôles session/propriété ; une fenêtre ne constitue pas une authentification |

Sources de l’inventaire : `reloadable_stream_scraper.rs::register_routes`, `stream_scraper/registration.rs`, `api.rs` traduction, Core REST `service.rs`, Tauri `ipc.rs`, `protocol.rs`, `stream_response.rs`. Les requêtes sortantes fournisseur restent distinctes ; les contrôles portent sur les points d’entrée applicatifs. Aucun chemin média anonyme alternatif. Les médias tiers chargés directement par un lecteur déjà autorisé dépendent des contrôles du fournisseur ; Arachnea ne révoque pas une URL déjà divulguée.

Tauri capture désormais label/URL de la véritable webview pour IPC et protocole personnalisé. Les documents externes ne peuvent pas fournir ces métadonnées ; les commandes d’administration exigent la fenêtre admin de confiance. Le gestionnaire applicatif lie les onglets publics de confiance à la même session persistée, conserve son secret dans le coffre système par installation/compte et ne transmet que l’état à JavaScript. Les nouveaux onglets reprennent via ce gestionnaire. Keychain, Credential Manager et Secret Service Linux (`secret-tool`) portent la persistance ; coffre indisponible/verrouillé = session en mémoire signalée, reconnexion après redémarrage, aucun repli en clair. Les événements natifs complètent BroadcastChannel, focus/visibilité et contrôle de session toutes les 30 secondes. Cycle de vie graphique natif et coffre restent à vérifier manuellement sur plateforme.

## 7. Vérifications existantes et limites de validation

Aucun test ni infrastructure ajouté à l’étape 1. Aucun fichier de suite frontend test/spec ni script de test identifié dans les deux applications. Suites backend existantes à réévaluer lors de l’implémentation :

| Suite existante | Contrat affecté |
| --- | --- |
| `arachnea-core/tests/persistence/sqlite_store.rs`, `tests/persistence/typed_store/{memory,file}.rs`, `tests/support/{persistence,typed_store}.rs` | Schémas typés, vues de tables partagées, atomicité locale et parité des constructeurs |
| `arachnea-core/tests/controler/rest_settings.rs` | Priorité configuration JSON, échec de persistance, application à chaud |
| `arachnea-scrapyfy/tests/scrapyfy/admin_settings.rs` | Options additionnelles, conservation des réglages existants |
| `arachnea-stream/tests/stream_scraper.rs`, `tests/support/stream_scraper.rs`, `tests/support/application.rs` | Constructeurs mémoire, composition, handlers catalogue |
| `arachnea-stream/tests/stream_resolver.rs`, `tests/support/stream_resolver.rs` | URLs proxy/média/licence et contexte d’enregistrement |
| `arachnea-scrapyfy/tests/scrapyfy/global_etag.rs` | Réponses catalogue en cache précédées d’authentification |

Les tests actuels scraper/resolver ne prouvent pas l’autorisation des routes. Aucune suite compte/profil/session existante ; vérifier manuellement redémarrage/inactivité, restriction après reset, isolement, quota/noms concurrents, panne entre stores, écritures tardives pendant suppression, refus GET/POST/range/média anonyme, préfixe, contexte Tauri onglets/protocole, avatars embarqués, conflit/brouillon et anciennes données locales inactives. Ne créer aucun test sans demande explicite. Exécuter les suites affectées et checks Rust/frontend lorsque le code change. Pour l’étape 1 : liens, couverture de l’inventaire, parité bilingue et `git diff --check` ; compilation inutile pour ces seuls documents.

## 8. Implémentation de l’étape 2 et limites d’exploitation

`ProfileStores::open(application_path)` crée/rouvre uniquement les deux nouvelles bases sous `data/`, avec six tables communes et quatre streaming. Les métadonnées des schémas typés existants réconcilient les déclarations physiques ; documents de profils et payloads d’opérations commencent à `PROFILE_DOCUMENT_VERSION = 1`. UUID typés en Rust, persistés comme texte canonique, clés de référence composées incluses. La création commune contrôle quota/unicité dans la transaction qui réserve identité et intention durable. Quota effectif et nom/avatar/préférences validés sont injectés par les futurs services ; aucun endpoint admin/configuration n’est implémenté ici.

`ProfileStores::memory()` utilise deux bases SQLite indépendantes en mémoire, sans fichiers et avec les mêmes transactions/reprises. Les anciens constructeurs mémoire du scraper sont conservés. `main.rs` compose séparément ces stores, reprend les opérations avant les contrôleurs, puis vérifie les rattachements compte/profil/préférences/guide/sessions et streaming. Une erreur de reprise/schéma/référence interrompt le démarrage avec contexte ; aucune donnée orpheline n’est supprimée. Les opérations reprenables restent durables pour le prochain démarrage/essai.

Le repository commun traite les payloads d’opération comme du JSON versionné opaque et ne dépend d’aucune table streaming. Le coordinateur applicatif couvre création commune, initialisation streaming, préférences mixtes et suppression profil/compte. Il conserve reçus/marqueurs, rend caduques les intentions en attente lors d’une suppression et conserve les reçus après nettoyage. Stores/coordinateurs clonés partagent leurs verrous. Les mutations sont sérialisées par compte (profils différents inclus), avec verrou de profil lorsque pertinent, conservés dans les tâches bloquantes malgré l’annulation du futur. Une seule application écrit par répertoire de données ; aucun coordinateur interprocessus.

Préférences communes typées ; contenus imbriqués streaming homes/sections/signets/préférences en enveloppes JSON versionnées, dans l’attente des validateurs/API des étapes suivantes. Repositories et coordinateur sont des briques internes, pas des frontières d’authentification ; les appelants doivent fournir données validées et identité autorisée. À l’étape 2, authentification/routes restaient à développer ; l’étape 3 ci-dessous implémente comptes, sessions et protection du contenu. Validation des avatars et routes de profils restaient à développer à ce stade ; l’étape 4 ci-dessous les implémente.

Le scénario SQLite de lecture/écriture existant est adapté pour les commits/rollbacks multitables, unicité composée, refus interbase, réouverture des index et fonctionnement mémoire ; aucune nouvelle suite ajoutée. À l’étape 2, panne/reprise et suppression concurrente n’étaient pas couvertes. L’étape 3 valide manuellement la reprise après commit partiel et la cascade peuplée ; la suppression pendant une lecture réelle concurrente reste à vérifier. Les tests scraper existants ne couvrent pas les parcours de comptes.

Vérifications étape 2 réussies depuis `server/` : `cargo fmt --all --check`, `cargo check -p arachnea-stream --offline`, 9 tests SQLite Core existants et 14 tests de bibliothèque Stream (5 scénarios fournisseurs réels ignorés). Stream a nécessité une relance autorisant les sockets locaux, refusés par le sandbox pour ses serveurs HTTP de test. `git diff --check` est également passé.

## 9. Implémentation et validation de l’étape 3

Connexion/état/changement de mot de passe/déconnexion et opérations admin liste/création/réglage de l’obligation/reset/suppression utilisent les enveloppes de la section 4. La création impose par défaut le remplacement du mot de passe temporaire et l’admin peut décocher cette obligation ; le reset l’impose toujours. Un remplacement identique au mot de passe actuel est accepté après avertissement et confirmation côté client. Hachage par la primitive Argon2id partagée ; seuls les digests SHA-256 de secrets aléatoires entrent dans SQLite. Reset/remplacement révoquent atomiquement les anciennes sessions ; le remplacement renouvelle le secret courant. Une connexion réussie révoque atomiquement la session précédente présentée par ce transport. Les sessions indépendantes d’autres appareils restent valides jusqu’à reset/remplacement/suppression.

POST JSON et origine vérifiée pour les mutations ; `login_rate_limited` (429) limite les échecs de connexion publique. La validation peut fournir des erreurs de champ ; les pannes exposent un diagnostic sûr et l’ID d’opération, sans identifiants secrets. Les opérations locales à mot de passe conservent atomiquement des reçus terminés sans secret ; une répétition relit le résultat d’origine sans clair ni empreinte de requête dérivée du mot de passe. Si la réponse de remplacement est perdue avant réception du nouveau cookie, se reconnecter avec le nouveau mot de passe.

`ContentController` enveloppe tout enregistrement JSON/binaire du scraper avant callback/ETag : descendants proxy/ranges, manifestes/segments/sous-titres/images, DRM et traduction inclus. Shell de connexion et ressources restent publics ; l’admin conserve sa politique indépendante. Cette politique existante autorise loopback et fenêtre admin native sans compte public et exige l’authentification admin pour les clients serveur distants. Un cookie public ne donne aucune permission admin ; les webviews publiques natives ne peuvent pas invoquer les commandes admin.

L’expiration serveur progresse à chaque requête authentifiée. Les cookies navigateur sont renouvelés par la lecture d’état rapide (toutes les 30 secondes si visible) ; les réponses de contenu lentes ne réinstallent pas le cookie d’un ancien compte après changement. Path suit le préfixe API entrant. Origin/Referer HTTPS conserve Secure ; une lecture d’état sans métadonnées de schéma ne réécrit pas le cookie. Les reverse proxies doivent préserver Host et Origin/Referer cohérents et utiliser la racine configurée. Événements natifs/BroadcastChannel vident le contexte mémoire ; les réponses tardives sont refusées par génération.

Le front Vue public utilise AccountSessionGate/formulaire dédiés et n’initialise le contenu qu’après authentification sans restriction. L’entrée admin indépendante reste disponible avant connexion pour créer le premier compte. Le front admin affiche les comptes dans un tableau avec une case d’obligation de remplacement par ligne, les actions de réinitialisation/suppression et un bouton Ajouter à côté d’Actualiser ouvrant une boîte de dialogue d’identifiants ; il conserve le formulaire de mot de passe temporaire et la confirmation de suppression. Le formulaire public de remplacement affiche un indicateur de complexité et avertit avant de réutiliser le mot de passe actuel. L’ancien stockage personnel local reste actif après connexion jusqu’aux étapes 5 à 8 ; aucun import, guide de profil serveur ni API de profils n’est annoncé ici.

Validation manuelle HTTP sur copie isolée du serveur, comptes/bases générés sous `/private/tmp`, préfixe `/stage3`. Contrôlés : refus anonymes JSON/proxy/range/licence/traduction, méthodes/origines, logins normalisés/uniques et répétition de création, restriction temporaire, remplacement, catalogue autorisé, reset révoquant deux sessions, déconnexion, expiration forcée, reprise après redémarrage, reprise de sauvegarde mixte partiellement appliquée sans double révision, cascade complète dans les deux stores, suppression répétable et maintien de l’accès admin local indépendant. Serveur temporaire arrêté ensuite. Parcours graphiques/coffres natifs et exécution Linux/Windows restent des vérifications de plateforme ; aucune nouvelle suite ajoutée.


## 10. Implémentation et validation de l’étape 4

Les opérations REST/Tauri de `profiles/list` à `profiles/operation` de la section 4 sont implémentées. Les DTO stricts refusent les champs inconnus. La validation de session et du changement obligatoire du mot de passe est répétée sous les verrous compte/profil avant les accès aux stores : reset/suppression ne peuvent pas concurrencer une mutation déjà autorisée. Les noms suivent trim/NFC, puis case folding Unicode et NFC pour l’unicité. Propriété, nom, quota et révision sont contrôlés dans les transactions communes. Identité/guide possèdent des reçus atomiques ; les sauvegardes mixtes réutilisent la reprise coordonnée. Les snapshots excluent clés d’unicité internes, mots de passe et payloads durables. Le suivi d’opération répond 202 pour un travail incomplet, 200 pour un reçu terminé.

Les préférences communes acceptent les locales `en`/`fr` existantes (ou null) et les sept clés de thèmes front. L’initialisation streaming est explicite et complète les champs omis depuis `resources/profile-stream-defaults.json` ; une sauvegarde complète exige toutes les clés prises en charge. La validation couvre enums, booléens, bornes, pistes, apparence des sous-titres et références de homes du même profil. Les alias ISO/casse/séparateurs suivent la normalisation front ; l’ancienne liste de sous-titres est renvoyée égale à la priorité canonique. Alias, limites, locales/thèmes et défauts serveur doivent rester alignés avec les catalogues front. La création commune ne crée aucune donnée streaming ; l’initialisation streaming ne crée aucune home/section/signet.

`resources/avatars/avatars.json` contient trois PNG. Le build refuse versions non prises en charge, identifiants invalides/dupliqués, chemins dangereux, fichiers manquants, liens symboliques sortants et signatures d’images non prises en charge ; les références `include_bytes!` générées embarquent toutes les images. `profiles/avatars` fournit URL du contrôleur actif et MIME ; les images sont authentifiées et no-store, y compris via le protocole binaire natif. Le fallback du catalogue permet d’afficher un avatar retiré sans réécrire son identifiant persisté. Toute nouvelle sélection doit appartenir au catalogue courant. Aucun répertoire avatars ni dépôt source n’est nécessaire à l’exécution.

Les Paramètres admin possèdent un formulaire de plafond indépendant, également en desktop. `admin/profile_settings` et `admin/save_profile_settings` lisent/persistent `max_profiles_per_user`, défaut 3, comme option additionnelle Core dans `data/config.json`. Le verrou admin commun sérialise ces changements avec port/réseau/mot de passe/proxy ; la sauvegarde précède la publication et ne reconstruit pas le scraper. Les entiers positifs sûrs JSON sont acceptés ; une baisse conserve les profils existants et bloque les créations tant que le plafond est atteint/dépassé. La map de reçus `profile_quota_operations` est enregistrée dans le même JSON pour garantir l’idempotence durable ; un retry renvoie le résultat initial. Un plafond persisté invalide bloque le démarrage.

Les contrôles HTTP isolés avec `/stage4` valident PNG embarqués/authentifiés sans répertoire externe, refus anonyme/session restreinte, isolation des comptes, unicité Unicode (`Straße`/`STRASSE`), retries création/modification/suppression, champs/révisions stricts, six créations concurrentes pour deux places restantes, initialisation différée, préférences mixtes/guide, baisse/hausse et valeurs invalides du plafond, échec JSON conservant la valeur effective, cascade complète et liste finale vide. Le redémarrage du binaire final conserve plafond, session et identité ; normalisation des alias et refus des langues/bornes/locales invalides sont également vérifiés. Cette liste permet le retour au guide ; sélection et guide graphique sont livrés au lot 6. Build/check/format Rust, 14 tests Stream (5 ignorés fournisseurs réels), 75 tests Scrapyfy et type-check/build admin réussis. Les tests de sockets ont nécessité une permission réseau locale. Aucune nouvelle suite n’est ajoutée. La vérification graphique Tauri et des plateformes natives reste à faire.


## 11. Implémentation et validation de l’étape 5

`streaming/operations` et `streaming/validation` implémentent les collections/CRUD/réordonnancements de la section 4. `coordination/streaming_api` revalide session, propriété active et absence de mutation coordonnée en attente sous les verrous partagés avant la transaction streaming. Le streaming doit être initialisé explicitement ; les mutations personnelles ne l’initialisent jamais implicitement. Le scraping reste inchangé.

Deux tables typées complètent la base streaming existante : `stream_profile_collections` conserve révisions de collection (initialement 1) et versions supprimées ; `stream_profile_operations` conserve requêtes normalisées et résultats terminés. Documents, incrément de collection et reçu sont validés dans la même transaction. Les retries revalident la propriété puis renvoient le résultat initial ; un corps différent provoque un conflit. Les tombstones empêchent une ancienne révision de redevenir valide après suppression/recréation d’un signet ou d’une section. La suppression du profil nettoie aussi ces tables, incluses dans les vérifications de rattachement au démarrage. `profiles/operation` expose les reçus terminés sans corps de requête. Aucun nouveau store physique.

Le schéma 1 des homes conserve références, blocs/options, ordre, fusion et visibilité des signets. UUID/dates appartiennent au serveur ; la création omet home_id, la modification fournit home_id/definition.id identiques. L’unicité des URL suit NFKD/suppression des marques/casse/underscores du front. Le timestamp du snapshot front est associé à sa révision serveur déjà lue, sans remplacement automatique par une version plus récente. Un réordonnancement exige liste complète/distincte du profil et révision de collection, sans perdre les autres champs. Supprimer la home par défaut efface defaultHomeId et incrémente les préférences dans la même transaction streaming. La destination par défaut est null/agrégation ou une home du profil ; les anciennes sélections locales service:* ne sont pas importées.

Les services homes/sections/signets front utilisent le transport REST/Tauri no-store et n’accèdent plus à IndexedDB. Focus/visibilité relisent le serveur ; les snapshots de signets non confirmés restent en mémoire sans reprise automatique sur une révision serveur plus récente. Les écritures conservent leur UUID après réponse incertaine. Sans contexte de profil, listes vides et écritures refusées ; `setActiveProfileContext()` est le point d’intégration explicite livré par le sélecteur du lot 6, après validation de propriété et initialisation streaming. Le lot 5 ne choisissait ni ne créait automatiquement un premier profil. Les autres préférences locales restent jusqu’aux lots 7/8.

Le lecteur transmet la position courante en mémoire ; la file de signets écrit au plus toutes les 10 secondes en lecture ordinaire, regroupe les mises à jour non envoyées et attend l’accusé précédent. Pause/fin/changement de média, démontage et passage en arrière-plan tentent un flush immédiat. Une réponse perdue impose le rejeu de la requête immuable avant le dernier snapshot en attente. Un composant visible expose enregistrements non confirmés/erreurs et retry explicite ; les conflits exigent rechargement/réapplication, jamais d’écrasement silencieux. Le changement de profil tente un flush borné à 5 secondes sous l’ancien UUID, puis rejette réponses/callbacks obsolètes. Aucune persistance hors ligne ni garantie à la fermeture brutale.

Validation réussie : format/check/build Rust, 14 tests Stream existants (5 fournisseurs réels ignorés), type-check/build public et lint CSS du nouveau composant. Contrôles HTTP isolés `/stage5` : homes composées, schémas/champs imbriqués/descripteurs avec credentials invalides, UUID/dates serveur, collisions de slugs accentués, ordres/révisions complets, conservation des options de sections, épisodes/position zéro, isolation profils/comptes, retries immuables, concurrence (un succès sur six), tombstones, suppression atomique du défaut, panne injectée sur le reçu avec rollback données/compteur, cascades profil/compte sur les six tables streaming, suppression concurrente et redémarrage/rejeu. Des exécutions manuelles des vrais modules front valident regroupement à 10 secondes, pause/réponse perdue/flush de profil et adaptateurs REST sans persistance locale. Aucun test/infrastructure ajouté au dépôt. Sélection/guide graphiques, lecture avec fournisseur réel et fenêtres/coffres Tauri restent à vérifier aux lots suivants.

## 12. Implémentation et validation de l’étape 6

`AccountSessionGate` monte désormais `ProfileSessionGate` avec la clé de compte/session au lieu de `App`. La barrière crée un contrôleur `useProfiles()` par session authentifiée, le fournit au guide, au sélecteur et au shell applicatif, et ne monte `App` qu’une fois un profil validé par le serveur actif. Un changement de session libère le contrôleur, rejette les réponses tardives et empêche toute fuite d’état entre comptes.

L’initialisation liste en parallèle les profils du compte et le catalogue d’avatars embarqués. Le dernier profil n’est restauré depuis la clé locale de reprise (par serveur et par compte) que si le serveur le déclare encore actif ; sinon le sélecteur ou le guide de premier profil est affiché. La restauration ou la sélection lit l’instantané du profil, attend toute opération durable en attente, termine le guide `common` si nécessaire, initialise le streaming avec les défauts serveur (`{}`) au premier usage du service, termine le guide `arachnea-stream`, puis publie le contexte du profil. Une création/suppression/initialisation interrompue est reprise par suivi d’opération avec l’identité d’origine ; une création dont le stockage est indisponible est répétée avec le même identifiant d’opération, jusqu’à trois tentatives.

Le guide recueille nom, avatar, langue et thème, crée l’identité commune et termine son guide commun. L’étape service sélectionne ensuite le profil et effectue l’initialisation streaming différée. Les profils existants peuvent être sélectionnés ou supprimés depuis le sélecteur ; un profil supplémentaire réutilise le même guide. La navigation expose une boîte de dialogue de changement de profil. Les erreurs, l’épuisement du quota, une liste uniquement en attente et un profil inaccessible sont affichés sans monter le contenu.

Les clés de reprise de l’appareil sont isolées par identité serveur (encodée en base64url, `native` pour le shell desktop) et par identifiant de compte, afin qu’un autre serveur ou compte ne rouvre pas un profil sans rapport. Un `BroadcastChannel` par serveur, un événement Tauri natif, le focus de la fenêtre et la visibilité du document déclenchent la revalidation de la liste et des révisions ; la sélection d’un autre onglet n’est pas imposée localement. Un profil supprimé ou révoqué ailleurs efface le contexte actif et démonte le contenu. Les nouveaux profils démarrent avec la langue automatique (`null`) et le thème `arachnea-blue` ; les anciennes préférences locales ne sont pas importées.

Validation réussie : `npm run type-check`, `npm run build-only` et `npm run lint:css` dans `front/public-app`. Aucun test ni infrastructure de test ajouté. Les parcours graphiques de première connexion/interruption/reprise, les fenêtres/coffres Tauri réels et une lecture avec écritures regroupées restent à vérifier manuellement sur macOS, Linux et Windows ; les brouillons explicites de préférences sont livrés au lot 7 ci-dessous.


Corrections de review du lot 6 : le menu mobile natif se ferme avant l’ouverture du dialogue de profils. Resélectionner le profil actif revalide son état sans renouveler sa génération, afin de conserver les callbacks de signets/progression des composants montés. Les suppressions incertaines gardent leur UUID et révision d’origine en mémoire dans le contrôleur de session ; le sélecteur propose une reprise explicite même quand le profil n’est plus dans la liste active. Une erreur de suppression n’invalide que sa cible. Après erreur de transport, la liste permet aussi d’écarter un profil actif dont la suppression a déjà commencé. La fermeture de la page/session perd la requête locale, mais les opérations acceptées restent durables et sont reprises au redémarrage du serveur. Aucun nouvel endpoint ni stockage navigateur. Validation : type-check/build/lint CSS et scénarios ciblés avec transport simulé ; l’ordre du gestionnaire mobile est vérifié, le parcours graphique reste à valider.

## Paramètres en brouillon (lot 7)

Le dialogue charge les révisions courantes du serveur et édite des refs détachées. Langue et thème utilisent le même formulaire que le guide. Enregistrer envoie uniquement les domaines modifiés à `profiles/save_preferences`, avec leurs révisions attendues ; les champs non exposés par le formulaire restent conservés. Aucun réglage global n’est appliqué avant achèvement de l’opération et relecture confirmée. La sélection d’un profil et les rafraîchissements appliquent également ses réglages streaming confirmés, sans réinitialiser les pistes quand les valeurs sont identiques. Une opération partielle n’est pas appliquée au runtime.

Annuler confirme l’abandon si nécessaire, sans écriture. Une sauvegarde en cours ou incertaine bloque les contrôles et la fermeture ordinaire ; une réponse perdue ou un état partiel garde le corps exact et le même UUID pour Réessayer. L’UI distingue les préférences communes déjà enregistrées d’un enregistrement entièrement confirmé. Le brouillon reste en mémoire ; une fermeture forcée perd les modifications non envoyées. Les opérations acceptées restent durables côté serveur et reprennent au redémarrage. Le dialogue peut relire une opération appartenant à une autre fenêtre mais ne reconstruit pas son intention : son émetteur doit la terminer, ou le serveur la reprendre au redémarrage.

En cas de conflit, le brouillon reste disponible. L’utilisateur choisit explicitement de charger les valeurs serveur ou de réappliquer seulement ses champs modifiés sur le dernier snapshot, puis vérifie et enregistre avec les nouvelles révisions. Aucun écrasement automatique. Le retrait des accès locaux hérités et les préférences annexes du lecteur sont livrés au lot 8.

Validation : type-check, build, lint CSS et scénarios ciblés sur le composable réel avec Vue/transport simulé (annulation, double clic, révisions mixtes, réponse perdue, état partiel, conflit/réapplication, réponses obsolètes). Aucun test ajouté au dépôt. Restent à vérifier : parcours graphiques et Tauri, redémarrage et panne réelle entre stores depuis ce dialogue. La coordination serveur existante n’est pas modifiée.

## Retrait du stockage personnel local (lot 8)

Livré : aucun accès actif à IndexedDB ni aux anciennes clés personnelles `localStorage`. Les paramètres démarrent depuis des défauts détachés puis le snapshot serveur ; les changements de contexte les réinitialisent, y compris langue/thème. Volume, mode muet, navigation et reprise par compte/serveur restent locaux ; cookie/coffre natif inchangés. Aucune importation ni suppression des anciennes données.

Qualité, pistes manuelles et apparence des sous-titres sont restaurées depuis le streaming confirmé et enregistrées par `save_preferences` avec révision attendue. Les changements lecteur sont sérialisés, sans écriture lors d’une simple application du snapshot. Une réponse perdue conserve corps et UUID ; Réessayer dans le bandeau reprend la demande. Un conflit bloque les écritures lecteur jusqu’à réapplication du snapshot serveur (rechargement/sélection/rafraîchissement), sans écrasement automatique. Les règles de pistes modifiées via Enregistrer effacent aussi les choix manuels persistés. Le flush avant changement de profil est borné à cinq secondes ; les callbacks obsolètes sont rejetés. Aucun stockage hors ligne : la fermeture forcée peut perdre les demandes non confirmées.

Validation : type-check, build, lint CSS publics et vérifications ciblées des modules réels avec transport simulé. Parcours graphiques navigateur vierge/déjà utilisé, fournisseur réel et fenêtres/coffres Tauri restent à qualifier (voir bilan du lot 9).

## Bilan de livraison (lot 9, 18 novembre 2026)

Les lots 1 à 9 sont livrés avec validation locale sur `aarch64-apple-darwin` (Rust 1.98.1). `cargo fmt --all --check`, `cargo check --workspace --locked --offline`, 31 tests Core avec `sqlite-persistence`, puis 14 tests Stream réussissent ; les 5 scénarios fournisseurs restent ignorés. Les 5 échecs Stream initiaux provenaient uniquement de sockets loopback interdites par le bac à sable ; la relance autorisée réussit. Les deux fronts passent type-check/build, le front public passe le lint CSS et les vérifications ciblées du lot 8 sont relancées avec succès. Aucune nouvelle suite ni infrastructure de tests.

Le [bilan détaillé](../dev-tracking/server-user-profiles-analysis.md#étape-9--vérifier-et-documenter-la-livraison) distingue contrôles actuels et validations historiques. Les parcours graphiques, pannes réelles entre stores depuis le dialogue, fournisseurs et coffres/fenêtres Tauri sur les trois plateformes restent à qualifier ; ce bilan ne certifie pas ces scénarios. Les tâches restent dans `docs/TODO.md`.

La [procédure d’exploitation](../../server/crates/arachnea-stream/README.md#profile-backup-and-restore) demande un arrêt de tous les écrivains puis une sauvegarde commune de `data/`, dont les deux bases, leurs éventuels sidecars SQLite et `config.json` (quota/reçus inclus). Restaurer un ensemble cohérent sans mélanger d’anciens sidecars ; la reprise idempotente et les contrôles de rattachement précèdent l’ouverture des écritures. Une incohérence bloque le démarrage sans suppression corrective silencieuse. Ces contrôles ne prouvent pas à eux seuls la simultanéité des deux captures ; ne jamais restaurer une seule base indépendamment. Les secrets du coffre natif sont hors sauvegarde des bases et un déplacement peut imposer une reconnexion.
