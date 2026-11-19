# Profils utilisateurs persistés côté serveur — analyse et plan

Date : 18 novembre 2026. Statut : lots 1 à 9 réalisés ; contrôles locaux de livraison réussis sur macOS ARM64, qualifications graphiques/natives restantes détaillées au lot 9. Comptes, sessions, administration des utilisateurs et protection des routes sont branchés ; le lot 9 consigne la validation locale et les limites de qualification.

Ce document décrit la cible demandée et les écarts constatés dans le dépôt. Les noms de tables, d’opérations et de composants ci-dessous décrivent la cible ; le statut de livraison est précisé par étape. Les deux stores et la coordination interne sont implémentés à l’étape 2 ; les API de comptes/sessions et la protection des accès sont exposées depuis l’étape 3 ; les API de profils, les avatars embarqués et le quota administrable sont exposés depuis l’étape 4 ; les collections personnelles streaming et leurs adaptateurs front sont livrés à l’étape 5.

## 1. Besoin et périmètre

Un compte utilisateur possède un ou plusieurs profils. Chaque profil possède un nom, un avatar, ses homes personnalisées, ses signets et ses préférences, dont les langues de lecture et la langue de l’interface. Ces données deviennent persistantes côté serveur et disponibles depuis plusieurs appareils.

Le navigateur conserve seulement les informations nécessaires à la reprise du compte connecté et du dernier profil sélectionné, ainsi que les états purement locaux à l’appareil. La session reste validée par le serveur. Un identifiant enregistré dans le navigateur ne constitue jamais une authentification.

Aucun accès à l’application ni aux API de contenu n’est autorisé sans compte authentifié. Seuls les points d’entrée indispensables à la connexion et leurs ressources restent accessibles avant authentification. Les comptes sont gérés dans l’interface admin : création, suppression et réinitialisation du mot de passe.

La limite de profils par compte est un paramètre global administrable, avec une valeur par défaut de **3**, persisté dans `data/config.json` avec les autres options du serveur. La première connexion d’un compte sans profil ouvre un guide : création du profil, puis définition des préférences. Le formulaire de profil doit être réutilisé lors des créations suivantes.

Les paramètres sont modifiés dans un brouillon. **Enregistrer** valide et persiste les modifications côté serveur ; **Annuler** les abandonne. Les modifications de homes et de signets réalisées en dehors des paramètres continuent à être enregistrées par leurs actions dédiées.

## 2. État initial constaté (avant les lots d’implémentation)

### Front public

| Élément | Implémentation actuelle | Conséquence |
| --- | --- | --- |
| Paramètres | `front/public-app/src/services/storage/index.ts`, `keys.ts`, `types.ts` | Store Pinia initialisé depuis `localStorage`, avec des watchers qui enregistrent les changements. Aucun propriétaire compte/profil. |
| Langues | Paramètres `language`, `videoPlayer.trackProfiles`, `subtitleLanguagePriority`, `subtitleTranslationByLanguage`, `subtitleLanguages`, `allowAudioDescription` | Les « track profiles » actuels sont des combinaisons audio/sous-titres, distinctes des nouveaux profils utilisateurs. Conserver cette distinction dans les types et les libellés. |
| Homes personnalisées | `src/services/customHomes.ts`, store IndexedDB `custom_homes` | Définitions versionnées : identité, nom, ordre, références sources/catégories/sections et options d’affichage. |
| Préférences de sections | `src/services/homeSections.ts`, store `home_sections` | Épinglage, ordre et présentation des sections à rattacher au profil. |
| Signets | `src/services/entryBookmarks.ts`, store `entry_bookmarks`, bridge `storage/bookmarks.ts` | Sélection d’épisode/groupe, position de lecture, état terminé et instantanés de présentation. Identité actuelle basée sur la source et l’URL d’entrée. |
| Lecteur | `src/services/storage/videoPreferences.ts` et paramètres de lecture | Plusieurs valeurs locales supplémentaires doivent être réparties explicitement entre profil et appareil. |
| Paramètres UI | `src/components/Parameters.vue`, `parameters/ParametersDialog.vue` | Les contrôles modifient directement les refs partagées. La boîte de dialogue possède un bouton de fermeture, sans transaction de formulaire. |
| Démarrage | `src/main.ts`, `src/App.vue`, `src/i18n/index.ts` | Traductions, stockage et thème sont initialisés sans phase de session/profil. |
| Transport | `src/services/rustify/apiClient.ts` | Pont REST/Tauri ; les appels REST actuels de catalogue utilisent GET et un cache ETag. Les écritures de profil nécessitent un transport adapté. |

Le rafraîchissement actuel des sections et signets au retour au premier plan relit IndexedDB. Il devra relire le serveur pour le profil actif. Les éditeurs de homes accèdent aussi directement aux services IndexedDB : remplacer seulement `useStorage` ne suffira pas.

### Serveur et administration

- `arachnea-core` fournit déjà `SqliteDatabase`, `SqliteStoreConfig`, les vues `PersistenceTableConfig` et les stores d’entités typées. Une base physique peut donc accueillir plusieurs tables sans réinventer la couche de persistance.
- `server/crates/arachnea-stream/src/stream_scraper/persistence.rs` ouvre actuellement trois bases physiques pour les services, les sessions Cloudflare et les proxys. Les tables proxy partagent déjà une connexion. Les identifiants des sources sont portés par le stockage typé des services.
- La configuration applicative est également persistée dans `data/config.json`. Les paramètres administrables de Scrapyfy utilisent les options additionnelles de cette configuration.
- `arachnea-scrapyfy/src/admin/` fournit l’authentification **administrateur**, avec hachage Argon2id, cookie et sessions en mémoire. Ce mécanisme n’est pas une gestion de comptes publics persistants. Les authentifications des fournisseurs vidéo sont encore un autre domaine.
- Aucun modèle de comptes/profils publics ni parcours de connexion correspondant n’a été identifié dans les chemins applicatifs examinés. Il faut donc prévoir cette fondation et la gestion du cycle de vie des comptes.
- `front/admin-app/src/views/SettingsView.vue` et `src/services/adminApi.ts` sont les points d’intégration du plafond de profils.

## 3. Architecture retenue

### Répartition des responsabilités

Le code des comptes et profils est regroupé dans un sous-dossier dédié de `arachnea-stream`, proposé sous `server/crates/arachnea-stream/src/profiles/`. Aucune crate spécifique n’est créée. Ce répertoire contient uniquement le **code source Rust** qui implémente les comptes et profils ; il sert à organiser la fonctionnalité pour les développeurs. Les profils utilisateurs sont bien créés comme des **lignes en base de données**, dans la table `profiles` du store commun `profiles`, avec leurs données liées dans les autres tables. Aucun répertoire ni fichier source n’est créé lors de la création d’un profil utilisateur.

Séparer dans ce module le socle commun comptes/profils et l’intégration streaming, puis les modèles/DTO, la persistance, les sessions, les opérations métier, les contrôleurs et les opérations administrateur. Composer le tout depuis `main.rs`. Le socle commun ne dépend pas des tables streaming ; l’application orchestre leur utilisation conjointe. Les primitives SQLite de Core restent indépendantes de ces deux domaines.

Les éventuelles extensions de transactions SQLite restent génériques dans Core. Le métier des profils reste hors du moteur YAML Scrapyfy et des façades de scraping. La limite de profils est un paramètre applicatif, pas un paramètre de fournisseur ni une préférence utilisateur. L’interface admin appelle des opérations applicatives Stream protégées par l’authentification administrateur existante, sans déplacer le métier des utilisateurs dans Scrapyfy.

### Deux stores selon le domaine des données

L’application pourra accueillir d’autres services que le streaming. Créer deux nouveaux stores physiques avec une instance `SqliteDatabase` par store, et injecter leurs vues typées aux consommateurs :

| Store | Fichier dans la convention actuelle | Responsabilité |
| --- | --- | --- |
| `profiles` | `data/profiles.sqlite3` | Comptes, mots de passe, sessions, profils, nom/avatar, langue de l’interface, thème et état du guide commun. |
| `arachnea-stream-profiles` | `data/arachnea-stream-profiles.sqlite3` | Homes de streaming, sections, signets/progression, langues audio/sous-titres, affichage et réglages propres au lecteur streaming. |

Le socle commun génère le `user_id` stable du compte et le **`profile_id` UUID** lors de la création du profil. Le même UUID est réutilisé tel quel dans toutes les tables streaming du profil : aucun second UUID n’est généré pour représenter le même profil dans l’autre store. Le store commun fait autorité sur son existence et son propriétaire.

