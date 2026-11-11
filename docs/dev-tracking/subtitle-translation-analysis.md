# Analyse — traduction automatique des sous-titres

Date de l’analyse : 2026-11-06. Mise à jour : 2026-11-11. Statut : recommandations précisées par l’utilisateur ; plan révisé pour imposer Scrapyfy, séparer traduction et sous-titres, intégrer les langues favorites et sélectionner les services selon leurs métadonnées YAML. Lots 1 à 4 implémentés le 2026-11-10 ; qualification réelle des fournisseurs en attente d’accès ; correctifs des contrats des lots 1 à 4 livrés le 2026-11-11 (section 13) ; lot 5 livré le 2026-11-11 ; lots 6 à 10 : TODO.

Périmètre initial : analyse et plan, désormais suivi de l’implémentation des lots 1 à 5. L’implémentation précédente, non conforme aux exigences précisées, a été supprimée. Les lots 1 (contrats et séparation des couches), 2 (catalogue, credentials et administration), 3 (parser WebVTT, segmentation et planification documentaire) et 4 (exécution générique Azure/LibreTranslate) sont livrés selon le plan révisé ; le lot 5 est livré ; les lots 6 à 10 restent à réaliser. Les validations de l’implémentation supprimée ne valent pas validation des futurs livrables. Aucun appel fournisseur réel ni benchmark de qualité n’a été réalisé pour cette révision.

## 1. Objectif et recommandation

Ajouter un type de service dédié à la traduction, utilisable indépendamment du streaming, puis un orchestrateur backend qui reçoit l'URL d'un sous-titre et retourne un fichier WebVTT traduit complet. Chaque définition YAML de fournisseur déclare ses capacités optionnelles : traduction depuis une URL et/ou traduction d'un ou plusieurs blocs de texte avec des limites explicites.

Recommandation : traduction générique dans `arachnea-translation`, traitement documentaire dans le module `subtitles` d’`arachnea-stream`, services exécutés exclusivement par `arachnea-scrapyfy` et assemblage dans `arachnea-stream`. Commencer par **Azure Translator F0** et **LibreTranslate auto-hébergé**. Conserver Google Cloud Translation comme option complémentaire et DeepL comme intégration conditionnelle selon le compte disponible.

**Décision révisée le 2026-11-11 : le cache des fichiers traduits réside exclusivement dans le navigateur.** Le serveur ne conserve ni fichier traduit ni index de résultats. Un ETag composite transporte le validateur de la source (ETag amont ou hash du fichier source, éventuellement accompagné de Last-Modified), le hash YAML du service de traduction et la langue cible. Lors des appels suivants, le serveur revalide la source et le contexte de traduction avant tout appel fournisseur ; si rien n’a changé, il retourne `304` et le navigateur réutilise son fichier. Cette décision remplace la persistance backend prévue initialement ; les contrats des lots 1 à 4 sont adaptés (section 13), la récupération et l’orchestration du lot 5 sont livrées ; le transport public reste à réaliser.

## 2. État actuel du code