Les profils sont liés au compte par `user_id` dans le store commun. Dans le store streaming, les données propres à un profil utilisent `profile_id` ; il n’est pas nécessaire de recopier `user_id`, le nom ou l’avatar pour retrouver leur propriétaire. Si des données streaming propres au compte entier apparaissent ultérieurement, elles pourront être liées directement par le même `user_id`, sans dupliquer le compte.

Les stores existants des services, sessions Cloudflare et proxys restent séparés. Il n’y a aucune consolidation ni migration de leurs données. Les nouvelles installations et les installations existantes créent les mêmes nouvelles tables vides dans les deux nouveaux stores. Un futur service pourra posséder son propre store lié aux mêmes identifiants communs, sans imposer de tables métier dans `profiles`.

Le plafond `max_profiles_per_user` est persisté exclusivement dans **`data/config.json`**, au même niveau de configuration applicative que les options serveur existantes. Réutiliser le mécanisme des options additionnelles de Core ; ne pas créer de table de configuration pour ce plafond. L’admin valide et enregistre la valeur, puis publie la valeur effective utilisée pour les créations suivantes. En cas d’échec de persistance du JSON, la valeur active précédente est conservée. Sérialiser la mise à jour avec les autres écritures de configuration pour ne pas perdre les changements de port ou de mode réseau.

### Avatars embarqués

Créer `server/crates/arachnea-stream/resources/avatars/`, contenant un manifeste `avatars.json` et les fichiers images qu’il référence. Ce placement respecte la convention des ressources appartenant à la crate. Le manifeste contient une liste d’entrées avec un identifiant stable et un nom de fichier relatif ; les chemins doivent rester à l’intérieur du répertoire. Le profil persiste cet identifiant, jamais un chemin arbitraire fourni par le client.

Le manifeste et toutes les images référencées sont inclus dans l’exécutable au build et exposés par le serveur via un catalogue et des routes de ressources dédiés. Le front construit son sélecteur depuis ce catalogue. Les routes doivent fonctionner avec le préfixe applicatif configuré et en desktop ; aucun accès aux fichiers sources du dépôt n’est requis à l’exécution. Vérifier les fichiers manquants, les identifiants dupliqués, les types MIME et le cas d’un avatar retiré dans une version ultérieure. Une modification du catalogue nécessite une nouvelle compilation. Aucun upload n’est prévu.

### Modèle de données proposé

| Store | Table | Identité / rattachement | Contenu principal |
| --- | --- | --- | --- |
| `profiles` | `users` | `id` stable, identifiant de connexion unique | Identifiant normalisé, hash de mot de passe, changement obligatoire, état du compte, dates. |
| `profiles` | `user_sessions` | `id`, `user_id` | Empreinte du secret de session, expiration, révocation ; aucune conservation du secret en clair. |
| `profiles` | `profiles` | `id` UUID, `user_id` indexé | Nom, `avatar_id`, ordre, état actif/en création/en suppression, dates et révision. |
| `profiles` | `profile_settings` | `profile_id` UUID | Préférences communes typées/versionnées : langue UI et thème. |
| `profiles` | `profile_onboarding` | `(profile_id, scope)` | État du guide commun et de l’initialisation de chaque service, sans données métier streaming. |
| `profiles` | `profile_operations` | `operation_id`, `user_id`, `profile_id` selon opération | Suivi durable des opérations coordonnées entre stores, étape courante et reprise après échec. |
| `arachnea-stream-profiles` | `stream_profile_settings` | `profile_id` UUID commun | Langues de lecture, home par défaut, affichage, préférences audio/sous-titres et sécurité du lecteur. |
| `arachnea-stream-profiles` | `profile_homes` | `(profile_id, home_id)` | Définition versionnée et ordre de navigation de chaque home streaming. |
| `arachnea-stream-profiles` | `profile_home_sections` | `(profile_id, section_key)` | Ordre, épinglage et options d’affichage de sections streaming. |
| `arachnea-stream-profiles` | `profile_bookmarks` | `(profile_id, source_id, entry)` | Données du signet et de progression actuel, date serveur et révision. |

Les tables de suivi sont proposées pour rendre les opérations entre stores reprenables ; leur représentation précise sera figée avec les DTO et contrats techniques. Elles n’ajoutent aucune gestion de lecteurs externes.

Les blocs de home et les ensembles de préférences peuvent rester des documents versionnés dans les colonnes compatibles du store typé : reproduire chaque bloc dans une table séparée apporterait peu de valeur au périmètre actuel. Les champs utiles à l’autorisation, aux identités et aux recherches doivent rester explicites. Vérifier les possibilités du schéma typé avant de choisir la représentation physique exacte.

Les noms de profils sont uniques au sein d’un compte, sans distinction de casse ; le serveur applique cette unicité lors des créations et renommages, y compris concurrents. Le profil reste identifié par son ID stable.

Chaque lecture/écriture est restreinte par le compte authentifié et le profil demandé. Le serveur vérifie dans le store commun que le profil est actif et appartient au compte connecté avant toute opération streaming ; un `user_id` envoyé par le front ne fait pas autorité. Les homes par défaut et les clés de sections sont résolues dans le même profil.

### Transactions locales et coordination entre stores

Les transactions restent locales à chaque base : contrôle du quota et création de l’identité dans `profiles`, écritures de homes/signets et préférences streaming dans `arachnea-stream-profiles`. Le partage des UUID ne crée ni clé étrangère ni transaction automatique entre ces deux bases. L’API publique actuelle du store typé ne propose pas de transaction métier générique entre plusieurs tables : étendre seulement les capacités nécessaires ou fournir des repositories spécialisés.

Les opérations touchant les deux stores sont coordonnées par l’application avec un suivi durable et des étapes idempotentes. Une création incomplète n’est pas publiée comme profil prêt. Une suppression rend d’abord le compte/profil inaccessible dans le store commun, puis supprime ses données streaming, puis termine le nettoyage commun. Un échec laisse une opération à reprendre au redémarrage ou sur nouvelle tentative ; ne pas annoncer une suppression totalement achevée tant que le nettoyage reste incomplet.

Empêcher qu’une écriture streaming déjà en cours recrée des données après le nettoyage : sérialiser les opérations du profil avec sa suppression et revalider son état avant écriture. Les identifiants supprimés ne sont jamais réattribués. La restauration depuis sauvegarde doit considérer les deux stores ensemble, puis vérifier les rattachements et reprendre les opérations inachevées.

## 4. Contrats fonctionnels proposés

### Comptes et connexion persistante

Décision : comptes gérés depuis l’interface administrateur, avec liste, création, suppression et réinitialisation du mot de passe. La connexion à l’application utilise un identifiant et un mot de passe. Il n’y a pas d’inscription publique ni de récupération par e-mail dans ce périmètre.

La suppression d’un compte le rend immédiatement inaccessible et révoque ses sessions dans `profiles`, puis coordonne la suppression de ses profils et de leurs données dans les deux stores selon le mécanisme de reprise décrit en section 3. L’interface admin présente une confirmation explicite de cette suppression. Les comptes publics sont distincts de l’identité administrateur ; leur suppression ne doit pas retirer l’accès admin au serveur.

Décision validée : l’admin définit un mot de passe temporaire lors du reset. La réinitialisation remplace le hash, révoque les sessions existantes et impose le remplacement de ce mot de passe à la connexion suivante. À la création, l’admin peut au contraire décocher cette obligation pour la prochaine connexion. Ajouter un état serveur de changement obligatoire, un écran et une opération de changement de mot de passe. Tant que ce remplacement n’est pas confirmé, la session ne permet que les opérations nécessaires à ce changement et à la déconnexion, sans accès au contenu ni aux profils.

Réutiliser les primitives cryptographiques adaptées de l’existant sans confondre permissions admin et compte public. Les sessions publiques sont persistées pour permettre la reprise après redémarrage du serveur, avec expiration après **30 jours d’inactivité**, renouvelée à chaque utilisation authentifiée, et révocation explicite. Cette règle s’applique aussi aux sessions desktop. Une suppression du compte ou une réinitialisation du mot de passe invalide ses sessions.

Dans le navigateur, utiliser un cookie de session HttpOnly avec les attributs adaptés au déploiement. Conserver le dernier profil dans une clé rattachée à l’instance serveur et au compte ; un éventuel identifiant de dernier compte ne contient aucun secret. Ne stocker ni mot de passe ni jeton de session dans `localStorage`.