| Élément | Constat et conséquence |
| --- | --- |
| `server/crates/arachnea-stream/src/services/player_resolver.rs` | `ResolvedPlayerSubtitle` contient `lang`, `label`, `link`, `auto_translate`. Le marqueur automatique existe déjà ; il ne déclenche aucune traduction. |
| `docs/specifications/arachnea-stream-en.md`, section des pistes externes | Le contrat actuel expose des URL WebVTT lisibles par le navigateur. Les URL nécessitant des en-têtes sont proxifiées par le résolveur ; les pistes HLS/DASH intégrées sont gérées par les moteurs de lecture. |
| `front/public-app/src/types/entry.ts` et `src/services/rustify/entry/stream.ts` | Le front normalise les pistes en `ResolvedPlayerSubtitle`, avec `autoTranslate`. Pas de fournisseur ni d'état de traduction. |
| `front/public-app/src/composables/entry-details/entryPlayerStream.ts` et `entryPlayerMedia.ts` | Les pistes du lecteur et du flux résolu sont transmises à la source média. C'est une chaîne à conserver. |
| `front/public-app/src/composables/video/video-js-media-renderer/subtitles.ts` | `addRemoteSubtitleTracks()` appelle `Video.js.addRemoteTextTrack()` ; le player sait donc déjà consommer une URL de WebVTT traduite. Pas de traduction ni de chargement applicatif du fichier. |
| `front/public-app/src/composables/video/video-js-media-renderer/sourceController.ts` | Le contrôleur possède les pistes distantes, les supprime et invalide les chargements lors des changements de source. La traduction doit respecter ce cycle sans réappliquer la source vidéo. |
| `front/public-app/src/services/rustify/apiClient.ts` | `call_api()` attend une réponse JSON, conserve les ETags et payloads en mémoire et rejoue `If-None-Match` et `arachneaEtag`. Le chemin Tauri utilise `invoke`. Ce client ne convient pas directement à un fichier `text/vtt`. |
| `server/crates/arachnea-core/src/controler/contracts.rs` | `ControlerStreamInput` transporte méthode/en-têtes/contexte d'URL ; `ControlerStreamOutput` accepte octets, statut, type MIME et en-têtes. Réutilisable sans créer un contrôleur supplémentaire. |
| `server/crates/arachnea-core/src/controler/rest/service/responses.rs` | La réponse de fichier peut porter `ETag`, `Cache-Control`, etc. Le callback doit produire un corps vide pour `304`. `HEAD` supprime le corps mais exécute le callback : éviter qu'il lance une traduction coûteuse. |
| `server/crates/arachnea-core/src/controler/tauri/stream_response.rs` | Le protocole applicatif transmet les en-têtes et matérialise les réponses binaires. Réutiliser les chemins publics fournis par le contrôleur ; ne pas coder `/api` en dur. La persistance du cache navigateur sur ce protocole reste à qualifier. |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/{conditional.rs,global_etag.rs,scraper_cache.rs}` | Validation distante, agrégation d'ETags par source et cache de lignes extraites. Ce cache n'est pas un cache de fichiers traduits ; l'ETag du fichier final doit être distinct de celui de la source. |
| `server/crates/arachnea-scrapyfy/src/scrapyfy/{service_catalog.rs,scraper_query_collection.rs}` | Catalogues à partir de manifestes récursifs et YAML, paramètres et identifiants de groupe. Manifestes/YAML avec requêtes effectives, paramètres et métadonnées ; le mapping typé des capacités de traduction reste à définir. Réutiliser ce moteur exclusivement pour les fournisseurs. |
| `server/crates/arachnea-stream/src/{stream_scraper.rs,main.rs,admin_composition.rs,reloadable_stream_scraper.rs}` | Assemblage applicatif, stores typés, credentials, routes et rechargement. L'adaptateur admin reconnaît explicitement certains groupes ; ajouter un groupe nécessite de traiter son chargement et son rechargement. |

Les lots 1 à 3 ont créé la crate `arachnea-translation` et ajouté le module `subtitles` d’`arachnea-stream`, avec un parseur/sérialiseur WebVTT interne. SRT reste hors du chemin initial ; le traitement des storyboards VTT côté front ne remplace pas un parseur de sous-titres.

Les langues favorites sont déjà persistées sous `videoPlayer.subtitleLanguagePriority` (avec compatibilité de l’ancienne clé `videoPlayer.subtitleLanguages`). `ParameterSubtitleLanguages.vue` édite leur ordre ; `trackPreferences.ts` les utilise pour les profils `auto`, avec normalisation des langues et préférence pour les pistes humaines. Ces mécanismes sont à étendre, sans liste de langues concurrente.

## 3. Périmètre fonctionnel proposé

- Première version : pistes **externes WebVTT complètes** des vidéos à la demande. Une action explicite ou la sélection automatique autorisée par les préférences choisit une piste source et une langue cible ; seule la traduction nécessaire est calculée, sans générer toutes les langues favorites. Conserver la piste originale.
- Prévoir une normalisation SRT vers WebVTT dans l'orchestrateur, en extension explicite ; les résolveurs actuels promettent du WebVTT et ne doivent pas se mettre à exposer du SRT brut au player.
- Les sous-titres intégrés/segmentés HLS ou DASH, le direct, TTML/ASS, la transcription audio et les vidéos accessibles seulement en iframe demandent un autre travail. Les commandes de traduction sont indisponibles sans piste externe exploitable. Ce point limite la couverture initiale.
- Utiliser la langue déclarée lorsqu'elle est fiable. Si absente, détecter une fois sur un échantillon représentatif ou utiliser la détection du fournisseur sur l'ensemble, puis stabiliser ce choix ; éviter de redétecter chaque réplique courte.
- Traduction vers une seule langue par fichier ; ne pas retraduire une piste déjà dans cette langue et proposer par défaut une piste originale plutôt qu'une traduction de traduction.

## 4. Architecture révisée et séparation des responsabilités

Les précisions de l’utilisateur remplacent la recommandation antérieure de configuration autonome et d’adaptateurs Rust par fournisseur. Le moteur des services de traduction est **exclusivement `arachnea-scrapyfy`**. Du Rust supplémentaire est autorisé pour des primitives génériques réutilisables, sans branchement sur Azure, LibreTranslate ou un autre identifiant de fournisseur.

### Répartition recommandée

| Couche | Responsabilités | Dépendances et limites |
| --- | --- | --- |
| `arachnea-scrapyfy` | Exécuter les requêtes YAML, requêtes HTTP, authentification configurée, extraction et normalisation des réponses par actions/post-processeurs génériques. | Endpoints, en-têtes, sérialisation fournisseur, codes de langues et mapping des réponses sont décrits par les services YAML. Aucun endpoint fournisseur appelé directement depuis Translation ou Stream. |
| `arachnea-translation` | Contrats de texte/URL, capacités, langues normalisées, identifiants de blocs, planification générique, bornes, budgets et reprises. Un pont unique appelle Scrapyfy et valide sa sortie normalisée. | Aucune connaissance de WebVTT, cue, player ou piste vidéo ; aucune implémentation Rust Azure/LibreTranslate. Dépendance vers Scrapyfy pour le pont, jamais l’inverse. |
| Module `subtitles` d’`arachnea-stream` | Parsing et sérialisation WebVTT, protection du balisage, extraction des cues en blocs, segmentation compatible avec le document, reconstruction, validation temporelle et revalidation conditionnelle avant traduction. | Dépend de la crate `arachnea-translation`, jamais d’un fournisseur ; reçoit la récupération conditionnelle autorisée par injection, sans store de fichiers traduits. |
| `arachnea-stream` (hors module `subtitles`) | Résolution des pistes et proxys internes, droits, assemblage des couches, commandes JSON et transport REST/Tauri. | Fournit clients et stores existants ; ne contient ni mapping fournisseur ni logique WebVTT hors de son module `subtitles`. |
| Front public | Préférences par langue, décision de demander une piste, chargement et cycle de vie Video.js. | Consomme les capacités publiques et conserve les fichiers traduits avec leur ETag, sans credentials ni protocole fournisseur. |

La séparation fonctionnelle garde le moteur texte utilisable sans dépendre de la vidéo. Un crate dédié aux sous-titres a été écarté : son unique consommateur est `arachnea-stream`, et le module `subtitles` y matérialise la frontière sans ajouter de paquet. La crate `arachnea-translation` reste indépendante et ne dépend jamais de `arachnea-stream`.

Le planificateur Translation travaille sur des blocs abstraits ; la couche sous-titres reste propriétaire du lien `cue → segments`, du découpage respectant le balisage et des timestamps. L’identité de revalidation décrit source, service/langues/options, révisions du parseur/pipeline et périmètre d’accès ; elle est transportée dans l’ETag composite, pas utilisée pour indexer un cache documentaire backend.

### Configuration Scrapyfy et administration

Conserver `server/services/arachnea-translation/services.json` comme manifeste du catalogue Scrapyfy et ajouter des définitions `azure-translator.yaml` et `libretranslate.yaml` contenant de **vraies queries**. Il n’existe pas de catalogue JSON fournisseur parallèle à charger directement depuis Translation. Réutiliser paramètres, credentials chiffrés, activation et rechargement de Scrapyfy.

Définir un contrat normalisé d’entrée (blocs identifiés, langues, contexte optionnel) et de sortie (blocs traduits identifiés, langue détectée éventuelle, erreurs normalisées). Le YAML prend en charge les tableaux/objets exigés par chaque API et l’association de la réponse aux entrées lorsque le fournisseur répond par position. Le Rust générique vérifie la cardinalité et les identifiants.

Le moteur dispose déjà de méthodes HTTP, d’en-têtes et de construction de corps via actions. Auditer ses capacités pour transmettre des tableaux JSON et produire une requête sans effet de réseau afin de mesurer ses octets réels. Compléter uniquement les primitives génériques manquantes ; ne pas promettre que tous les besoins de traduction sont déjà couverts par le schéma actuel. Les capacités typées sont retournées par la query YAML `get_translation_metadata` décrite ci-dessous ; le moteur générique valide cette sortie. Compléter le schéma uniquement si nécessaire et documenter les modifications Scrapyfy en FR/EN. Le champ `metadata` existant à la racine reste un ensemble de tags, pas un substitut à cette sortie structurée.

Le catalogue public expose seulement les capacités effectives des services activés et configurés. Les secrets restent côté backend. La révision du résultat inclut les YAML/actions et paramètres qui modifient la requête ou sa normalisation ; une simple rotation de clé n’invalide pas nécessairement les traductions.

## 5. Contrat de service et limites

Interface générique indicative, mise en œuvre par un pont Scrapyfy commun : `TranslationService` expose `capabilities()`, `translate_url(...)` et `translate_texts(...)`. `translate_texts` est la voie de base ; `translate_url` est optionnelle. Une capacité URL absente ne rend pas un service texte inutilisable : le moteur récupère le fichier puis utilise la voie texte, avec l’adaptateur de document approprié. Les résultats texte doivent avoir une correspondance vérifiable avec les identifiants d'entrée, pas une chaîne contenant plusieurs blocs séparés arbitrairement.

| Capacité / limite | Sens proposé |
| --- | --- |
| `url_translation` | Option contenant formats acceptés, langues, limites du document et mode de récupération du résultat. Une traduction de page HTML n'implique pas une prise en charge des sous-titres. |
| `text_translation` | Option déclarant notamment si une liste est acceptée. Un service mono-texte a `max_blocks_per_request = 1`. |
| `max_blocks_per_request` | Nombre maximal de blocs dans une requête. |
| `max_units_per_block` | Taille maximale d'un bloc individuel. |
| `max_units_per_request` | Somme des tailles facturées/limitées dans la requête. |
| `size_unit` | Unité explicite : octets UTF-8, points de code Unicode ou autre unité réellement utilisée par l'API. |
| `max_request_bytes` | Taille de requête sérialisée, distincte de la taille du texte : JSON, échappements et contexte comptent. |
| `max_document_bytes`, `max_document_units` | Limites propres à la capacité document/URL. |
| `context_support`, `markup_support` | Capacités optionnelles, sans supposer qu'un tableau de textes partage un contexte linguistique. |
| Débit et quota | Requêtes/caractères par période, concurrence, budget gratuit restant et politique de reprise. Distincts des limites par requête. |

Une limite absente signifie « pas de limite fournisseur déclarée », pas « ressources infinies » : des bornes locales s'appliquent toujours. Une limite inconnue doit être identifiée comme telle et utiliser une valeur prudente configurable. Pour LibreTranslate, les plafonds configurés par l'administrateur doivent refléter ceux de l'instance ; ne pas supposer une découverte automatique.

### Métadonnées de service et langues disponibles

Chaque service expose une query Scrapyfy `get_translation_metadata`, statique ou alimentée par l’API du fournisseur. Sa sortie normalisée, versionnée, contient obligatoirement `languages`, une liste de codes de langues à **deux lettres minuscules**, par exemple `[en, fr, de]`. Valider les codes contre le catalogue de langues retenu, supprimer les doublons et rejeter les valeurs invalides ; une liste vide n’autorise aucune traduction. Les alias des pistes sont normalisés avant comparaison. Ne jamais déduire une langue disponible d’un simple test « deux caractères ».

`languages` est la liste commune requise. `source_languages` et `target_languages` sont optionnelles et valent chacune `languages` lorsqu’elles sont absentes. `supported_pairs` est **optionnel** : absent, tous les couples entre les langues source et cible déclarées sont considérés disponibles, hors traduction vers la même langue ; présent, il restreint les directions aux seuls objets `{source, target}` listés. Une liste explicitement vide n’autorise aucun couple. Chaque couple doit appartenir aux listes déclarées ; sa direction inverse n’est pas implicite. Le champ `all_pairs_supported` est supprimé pour éviter deux déclarations redondantes ou contradictoires.

Le contrat initial reste à deux lettres : ne pas tronquer arbitrairement un code fournisseur sans équivalent, ni assimiler des variantes régionales ou d’écriture quand cela change la prise en charge. Les correspondances nécessaires restent dans le YAML. Une langue sans représentation dans ce contrat est exclue explicitement de la première version ; un futur support de variantes demandera une extension du contrat.

La même sortie expose les capacités texte/URL, unités, plafonds, détection de langue et, si disponibles, restrictions de contexte/balisage. Les valeurs déclarées proviennent du YAML ou d’une découverte API normalisée par celui-ci. Les plafonds effectifs sont les plus restrictifs entre fournisseur, instance et configuration locale. Un plafond inconnu utilise une borne locale prudente, jamais une valeur illimitée implicite.

Les métadonnées sont chargées et validées au chargement/rechargement des services, puis mises en cache avec une fraîcheur bornée pour la découverte distante. Une absence ou une incohérence exclut le service de la sélection et produit un diagnostic admin contextualisé. Un échec de rafraîchissement ne transforme pas des capacités périmées en capacités garanties : conserver le dernier état valide seulement pendant sa fraîcheur annoncée, puis rendre le service indisponible. Aucune traduction n’est lancée pour découvrir ses capacités.

### Deux modes de traduction et repli depuis une URL

- `translate_texts` reçoit le texte fourni par l’appelant, sous forme d’un ou plusieurs blocs identifiés ; le moteur planifie les lots selon les limites du service et retourne les blocs traduits.
- `translate_url` reçoit l’URL d’un fichier que le **service distant** récupère lui-même. Cette query et sa capacité sont optionnelles ; leur déclaration précise formats, limites et forme du résultat complet. Elles ne désignent pas une simple page HTML ni un upload.
- Pour une entrée URL, utiliser cette voie seulement si elle est déclarée, compatible avec le document et accessible au fournisseur. Sinon, le moteur récupère le fichier de manière contrôlée, extrait les blocs, appelle `translate_texts`, puis reconstruit le document. Cette décision intervient avant l’appel fournisseur ; une erreur de traduction URL ne provoque pas une seconde tentative texte facturable implicite.

La façade URL est générique : elle orchestre récupération injectée, choix de capacité et adaptateur de document. Un fichier texte simple peut employer un adaptateur générique ; un WebVTT passe par le module `subtitles` d’`arachnea-stream`, qui reste propriétaire du parsing, des cues, des timestamps et de la reconstruction. La couche Translation ne traduit jamais le contenu brut d’un WebVTT comme une seule chaîne incluant son en-tête et ses timings. Stream fournit les droits et la récupération des proxys applicatifs. Ce repli est une fonction du moteur, pas une fausse query `translate_url` ajoutée à tous les YAML.

### Détection optionnelle de la langue source

Les métadonnées peuvent déclarer `source_detection` ; absente, la détection n’est pas disponible. Proposition de structure : `automatic` indique que la traduction accepte une langue source non fournie, `returns_detected_language` indique si la réponse expose la langue effective, et `query` peut désigner une query Scrapyfy de détection séparée. Une query séparée n’est pas obligatoire pour utiliser le mode automatique d’un service. Si la détection diffère selon la voie texte/URL, une surcharge dans la capacité concernée précise sa disponibilité.

L’entrée normalisée accepte `source_language: null` pour demander la détection ; `auto` n’est pas ajouté à la liste de codes à deux lettres. Le pont fournit le mode automatique au YAML, qui le convertit en omission du champ, valeur `auto` ou autre représentation propre à l’API. Une source connue et fiable reste envoyée explicitement. Sans source connue, sélectionner un service déclarant la détection pour la voie choisie ou utiliser sa query de détection ; sinon signaler `source_language_required`.

Pour plusieurs lots, détecter sur un échantillon représentatif via la query dédiée, ou récupérer la détection d’un premier lot représentatif puis fixer la source pour les suivants. Un service acceptant l’auto-détection sans restituer la langue peut servir une requête unique ; pour un document nécessitant plusieurs lots, il faut une détection préalable permettant de stabiliser la source, plutôt que laisser chaque cue ou lot détecter indépendamment. La sélection tient compte de cette contrainte. Les appels de détection sont également bornés et comptabilisés lorsqu’ils consomment du quota.

Quand elle est retournée, normaliser la langue détectée, vérifier les restrictions éventuelles de `supported_pairs` et l’enregistrer dans les métadonnées du résultat. Sans langue retournée, conserver explicitement le mode automatique et une langue effective inconnue dans l’identité de revalidation ; ne pas inventer de code source. Avec des couples restreints et une source inconnue, exiger une détection préalable si le moteur ne peut pas établir l’éligibilité du couple. Si la détection révèle que source et cible sont identiques, réutiliser l’original sans poursuivre les lots ni publier une fausse piste traduite ; le coût d’un premier appel déjà accepté reste possible.

### Sélection automatique du service

La nouvelle demande de sélection automatique complète les choix validés : un fournisseur préféré sert à classer les candidats compatibles ; il n’est plus nécessaire d’imposer un fournisseur unique à toutes les langues. Un choix manuel explicitement contraignant reste possible.

1. Constituer les candidats activés, autorisés, correctement configurés et disposant de métadonnées valides. Respecter le périmètre de compte et les destinations fournisseur autorisées ; une sélection automatique n’autorise pas une offre payante supplémentaire.
2. Filtrer sur le couple source/cible, la capacité utile (texte ou URL du format concerné), le contexte et le balisage requis. Si la source est inconnue, appliquer la politique de détection ci-dessus, sans exiger une query de détection séparée pour un service acceptant le mode automatique. Vérifier les couples restreints lorsqu’ils sont déclarés ; sans `supported_pairs`, utiliser les listes de langues par défaut.
3. Avant détection, mesure, simulation ou réservation de budget, tenter la revalidation du résultat navigateur selon la section 7, avec le service indiqué dans son ETag s’il reste autorisé et compatible avec la politique. Un budget de traduction épuisé n’empêche pas un `304` valide. Si une traduction est nécessaire, mesurer le document et simuler les lots pour chaque candidat sans appel de traduction. Une taille maximale **par requête** ne disqualifie pas un service si le découpage respecte ses limites ; une limite de document, un bloc non découpable, une durée estimée hors budget ou un quota insuffisant peuvent le disqualifier. Mesurer les octets avec la construction réelle de requête Scrapyfy.
4. Un ETag absent, inutilisable ou invalidé impose une traduction complète ; aucun résultat backend n’est recherché. En mode contraint, retenir uniquement le service demandé. En mode automatique, revalider une traduction du service précédent tant qu’il reste admissible ; sinon appliquer le classement ci-dessous. Un simple changement d’ordre ou de préférence ne rend pas ce service inadmissible.
5. À défaut de résultat, classer les candidats réalisables : fournisseur préféré s’il est compatible, ordre administrateur, puis nombre estimé de lots et identifiant stable pour départager. Ce classement déterministe évite d’inventer une préférence de qualité ou de prix non mesurée. Réserver le budget et la concurrence ; si une réservation échoue avant tout envoi, examiner le candidat suivant sans dépense fournisseur.
6. Figer un fournisseur pour tout le document avant le premier appel de traduction. Inclure son identité, ses révisions et le contexte des langues dans l’ETag retourné au navigateur. Aucun mélange de fournisseurs entre lots ni basculement automatique vers un autre après un appel de traduction en échec ; les reprises bornées sur le même fournisseur restent applicables.

Le résultat de sélection comprend le service retenu et des raisons normalisées pour les exclusions (`unsupported_language_pair`, `unsupported_capability`, `size_limit`, `budget_exhausted`, `metadata_unavailable`). En l’absence de candidat, retourner une erreur contextualisée et laisser le front poursuivre le repli entre langues favorites. Ne pas journaliser les textes ni les secrets.

### Proposition de format YAML

L’exemple suivant décrit un **service fictif** pour illustrer le contrat ; les langues et les plafonds sont des valeurs locales d’exemple, sans rapport avec les offres Azure ou LibreTranslate. Il n’est pas un fichier fournisseur prêt à charger. Les queries, types de sortie et champs usuels reprennent la structure Scrapyfy existante ; les noms de query de traduction et la sortie normalisée constituent le nouveau contrat proposé.

L’action `build_json` et sa syntaxe `fields` ont été **implémentées comme extension générique au lot 1** ; `build_json_array` et `json_array_projection` ont été ajoutées au lot 2. `build_json` construit et sérialise un objet typé, en échappant les chaînes ; `json_parameter` décode une valeur JSON validée et `string_parameter` insère une chaîne. Elle doit pouvoir être utilisée par n’importe quel service Scrapyfy et par la mesure de requête sans appel réseau. Les noms des paramètres d’exécution ci-dessous sont également proposés : les secrets sont injectés depuis les credentials backend, sans valeur persistée dans le YAML ni exposition publique.

```yaml
id: example-translation
title: Example translation service
http:
  mode: direct