Le transport Tauri doit transmettre un contexte de session équivalent et respecter les mêmes contrôles de propriété. Vérifier les contraintes du bridge et des fenêtres/onglets natifs avant de choisir le support persistant du secret ; ne pas assimiler automatiquement une fenêtre desktop à un utilisateur public authentifié.

L’authentification est obligatoire pour les vues de contenu et pour les API catalogue, recherche, détails, lecture et données personnelles, y compris les chemins de proxy média, manifestes, segments, sous-titres et licences qui exposent le contenu. Un simple verrouillage de navigation front ne suffit pas. Inventorier aussi les routes binaires utilisées par la lecture dans Arachnea. Les requêtes serveur-vers-fournisseur conservent leurs propres mécanismes d’authentification.

Les seules exceptions d’accès initial sont la connexion, son état minimal et les ressources nécessaires à son affichage ; l’admin conserve sa propre authentification. Par défaut, exposer les avatars après authentification. La gestion des lecteurs externes est hors périmètre pour cette évolution.

La déconnexion révoque la session, vide les données du profil en mémoire et demande une authentification au prochain accès. La langue avant connexion suit provisoirement le navigateur ; celle du profil remplace ensuite ce choix.

### Création, guide et changement de profil

1. Restaurer ou ouvrir la session, puis demander la liste des profils au serveur.
2. Si aucun profil n’existe, ouvrir le guide : nom/avatar et préférences communes, puis préférences streaming lorsque ce service est utilisé. Le socle commun ne doit pas imposer une configuration streaming à un profil destiné à un autre futur service.
3. Garder le formulaire initial en brouillon jusqu’à validation. Enregistrer l’identité et les préférences communes dans `profiles`. Initialiser les données streaming au premier usage du service, sous le même UUID. Si le parcours initial inclut les deux domaines, coordonner les écritures et ne publier le profil comme prêt qu’après leur confirmation ; une interruption pendant l’enregistrement laisse une opération reprenable, jamais un profil incomplet utilisable. Suivre séparément la fin du guide commun et celle du guide de service.
4. Pour un compte déjà équipé, restaurer le dernier profil local s’il appartient encore au compte ; sinon afficher le sélecteur. Un nouvel appareil retrouve les profils serveur mais ne reprend pas nécessairement le choix d’un autre appareil.
5. Réutiliser le formulaire nom/avatar et les contrôles de préférences pour les profils suivants. Tous les profils commencent avec des valeurs par défaut indépendantes ; pas de copie implicite du profil courant.

Le formulaire charge le catalogue d’avatars embarqué décrit en section 3. L’identifiant choisi est validé côté serveur lors de la création et de la modification du profil.

Le changement de profil doit arrêter la lecture, traiter un éventuel brouillon non enregistré, vider l’état personnel précédent puis charger un instantané cohérent du nouveau profil. Les requêtes en cours sont annulées ou leurs réponses ignorées si elles visent l’ancien contexte. Chaque écriture capture son profil à son lancement ; une écriture retardée ne doit jamais utiliser le nouvel identifiant actif.

### Suppression des profils

Décision validée : l’utilisateur peut supprimer un profil de son propre compte, après confirmation. Marquer le profil comme en suppression dans le store commun puis supprimer ses données streaming et ses données communes par étapes reprenables, conformément à la section 3. Si le profil supprimé est actif, arrêter sa lecture et vider son état en mémoire ; afficher le sélecteur s’il reste des profils, sinon revenir au guide de création. Une référence locale à un profil supprimé ne permet pas de le restaurer. Supprimer le dernier profil ne supprime pas le compte.

### Lecture prise en charge

La lecture reste dans Arachnea, avec le lecteur intégré et les lecteurs fournisseurs en iframe déjà utilisés. Les applications distinctes telles que VLC, mpv ou IINA sont hors périmètre pour le moment. Ne prévoir ni parcours de lancement, ni jeton délégué, ni URL signée, ni table ou API dédiée à leur autorisation. La protection des routes média utilisées par Arachnea reste requise.

### Limite administrable

- Entier strictement positif, valeur par défaut serveur centralisée à 3, persistée dans `data/config.json`.
- Application immédiate aux nouvelles créations, contrôle serveur dans la même transaction que l’insertion pour résister aux créations simultanées.
- Si la limite baisse sous le nombre de profils existants, ne rien supprimer : autoriser les profils actuels et refuser les nouvelles créations jusqu’au retour sous la limite.
- Le front affiche nombre actuel et maximum, mais son bouton désactivé ne remplace pas le contrôle serveur.

### Enregistrer / Annuler

- À l’ouverture, cloner l’état confirmé dans un brouillon indépendant et conserver sa révision serveur.
- Tous les contrôles, y compris langue et thème, modifient ce brouillon. Recommandation : appliquer leurs effets globaux seulement après enregistrement, pour donner à Annuler un comportement simple et fiable.
- Enregistrer envoie un ensemble validé de paramètres avec des révisions distinctes pour les préférences communes et streaming. Les écritures sont atomiques dans chaque store. Si le formulaire modifie les deux domaines, le serveur coordonne leur enregistrement avec un identifiant d’opération et un suivi durable ; une reprise après réponse perdue réutilise cet identifiant. Le front ne publie le nouvel ensemble qu’après confirmation des deux domaines.
- Il n’existe pas de garantie de commit simultané entre les bases : le contrat prévoit un état d’enregistrement en cours ou partiellement appliqué, une reprise idempotente et des erreurs explicites. Une fois une opération acceptée, le serveur la mène à terme ; Annuler ne prétend pas défaire une écriture déjà confirmée. Prévalider les deux ensembles et leurs révisions avant écriture, et sérialiser les changements concernés pour éviter qu’une reprise écrase une modification ultérieure.
- En cas d’erreur réseau ou de validation, garder le brouillon et expliquer l’erreur. Empêcher les doubles soumissions. Aucun message de succès avant confirmation serveur.
- Annuler abandonne le brouillon sans écriture. Fermer par Échap, clic extérieur ou navigation suit la même règle ; si le brouillon est modifié, demander Enregistrer / Abandonner / Rester.
- Un conflit de révision renvoie une erreur explicite et laisse le choix de recharger ; ne pas écraser silencieusement les réglages modifiés sur un autre appareil.
- Les signets et les actions des éditeurs de homes restent des opérations indépendantes. Une mise à jour des paramètres ne réécrit pas l’ensemble des signets et homes.

### Synchronisation et données locales

Les préférences appartiennent au profil, mais leur propriétaire métier détermine le store : langue UI et thème dans `profiles` ; langues de lecture, affichage streaming et homes dans `arachnea-stream-profiles`. Maintenir des types et révisions propres à chaque domaine plutôt qu’une table générique mélangeant tous les services. Le volume, le mode muet, la taille des fenêtres et les états temporaires de navigation restent des préférences d’appareil. Le maintien local du volume et du mode muet est validé. Établir la liste exhaustive des clés avant remplacement du stockage personnel, notamment la qualité et les dernières pistes choisies, pour éviter une seconde source de vérité.

Le serveur est la source persistante ; le front utilise un cache réactif en mémoire. Aucun mode hors ligne avec file de synchronisation n’est prévu dans cette première version. Une panne serveur produit une erreur visible et conserve le brouillon en mémoire, sans basculer silencieusement vers l’ancien stockage local.

Les signets de lecture nécessitent une temporisation des écritures et un enregistrement sur pause/fin/changement de média. Ne pas envoyer une requête à chaque événement `timeupdate`. Une dernière écriture à la fermeture du navigateur ne peut être garantie : documenter la perte maximale correspondant à la temporisation retenue. Les mises à jour concurrentes utilisent une révision et une politique explicite de conflit ; ne pas retenir systématiquement la position maximale, car revenir en arrière est un usage légitime.

Les caches de réponses personnelles doivent être cloisonnés par serveur, compte et profil ou désactivés. Les caches de catalogue partageables restent séparés. Revalider le profil actif au retour au premier plan et tenir compte des déconnexions entre onglets ; les caches des onglets et fenêtres Tauri doivent aussi être invalidés.

## 5. Démarrage sans migration ni import

Les nouveaux profils commencent avec **aucune home personnalisée ni aucun signet**, et des préférences par défaut proposées dans le guide. Le catalogue de contenu du serveur reste accessible après connexion et création du profil ; un profil vide ne signifie pas un catalogue vide.

Aucune donnée du navigateur n’est importée vers le serveur. Ne prévoir ni écran d’import, ni endpoint d’import, ni marqueur d’import, ni fusion des données. Les anciens stores IndexedDB et paramètres personnels de `localStorage` ne sont plus lus ni écrits par le parcours actif. La nouvelle initialisation des préférences et de l’i18n doit donc partir des valeurs par défaut puis de l’état serveur, sans réutiliser les anciennes valeurs locales.

Leur suppression physique n’est pas nécessaire à cette évolution : les laisser inactifs évite une opération destructive sans utilité fonctionnelle. Préserver les clés nécessaires à la reprise du compte/profil et les préférences retenues comme locales à l’appareil ; ne pas appeler `localStorage.clear()`.

Les bases serveur existantes ne sont ni migrées, ni fusionnées, ni vidées. Les deux nouveaux stores `profiles` et `arachnea-stream-profiles` sont créés. Prévoir leur initialisation idempotente, leur réouverture après redémarrage et la reprise des opérations coordonnées ; les évolutions futures de leurs schémas suivront les mécanismes existants du store typé.

## 6. API et composants à prévoir

Les opérations suivantes sont logiques : leurs noms exacts et leur préfixe seront alignés sur l’enregistrement des contrôleurs existants. REST utilise GET pour les lectures et POST avec corps JSON pour les mutations ; les commandes Tauri invoquent les mêmes handlers métier. Les mutations ne transitent pas par le GET générique du catalogue.

| Domaine | Opérations proposées |
| --- | --- |
| Session | connexion, état de session/compte courant, changement obligatoire du mot de passe temporaire et déconnexion |
| Profils | liste avec quota, création de l’identité commune et préférences, lecture de l’instantané, modification du nom/avatar et suppression coordonnée |
| Préférences | lecture et enregistrement avec révision par domaine ; coordination reprenable si les deux stores sont modifiés |
| Homes et sections | liste, création/modification/suppression, réordonnancement et préférences de sections |
| Signets | liste, mise à jour et suppression avec contexte de profil et révision |
| Avatars | lecture du catalogue embarqué et des images référencées, après authentification |
| Administration | lecture/modification du plafond via `config.json` ; liste, création, suppression des utilisateurs, réinitialisation de leur mot de passe et réglage de l’obligation de changement |

Définir des erreurs stables : session absente/expirée, profil inaccessible, quota atteint, données invalides, conflit de version et stockage indisponible. Les handlers REST vérifient aussi les méthodes et l’origine des écritures ; cookies et réponses personnelles ne doivent pas entrer dans les caches publics. Les commandes Tauri appliquent les mêmes autorisations métier.

Carte de composants proposée, en conservant Composition API et TypeScript :

| Élément | Responsabilité / contrat |
| --- | --- |
| `AccountSessionGate` | Orchestre restauration, connexion, guide ou application ; expose uniquement un état prêt. |
| `ProfileForm` | Formulaire nom/avatar réutilisable alimenté par le catalogue serveur ; props valeurs/erreurs/état occupé, événements de modification et validation. Aucun appel réseau. |
| `ProfileSwitcher` | Affiche liste et quota ; émet sélection ou demande de création. |
| `ProfileOnboarding` | Compose le guide commun et l’initialisation streaming au premier usage ; utilise `ProfileForm` et des champs de préférences séparés par domaine. |
| `ProfilePreferencesForm` | Compose les préférences communes et une section streaming distincte via props et événements, sans accéder directement à la persistance globale. |
| `ParametersDialog` | Possède le brouillon, les actions Enregistrer/Annuler et la protection de fermeture. |
| Store/composable de session et profil | Charge l’état confirmé, change le contexte actif et invalide les requêtes obsolètes. |
| Client et repositories de profils | Isolent REST/Tauri et les opérations serveur ; adaptent les services homes/signets existants. |
| Vue admin utilisateurs et formulaires associés | Liste et création des comptes, confirmation de suppression et réinitialisation du mot de passe ; état et erreurs explicites. |

Éviter de placer tout le guide et la gestion réseau dans `App.vue` ou de transformer `useStorage` en module cumulant authentification, formulaires et accès aux ressources.

## 7. Plan d’implémentation par étapes

### Étape 1 — Valider les décisions et figer les contrats

État : livrable documentaire réalisé le 18 novembre 2026, prêt pour revue ; cadrage fonctionnel et décisions de la section 9 déjà validés. Les demandes explicites des étapes 2, 3 puis 4 autorisent les parties correspondantes de ces contrats ; les autres restent la cible des étapes suivantes.

**Livrable :** contrat comptes/profils, matrice profil/appareil et périmètre des routes protégées rédigés ; base des étapes 2 et 3 demandées explicitement.

### Étape 2 — Créer les modules et les deux stores

État : implémentée le 18 novembre 2026 dans `arachnea-stream/src/profiles/`. Les repositories commun/streaming possèdent des vues typées indépendantes ; le repository commun génère les UUID. Core fournit les transactions multitables locales, les index uniques vérifiés et SQLite en mémoire. Le coordinateur conserve les intentions, marqueurs d’application et reçus, bloque les identités avant nettoyage, rend caduques les anciennes opérations et reprend au démarrage. Un contrôle de cohérence des rattachements suit la reprise. Les services existants ne changent ni d’emplacement ni de contenu.

La composition est réalisée dans `main.rs`, séparément des stores du scraper. `ProfileStores::memory()` offre les mêmes transactions en mémoire sans fichier ; les anciens constructeurs mémoire restent inchangés. Les mutations d’un même compte sont sérialisées, y compris entre profils, avec verrous conservés dans la tâche bloquante après annulation du futur. Une seule instance applicative écrit par répertoire de données. Les documents streaming sont des enveloppes versionnées ; la validation métier des contenus appartient aux prochaines étapes. Aucun endpoint n’est enregistré à cette étape.

Vérifications exécutées pour l’étape 2 : `cargo fmt --all --check` et `cargo check -p arachnea-stream --offline` réussis depuis `server/` ; suite SQLite existante Core (`cargo test -p arachnea-core --features sqlite-persistence --lib persistence::sqlite_store::tests --offline`) : 9 réussites ; `cargo test -p arachnea-stream --lib --offline` : 14 réussites, 5 ignorés (fournisseurs réels). Le premier lancement Stream dans le sandbox refusait l’ouverture des sockets HTTP locaux de cinq scénarios ; relance avec permissions réseau locales réussie. Aucun nouveau test ni infrastructure ajouté ; un scénario SQLite existant est adapté. À la fin de l’étape 2, crash entre commits, suppression concurrente et reprise sur profils peuplés restaient à valider. L’étape 3 valide manuellement la reprise interstores et la cascade peuplée ; la concurrence avec une lecture réelle reste à vérifier.


Créer `arachnea-stream/src/profiles/` avec une séparation interne commune/streaming, ses modèles et sa composition. Ouvrir `profiles` et `arachnea-stream-profiles`, répartir les nouvelles tables selon leur domaine et déclarer leurs versions. Ajouter les transactions locales nécessaires et la coordination durable/idempotente des opérations entre stores. Générer les UUID des profils dans le socle commun et les réutiliser côté streaming. Conserver les stores mémoire utiles aux constructeurs et vérifications existantes. Ne modifier ni l’emplacement ni les données des stores actuels.

**Validation :** création des deux stores sur installation vide et existante, conservation des UUID entre domaines, transactions locales, redémarrage et reprise après échec d’initialisation ou interruption d’une opération entre stores. Vérifier le blocage des accès à un profil en suppression et l’absence de recréation tardive de ses données.

### Étape 3 — Ajouter comptes, sessions et gestion admin des utilisateurs

État : implémentée le 18 novembre 2026. `profiles/accounts/` fournit validations et Argon2id, création/liste/reset des comptes, sessions SHA-256 persistées avec expiration glissante de 30 jours et changement du mot de passe temporaire configurable à la création (obligatoire par défaut et après reset). Le reset et le changement du mot de passe révoquent les anciennes sessions atomiquement ; la suppression passe par le coordinateur et reste reprenable. Les reçus ne contiennent aucun mot de passe ni empreinte dérivée de celui-ci. Les routes JSON et binaires du scraper sont enveloppées par `ContentController` avant tout callback/ETag. Les écritures de comptes contrôlent méthode et origine ; les réponses personnelles sont `no-store`.

Le transport REST utilise un cookie HttpOnly SameSite=Lax limité au préfixe API de l’instance, Secure sur HTTPS. L’état de session renouvelle périodiquement le cookie ; tous les usages authentifiés prolongent l’expiration côté serveur. Les réponses de contenu lentes ne réémettent pas le cookie, afin de ne pas réinstaller celui d’un ancien compte après changement de session. Les origines de connexion et les Referer HTTPS déterminent Secure ; sans information de schéma, la lecture d’état ne réécrit pas le cookie. Le front et l’API doivent partager la même origine, directement ou via le proxy Vite. Le proxy de déploiement doit préserver Host et Origin/Referer cohérents.