credentials:
  required: true
constants:
  - name: base_url
    value: https://translator.example.invalid

queries:
  - name: get_translation_metadata
    scraper_type: static
    media_types:
      - application/arachnea-translation-metadata
    entries:
      - name: translation
        type: object
        value:
          schema_version: 1
          languages: [en, fr, de]
          source_detection:
            automatic: true
            returns_detected_language: false
          text_translation:
            query: translate_texts
            accepts_multiple_blocks: true
            size_unit: unicode_code_points
            max_blocks_per_request: 50
            max_units_per_block: 5000
            max_units_per_request: 10000
            max_request_bytes: 65536
            context_support: false
            markup_support: false
          url_translation: null

  - name: translate_texts
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}/translate"
    request_method: post
    request_headers:
      - name: Content-Type
        actions:
          - type: format_text
            argument: application/json
      - name: Authorization
        actions:
          - type: format_text
            argument: "Bearer {translation_access_token}"
    request_body_actions:
      - type: build_json
        fields:
          source: {string_parameter: translation_source_language}
          target: {string_parameter: translation_target_language}
          blocks: {json_parameter: translation_blocks_json}
    media_types:
      - application/arachnea-translation-result
    row_pointer: /translations/*
    entries:
      - name: id
        type: string
        pointer: /id
      - name: text
        type: string
        pointer: /text
```

Dans cet exemple courant, `source_languages`, `target_languages` et `supported_pairs` sont omis : les langues de `languages` sont utilisables dans les deux directions. Un service restreint pourrait ajouter, dans l’objet `translation` :

```yaml
source_languages: [en, fr]
target_languages: [en, fr, de]
supported_pairs:
  - {source: en, target: fr}
  - {source: fr, target: en}
  - {source: en, target: de}
```

Le service fictif accepte `auto` dans son champ `source` : pour une entrée normalisée nulle, le YAML utilise donc `translation_source_language = "auto"`. Cette convention d’API ne fait pas partie de `languages`. L’exemple ne retourne pas de langue détectée ; il illustre le mode automatique en une seule requête, avec la restriction multi-lots décrite ci-dessus.

Pour un service disposant aussi d’une voie URL, remplacer `url_translation: null` dans ses métadonnées et ajouter une query dédiée. Fragment proposé, toujours pour une API fictive retournant le contenu traduit directement dans un objet JSON :

```yaml
# Inside the translation metadata object.
url_translation:
  query: translate_url
  accepted_formats: [text/plain, text/vtt]
  max_document_bytes: 1048576
  result_mode: inline_document

# Additional item in the service queries list.
```

```yaml
- name: translate_url
  scraper_type: json
  base_url: "{base_url}"
  query_url: "{base_url}/translate-url"
  request_method: post
  request_headers:
    - name: Content-Type
      actions:
        - type: format_text
          argument: application/json
    - name: Authorization
      actions:
        - type: format_text
          argument: "Bearer {translation_access_token}"
  request_body_actions:
    - type: build_json
      fields:
        url: {string_parameter: translation_document_url}
        source: {string_parameter: translation_source_language}
        target: {string_parameter: translation_target_language}
  media_types:
    - application/arachnea-translation-document
  row_pointer: /document
  entries:
    - name: content
      type: string
      pointer: /content
    - name: content_type
      type: string
      pointer: /content_type
```

La sortie de document normalisée contient ici `content` et `content_type` ; le contenu complet est validé par l’adaptateur du format, puis encodé en UTF-8. Un service retournant une URL de résultat ou une tâche doit déclarer ce mode et ses queries de récupération ; ne pas annoncer `inline_document` dans ce cas. Ces modes restent des extensions à implémenter selon les fournisseurs retenus. Sans capacité URL, aucun fragment supplémentaire n’est nécessaire : le moteur télécharge et utilise `translate_texts`.

Entrée de traduction générique proposée : `translation_source_language = "en"`, `translation_target_language = "fr"` et `translation_blocks_json = [{"id":"block-1","text":"Hello"}]` sérialisé en JSON. Les identifiants sont des blocs abstraits, sans connaissance de cues. Dans cet exemple fictif, la requête contient ces blocs et la réponse distante suit `{"translations":[{"id":"block-1","text":"Bonjour"}]}`. Scrapyfy extrait les lignes `{id, text}` ; le pont générique vérifie les identifiants et la cardinalité avant de retourner le résultat.

Pour une API répondant par position ou utilisant d’autres codes de langue, adapter les actions et post-processeurs YAML, sans créer de code Rust dédié au fournisseur. La découverte dynamique remplace la query statique par une query JSON qui produit le même objet `translation` ; elle expose les langues réellement disponibles sur l’instance et n’ajoute `supported_pairs` que si les directions doivent être restreintes. Les budgets locaux, l’ordre de sélection et le fournisseur préféré sont des paramètres d’administration distincts des capacités annoncées, pour qu’un YAML fournisseur ne décide pas seul d’une dépense autorisée.

## 6. Traitement backend d'une URL

1. Valider l’accès à la piste, la langue cible et la politique de sélection. Résoudre le descripteur source et les candidats autorisés ; décoder les ETags clients utilisables et vérifier leur rattachement à cette source, ce périmètre et cette configuration.
2. Revalider/récupérer la source avec ses en-têtes backend : `If-None-Match` amont, ou `If-Modified-Since` si disponible, sinon téléchargement et comparaison du hash source. Accepter les URL HTTP(S) autorisées et les liens de proxy applicatif reconnus. Résoudre ces derniers en interne, sans requêter aveuglément une adresse locale ou un protocole Tauri.
3. Si source, langue cible, hash YAML et contexte de traduction correspondent au validateur client, retourner `304` sans corps et sans détection, planification, réservation de quota ni traduction. Sinon, obtenir les octets complets de la source ; après un `304` amont qui ne permet pas un `304` client (par exemple YAML modifié), effectuer une récupération non conditionnelle, car aucun corps source ou traduit n’est conservé au serveur.
4. Mesurer octets, texte et cues, vérifier format/encodage/bornes et sélectionner un fournisseur selon la section 5. La voie URL est éligible seulement si le service accepte ce format, si l’URL lui est accessible et si les limites sont respectées ; vérifier le résultat complet et sa structure temporelle. Ne pas transmettre URL privée, cookies, URL applicative ou secrets de proxy.
5. Sinon, parser, extraire les blocs, traduire les lots et réassembler par la voie texte. Cette voie reste disponible sans query URL, pour une URL privée ou un format non accepté par la capacité URL. Aucun fournisseur initial n’est supposé offrir une traduction de sous-titres par URL arbitraire.
6. Valider puis sérialiser le WebVTT UTF-8 et retourner `200` avec le fichier complet et son ETag composite. Le navigateur conserve les octets et le validateur ; aucune persistance backend du résultat. Aucun résultat partiellement traduit n’est publié comme succès.

Une API d'upload de document constitue une capacité différente de la traduction par URL. Elle pourra être ajoutée plus tard ; ne pas annoncer `url_translation` parce qu'un fournisseur possède un endpoint d'upload.

### Préservation des sous-titres et constitution des lots

Parser le document en structure, jamais par remplacement global du texte. Conserver identifiants, ordre, timestamps, paramètres de cues, `STYLE`, `REGION`, `NOTE` et métadonnées WebVTT ; ne traduire que le contenu visible prévu. Protéger les balises autorisées et timestamps internes, puis vérifier leur restitution. Rééchapper le texte retourné pour éviter qu'il introduise de nouvelles balises ou séparations de cues.

Une cue est l'unité logique initiale ; ses retours de ligne ne sont pas forcément des frontières de phrase. La traduction peut modifier les coupures de lignes et être moins naturelle lorsque la phrase s'étend sur plusieurs cues. Fournir un contexte voisin quand l'API le permet ; ne pas concaténer des cues avec des séparateurs que le moteur pourrait traduire ou supprimer. Le regroupement linguistique avec réalignement temporel est une amélioration séparée.

Former des lots en respectant simultanément nombre de blocs, taille individuelle, taille cumulée et octets sérialisés. Préserver une table `cue_id → segments → résultat`. Scinder une cue trop longue aux frontières de phrases puis de mots, sans couper Unicode ni balisage ; recomposer ses fragments dans la même cue. Vérifier cardinalité, ordre et segments non vides. Un changement de stratégie doit être borné, pas une boucle de tentatives consommant tout le quota.

Limiter la concurrence par fournisseur/compte ; respecter `Retry-After` et les reprises temporisées bornées. Distinguer erreurs d'authentification, langue non prise en charge, contenu trop volumineux, quota et incident temporaire. Aucun basculement automatique vers une offre payante ; la sélection d’un autre fournisseur reste limitée à la phase préalable, sans basculement implicite après un appel de traduction en échec.

## 7. Cache navigateur et ETag composite

### Référence existante : `get_entry`

`server/crates/arachnea-stream/src/stream_scraper/catalog.rs::get_entry` utilise `CacheType::ClientCache`. Les primitives de `arachnea-scrapyfy/src/scrapyfy/conditional.rs` construisent des fragments `E:<yamlhash>-<etag>` ou `C:<yamlhash>-<last_modified_unix>-<hash>`. Elles extraient le validateur renvoyé par le client, appliquent `If-None-Match` ou `If-Modified-Since`, et comparent le contenu lorsque nécessaire. `global_etag.rs` assemble et décode ces fragments. Reprendre ce principe pour la traduction, sans détourner le cache de lignes Scrapyfy en cache de fichiers.

### Composition et identité

L’ETag doit être **structuré, versionné et décodable** : un hash global seul ne permettrait pas de retrouver le validateur source sans état serveur. Les composants obligatoires sont :

- identité de la piste source et périmètre d’accès, sous une forme opaque sans URL signée ni secret ;
- type de validation source : `E` avec l’ETag amont exact, ou `C` avec le hash des **octets du fichier source**, et le timestamp Last-Modified si fourni ;
- identifiant du service ayant traduit et hash de son fichier YAML ;
- code normalisé de langue cible.

Conserver aussi les paramètres influant sur la traduction, la langue source demandée (ou le mode automatique), les options et les révisions Translation/WebVTT/pipeline. La `configuration_revision` du catalogue couvre déjà hash YAML et paramètres éditables ; la réutiliser en complément du hash YAML requis. Ne jamais inclure credentials, cookies ou contenu de sous-titre. Encoder les composants pour préserver guillemets, séparateurs et ETag amont faible ; borner la taille et valider le format reçu.

Format implémenté : `W/"st1.<payload>"`, où le payload est le JSON versionné des champs de `SubtitleTranslationValidator`, encodé en base64 URL sans remplissage. Taille maximale : 4096 octets. Le contenu est décodable et non chiffré ; les empreintes de source/périmètre/contexte doivent être opaques et ne contenir aucun secret. L’ETag décrit l’équivalence de la représentation selon ces entrées, pas un hash des octets traduits ; le validateur faible évite de promettre une identité binaire entre deux exécutions fournisseur. La stabilité d’un modèle distant sans changement YAML n’est pas détectable ; une modification connue du modèle doit changer une révision de configuration. Le catalogue expose `service_yaml_hash` sous forme optionnelle : sans hash connu, ne pas émettre de validateur présenté comme fiable.

Le terme « hash du fichier de destination » dans la demande est interprété ici comme **hash du fichier source** : le fichier traduit reste dans le navigateur, et son hash seul ne permet pas de savoir si la source a changé avant de traduire.

### Premier appel et revalidation

Au premier GET sans validateur utilisable, récupérer la source et conserver son ETag amont lorsqu’il existe. Sinon calculer son hash, en ajoutant Last-Modified si disponible. Traduire une seule fois, construire l’ETag composite avec le YAML et la cible, puis retourner `200 text/vtt; charset=utf-8` avec tous les octets.

Lors des GET suivants, contrôler les droits puis extraire du validateur client la composante source et le contexte de traduction. Ne jamais conclure à l’absence de modification à partir du seul ETag client : interroger la source ou comparer ses octets téléchargés.

| Situation | Vérification source | Réponse / traduction |
| --- | --- | --- |
| ETag source disponible | Le rejouer tel quel dans `If-None-Match` amont ; `304` prouve la stabilité de la source | Si contexte identique : `304` client, aucune traduction |
| Pas d’ETag source, Last-Modified disponible | Rejouer le timestamp avec `If-Modified-Since` ; sur `200`, comparer le hash des octets au hash source reçu | Si source et contexte identiques : `304` client, aucune traduction |
| Aucun validateur amont | Télécharger la source et comparer son hash au hash source reçu | Si hash et contexte identiques : `304` client, aucune traduction ; le téléchargement source reste nécessaire |
| Amont ignore le conditionnel et renvoie `200` | Comparer le validateur source applicable ; en mode `C`, recalculer le hash | Si stabilité démontrée et contexte identique : `304` client ; sinon traduction et `200` |
| Source modifiée, YAML/révision/options/cible différents, service précédent devenu inadmissible | Récupérer le corps source complet, même si une tentative conditionnelle avait renvoyé `304` | Traduction complète et `200` avec nouvel ETag |
| ETag absent, ancien, malformé ou lié à une autre source / un autre périmètre | Traiter comme un appel sans validateur réutilisable | Traduction complète et `200` ; aucun fichier backend à relire |
| Source indisponible ou accès refusé | Aucun état navigateur ne prouve sa fraîcheur actuelle | Erreur contextualisée, sans `304` ni traduction de remplacement |
| Serveur redémarré, descripteur encore résoluble et corps navigateur conservé | Même revalidation à partir de l’ETag client | `304` possible sans fichier ni index backend |

Un timestamp n’est pas envoyé comme un ETag HTTP arbitraire : il alimente `If-Modified-Since`, comme le fragment `C:` de Scrapyfy. La fiabilité du `304` dépend du respect des validateurs par l’amont. En mode hash seul, économiser la traduction n’économise pas le téléchargement source. Un ETag amont différent invalide le mode `E` ; ne pas promettre une déduplication par contenu entre URL signées renouvelées sans hash source comparable.

Prendre en charge les listes `If-None-Match` et les validateurs faibles en évaluant un candidat complet, sans mélanger les composants de plusieurs tags. Le joker `*` suit la sémantique HTTP d’existence de représentation ; il ne transporte aucun validateur source et ne sert pas à prouver sa stabilité. Sa réponse sans corps ne permet pas à un client ayant perdu ses octets de les récupérer. Le client applicatif renvoie uniquement un tag dont il possède le corps.

Renvoyer `ETag` et `Cache-Control: private, no-cache` avec `200` et `304`, et exposer ETag par CORS si nécessaire. Une absence de changement de langue ne suffit pas : source, hash YAML et contexte doivent tous correspondre. Ne pas appeler la détection ou les services de traduction pour valider un `304`.

### Stockage et concurrence

Le navigateur possède le couple `ETag + octets`. S’il perd le corps, il refait un GET sans validateur : une nouvelle traduction est alors nécessaire. Le serveur peut garder les métadonnées de catalogue, credentials, descripteurs autorisés et budgets existants ; cela ne constitue pas un cache de fichiers traduits. La persistance éventuelle des descripteurs sert à résoudre les jetons, pas à réutiliser des traductions.

Une mutualisation des demandes strictement identiques **en cours** peut partager leur résultat temporaire en mémoire, isolé par périmètre d’accès ; supprimer cet état à la fin de l’exécution, sans rétention pour des appels ultérieurs. Aucun index source/résultat, store documentaire, cache de lots ou réservation persistante de cache n’est requis. Sans coordination entre processus, des traductions simultanées peuvent être dupliquées ; documenter cette limite et maintenir les budgets fournisseur.

`HEAD` retourne `405 Method Not Allowed`, sans récupération ni traduction ; le lot 6 doit appliquer ce contrat et l’en-tête `Allow` adapté. Les erreurs ne sont jamais conservées comme des fichiers VTT réussis.

## 8. Contrat backend/front proposé

Séparer la préparation JSON du transport du fichier pour conserver `call_api()` et les deux modes REST/Tauri.

1. `get_translation_services` : catalogue public des services utilisables, capacités et langues ; aucune clé.
2. `prepare_subtitle_translation` : reçoit `url`, identité/contexte de la piste, langue source facultative, langue cible et politique de sélection : `auto` avec fournisseur préféré facultatif, ou service contraint. Vérifie les droits et les candidats possibles puis produit un descripteur sans lancer de traduction. En mode automatique, le fournisseur définitif est choisi au GET après mesure du contenu ; la préparation ne peut pas garantir sa compatibilité de taille. L'URL reste l'entrée fonctionnelle ; le front n'envoie pas le contenu du fichier.
3. La réponse contient une piste compatible avec le contrat existant : `{ link, lang, label, auto_translate: true }`. `link` est construit avec `stream_public_path()` et identifie la demande par un jeton opaque, sans credentials ni URL source en clair.
4. `GET <public-path>/subtitle_translation/<token>.vtt` revalide avant toute traduction : `304` vide si source et contexte correspondent, sinon traduction complète et `200` avec ETag composite. Le jeton doit rester stable pour une même demande autorisée pendant sa durée de vie afin que le cache navigateur soit utile ; son renouvellement doit préserver l’identité logique de demande ; une nouvelle URL ne retrouve pas automatiquement le corps du cache HTTP natif.

La préparation utilise le pont JSON existant, qui emploie actuellement GET en REST. Garder cette préparation sans traduction ni autre effet coûteux. Un éventuel passage à POST doit être explicitement implémenté/documenté ; ne pas supposer que `call_api()` l'envoie déjà.

La résolution du jeton doit fonctionner après redémarrage pendant sa durée de validité annoncée : descripteur persisté ou jeton signé avec gestion explicite de son expiration. Vérifier l'accès lors du téléchargement, et empêcher qu'un jeton d'un utilisateur donne accès aux sous-titres privés d'un autre. Les statuts d'erreur du callback fichier sont construits explicitement, car les adaptateurs actuels convertissent un `Err` générique en `400` texte.

### Langues favorites et autorisation de génération

Étendre la liste existante, en conservant `videoPlayer.subtitleLanguagePriority` comme ordre de référence. Proposition : une table persistée `videoPlayer.subtitleTranslationByLanguage` associe à chaque code normalisé un booléen « Générer par traduction si absent », désactivé par défaut. Ce nom est indicatif. Éviter de convertir silencieusement les `string[]` existants en objets incompatibles ; les réglages absents migrent vers `false`. Réordonner conserve le réglage de la langue ; suppression, remplacement et doublons normalisés sont traités explicitement par les sanitizers.

Pour un profil demandant des sous-titres `auto`, parcourir les langues favorites dans leur ordre. Pour chaque langue : réutiliser une piste correspondante selon les règles exact/base et humain/automatique existantes ; à défaut, si la génération est autorisée, qu’une source externe originale est exploitable et qu’un service prend en charge le couple de langues, demander une seule traduction. Pendant l’attente, conserver la lecture et la piste courante ; en cas d’échec, poursuivre le repli sans boucle de requêtes. Une langue prioritaire autorisée à la traduction peut ainsi être générée avant de retenir une piste d’une langue moins prioritaire.

Une sélection manuelle ou la désactivation des sous-titres prime sur ce déclenchement automatique. Les profils `disabled` ne génèrent rien. Un profil ciblant une langue explicite utilise le même réglage si cette langue figure dans la liste ; sinon, aucune génération automatique sans demande manuelle. Une action manuelle reste possible et explicite, indépendamment de l’autorisation de génération automatique. Ne pas calculer toutes les langues favorites au chargement. La restauration du profil peut déclencher la génération seulement si le réglage l’autorise ; annuler l’ajout au moindre changement de source, de choix manuel ou de préférence. Le fournisseur est sélectionné automatiquement parmi les services compatibles et autorisés, en tenant compte du fournisseur préféré configuré ; une sélection manuelle contraignante reste possible. Aucun changement d’offre facturable n’est autorisé implicitement.

### Intégration Vue / Video.js

Ajouter une commande de traduction dans le menu des sous-titres, avec piste source, langue cible et état de chargement/erreur, en réutilisant les langues favorites et leurs autorisations de génération. Un composable dédié orchestre sélection, préparation et récupération ; `subtitles.ts` reste responsable de l'ajout/retrait des pistes. Garder la vidéo en lecture pendant le calcul et n'activer la traduction qu'après chargement réussi.

Deux transports sont possibles : URL directement donnée à Video.js (simple, cache HTTP natif, retour d'erreur moins contrôlable), ou `fetch` dédié puis `Blob` WebVTT (meilleur contrôle des erreurs, annulation et sélection après chargement). **Recommandation : `fetch` dédié**, s'appuyant sur le cache HTTP en REST, avec ajout du Blob comme piste après réception complète. Ne pas passer ce fichier à `call_api()`.

Avec le cache HTTP natif, `fetch` restitue habituellement le corps revalidé ; si l'application gère elle-même `If-None-Match`, elle doit conserver le corps associé et traiter un vrai `304` sans corps. Sur le protocole Tauri, qualifier le cache natif ; au besoin prévoir un cache applicatif `ETag + octets`, borné, et une persistance IndexedDB si nécessaire au-delà de la session. Ne pas confondre URL Blob et identité de cache de la requête backend.

Invalider les opérations au changement de source/langue ou au démontage ; ignorer les réponses tardives et révoquer les URL Blob retirées. Réutiliser le mécanisme de propriété des pistes et éviter de modifier la liste source d'une manière qui redémarre la vidéo. Étendre les préférences avec une identité stable de traduction, jamais le jeton temporaire ou l'URL Blob. Ajouter les libellés i18n et distinguer visuellement les pistes automatiques. Le front admin reçoit les paramètres du nouveau groupe sans exposer les secrets au catalogue public.

## 9. Taille des fichiers et offres gratuites

### Hypothèses de dimensionnement

Les chiffres suivants sont des **scénarios de calcul**, pas des mesures de sous-titres du dépôt. La recherche n'a pas trouvé de fichiers `.vtt`/`.srt` locaux permettant un échantillonnage représentatif.

| Scénario, une langue cible | Cues | Texte envoyé, hypothèse | Conséquence |
| --- | ---: | ---: | --- |
| Épisode | 800 | 32 000 caractères | Peut tenir dans un lot Azure, sous réserve des autres limites |
| Film de référence | 2 000 | 80 000 caractères | Au moins deux lots Azure, trois lots Google v3 à son maximum de caractères |
| Contenu long/dense | 5 000 | 200 000 caractères | Au moins cinq lots Azure à cause du nombre de blocs, sept lots Google v3 à cause des caractères |

Calcul Azure : `max(ceil(cues / 1 000), ceil(caractères / 50 000))` est une **borne inférieure**, pas un nombre garanti : tailles individuelles, marges et sérialisation peuvent augmenter le nombre de lots. Plusieurs langues multiplient le volume. Les timestamps et métadonnées augmentent le fichier sans nécessairement augmenter le texte facturé lorsque seuls les blocs sont envoyés. Toujours distinguer octets et caractères Unicode.

Le YAML initial applique des plafonds plus prudents que le maximum Azure officiel : 100 blocs déclarés et 50 par défaut local, soit au moins 16/40/100 lots pour ces scénarios si chaque cue produit un bloc. La borne théorique ci-dessus ne décrit donc pas le nombre de requêtes actuellement exécutées. Les limites officielles Azure et les contrats `/translate` et `/detect` LibreTranslate ont été revérifiés le 2026-11-10 ; `/detect` LibreTranslate attend `q` sous forme de chaîne ([référence officielle](https://docs.libretranslate.com/api/operations/detect/)). Les plafonds LibreTranslate du YAML (50 blocs, 5 000 caractères par bloc et 10 000 cumulés) sont des choix applicatifs, à adapter aux paramètres de l'instance.

### Sélection proposée

| Service | Gratuité documentée | Limites pertinentes | Décision |
| --- | --- | --- | --- |
| **Azure Translator F0** | 2 millions de caractères/mois selon la page de tarifs pour la combinaison admissible de traduction standard et entraînement personnalisé ; la page de limites actuelle indique aussi un plafond F0 de 2 millions de caractères/heure | Traduction texte : 1 000 éléments, 50 000 caractères par élément et par requête, somme sur toutes les langues cibles. Débit également limité ; respecter une consommation régulière. | **Premier fournisseur hébergé recommandé** : environ 25 films de référence/mois pour un compte dédié, avant autres usages/reprises, sous réserve du plafond horaire et des conditions actuelles. Créer une ressource F0, pas une ressource facturable S1. La traduction de documents n'est pas supposée gratuite dans ce plan. |
| **LibreTranslate auto-hébergé** | Logiciel libre, sans quota mensuel imposé par un abonnement SaaS ; matériel, exploitation et hébergement à financer | `/translate` accepte un texte ou un tableau. Limites `--char-limit`, `--batch-limit`, débit et modèles à configurer pour l'instance. | **Deuxième fournisseur recommandé**, adapté aux volumes récurrents. Démarrage prudent possible à 50 blocs et 5 000 caractères cumulés par lot, valeurs locales à mesurer, sans les présenter comme limites officielles. Qualité et langues à qualifier. Le service hébergé libretranslate.com demande une clé payante. |
| **Google Cloud Translation NMT** | 500 000 premiers caractères/mois, sous forme d'un crédit mensuel de 10 USD | v3 : 30 000 points de code/requête, recommandation de 5 000 ; v2 : 100 000 octets/requête. | **Option complémentaire**, environ six films de référence/mois. Projet avec facturation ; dépassement payant. Mettre un plafond applicatif au crédit gratuit ; une alerte de budget ne constitue pas un arrêt automatique. Ne pas assimiler les autres modèles ou la traduction de documents à ce crédit NMT. |
| **DeepL API Developer / ancien Free** | Developer : 1 million de caractères au total, non renouvelable. L'ancien Free à 500 000/mois n'est plus disponible à la souscription selon le centre d'aide consulté. | API texte : requête totale 128 KiB ; tableau de textes. La référence actuelle consultée n'affiche pas de plafond de 50 textes : un plafond local de 50 serait prudent, pas un contrat fournisseur établi. Upload SRT/WebVTT : 150 KB par document. | **Conditionnel**, pas une offre gratuite récurrente pour un nouveau compte. Environ douze films de référence sur l'essai, ou six/mois pour un compte historique Free. Préférer les lots texte pour ne pas exclure les fichiers dépassant 150 KB. |

Sources officielles : [tarifs Azure](https://azure.microsoft.com/en-us/pricing/details/cognitive-services/translator/), [limites Azure](https://learn.microsoft.com/en-us/azure/ai-services/translator/service-limits), [API texte LibreTranslate](https://docs.libretranslate.com/api/operations/translate/), [installation et limites LibreTranslate](https://docs.libretranslate.com/guides/installation/), [clés LibreTranslate](https://docs.libretranslate.com/guides/manage_api_keys/), [tarifs Google](https://cloud.google.com/products/translate/pricing?hl=en), [quotas Google](https://docs.cloud.google.com/translate/quotas), [offres DeepL](https://support.deepl.com/hc/en-us/articles/360021200939-DeepL-API-plans), [limites DeepL](https://developers.deepl.com/docs/resources/usage-limits), [API texte DeepL](https://developers.deepl.com/api-reference/translate/request-translation).

Les équivalents en films sont des divisions du quota par 80 000 caractères, arrondies à l'entier inférieur. Ils ne comprennent ni autres usages du compte ni traductions répétées et ne constituent pas une garantie commerciale. Les offres doivent être revérifiées au moment de l'intégration ; aucune inscription ni validation de compte n'a été effectuée.

### Solutions écartées du lot initial

- **MyMemory** : 500 octets par requête et 5 000 caractères/jour anonymement, 50 000 avec une adresse email. Le scénario film dépasse même ce second quota journalier et impose trop de petites requêtes. [Spécification officielle](https://mymemory.translated.net/doc/spec.php), [quotas officiels](https://mymemory.translated.net/doc/usagelimits.php).
- **Amazon Translate** : volume intéressant, mais gratuité annoncée de 2 millions de caractères/mois limitée à douze mois ; ne répond pas au choix prioritaire d'une gratuité récurrente durable. Ce n'est pas une exclusion pour taille insuffisante. [Tarifs officiels](https://aws.amazon.com/translate/pricing/).
- Endpoints internes de sites de traduction et miroirs publics sans quotas/contrat vérifiables : aucune capacité garantie pour des fichiers complets. Ne pas les présenter comme APIs gratuites prises en charge.
- Offres LLM : hors première sélection faute de contrat déterministe de correspondance entre cues et de budget gratuit durable vérifié dans cette analyse. Leur ajout demanderait une qualification distincte.

## 10. Bornes et exploitation à prévoir

La récupération d'une URL fournie au backend introduit un accès réseau indirect : rattacher la demande à une piste autorisée, limiter HTTP(S), contrôler résolutions et redirections, refuser les destinations privées/métadonnées hors proxy applicatif reconnu. L'endpoint LibreTranslate local est une configuration administrateur distincte, pas une permission de charger n'importe quelle URL locale.

Proposer des plafonds configurables de fichier décompressé, nombre de cues, taille traduisible totale, durée de récupération et durée de traduction, plus une concurrence faible par compte. Les fixer après mesures ; un document complet ne doit pas permettre des allocations ou attentes non bornées. Pour les contenus lents, qualifier la durée tolérée par REST/reverse proxy/Tauri : si elle est insuffisante, ajouter une préparation asynchrone avec état de tâche et fichier final séparé, sans retourner du JSON `202` là où Video.js attend du WebVTT.

Mesurer réponses `304`, revalidations et téléchargements source, caractères envoyés, lots, durée, erreurs et quota consommé. Ne pas journaliser textes, credentials ou URL signées complètes. Le quota gratuit est partagé au niveau du compte fournisseur ; un compteur local seul ne connaît pas les usages extérieurs. Garder une marge et interroger l'usage officiel quand disponible. L'annulation côté navigateur empêche un ajout obsolète ; elle ne garantit pas l'annulation d'une traduction déjà acceptée et facturée par le service.

## 11. Décisions acquises et questions restantes

L’utilisateur a précisé les recommandations : Scrapyfy obligatoire, Rust générique, séparation traduction/sous-titres et intégration des favoris. Le plan ci-dessous remplace les choix antérieurs incompatibles. L’implémentation précédente a été supprimée ; les lots 1 à 4 sont livrés selon le contrat initial. Les adaptations de la section 13 sont livrées ; le lot 5 est livré ; les lots 6 à 10 restent à TODO.

### Décisions acquises

- Deux couches indépendantes : `arachnea-translation` générique et le module `subtitles` d’`arachnea-stream` documentaire ; assemblage dans Stream.
- Fournisseurs exclusivement décrits et exécutés par Scrapyfy ; aucune configuration autonome ni adaptateur Rust spécifique.
- Réutilisation de la liste ordonnée de langues favorites avec autorisation de génération par langue, désactivée par défaut ; générer la langue prioritaire autorisée avant de retenir une piste d’une langue moins prioritaire.
- Privilégier une piste source originale non traduite, puis une autre piste humaine exploitable.
- Métadonnées de service issues d’une query YAML, avec codes de langues à deux lettres, couples optionnels et plafonds ; sélection générique automatique selon le besoin, un fournisseur par document.
- `translate_texts` pour le texte fourni et `translate_url` optionnelle pour une URL récupérée par le service ; sans voie URL compatible, récupération par le moteur et traduction des blocs texte. Détection source optionnelle, intégrée à la traduction ou query séparée.
- Fournisseurs initiaux : Azure Translator F0 et LibreTranslate auto-hébergé, désactivés tant que leur configuration Scrapyfy n’est pas valide. Sélection automatique préalable selon langues/capacités/tailles et budgets, avec fournisseur préféré configurable ; aucun basculement payant ni changement de fournisseur après un appel de traduction en échec.
- Première livraison : pistes WebVTT externes complètes à la demande, une langue cible par demande, fournisseur sélectionné/configuré et conservation de la piste originale.
- Préparation JSON sans traduction, récupération du fichier complet par le chemin public REST/Tauri, cache des fichiers exclusivement dans le navigateur et ETag composite source + YAML + cible, validé avant traduction.
- Chargement frontend par `fetch`, ajout du Blob après succès, sans réinitialisation de la source vidéo.
- Aucun nouveau test ni infrastructure de test sans demande explicite. Mise à jour des tests existants affectés, checks existants et qualification manuelle.

### Questions et points de qualification

Aucune question produit n’empêche de commencer les lots de fondation. Les points suivants doivent être résolus avant les validations qui en dépendent ; ils ne remettent pas en cause les recommandations acceptées.

| Point | Proposition / traitement | Échéance |
| --- | --- | --- |
| Comptes et accès réels | Utiliser une ressource Azure F0 et une instance LibreTranslate administrée. L’administrateur devra fournir endpoint, région éventuelle et credentials via le stockage chiffré existant ; aucune valeur secrète dans ce document. Sans accès, la définition YAML peut être implémentée mais sa qualification réelle reste en attente. | Lot 4, puis qualification finale |
| Langues et corpus de référence | S’appuyer sur les langues effectives des fournisseurs ; sélectionner quelques pistes autorisées et des couples de langues représentatifs de l’usage pour la qualification. Les 80 000 caractères du scénario film restent une hypothèse. | Lots 3–4 et 9 |
| Bornes locales et cache navigateur | Mesurer puis fixer des valeurs configurables : taille décompressée, cues, caractères, concurrence, budgets, délais, taille des ETags et plafond du cache client. Distinguer valeurs locales et limites fournisseur. | Contrats livrés au lot 1, valeurs qualifiées au lot 9 |
| Nombre de processus | Mutualisation en cours possible dans un processus, sans cache de résultats ; documenter les doublons possibles entre processus et la portée des budgets fournisseur. | Lot 5 |
| Durée acceptable d’un GET | Qualifier REST, reverse proxy et Tauri sur les volumes représentatifs. Si les délais imposent des tâches asynchrones, présenter ce changement de contrat avant de le réaliser. | Lots 6 et 9 |
| Cache du protocole Tauri | Mesurer son comportement ; employer si nécessaire un cache applicatif borné `ETag + octets`. Aucun cache de résultats backend ; IndexedDB peut assurer la conservation du corps et de son ETag entre sessions lorsque le cache natif est insuffisant. | Lot 7 |
| Parseur WebVTT | Résolu au lot 3 : parser et sérialiseur internes au module `subtitles` d’`arachnea-stream`, sans dépendance externe ni contrainte de licence supplémentaire. La qualification structurelle reste couverte par les scénarios manuels du lot 3 et le lot 9. | Fait (lot 3) |

SRT, Google, DeepL, upload de documents, modes URL avec tâches/récupération différée, regroupement linguistique entre cues restent des extensions distinctes. Un cache de lots backend est exclu de cette révision. Le contrat URL optionnel et son repli vers le texte font partie du socle ; aucun fournisseur initial n’est annoncé compatible URL sans qualification. Leur réalisation n’est pas nécessaire pour terminer la première livraison.

## 12. Plan d’implémentation détaillé par lot

Les lots sont des unités de réalisation et de revue, pas des estimations calendaires. Chaque lot met à jour les README et spécifications concernés, le suivi ici et `docs/TODO.md`, ainsi que `CHANGELOG.md` lorsque le comportement, un contrat ou la structure évolue. Ne pas reporter toute la documentation au dernier lot. Les chemins nouveaux restent indicatifs jusqu’à leur création.

### Lot 1 — Contrats et séparation des couches

**Statut : DONE (2026-11-10), contrats adaptés le 2026-11-11 (section 13).**

**Livraison initiale (2026-11-10) :** crate `arachnea-translation` inscrite au workspace et module `subtitles` ajouté à `arachnea-stream` ; contrats de langues, blocs, capacités, limites, métadonnées, sélection, planification, identité de cache, descripteur persistant, ports de récupération/cache et statuts HTTP ; pont Scrapyfy générique et action `build_json` ; README de la crate Translation, spécifications Translation FR/EN et documentation Scrapyfy mise à jour. Aucun endpoint public de traduction.

**Dépendance :** recommandations validées.

**Travaux :**

1. Créer `arachnea-translation` et l’inscrire au workspace ; ajouter le module `subtitles` à `arachnea-stream`, avec les frontières de responsabilité définies en section 4.
2. Définir les types de langues, identifiants de blocs, capacités URL/texte, unités de taille et limites de la section 5. Définir les erreurs contextualisées et les correspondances d’entrée/sortie.
3. Définir le pont générique Scrapyfy, les blocs texte et la mesure de requête côté Translation ; réserver récupération conditionnelle de piste, revalidation et types WebVTT au module `subtitles` d’`arachnea-stream`. Définir les ports injectés par les services Stream.
4. Définir entrées/sorties normalisées des queries et capacités/limites typées issues du catalogue Scrapyfy. Spécifier `get_translation_metadata`, langues à deux lettres, couples directionnels optionnels, détection source intégrée/séparée, plafonds et fraîcheur. Définir les deux voies texte/URL optionnelle et le repli URL vers récupération + blocs via un adaptateur de document. Définir la sélection automatique/préférée/contrainte et les erreurs d’exclusion ; aucune configuration fournisseur autonome. Inclure les révisions YAML/actions dans l’identité de résultat.
5. Spécifier les commandes JSON, le descripteur autorisé, les règles d’accès, l’ETag composite et la table des erreurs HTTP ; remplacer les ports de cache documentaire et fixer la politique `HEAD` sans stockage de résultat.
6. Centraliser les constantes dans le `src/lib.rs` de la crate propriétaire ; documenter les frontières et dépendances, ajouter rustdoc anglaise, README de la crate Translation et documentation du module `subtitles`. Créer les spécifications Translation FR/EN selon les conventions du dépôt.

**Livrable :** une crate compilable avec son module documentaire, frontières documentées et contrat Scrapyfy normalisé, sans traduction accessible au public.

**Validation / sortie :** `cargo check -p arachnea-translation -p arachnea-scrapyfy -p arachnea-stream` depuis `server/` ; revue des unités, versions et dépendances ; configuration invalide rejetée explicitement. Les statuts et le périmètre d’accès ne restent pas implicites pour les lots suivants.

### Lot 2 — Catalogue, credentials et administration

**Statut : DONE (2026-11-10).**

**Livré :** manifeste `server/services/arachnea-translation/services.json` avec Azure Translator et LibreTranslate désactivés par défaut ; métadonnées statiques `get_translation_metadata` et queries `translate_texts` exécutées par Scrapyfy ; actions génériques `build_json_array` et `json_array_projection` ; recherche groupée de credentials chiffrés ; catalogue effectif `TranslationCatalog` chargé par Stream avec validation des langues/couples, ordre administrateur, préférence fournisseur, révision de configuration et diagnostics sans secret, et pont Scrapyfy générique adossé à ce catalogue ; la révision couvre le hash YAML et les paramètres éditables hors ordre/préférence, tandis qu’un changement de credentials ne l’invalide pas ; groupe `arachnea-translation` ajouté au rechargement admin ; entrées de configuration et libellés admin. Le pont texte accepte `preserves_request_order` pour les fournisseurs qui ne renvoient pas d’identifiants de blocs. Aucun endpoint public de traduction n’est exposé.

**Dépendance :** lot 1.

**Travaux :**

1. Remplacer les éventuelles définitions JSON autonomes par un manifeste Scrapyfy `server/services/arachnea-translation/services.json` et les YAML Azure/LibreTranslate avec queries effectives ; conserver les fournisseurs désactivés par défaut et les références de credentials non secrètes.
2. Réutiliser le catalogue, les paramètres et les credentials chiffrés Scrapyfy, puis brancher le pont générique dans Stream ; une configuration absente laisse la fonctionnalité indisponible sans empêcher les autres services de fonctionner.
3. Ajouter le groupe à l’adaptateur admin et au rechargement existant. Valider une nouvelle configuration avant de la publier ; préserver un état cohérent pour les opérations en cours.
4. Exécuter `get_translation_metadata` via Scrapyfy, valider listes de langues/couples/plafonds et gérer fraîcheur/rechargement ; construire le catalogue public effectif. Exposer les langues possibles sans promettre une compatibilité de taille avant mesure. Ajouter préférence fournisseur et ordre administrateur ; ne renvoyer aucun secret ni détail privé de configuration.
5. Distinguer changement de credential et changement influant sur la traduction pour calculer la révision intégrée à l’ETag. Documenter activation, désactivation et rechargement.

**Livrable :** configuration administrable et catalogue effectif, fournisseurs désactivés par défaut.

**Validation / sortie :** checks Rust des crates touchés et checks admin existants si modifié ; vérification manuelle activation/désactivation/rechargement, credential absent et absence de secret dans le catalogue ou les logs. Mettre à jour les tests existants affectés.

### Lot 3 — Documents WebVTT et planification des lots

**Statut : DONE (2026-11-10).**

**Livré :** parseur et sérialiseur WebVTT UTF-8 internes au module `subtitles` d’`arachnea-stream` (signature, métadonnées d’en-tête, identifiants de cues, timing brut et paramètres, payloads, `NOTE`/`STYLE`/`REGION`, validation des timestamps) ; extraction du seul texte visible avec balises et timestamps internes protégés comme frontières de segments ; décodage des entités puis rééchappement de la sortie ; découpage des cues longues aux frontières de phrases puis de mots puis de caractères Unicode, avec conservation des espaces de jointure ; table de correspondance cue/segment et identifiants de blocs stables ; mesure des octets décompressés, cues et unités fournisseur avec plafonds locaux `LocalSubtitleLimits` ; planification déléguée à `plan_batches` et reconstruction refusant les segments manquants, en trop, réordonnés ou vides ; pipeline `SubtitleDocumentPlan` indépendant de Scrapyfy et du player. Aucun nouvel endpoint ni accès réseau.

**Validation effectuée (2026-11-10) :** `cargo check --workspace`, `cargo test -p arachnea-stream --no-run`, `cargo fmt` et `cargo clippy -p arachnea-stream --all-targets` sans avertissement Stream ; vérification aller-retour sur BOM/CRLF, `NOTE`/`STYLE`/`REGION`, entités, balises, cues longues et refus d’un timestamp inversé ou d’un balisage non fermé. Aucun test permanent ajouté.

**Dépendance :** lot 1 ; réalisable indépendamment des comptes fournisseurs.

**Travaux :**

1. Dans le module `subtitles` d’`arachnea-stream`, qualifier le choix du parseur puis implémenter lecture, validation et sérialisation WebVTT UTF-8 avec conservation des identifiants, timestamps, paramètres, blocs et métadonnées décrits en section 6.
2. Extraire uniquement le texte traduisible, protéger le balisage et les timestamps internes, conserver une table de correspondance vérifiable entre cues et segments.
3. Mesurer séparément fichier décompressé, cues, texte et unités fournisseur ; appliquer les plafonds locaux avant traduction.
4. Confier à Translation les lots de blocs abstraits et la mesure de leur requête Scrapyfy ; garder dans le module `subtitles` le découpage des cues et la correspondance des segments. Respecter simultanément blocs, unités et octets réellement sérialisés, sans couper Unicode ni balisage.
5. Recomposer les fragments, contrôler cardinalité et structure et rééchapper le texte traduit. Un échec ou une réponse incomplète ne produit jamais un fichier final partiel.

**Livrable :** pipeline de document indépendant des adaptateurs et du player ; SRT toujours hors du chemin initial.

**Validation / sortie :** qualification manuelle aller-retour sans traduction, accents et caractères multioctets, balises, blocs `STYLE`/`REGION`/`NOTE`, cues longues et fichiers invalides. Vérifier les trois volumes de la section 9 et l’absence d’altération temporelle. Exécuter les checks du crate et les tests existants affectés, sans en créer.

### Lot 4 — Services YAML Azure/LibreTranslate et exécution générique

**Statut : DONE (implémentation et validation locale, 2026-11-10) ; qualification réelle Azure/LibreTranslate en attente d’accès.**

**Livré :** queries YAML Azure F0 et LibreTranslate exécutées par le pont Scrapyfy commun, avec `translate_texts_auto` pour Azure, queries `detect_language`, mappings de lignes via `result_item_field` et rejet des statuts HTTP fournisseur (`error_on_statuses`). Le pont gère `source_language: null` par détection dédiée ou auto-query, stabilise la source sur les documents multi-lots, réutilise l’original lorsque source et cible sont identiques, applique des reprises bornées avec jitter et `Retry-After`, réserve concurrence et quota par compte avant chaque tentative et expose des métriques sans texte ni secret. Stream branche un budget en mémoire partagé par service ; le pont générique classifie les statuts HTTP en catégories fournisseur normalisées ; les fournisseurs restent désactivés par défaut et aucun endpoint public n’est exposé.

**Compléments de finalisation :** la planification d'exécution et la sélection Stream mesurent les corps POST avec les résolveurs réellement utilisés par Scrapyfy, après résolution des paramètres et credentials, sans réseau ni exposition du corps. Le préfiltrage tient compte du quota du document et de la concurrence disponibles ; chaque tentative conserve sa réservation atomique et aucun fournisseur de repli n'est choisi après un échec. Les limites locales sélectionnées sont réutilisées à l'exécution. Les budgets partagés sont reconfigurés sans perdre les unités consommées ni les réservations actives. Le pont commun classifie les statuts HTTP, même hors Stream, borne chaque tentative à 60 s, ne reprend pas avant un `Retry-After` supérieur au plafond de reprise, rejette les sorties vides et détections contradictoires et journalise seulement les compteurs/statuts. La détection LibreTranslate envoie bien une chaîne `q`, conformément à son contrat officiel. Les catalogues de langues restent des sous-ensembles provisoires à qualifier.

**Validation effectuée (2026-11-10) :** compilation ciblée des trois crates, compilation de leurs cibles de test existantes (`cargo test -p arachnea-translation -p arachnea-scrapyfy -p arachnea-stream --no-run`) et 75 tests Scrapyfy existants réussis (`cargo test -p arachnea-scrapyfy --lib`). Programme temporaire avec serveur HTTP local simulé couvrant les deux YAML : détection unique, trois lots et restitution de 101 blocs, égalité entre tailles planifiées et corps réellement envoyés pour une source connue, majoration prudente en mode auto, réutilisation de l'original, cardinalité invalide, détections contradictoires, JSON invalide sans reprise, quota `429`, authentification `401`, reprise sur `503`, sélection avec exclusion d'un favori sans budget et sélection contrainte, libération de concurrence et reconfiguration sans remise à zéro du quota. `cargo fmt` et `cargo clippy` ciblé sur toutes les cibles (avertissements préexistants hors périmètre). Le programme temporaire a été retiré ; aucun test permanent ajouté. Aucune qualification sur endpoint réel faute de credentials.

**Dépendances :** lots 1–3 pour l’intégration complète.

**Travaux :**

1. Revérifier les documentations officielles des offres, endpoints, langues, unités et quotas au moment du codage ; corriger la section 9 si nécessaire.
2. Implémenter les queries texte Azure F0 et LibreTranslate dans les YAML : endpoints, méthodes, en-têtes, corps JSON, paramètres de langues et normalisation des réponses par actions. Exécuter exclusivement via le pont Scrapyfy commun ; capacités URL désactivées.
3. Normaliser les langues, prendre en charge `source_language: null` et le mode automatique déclaré par le YAML ; stabiliser la source pour les documents multi-lots selon la section 5, sans détection par réplique. Respecter les couples optionnels et réutiliser l’original si source et cible sont identiques.
4. Valider génériquement la correspondance des réponses normalisées avec les blocs envoyés. Les mappings fournisseur restent en YAML ; ajouter seulement les actions/post-processeurs génériques manquants et documenter le schéma Scrapyfy FR/EN. Mesurer la taille depuis la même construction de requête que celle réellement exécutée, sans envoyer de requête de mesure.
5. Implémenter le filtrage générique des candidats, la simulation des lots, le classement déterministe et la réservation avant envoi ; conserver le même fournisseur pour tous les lots. Appliquer concurrence et budgets par compte, délais et reprises bornées avec `Retry-After`. Distinguer authentification, quota, langue, taille et incident temporaire ; aucun repli vers un autre fournisseur après un appel de traduction en échec.
6. Exposer des mesures de volume, lots, durée et erreurs sans texte ni secrets. Documenter le partage du quota avec les usages externes au serveur.

**Livrable :** traduction texte indépendante des sous-titres, deux services YAML exécutés par un pont unique ; aucun adaptateur fournisseur en Rust.

**Validation / sortie :** checks du crate ; qualification sur endpoints réels lorsque disponibles, avec un volume borné, puis scénarios manuels de réponse incomplète, quota, authentification et indisponibilité. Signaler séparément tout fournisseur non qualifié faute d’accès ; ne pas le déclarer validé.

### Lot 5 — Récupération autorisée et revalidation sans cache serveur

**Statut : DONE (2026-11-11), implémentation et qualification locale avec fournisseur/proxy simulés ; qualification réelle des fournisseurs toujours dépendante des accès.**

**Dépendances :** lots 1–4 corrigés.

**Travaux :**

1. Fournir une résolution de piste autorisée avec proxys internes, en-têtes et contexte d’accès ; contrôler destinations et redirections. Implémenter le port conditionnel livré, qui reçoit ETag/timestamp/hash source et retourne `NotModified` ou un document complet.
2. Implémenter le flux ETag client → vérification du contexte → revalidation source → `304` sans traduction, ou récupération complète → sélection URL/texte → traduction → reconstruction. Ne pas lancer de détection, mesure fournisseur ni réservation de quota avant la décision de revalidation.
3. Les contrats de cache documentaire ont été retirés ; ne créer ni index source ni store de fichiers traduits. Conserver uniquement les descripteurs nécessaires à la résolution des jetons et les états de catalogue/budget.
4. Utiliser le codec ETag versionné livré avec validateur source, hash YAML, langue cible et contexte complet. Réutiliser les primitives génériques de Scrapyfy si adaptées, en traitant les octets source sans normalisation destructive. Prévoir le second GET non conditionnel si le contexte a changé après un `304` amont.
5. Mutualiser si utile les demandes identiques en cours, sans rétention après exécution ; documenter les limites entre processus. Préserver isolation d’accès et budgets.
6. Garantir qu’un échec, arrêt ou rechargement n’expose aucun résultat partiel ; mettre à jour README et spécifications Translation/Stream FR/EN en même temps que les contrats concernés.

**Livrable :** traduction complète ou revalidation sans traduction, sans persistance backend de source/résultat et exploitable après redémarrage avec le couple navigateur `ETag + octets`.

**Validation / sortie :** qualification manuelle premier `200`, ETag amont et timestamp donnant `304`, hash seul identique puis modifié, amont ignorant les conditions, YAML/cible/options modifiés, second GET après `304` amont, tag malformé, source indisponible, perte du corps navigateur, redémarrage et concurrence. Vérifier absence d’appel traduction/détection/budget sur `304`, absence de fichiers conservés, isolation d’accès et destinations ; checks existants des crates concernés.

### Lot 6 — Commandes et transport fichier REST/Tauri

**Statut : TODO.**

**Dépendance :** lot 5.

**Travaux :**

1. Enregistrer `get_translation_services` et `prepare_subtitle_translation` avec les contrôleurs existants ; inclure politique automatique/préférée/contrainte et langues effectives. Le descripteur automatique conserve la politique, pas un fournisseur choisi prématurément ; la préparation valide et conserve le descripteur sans récupérer le document ni traduire.
2. Produire une piste compatible avec `ResolvedPlayerSubtitle` et un jeton opaque stable pendant sa durée de vie. Persister sa résolution et revérifier les droits au téléchargement.
3. Enregistrer `subtitle_translation/<token>.vtt` par les chemins publics du contrôleur ; retourner uniquement le fichier complet en `text/vtt` lors d’un succès.
4. Implémenter `If-None-Match` (listes, validateurs faibles et `*`), `304` vide, `Cache-Control: private, no-cache` et exposition CORS d’ETag selon le déploiement.
5. Retourner `405` à `HEAD` avec l’en-tête `Allow` adapté, sans traduction ni récupération ; mapper explicitement les statuts d’erreur, l’expiration et les erreurs de quota.
6. Qualifier le comportement REST et le protocole Tauri, y compris préfixe personnalisé, redémarrage, headers et durée des requêtes. Documenter les éventuelles limites du mode synchrone.

**Livrable :** API JSON et fichier WebVTT exploitables depuis les deux modes applicatifs.

**Validation / sortie :** parcours manuel préparation → GET `200` → GET conditionnel `304`, `HEAD` avec `405` sans cache serveur, jeton expiré ou d’un autre périmètre, erreurs fournisseur et absence d’appel lors de la préparation. Vérifier que les erreurs ne sont jamais servies comme VTT ; checks REST/Tauri existants adaptés aux plateformes disponibles.

### Lot 7 — Client de traduction et cycle de vie des pistes

**Statut : TODO.**

**Dépendance :** lot 6.

**Travaux :**

1. Ajouter les types partagés et appels JSON du catalogue/de la préparation dans les services frontend existants ; télécharger le fichier par un client dédié, hors `call_api()`.
2. Utiliser le cache HTTP natif en REST. Qualifier Tauri et, si nécessaire, conserver un cache borné associant ETag et octets ; traiter un vrai `304` uniquement avec son corps précédemment conservé et refaire un GET sans validateur si ce corps manque. Borner le cache client et définir son isolation par serveur/utilisateur, son invalidation et son éventuelle persistance IndexedDB.
3. Ajouter un composable dédié en Composition API/TypeScript pour préparation, téléchargement, états et annulation. Le sélecteur de préférences produit une demande de langue ; ce composable l’exécute ; le contrôleur des pistes conserve leur propriété. Garder les types de préférences distincts des URL temporaires.
4. Ajouter le Blob WebVTT via le propriétaire des pistes distantes et ne sélectionner la piste qu’après chargement réussi. Conserver la piste originale et la position de lecture.
5. Invalider les opérations obsolètes lors des changements de source/langue et du démontage ; ignorer les réponses tardives, retirer les pistes et révoquer les URL Blob selon leur cycle de vie.

**Livrable :** chargement technique d’une traduction et gestion cohérente des pistes sans réappliquer la source vidéo.

**Validation / sortie :** type-check et build existants ; qualification manuelle REST/Tauri, fichier en cache, téléchargement lent, changement rapide de source et démontage, absence de reprise de lecture ou d’ajout tardif. Actualiser les tests existants affectés seulement.

### Lot 8 — Menu, préférences et expérience utilisateur

**Statut : TODO.**

**Dépendance :** lot 7.

**Travaux :**

1. Étendre `ParameterSubtitleLanguages.vue` avec le réglage par langue « Générer par traduction si absent ». Conserver `videoPlayer.subtitleLanguagePriority`, ajouter la table de booléens de la section 8 et adapter storage, types, sanitizers et compatibilité des anciennes valeurs ; valeurs absentes à `false`. Relier le menu manuel à ces favoris sans seconde liste.
2. Rendre l’action indisponible sans piste externe exploitable, notamment en iframe ou avec les seules pistes natives/segmentées. Préserver le comportement des pistes HLS/DASH existantes.
3. Afficher chargement, erreur contextualisée et identification de la traduction automatique ; permettre de poursuivre la lecture et de conserver/retrouver la piste originale.
4. Intégrer `trackPreferences.ts` et la réapplication des paramètres : parcourir les favoris dans l’ordre, utiliser une piste correspondante avant de générer, demander uniquement la première cible nécessaire autorisée et reprendre le repli en cas d’échec. Respecter choix manuels et profils `disabled` ; une restauration ne génère que selon l’autorisation explicite par langue. Persister une identité stable sans Blob ni jeton temporaire.
5. Ajouter les traductions i18n et vérifier clavier, focus et affichage responsive sans refonte du player.

**Livrable :** parcours utilisateur complet pour demander et sélectionner une traduction.

**Validation / sortie :** `npm run type-check`, `npm run build-only` et `npm run lint:css` depuis `front/public-app/` ; qualification manuelle migration des anciennes préférences, activation par langue, réordonnancement/suppression/alias, profil `auto` et cible explicite, priorité humain/automatique, piste native tardive, changement de choix pendant traduction, absence de génération pour `disabled`, langue identique, erreurs/quota et absence de fournisseur. Vérifier qu’aucune génération en série de tous les favoris ne se produit.

### Lot 9 — Qualification de bout en bout et réglages opérationnels

**Statut : TODO.**

**Dépendances :** lots 1–8.

**Travaux :**

1. Qualifier les trois scénarios de volume et des documents représentatifs autorisés ; comparer structure et timestamps avant/après et apprécier la lisibilité sur les couples de langues retenus.
2. Mesurer délais, mémoire, tailles, lots et consommation réelle ; fixer et documenter les bornes configurables avec une marge adaptée aux offres gratuites.
3. Rejouer le parcours de cache complet : revalidation, changement de source/YAML/cible/options/modèle, redémarrage serveur, perte du corps ou éviction côté navigateur et concurrence dans la topologie prise en charge. Vérifier aucun store documentaire serveur et aucun appel traduction sur `304`.
4. Qualifier langues absentes, couples omis ou explicitement vides, directions non symétriques, source automatique avec/sans langue restituée et documents multi-lots, voie URL optionnelle et repli par téléchargement, limites de blocs/octets, découpage sur plusieurs requêtes, budget indisponible, métadonnées périmées et choix déterministe entre services. Vérifier un seul fournisseur par document et l’ETag portant son identité réelle. Qualifier URLs proxifiées/signées, droits, redirections, source indisponible, fournisseur en panne, quota et annulation frontend. Vérifier qu’aucun texte, secret ou URL signée complète n’apparaît dans les logs.
5. Vérifier les délais des environnements cibles et le cache Tauri. Si un contrat asynchrone devient nécessaire, documenter l’analyse et obtenir la validation de ce changement avant son implémentation.
6. Consigner environnement, fournisseur, volumes, résultats, limites et validations non exécutées. Mettre à jour uniquement les tests existants concernés et exécuter les checks Rust/frontend pertinents.

**Livrable :** compte rendu de qualification et valeurs opérationnelles justifiées.

**Validation / sortie :** aucun résultat partiel ni mélange de périmètres ; cache valide sans retraduction ; player stable pendant les chargements. Les plateformes ou comptes non disponibles restent explicitement à qualifier, sans statut de validation implicite.

### Lot 10 — Distribution et clôture de la première livraison

**Statut : TODO.**

**Dépendance :** lot 9 ; préparation des ressources possible dès le lot 2.

**Travaux :**

1. Inclure les configurations non secrètes du nouveau groupe dans les paquets backend/desktop et vérifier leur présence dans le déploiement Docker ; conserver les fournisseurs inactifs sans credentials.
2. Vérifier l’absence de répertoire/store serveur de traductions, la résolution des descripteurs après redémarrage et la conservation/invalidation du cache navigateur ou Tauri entre sessions et mises à jour.
3. Relire le README de la crate Translation, les contrats Translation/Stream FR/EN, aide admin et documents de distribution ; documenter activation, credentials, plafonds, cache, erreurs et limitations de couverture. Modifier les spécifications Scrapyfy uniquement si son contrat a réellement changé.
4. Ajouter les entrées de changelog sous les modules concernés, au format release → module → Added/Changed/Fixed. Clore dans `docs/TODO.md` les éléments réellement livrés et conserver les qualifications ou extensions ouvertes.
5. Réaliser un parcours sur un paquet produit : démarrage sans fournisseur, configuration admin, traduction, relecture après redémarrage et désactivation du fournisseur.

**Livrable :** première version distribuable, documentation synchronisée et suivi des limites restantes.

**Validation / sortie :** contrôles de packaging existants et parcours sur les plateformes disponibles ; aucun credential inclus dans les artefacts. La clôture distingue clairement implémentation, qualification réelle et extensions futures.

### Ordre et jalons

| Jalon | Lots terminés | Résultat vérifiable |
| --- | --- | --- |
| Socle | 1–2 | Contrats, configuration et administration, sans fonctionnalité de traduction exposée |
| Moteur | 3–5 | Traduction complète avec revalidation sans cache serveur, fournisseurs qualifiés selon les accès disponibles |
| Backend accessible | 6 | Préparation JSON et fichier conditionnel REST/Tauri |
| Parcours utilisateur | 7–8 | Demande et sélection de traduction sans redémarrage de la vidéo |
| Livraison qualifiée | 9–10 | Bornes mesurées, distribution et documentation, limites résiduelles consignées |

Les lots 1 à 4 et leurs adaptations de la section 13 sont livrés ; le lot 5 est livré ; les lots 6 à 10 restent à TODO. La validation locale du lot 4 ne vaut pas qualification des endpoints réels. Le reste suit les dépendances indiquées. Une extension hors première livraison ou un changement de contrat découvert en cours de travail fait l’objet d’une analyse spécifique avant réalisation ; les recommandations déjà validées ne nécessitent pas une nouvelle confirmation à chaque lot.


## 13. Impact de la révision du 2026-11-11 sur les lots livrés

### Périmètre de la vérification

Lecture des commits `7c9208d3` (lots 1–2), `49b96042` (lot 3) et `8bd1dff4` (lot 4), confrontée aux contrats présents dans le dépôt et au chemin `get_entry` / `conditional.rs`. Cet audit décrit les contrats constatés avant leur correction du 2026-11-11 ; les résultats de cette correction sont consignés ci-dessous. L’orchestration HTTP et le frontend restent à réaliser.

Au moment de l’audit, il existait des **types et ports de cache**, mais aucun endpoint public de traduction ni implémentation du store documentaire du lot 5. Il n’y a donc pas de cache de fichiers traduits déployé à migrer dans ces trois commits. L’impact est réel sur les contrats et la documentation, mais ne nécessite pas de réécrire le moteur de traduction.

| Lot / commit | Éléments constatés | Impact et action à prévoir |
| --- | --- | --- |
| 1 — `7c9208d3` | `arachnea-translation/src/cache.rs` définit `SourceFingerprint`, `TranslationCacheIdentity`, `TranslationCacheKey` et `AccessScope`. Les contrats documentaires ont été créés dans `arachnea-subtitles`, puis déplacés au lot 3. | Conserver l’isolation `AccessScope` et les composantes utiles d’identité ; remplacer leur usage de clé persistante par une identité de revalidation. Un hash de contenu seul ne représente pas les deux modes ETag/hash attendus. Définir un validateur source typé et un ETag composite décodable ; retirer les contrats de stockage devenus inutiles et actualiser exports, constantes et documentation. |
| 2 — `7c9208d3` | `arachnea-stream/src/translation_catalog.rs` calcule `configuration_revision` avec le hash YAML et les paramètres éditables, hors ordre et préférence ; les credentials sont séparés. | Base compatible et réutilisable. Rendre le hash YAML accessible au composite, directement ou via une révision explicitement documentée comme le couvrant ; garder les autres paramètres influant sur le résultat. Aucun changement nécessaire au groupe admin, aux credentials ni aux actions JSON. La revalidation doit vérifier que le service précédent reste activé, autorisé et conforme à la politique. |
| 3 — `49b96042` | `arachnea-stream/src/subtitles/cache.rs` expose `SubtitleTranslationCacheIdentity`, `SubtitleTranslationCacheKey`, `CachedSubtitleTranslation` avec octets et expiration. `ports.rs` expose `SubtitleTranslationCachePort::{lookup, store}`. | Ces contrats imposent le stockage backend refusé par la nouvelle décision : les retirer ou les remplacer par les seuls contrats de revalidation, sans store documentaire. Le parseur, la segmentation, la planification et la reconstruction restent réutilisables. |
| 1 → 3 — contrats déplacés | `SubtitleRetrievalPort::retrieve` ne reçoit aucun validateur et renvoie toujours `RetrievedSubtitle` avec un corps ; ce dernier possède `remote_etag`, mais aucun Last-Modified ni état explicite `NotModified`. | Étendre la récupération avec paramètres conditionnels, timestamp et résultat typé `NotModified` / document complet. Ne pas représenter un `304` amont par un document vide. Calculer le hash de repli sur les octets source. |
| 1 → 3 — réponses et erreurs | `TranslatedSubtitleDocument` et `CachedSubtitleTranslation` documentent un ETag fort du fichier final ; `SubtitleTranslationError` expose `CacheUnavailable` et `HeadUnavailable` (`404` tant qu’aucun GET n’a produit de résultat). | Remplacer le contrat d’ETag des octets finaux par celui de l’ETag composite faible. Adapter la validation et la rustdoc du constructeur pour `W/"…"`, retirer l’erreur du store supprimé et préciser `HEAD` sans dépendre d’un fichier serveur. Garder `private, no-cache`, déjà adapté. |
| 4 — `8bd1dff4` | Pont Scrapyfy, détection, budgets/concurrence en mémoire, mesure exacte des requêtes, sélection, reprises et YAML Azure/LibreTranslate. `translation_port.rs` transmet déjà la révision du catalogue au pont. | Pas de modification fournisseur imposée : ces éléments restent utilisables. L’orchestrateur du lot 5 doit décider du `304` **avant** de les appeler : aucune détection, traduction ou réservation sur une revalidation réussie. Le filtrage de budget existant sert à sélectionner une nouvelle traduction, pas à refuser la réutilisation d’un fichier navigateur valide. |

### Approche et ordre de correction

L’approche retenue remplace les contrats persistants avant de construire le lot 5. Garder un port de cache backend inutilisé limiterait les changements immédiats mais laisserait une API et une documentation contraires à la décision ; ne pas introduire un faux store vide pour la préserver. Le coût attendu est une adaptation locale des types, exports, erreurs et documentation, puis le travail de récupération conditionnelle déjà attendu dans les lots 5–7.

1. Corriger les contrats des lots 1/3 : identité de revalidation, validateurs source, port conditionnel, résultats sans corps, ETag et politique `HEAD`. Conserver les types de droits, descripteurs, parseur et moteur texte utiles.
2. Relier le hash YAML / la révision existante du lot 2 au composite ; adapter uniquement l’exposition nécessaire. Ne pas supprimer les caches de métadonnées ou les budgets du lot 4 : ils ne stockent pas les fichiers traduits.
3. Mettre à jour `arachnea-stream/README.md`, `arachnea-translation/README.md`, rustdoc et spécifications Translation FR/EN lors de cette correction ; modifier les spécifications Stream/Scrapyfy si leurs interfaces changent. Revoir les assertions et usages existants affectés, sans créer de nouveaux tests.
4. Réaliser les lots 5–7 révisés : revalidation sans état de résultats au serveur, transport `200`/`304`, puis conservation du corps et de l’ETag dans le navigateur. Qualifier REST et Tauri séparément.

**Contrôles des contrats corrigés :** `cargo check -p arachnea-translation -p arachnea-scrapyfy -p arachnea-stream` et compilation des cibles de test existantes (`cargo test -p arachnea-translation -p arachnea-scrapyfy -p arachnea-stream --no-run`) réussis. Les quatre tests Stream de schéma et 78 tests Scrapyfy existants passent ; deux tests Scrapyfy utilisant un socket local échouent avec `Operation not permitted` dans le sandbox. Une vérification manuelle temporaire a validé l’aller-retour des ETags amont faibles et du hash avec date, le rejet de validateurs malformés et le statut `HEAD` 405. La matrice de revalidation HTTP des sections 7 et 12 reste à exécuter avec les lots 5–7 ; aucun appel fournisseur réel effectué.

**Correction livrée le 2026-11-11 :** `TranslationRevision` et `SubtitleTranslationValidator` remplacent les anciennes clés et le store documentaire. Le validateur composite est encodable/décodable et borné ; le port de récupération accepte ETag ou hash/timestamp et distingue `NotModified` d’un corps complet. Le catalogue expose le hash YAML optionnel. Le contrat du document traduit accepte le validateur composite faible et `HEAD` est associé à `405`. Le moteur, les budgets et le parseur restent inchangés. Les README et spécifications FR/EN sont synchronisés.

**Travaux restants :** les lots 6–7 doivent exposer la réponse HTTP `304` et stocker le fichier dans le navigateur. Le lot 5 est implémenté et qualifié localement ; les fournisseurs réels restent à qualifier avec leurs accès. Aucun endpoint public de traduction n’est encore enregistré.


### Livraison du lot 5 — 2026-11-11

`revalidate_subtitle` contrôle le descripteur, les droits, l’expiration, le fournisseur et les révisions avant toute sélection, mesure fournisseur, détection ou réservation de quota. Un ETag amont, un timestamp ou un hash source inchangé permet de retourner `NotModified`. Si la langue, le YAML ou le contexte ne correspondent plus, le premier téléchargement est volontairement non conditionnel : il fournit directement le corps nécessaire à la nouvelle traduction, sans premier GET donnant `304` à refaire. Un `304` inattendu sans condition valide est refusé.

`translate_subtitle` segmente et mesure les candidats texte, classe selon préférence, ordre, nombre de lots puis identifiant, et conserve un seul pont pour tout le document. La voie URL est limitée aux URL publiques directes explicitement autorisées, sans query/credentials/en-têtes requis/redirection, et aux services sans budget d’unités ni limite d’unités dépourvue d’unité déclarée. Le résultat conserve structure, métadonnées, temps et balisage ; la stabilité de la source est revérifiée. Un échec ne publie aucun document partiel et ne provoque aucun repli fournisseur.

`StreamScraper::translate_subtitle` fournit le cœur proxy et conserve un instantané applicatif pour l’opération ; son empreinte de contexte inclut les en-têtes, la politique de voie, les options de l’appelant et les limites. Les liens internes au proxy sont décodés sans requête au contrôleur local, en réutilisant le décodeur des options de la route proxy. Pays, repli direct, cookies, actions de réponse/redirection, classificateurs et TLS autorisé restent appliqués. Le client proxy générique conserve le nom logique pour le routage et TLS tout en fixant l’IP validée pour la connexion/tunnel ; les redirections sont vérifiées séparément, les en-têtes et cookies retirés entre origines et les corps bornés. Une source redirigée ou transformée utilise un hash. Les réponses proxy doivent respecter `Accept-Encoding: identity` ; un encodage inattendu est refusé explicitement.

Aucun fichier ni index de résultats n’est conservé. La mutualisation en cours est facultative et n’est pas activée : des demandes simultanées sans corps client peuvent traduire séparément, dans les budgets de concurrence existants. Le validateur permet toujours une revalidation après redémarrage sans état documentaire serveur.

**Qualification manuelle locale :** programme temporaire avec service LibreTranslate YAML exécuté par Scrapyfy, query URL générique et proxy HTTP simulés. Scénarios réussis : première traduction complète ; ETag, date et hash inchangés sans nouvel appel fournisseur ; source modifiée ; langue/YAML/options modifiés ; tag malformé ; accès/expiration/source indisponible ; nouvel assemblage après redémarrage ; demandes concurrentes ; budget épuisé laissant passer un `304` valide ; échec fournisseur sans résultat ; voie URL valide, structure temporelle modifiée refusée, URL signée renvoyée à la voie texte ; proxy interne avec cookies/en-têtes/paramètres, CONNECT vers l’IP fixée et Host logique, `304`, redirections, retrait des secrets entre origines, transformation VTT, refus d’une destination privée, dépassement de taille et refus du TLS non autorisé. Programme temporaire retiré ; aucun nouveau test permanent.

**Limites de validation :** aucun appel Azure/LibreTranslate réel faute d’accès. Les transports REST/Tauri et le cache du navigateur ne sont pas encore présents ; ils appartiennent aux lots 6–7. Les tests existants et checks de compilation/formatage sont consignés ci-dessous après leur exécution finale.

**Checks finaux du lot 5 :** compilation ciblée Proxy/Stream/Translation/Scrapyfy et compilation des cibles de test existantes réussies. Tests existants : Proxy 90/90 avec fonctionnalités par défaut et 96/96 avec `controller-service`, Scrapyfy 80/80, Stream 14/14 (5 scénarios fournisseurs réels ignorés), Translation aucune suite unitaire existante. Les tests utilisant des sockets locaux ont été exécutés avec l’accès local nécessaire ; les deux échecs de sandbox consignés pour les contrats des lots 1–4 ne se reproduisent pas. Formatage et contrôle des différences validés.