Core transmet aux commandes IPC et au protocole binaire la véritable identité de webview capturée par Tauri. Les documents externes ne sont pas des appelants natifs de confiance ; les commandes admin IPC exigent la fenêtre admin. Le gestionnaire applicatif conserve le secret dans Keychain (macOS), Credential Manager (Windows) ou Secret Service via `secret-tool` (Linux), sous un espace lié à l’installation. Si le coffre est indisponible, session en mémoire avec avertissement visible et reconnexion après redémarrage, jamais de fichier en clair. Les événements natifs et BroadcastChannel invalident les vues ; focus/visibilité et un contrôle toutes les 30 secondes les revalident.

Le front public possède `AccountSessionGate` et un formulaire connexion/remplacement, avec entrée vers l’admin indépendante pour créer le premier compte. Il ne démarre les services de contenu qu’après validation de session. L’admin possède une vue Utilisateurs, des formulaires de création/reset et une confirmation de suppression. La politique admin existante est conservée : accès local implicite en loopback et dans la fenêtre desktop admin, authentification admin pour les clients serveur distants. Un compte public n’accorde aucune permission admin. Les anciennes données personnelles locales restent dans le parcours existant après connexion jusqu’aux étapes 5 à 8 ; aucune synchronisation ni import n’a été ajouté.

Validation manuelle HTTP effectuée sur une copie temporaire du serveur avec bases isolées et préfixe `/stage3` : refus JSON/proxy/range/licence/traduction anonymes, méthodes et origines, logins normalisés/uniques, répétition de création, restriction temporaire, changement de mot de passe, accès catalogue autorisé, reset révoquant deux sessions, déconnexion, expiration forcée, reprise de session après redémarrage, reprise d’une sauvegarde interrompue entre stores sans double incrément et suppression d’un compte peuplé avec nettoyage complet des deux stores/reçu répétable. L’accès admin local reste disponible après suppression. Les parcours graphiques Tauri, le coffre système et les cibles Linux/Windows restent à vérifier sur leurs plateformes ; aucune nouvelle suite de tests n’a été créée.

Checks existants exécutés : `cargo fmt --all --check`, `cargo check -p arachnea-stream --offline`, 31 tests Core, 75 tests Scrapyfy, 14 tests Stream (5 ignorés car fournisseurs réels) et 6 tests proxy ciblés ; `npm run type-check` / `npm run build-only` réussis pour les deux fronts. Les tests HTTP locaux ont été lancés avec permission d’ouverture de sockets. Les builds Vite signalent leurs avertissements de répertoire de sortie et de taille de chunks, sans erreur. `git diff --check` est vérifié en fin de changement.

Implémenter liste/création/suppression des comptes et reset de mot de passe sous autorisation admin, puis connexion publique, état de session et déconnexion. Ajouter la vue admin et ses formulaires, dont la confirmation de suppression et la définition du mot de passe temporaire. Implémenter le changement obligatoire de ce mot de passe avant accès au contenu. Persister comptes et sessions dans `profiles` et gérer expiration/révocation ainsi que le nettoyage streaming lors d’une suppression de compte. Brancher REST/Tauri avec le même modèle d’autorisation et ajouter l’écran de connexion. Protéger les routes de contenu, y compris les routes média, sans conserver de contournement anonyme.

**Validation :** opérations admin autorisées/refusées, cascade de suppression, reset invalidant les anciennes sessions et imposant le changement avant accès au contenu, compte valide/invalide, redémarrage serveur avec reprise, session expirée/révoquée, isolement admin/public, refus des appels catalogue/média anonymes et contexte de session desktop.

### Étape 4 — Ajouter les profils, les avatars embarqués et le quota admin

État : implémentée le 18 novembre 2026 à la demande explicite de ce lot. Les opérations REST/Tauri `profiles/list`, `create`, `get`, `update`, `delete`, `initialize_streaming`, `save_preferences`, `complete_onboarding` et `operation` revalident la session sous les verrous de cycle de vie partagés. Les noms suivent trim/NFC et case folding Unicode ; nom/quota sont contrôlés dans la transaction commune. Les préférences communes sont validées ; les préférences streaming utilisent un document complet validé, initialisé avec les seuls défauts serveur au premier usage explicite. Aucune home, section ou signet n’est créé implicitement.

Trois avatars PNG et leur manifeste sont embarqués au build. Le build refuse version, identifiants, chemins, fichiers et signatures MIME invalides ; le catalogue fournit le fallback et les URL du contrôleur actif. Les routes JSON/images exigent une session sans restriction. Un avatar retiré conserve son identifiant en base ; les consommateurs doivent utiliser le fallback du catalogue. L’interface de sélection et le retour visuel au guide sont livrés au lot 6 : après suppression du dernier profil, la liste expose `used_slots: 0` et le guide reprend.

L’admin propose un bloc autonome de plafond dans Paramètres, également disponible en desktop. Les opérations `admin/profile_settings` / `admin/save_profile_settings` utilisent le même verrou que les écritures port/réseau/configuration existantes. `max_profiles_per_user` vaut 3 par défaut, est persisté dans `data/config.json`, puis lu à chaud pour les créations ; les reçus `profile_quota_operations` sont enregistrés dans le même JSON pour garantir les retries après réponse perdue. Une baisse ne supprime aucun profil. Aucun rechargement du scraper n’est déclenché. Les catalogues de validation serveur (locales/thèmes/langues et défauts de lecture) doivent rester alignés avec les catalogues front.

Vérifications : compilation Rust et build du binaire, formatage ; 14 tests Stream réussis (5 ignorés fournisseurs réels), 75 tests Scrapyfy réussis ; type-check/build admin réussis. Les scénarios HTTP utilisent une copie temporaire isolée avec préfixe `/stage4` : ressources embarquées sans répertoire avatars externe, session restreinte/anonyme refusée, isolation entre comptes, unicité `Straße`/`STRASSE`, création répétable, six créations concurrentes pour deux places restantes (deux succès), révisions/conflits, champs inconnus, initialisation streaming différée, sauvegarde mixte, état du guide, baisse/hausse du plafond, panne de sauvegarde JSON conservant la valeur active, suppression complète/répétable et liste vide après dernier profil. Le redémarrage du binaire final conserve plafond, session et identité du profil ; les alias ISO/casse/séparateurs sont normalisés, les langues/bornes/locales invalides et la double initialisation sont refusées. Les tests HTTP existants ont nécessité la permission de sockets locales. Aucun test ni infrastructure n’a été ajouté. Les parcours graphiques Tauri et l’affichage réel du guide restent à vérifier aux lots suivants.

Créer `resources/avatars/avatars.json` et les fichiers référencés, les embarquer dans l’exécutable et exposer le catalogue ainsi que les images par le serveur. Implémenter liste/création/lecture/modification/suppression de profils et préférences, validation de l’avatar, propriété et quota transactionnel dans le store commun. Initialiser les préférences streaming au premier usage, sans imposer de données streaming à tous les profils. Ajouter le champ admin avec défaut à 3, persistance dans `data/config.json` et application à chaud, sans reconstruire le scraper pour ce seul changement.

**Validation :** images accessibles dans un exécutable distribué sans le dépôt, préfixe configuré et desktop, manifeste valide, comptes isolés, créations simultanées à la limite, modification du plafond, baisse sous le nombre actuel, échec de sauvegarde JSON, suppression en cascade, retour au guide après suppression du dernier profil et redémarrage.

### Étape 5 — Exposer homes, sections et signets par profil

État : implémentée à la demande explicite du lot 5. `profiles/homes`, `sections`, `bookmarks`, leurs mutations `save_*` / `delete_*` et `reorder_homes` / `reorder_sections` partagent REST/Tauri. La session, l’état actif et la propriété sont revalidés sous les verrous compte/profil avant tout accès ; les écritures refusent les opérations coordonnées en attente et exigent une initialisation streaming explicite.

Deux tables techniques complètent le store streaming existant : `stream_profile_collections(profile_id, kind)` conserve révision de collection et versions supprimées ; `stream_profile_operations(operation_id)` conserve requête normalisée/résultat. Une mutation, ses révisions et son reçu sont validés dans une même transaction SQLite. Les versions supprimées évitent qu’un ancien snapshot devienne de nouveau valide après suppression/recréation d’un signet ou d’une section. Les deux tables sont nettoyées avec le profil et contrôlées au démarrage. Aucun nouveau store physique ni changement du scraper/Core n’est introduit.

Les homes conservent références, blocs, options et schéma 1 ; création sans `home_id`, UUID et dates définitifs attribués par le serveur. Les éditeurs gardent leur signature `expectedUpdatedAt` ; l’adaptateur l’associe à la révision serveur du snapshot, sans relire une révision plus récente pour écraser une modification. L’unicité d’URL suit le slug front NFKD/suppression des marques/casse/underscores. Les réordonnancements sont complets, atomiques et protégés par la révision de collection. La suppression d’une home efface sa référence par défaut dans la même transaction streaming et incrémente les préférences si nécessaire. La home par défaut est désormais une home du profil ou l’agrégation ; les anciennes valeurs locales `service:*` ne sont pas reprises.

`customHomes.ts`, `homeSections.ts` et `entryBookmarks.ts` ne lisent/écrivent plus IndexedDB ; les anciens stores ne sont ni importés ni supprimés. Le transport personnel est no-store, sans cache ETag. Le contexte de profil est explicite via `setActiveProfileContext()` : le lot 6 l’active après sélection/validation et initialisation streaming. Le lot 5 ne choisissait ni ne créait automatiquement un profil. Sans profil actif, les listes sont vides et les écritures sont refusées avec une erreur visible. Les autres préférences locales restent en place jusqu’aux lots 7/8. Le formulaire de home par défaut utilise le snapshot streaming serveur et une révision attendue.

Les positions sont actualisées en mémoire à chaque événement lecteur ; les écritures ordinaires sont regroupées à 10 secondes par signet, puis sérialisées après confirmation de la révision précédente. Pause, fin, changement de média/démontage et passage en arrière-plan tentent un envoi immédiat. Les réponses perdues sont répétées avec le même UUID avant d’envoyer la dernière position en attente. Les échecs/conflits sont visibles, restent en mémoire et ne déclenchent aucun écrasement automatique ni stockage hors ligne. Le changement explicite de profil tente un flush borné à 5 secondes sous l’ancien UUID ; les réponses tardives et callbacks de l’ancien contexte sont rejetés. Aucune garantie d’envoi à la fermeture brutale. Sur connexion normale, jusqu’à environ 10 secondes de progression peuvent être perdues, davantage en cas de panne.

Validation : `cargo fmt --all --check`, `cargo check`/build Stream, 14 tests Stream existants réussis (5 fournisseurs réels ignorés), type-check/build public et lint CSS du nouveau composant. Contrôles HTTP sur bases isolées avec `/stage5` : homes composées, schéma/champs/credentials invalides, UUID/dates serveur, collisions de slugs accentués, ordres complets et conflits, options de sections conservées, signets d’épisode et position zéro, séparation profils/comptes, retries et corps divergents, six écritures concurrentes sur une révision (un succès), suppression/recréation sans révision réutilisée, home par défaut effacée atomiquement, panne injectée sur le reçu avec rollback des données et compteur, suppression profil/compte purgeant les six tables streaming, écritures concurrentes à une suppression et redémarrage/rejeu des reçus. Des vérifications manuelles chargent aussi les vrais modules front : temporisation réelle de 10 secondes, pause, réponse perdue, flush sous ancien UUID, adaptateurs REST homes/sections/default avec conflits et absence de persistance locale. Aucun test ni infrastructure ajouté au dépôt. Les parcours graphiques de sélection, le lecteur avec fournisseur réel et les fenêtres/coffres Tauri restent à vérifier avec les lots suivants.

Ajouter les repositories et opérations serveur dans `arachnea-stream-profiles` en conservant les identités de contenu, versions et sémantiques actuelles ; rattacher les données au UUID de profil commun après validation de sa propriété et de son état dans `profiles`. Gérer ordres, home par défaut et conflits. Adapter les services front, puis supprimer leur rôle de persistance locale active. Introduire la temporisation des mises à jour de lecture et les erreurs visibles.

**Validation :** homes composées et références conservées, réordonnancement, suppression de la home par défaut, signets d’épisodes et reprise de lecture, données isolées entre profils et appareils.

### Étape 6 — Intégrer sélection, reprise et guide initial

Brancher le chargement du profil avant les vues qui consomment ses données. Ajouter sélecteur et création réutilisable, guide commun nom/avatar/préférences puis initialisation streaming à l’usage du service. Gérer les opérations initiales reprises après interruption et les états de guide par domaine. Enregistrer localement le dernier profil par compte/serveur. Traiter réponses tardives, onglets, fenêtres desktop et profil devenu inaccessible.

**Validation :** première connexion, interruption/reprise du guide, connexion suivante, nouvel appareil, changement de compte/profil pendant des requêtes et pendant la lecture.

État : implémentée le 18 novembre 2026. `ProfileSessionGate` est monté par `AccountSessionGate` avec une clé de session ; il fournit un contrôleur de profils au guide, au sélecteur et à `App`. Au démarrage, le dernier profil du compte/serveur est restauré localement, les opérations durables en attente sont attendues, puis les guides `common` et `arachnea-stream` sont terminés et le streaming est initialisé avec `{}` au premier usage. La création réutilise une identité d'opération stable sur trois tentatives, complète le guide commun et laisse le guide service initialiser le streaming à la sélection. Le sélecteur permet de créer un profil supplémentaire, de supprimer un profil, de changer de compte et signale le quota ou un profil inaccessible. Un canal `BroadcastChannel` par serveur, un événement Tauri natif et les événements focus/visibilité rafraîchissent la liste et la révision active sans changer de profil dans les autres onglets/fenêtres. Les clés locales de reprise sont isolées par serveur et par compte. Aucune ancienne préférence personnelle n'est reprise : langue automatique et thème `arachnea-blue` par défaut.

Validation : `npm run type-check`, `npm run build-only` et `npm run lint:css` du front public réussis. Aucun test ni infrastructure ajouté. Les parcours graphiques première connexion/interruption/reprise, les fenêtres Tauri réelles et une lecture avec sauvegardes temporisées restent à vérifier manuellement sur les trois plateformes.

Corrections de review du lot 6 : le menu mobile natif est fermé avant l’ouverture du sélecteur ; resélectionner le profil actif revalide son état sans changer sa génération ni invalider les callbacks de signets du lecteur monté. Les suppressions incertaines conservent en mémoire leur UUID et leur révision d’origine dans le contrôleur de session. Le sélecteur propose une reprise explicite, y compris après disparition du profil de la liste active ; Réessayer dans l’état sans profil actif reprend également ces demandes. Une erreur de suppression cible uniquement le profil concerné ; un autre profil actif reste sélectionné. Si une erreur de transport masque une suppression partielle du profil actif, la liste est relue et le contenu est démonté dès confirmation de son état inaccessible. Les requêtes en attente ne survivent pas à la fermeture de la session/page ; les opérations acceptées restent durables côté serveur et reprennent au redémarrage. Aucun nouveau contrat serveur ni stockage navigateur ajouté.

Validation des corrections : type-check, build et lint CSS réussis. Vérifications ciblées du contrôleur réel avec Vue et transport simulé : génération et sauvegardes conservées lors d’une resélection, corps/UUID identiques après panne de nettoyage ou réponse perdue, double clic, reprise d’un profil retiré de la liste, retour à une liste vide après suppression du dernier profil et conservation d’un autre profil actif lors d’erreurs d’accès ou de révision. L’ordre de fermeture du menu mobile puis d’ouverture du sélecteur est vérifié sur le gestionnaire réel. Aucun test ajouté au dépôt ; validation graphique mobile et panne réelle des stores restent manuelles.

### Étape 7 — Passer les paramètres en brouillon explicite

Découpler les contrôles des refs globales persistées, mutualiser le formulaire avec le guide et ajouter Enregistrer/Annuler. Enregistrer avec révisions communes/streaming et coordination des deux stores si nécessaire ; appliquer l’ensemble confirmé après succès et protéger les fermetures. Exposer clairement un enregistrement en cours ou partiellement appliqué et permettre sa reprise sans double écriture. Adapter les traductions et commentaires qui décrivent aujourd’hui une persistance immédiate.

**Validation :** annulation sans écriture, sauvegarde puis redémarrage, erreurs conservant le brouillon, changement de langue/thème, conflit entre deux appareils, double clic, panne entre les deux écritures et reprise après réponse perdue.

État : implémentée le 18 novembre 2026 à la demande du lot 7. `usePreferenceDraft` charge un instantané serveur et construit des refs détachées, y compris pour les tableaux/objets imbriqués. `Parameters` reçoit exclusivement ces refs ; `ProfileCommonPreferences` est partagé avec le guide. `PreferenceEditor` porte Enregistrer/Annuler, confirmation d’abandon et protection des fermetures pendant une sauvegarde incertaine. La navigation et les actions homes/signets conservent leurs mutations dédiées.

Seuls les domaines modifiés sont envoyés à `profiles/save_preferences`, avec les révisions de l’instantané et un UUID stable. Une sauvegarde partielle ou une réponse perdue conserve le corps exact et l’UUID en mémoire ; Réessayer répète cette requête pour reprendre la coordination serveur existante. Le statut permet de distinguer le commit commun d’une attente de confirmation complète. Les contrôles restent bloqués tant que le résultat est incertain. Aucun document incomplet n’est appliqué globalement. Après achèvement et relecture serveur, le snapshot confirmé est appliqué aux préférences communes et aux réglages streaming du formulaire ; les autres vues sont invalidées. Les relectures inchangées ne réinitialisent pas les préférences de pistes. Les anciens chargements de dictionnaires ne remplacent plus la langue d’une sélection plus récente.

Un conflit conserve le brouillon. L’utilisateur peut charger les valeurs serveur, ou réappliquer explicitement les seuls champs qu’il avait modifiés aux dernières valeurs, les vérifier et enregistrer avec les nouvelles révisions. Aucune fusion ni écriture automatique. Les alias de langues de sous-titres et autorisations de traduction restent cohérents dans le document complet envoyé. Le brouillon et la requête incertaine ne sont pas persistés dans le navigateur : une fermeture forcée perd les changements non envoyés ; les opérations déjà acceptées restent durables et sont reprises par le serveur au redémarrage. Une opération en attente dans une autre fenêtre doit être terminée par son émetteur ou par cette reprise serveur ; le dialogue ne reconstruit pas son intention depuis un snapshot partiel. Les accès locaux hérités et préférences annexes du lecteur restent à retirer au lot 8.

Validation : type-check, build et lint CSS du front public réussis. Exécution ciblée du vrai composable avec Vue et un transport simulé : édition imbriquée/annulation sans écriture, sauvegarde mixte avec révisions, double clic, corps/UUID identiques après réponse perdue et état partiel, synchronisation des alias de langues, conflit et réapplication préservant les autres modifications serveur, réponse d’un ancien contexte ignorée. Aucun test ni infrastructure ajouté au dépôt. Les scénarios graphiques, le redémarrage avec sauvegarde depuis ce dialogue et une panne réelle entre stores, ainsi que les fenêtres Tauri restent à vérifier manuellement ; les contrôles HTTP du lot 4 couvraient déjà la coordination serveur, inchangée ici.

### Étape 8 — Retirer les anciens accès au stockage personnel local

Supprimer les chemins actifs de lecture/écriture IndexedDB et `localStorage` pour les données désormais serveur, ainsi que les watchers de persistance immédiate correspondants. Conserver seulement les états d’appareil explicitement retenus et les informations de reprise compte/profil. Vérifier l’i18n, le thème et le shell desktop pour qu’aucune ancienne préférence personnelle ne soit réinjectée avant le chargement du profil. Aucun import n’est développé.

**Validation :** mêmes profils initialement vides dans un navigateur vierge ou déjà utilisé ; anciennes homes, langues et signets locaux ignorés ; préférences d’appareil et reprise de connexion préservées.

État : implémentée le 18 novembre 2026. Le store initialise ses paramètres depuis des défauts détachés, sans lecture des anciennes clés `arachnea.param.*`, d’affichage home ou de préférences personnelles du lecteur. Les watchers et la fonction héritée d’écriture locale des paramètres sont retirés. Les changements de contexte remettent les préférences personnelles à leurs défauts ; i18n et thème ne relisent aucune ancienne valeur, y compris dans le shell desktop. Les anciennes données IndexedDB restent ignorées, sans import ni suppression.

Seuls volume/muet, navigation d’appareil et reprise du profil par compte/serveur restent persistés localement ; l’authentification conserve son cookie/coffre natif existant. Qualité (dont Auto), pistes manuelles et apparence des sous-titres sont restaurées depuis le snapshot streaming confirmé. Les changements du lecteur utilisent `save_preferences`, sérialisent les révisions et conservent le même corps/UUID en cas de résultat incertain ; le bandeau personnel permet Réessayer. Un conflit bloque les écritures lecteur jusqu’à une nouvelle application du snapshot serveur, sans rebasage automatique. Une modification des règles de pistes dans le brouillon efface aussi les anciens choix manuels dans le document envoyé. Les callbacks capturent leur génération ; un changement explicite de profil tente un flush borné à cinq secondes. Les demandes non confirmées restent uniquement en mémoire et ne sont pas garanties à la fermeture forcée.

Validation : type-check, build et lint CSS publics réussis. Vérifications ciblées hors dépôt avec les vrais modules store/lecteur et transport simulé : anciennes clés ignorées, volume/muet préservés, absence d’écriture personnelle locale et d’ouverture IndexedDB, réinitialisation du contexte, révisions sérialisées, qualité Auto, champs annexes conservés, requête identique après réponse perdue et callbacks/réponses tardives isolés. Aucun test ni infrastructure ajouté au dépôt. Les parcours graphiques sur navigateur vierge/déjà utilisé, une lecture avec fournisseur réel et les fenêtres/coffres Tauri sur les trois plateformes restent manuels pour le lot 9.

### Étape 9 — Vérifier et documenter la livraison

Exécuter les vérifications existantes pertinentes : `cargo fmt --check`, `cargo check` et suites existantes affectées depuis `server/`, puis `npm run type-check` et `npm run build-only` dans les deux fronts. Choisir les features Rust cohérentes avec les cibles REST et Tauri du dépôt. Ne pas ajouter de tests ni d’infrastructure de tests sans demande explicite ; adapter les tests déjà présents et consigner les scénarios manuels non couverts.

Mettre à jour README, `docs/TODO.md` et `CHANGELOG.md`. Documenter les deux stores, leurs identifiants communs, leur sauvegarde/restauration cohérente et la reprise des opérations coordonnées, la gestion admin des comptes et du reset, les accès authentifiés, les avatars embarqués, la durée des sessions, le plafond dans `config.json`, le démarrage sans import, l’absence de mode hors ligne et le comportement des conflits. Indiquer les contrôles impossibles à lancer et leur cause.

**Livrable :** fonctionnalité vérifiée, exploitation documentée et limites connues explicites.

État : réalisé le 18 novembre 2026. README racine, workspace et crate Stream alignés avec les lots livrés ; procédure de sauvegarde/restauration ajoutée au [README Stream](../../server/crates/arachnea-stream/README.md#profile-backup-and-restore). Le suivi des tâches distingue livraison locale et qualification de plateforme. Aucun changement de contrat, nouveau test ou infrastructure de tests introduit.

Environnement : hôte `aarch64-apple-darwin`, Rust `1.98.1`. Commandes Cargo depuis `server/`, dépendances existantes figées (`--locked`) et cache local (`--offline`). Stream n’a pas de feature REST/Tauri séparée : sa configuration par défaut compile les deux contrôleurs, les adaptateurs natifs macOS, la persistance SQLite et l’embarquement des avatars. Les tests Core activent explicitement `sqlite-persistence`. Les features optionnelles de diagnostics et les cibles croisées ne font pas partie de cette qualification.

| Contrôle exécuté | Résultat |
| --- | --- |
| `cargo fmt --all --check` | Réussi |
| `cargo check --workspace --locked --offline` | Réussi, bibliothèque et binaire Stream inclus |
| `cargo test -p arachnea-core -p arachnea-stream --lib --features arachnea-core/sqlite-persistence --locked --offline` | Core : 31 réussis. Premier passage Stream : 9 réussis, 5 échecs d’ouverture de socket loopback interdite par le bac à sable, 5 ignorés |
| `cargo test -p arachnea-stream --lib --locked --offline` avec autorisation des sockets locales | 14 réussis, 0 échec, 5 ignorés |
| `npm run type-check` et `npm run build-only` dans `front/admin-app/` | Réussis |
| `npm run type-check`, `npm run build-only`, `npm run lint:css` dans `front/public-app/` | Réussis |
| Vérifications ciblées temporaires du lot 8 relancées hors dépôt | Réussies : anciennes données locales ignorées, volume/muet préservés, absence d’accès personnel IndexedDB/localStorage, remise à zéro du contexte, sérialisation/réponse perdue/conflit/callback obsolète |

Les builds Vite signalent les répertoires de sortie hors racine (non vidés), la taille de certains chunks publics et l’avertissement expérimental Node sur l’import JSON de Stylelint ; ces avertissements ne bloquent pas les commandes. Les artefacts générés ne sont pas ajoutés à Git. Les contrôles HTTP isolés des lots 3–5 et simulations du lot 7 restent les validations historiques déjà consignées, sans nouvelle exécution ni extension de couverture revendiquée ici. Les suites existantes Core/Stream ne constituent pas une suite dédiée aux comptes/profils.

**Qualification manuelle restante, non couverte par cette session CLI :**

- Parcours graphiques navigateur vierge et déjà utilisé, premier profil et préférences par défaut sans import, interruption/reprise des guides, dernier profil supprimé et reprise du dernier profil par compte/serveur.
- Dialogue Enregistrer/Annuler : fermeture/abandon, double clic, conflits entre appareils, réponse perdue, redémarrage après acceptation et panne réelle entre les deux commits ; vérifier les états affichés et l’absence d’application partielle.
- Coffres et fenêtres/onglets Tauri macOS, Linux et Windows : reprise après redémarrage, coffre verrouillé/indisponible, nouvelle fenêtre, changement de compte et invalidation après reset/suppression. Compilation hôte et tests locaux ne prouvent pas ces cycles natifs ; aucun environnement natif Windows/Linux n’a été lancé ici.
- Lecture avec fournisseur réel : sauvegardes temporisées, pause/fin/arrière-plan, changement de profil en lecture, qualité Auto/pistes/sous-titres et reprise sur un autre appareil. Les cinq scénarios de fournisseurs restent ignorés par défaut ; ils nécessitent réseau, disponibilité et éventuels identifiants du fournisseur.
- Exercice opérateur de sauvegarde/restauration d’un jeu réel à l’arrêt, avec vérification après reprise et conservation du jeu de retour arrière. La procédure est documentée ; aucune base utilisateur existante n’a été restaurée ni modifiée pour cette validation.

## 8. Impacts et risques principaux

- Changement de contrat majeur : les données personnelles dépendent désormais d’une session et de la disponibilité du serveur. L’ordre d’initialisation des traductions, du thème et des homes change.
- Surface touchée : Core pour la persistance générique si nécessaire, Stream pour la composition et le domaine applicatif, administration pour le quota et les comptes, front public pour l’état et les parcours. Aucun changement YAML de scraping n’est nécessaire à ce besoin.
- La séparation en deux stores prépare les futurs services mais interdit de supposer un commit atomique global. Création, suppression et sauvegarde de paramètres mixtes nécessitent coordination, visibilité des états incomplets et reprise après échec. Sauvegarder/restaurer les deux stores de façon cohérente. Les stores techniques existants ne sont pas migrés.
- Risques de fuite entre profils via caches, callbacks retardés et fenêtres multiples ; l’identité du contexte doit accompagner toutes les opérations personnelles.
- Les données personnelles historiques du navigateur ne sont pas reprises : documenter ce choix et vérifier leur absence dans les nouveaux profils.
- L’authentification obligatoire touche les chargements média dans Arachnea ; une protection limitée aux API JSON laisserait des accès de contenu anonymes.
- Les opérations admin de suppression/reset doivent invalider immédiatement les accès des utilisateurs concernés, y compris les sessions déjà chargées en mémoire.
- Les écritures fréquentes de signets peuvent saturer le store streaming ; mesurer avec une lecture réelle après mise en place de la temporisation, sans ajouter une optimisation générale prématurée.

## 9. Décisions actées et détails d’implémentation

### Décisions utilisateur actées

| Sujet | Décision |
| --- | --- |
| Organisation Rust | Sous-dossier dédié dans `arachnea-stream`, sans nouvelle crate, avec séparation du socle commun et du domaine streaming. |
| Persistance | Deux nouveaux stores : `profiles` pour le commun, `arachnea-stream-profiles` pour le streaming ; aucune migration des stores existants. |
| Identifiants | Compte lié par le même `user_id` lorsque nécessaire ; UUID de profil généré par le socle commun et réutilisé dans les deux stores. |
| Préférences | Langue UI/thème communs ; homes, sections, signets/progression et préférences de lecture propres au streaming. |
| Initialisation des services | Données streaming créées au premier usage ; un profil commun ne dépend pas de la configuration d’un service. |
| Opérations entre stores | Coordination durable avec étapes idempotentes, blocage immédiat des profils supprimés et reprise après échec ; aucune hypothèse de transaction globale. |
| Avatars | Répertoire `avatars` avec manifeste JSON et fichiers embarqués dans l’exécutable, exposés par le serveur. Placement proposé : `resources/avatars/`. |
| Plafond | 3 profils par compte par défaut, configurable en admin et persisté dans le JSON de configuration serveur. |
| Données du navigateur | Aucun import ; nouveaux profils vides avec préférences par défaut. |
| Utilisateurs | Gestion en admin : création, suppression et réinitialisation du mot de passe. |
| Accès anonyme | Aucun accès à l’application et au contenu sans compte authentifié ; exceptions techniques limitées à la connexion et à son affichage. |
| Reset du mot de passe | Mot de passe temporaire défini par l’admin, changement obligatoire à la prochaine connexion et révocation des sessions existantes. |
| Connexion persistante | Expiration après 30 jours d’inactivité, renouvelée à l’usage, avec révocation lors d’une déconnexion, d’un reset ou d’une suppression. |
| Suppression des profils | Autorisée à l’utilisateur avec confirmation et suppression des données liées ; supprimer le dernier profil ramène au guide. |
| Volume et mode muet | Restent locaux à l’appareil. |
| Noms de profils | Uniques au sein d’un compte sans distinction de casse ; mêmes noms autorisés entre comptes. |
| Lecteurs externes | Hors périmètre pour le moment ; aucun mécanisme d’autorisation dédié à développer. |

### Détails d’implémentation

Aucun point fonctionnel supplémentaire n’est laissé ouvert par ces échanges. La gestion des lecteurs externes est explicitement exclue du plan actuel.

Les paramètres Enregistrer/Annuler sont livrés au lot 7, sans application globale avant confirmation complète. Le format exact du manifeste d’avatars, les noms d’opérations, la structure interne du sous-dossier et la temporisation des signets peuvent être arrêtés lors de l’implémentation sans nouvelle décision de périmètre.

Les demandes utilisateur autorisent les étapes 1 à 9. Le socle de l’étape 2 et les comptes/sessions/protections/fronts de l’étape 3 sont implémentés. Les API de profils, avatars et le quota administrable de l’étape 4 sont implémentés ; le lot 5 expose les données personnelles streaming et leur persistance front, avec temporisation de lecture ; le lot 6 livre la sélection, la reprise et le guide initial. Le lot 7 livre les brouillons explicites de paramètres et la reprise des sauvegardes. Le lot 8 retire les accès personnels locaux restants et rattache les préférences annexes du lecteur au serveur. Le lot 9 livre la documentation d’exploitation et les résultats des contrôles existants ; les qualifications manuelles restantes sont suivies dans `docs/TODO.md`.

## 10. Révision du 18 novembre 2026 — politique de mot de passe et tableau des utilisateurs

Décision utilisateur : le mot de passe minimal passe de 12 à **4 caractères**. La complexité n’est plus une condition d’acceptation mais un **indicateur** affiché dans les formulaires admin et public.

- `PUBLIC_PASSWORD_MIN_LENGTH` vaut 4 et le message de validation annonce « 4–128 ». La limite haute reste 128 valeurs scalaires Unicode.
- À la création, l’admin choisit si l’utilisateur devra remplacer son mot de passe à la connexion suivante (case cochée par défaut). `admin/create_user` accepte `must_change_password` (vrai si omis) et `admin/set_user_password_change` modifie ce drapeau sans toucher au mot de passe ni révoquer les sessions. Le reset continue d’imposer le remplacement et de révoquer les sessions.
- Le front admin remplace la liste et le formulaire de création par un tableau : identifiant, case « changement requis », boutons de réinitialisation et de suppression, et un bouton Ajouter à côté d’Actualiser ouvrant une boîte de dialogue dans le style de celle des identifiants de services. La navigation latérale place Utilisateurs puis Paramètres avant les services.
- Le front public autorise la réutilisation du mot de passe actuel : le serveur ne refuse plus l’égalité, le client affiche un avertissement et demande une confirmation explicite. Le formulaire affiche un indicateur de complexité partagé par `front/shared/password.ts`.
- Aucun test ni infrastructure ajouté. Vérifications : compilation Stream, type-check et build des deux fronts.
