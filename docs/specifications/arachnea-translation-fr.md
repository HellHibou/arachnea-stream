# Spécification des contrats de traduction Arachnea

Ce document décrit le contrat générique de traduction porté par
`arachnea-translation`, le pipeline documentaire de sous-titres porté par le
module `subtitles` d’`arachnea-stream` et leur intégration avec Stream. Le
schéma YAML général reste décrit dans
[arachnea-scrapyfy](arachnea-scrapyfy-fr.md) ; ce document ne décrit que les
requêtes et sorties normalisées propres à la traduction.

Statut : lots 1 à 4 implémentés (contrats, crate et module de sous-titres, pont
générique, catalogue effectif et administration, parseur WebVTT et
planification documentaire, exécution texte Azure Translator/LibreTranslate avec
reprises, budgets et métriques). La qualification réelle des fournisseurs
requiert encore des credentials. Les lots 5 à 10 ajoutent l’orchestration
récupération et revalidation du cache navigateur, le transport et l’interface utilisateur.

## 1. Séparation des couches

| Couche | Responsabilités | Interdictions |
|---|---|---|
| `arachnea-translation` | Langues à deux lettres, blocs abstraits, capacités et limites, métadonnées, sélection, planification, identité de révision de traduction, pont Scrapyfy. | Aucune connaissance de WebVTT, des cues, du player ou des pistes vidéo. Aucun fournisseur Rust spécifique. |
| `arachnea-stream` | Droits, résolution des liens de proxy, catalogues, assemblage REST/Tauri et transport du fichier, plus son module `subtitles` : descripteurs de piste source, parsing/sérialisation WebVTT, découpage des cues et planification documentaire, descripteur persistant et jeton, récupération conditionnelle autorisée, validateur composite et statuts HTTP. | Aucun mapping fournisseur ni Rust spécifique à un fournisseur ; aucun accès réseau non autorisé. |
| Front public | Préférences, décision de demander une traduction, cycle de vie Video.js. | Aucun credential ni protocole fournisseur. |

Les fournisseurs sont exclusivement décrits et exécutés par Scrapyfy. Le pont
générique appelle `get_translation_metadata`, puis la query texte ou URL
déclarée. Aucun endpoint n'est appelé directement depuis Translation ou Stream.

## 2. Query `get_translation_metadata`

Chaque service activé expose une query nommée `get_translation_metadata`, de
type `static` ou alimentée par l'API du fournisseur, avec le média
`application/arachnea-translation-metadata`. Sa sortie contient un objet
normalisé, généralement sous l'entrée `translation` :

```yaml
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
          query: detect_language
        text_translation:
          query: translate_texts
          auto_query: translate_texts_auto
          accepts_multiple_blocks: true
          size_unit: unicode_code_points
          max_blocks_per_request: 50
          max_units_per_block: 5000
          max_units_per_request: 10000
          max_request_bytes: 65536
          context_support: false
          markup_support: false
        url_translation: null
```

### Champs

| Champ | Obligatoire | Sens |
|---|---|---|
| `schema_version` | oui | Version du contrat normalisé. Seule la version `1` est acceptée. |
| `languages` | oui | Liste commune de codes à deux lettres minuscules. Une liste vide n'autorise aucune traduction. |
| `source_languages` | non | Sources autorisées. Absent : `languages`. Doit être un sous-ensemble de `languages`. |
| `target_languages` | non | Cibles autorisées. Absent : `languages`. Doit être un sous-ensemble de `languages`. |
| `supported_pairs` | non | Restrictions directionnelles optionnelles `{source, target}`. Absent : tous les couples source × cible sauf même langue. Présent et vide : aucun couple. |
| `source_detection` | non | `automatic`, `returns_detected_language` et éventuellement `query`. |
| `text_translation` | non | Capacité texte, query et plafonds. |
| `url_translation` | non | Capacité URL optionnelle. |

`supported_pairs` ne rend jamais la direction inverse implicite. Chaque couple
doit appartenir aux listes source et cible et ne peut pas traduire vers la même
langue. Les doublons, codes invalides, listes hors `languages` et plafonds nuls
sont rejetés. Un plafond absent signifie « non déclaré par le fournisseur », pas
« illimité » : les bornes locales restent applicables.

### Capacités

`text_translation` contient `query`, `auto_query` optionnelle,
`accepts_multiple_blocks`, `size_unit`, `max_blocks_per_request`,
`max_units_per_block`, `max_units_per_request`, `max_request_bytes`,
`context_support`, `markup_support` et `preserves_request_order`. `auto_query`
est la query utilisée lorsque la source est inconnue et que le fournisseur ne
peut pas exprimer la traduction automatique via
`translation_source_language` ; une source connue utilise toujours `query`.
`accepts_multiple_blocks: false` limite chaque requête à un bloc.
`preserves_request_order: true` permet à un fournisseur de retourner les lignes
dans l'ordre de la requête sans identifiants de blocs ; le pont les associe
alors par position tout en vérifiant la cardinalité.

`url_translation` contient `query`, `accepted_formats`, `max_document_bytes`,
`max_document_units` et `result_mode`. Seul `result_mode: inline_document` est
défini par le socle. Une simple page HTML n'est pas une traduction de
sous-titres ; un endpoint d'upload n'est pas une capacité URL.

`size_unit` accepte `utf8_bytes`, `unicode_code_points` ou une unité inconnue
conservée telle quelle. Une unité inconnue est mesurée prudemment en octets
UTF-8.

### Détection de la source

`automatic: true` indique que la traduction accepte une source non fournie.
`returns_detected_language: true` indique que la réponse de traduction expose la
langue effective. `query` désigne une query de détection séparée facultative ;
`auto_query` dans la capacité texte permet aussi une traduction sans source
lorsqu'aucune query dédiée n'est déclarée.

Une query de détection dédiée reçoit
`application/arachnea-translation-detection` et le paramètre normalisé
`translation_blocks_json` contenant exactement un bloc représentatif, borné à
2 000 unités fournisseur. Elle retourne une ligne avec `language` ou
`detected_source_language` ; le pont valide le code par rapport aux métadonnées
déclarées.

Une entrée normalisée `source_language: null` demande la détection ; `auto`
n'est pas un code de langue et n'apparaît jamais dans `languages`. Le pont
transmet `translation_source_language = "auto"` à une query qui l'attend, ou
omet le paramètre via `auto_query`, qui adapte la requête normalisée à l'API
(omission, valeur `auto` ou autre).

Pour une requête multi-lots avec source inconnue, le pont utilise d'abord la
query de détection dédiée lorsqu'elle est déclarée, puis envoie chaque lot avec
la source détectée. Sans query dédiée, le premier lot doit utiliser
`auto_query` et retourner `detected_source_language` ; les lots suivants
réutilisent cette langue. Une détection absente fait échouer la requête au lieu
de relancer une détection par lot. Avec des couples restreints et une source
inconnue, une détection préalable est requise pour établir l'éligibilité. Si la
source détectée égale la cible, les blocs originaux sont retournés sans résultat
traduit.

## 3. Query texte `translate_texts`

Paramètres normalisés fournis par le pont :

| Paramètre | Contenu |
|---|---|
| `translation_source_language` | Code à deux lettres ou `auto`. |
| `translation_target_language` | Code cible à deux lettres. |
| `translation_blocks_json` | Tableau JSON `[{"id":"block-1","text":"Hello"}]`. |

La requête YAML construit son corps avec les primitives génériques
`build_json`, `build_json_array` et `json_array_projection`. Un fournisseur qui
accepte un objet peut projeter les textes des blocs normalisés :

```yaml
request_body_actions:
  - type: build_json
    fields:
      q:
        json_array_projection:
          parameter: translation_blocks_json
          path: /text
      source: {string_parameter: translation_source_language}
      target: {string_parameter: translation_target_language}
```

Un fournisseur qui exige un tableau d'objets peut utiliser `build_json_array` :

```yaml
request_body_actions:
  - type: build_json_array
    source: {json_parameter: translation_blocks_json}
    fields:
      Text: /text
```

La sortie normalisée contient une ligne par bloc avec `text`, et soit un `id`,
soit l'ordre positionnel sous `preserves_request_order`, ainsi
qu'éventuellement `detected_source_language`. Le pont vérifie la cardinalité,
les identifiants manquants, dupliqués ou supplémentaires, puis restitue les
blocs dans l'ordre d'entrée. Une réponse incomplète échoue ; aucun résultat
partiel n'est publié.

Une source connue égale à la cible réutilise le texte original sans appel
fournisseur ni fausse piste traduite.

### Mise en forme de la réponse

Scrapyfy fusionne les lignes d'une query racine en une seule ligne normalisée,
sauf si `result_item_field` promeut un groupe déclaré en lignes de premier
niveau. Un YAML de traduction doit donc construire un élément de groupe par
bloc envoyé, puis le déplier :

```yaml
row_pointer: "/*"
result_item_field: translated_blocks
entries:
  - name: translated_blocks
    type: object[]
    pointer: /
    select: all
    entries:
      - name: text
        type: string
        pointer: /translations/0/text
      - name: detected_source_language
        type: string
        pointer: /detectedLanguage/language
```

Pour un fournisseur retournant des tableaux parallèles, sélectionner chaque
élément du tableau, par exemple `pointer: /translatedText/*`. Le pont exige
exactement une ligne par bloc envoyé et n'accepte jamais un lot partiel.

### Reprises, budgets et erreurs

Chaque tentative fournisseur réserve un emplacement de concurrence et ses
unités fournisseur mesurées avant l'envoi de la query. La politique par défaut
est de trois tentatives, un délai initial de 500 ms, un plafond de 8 s et un
jitter borné ; un délai `Retry-After` fournisseur est respecté dans la limite du
plafond ; une indication supérieure à ce plafond retourne l'échec sans reprise
anticipée. Chaque tentative expire après 60 s. Seules les erreurs temporaires et les échecs génériques d'exécution de
query sont repris ; les erreurs d'authentification, de requête, de langue, de
taille, de quota et de réponse invalide sont retournées immédiatement.

Les statuts HTTP Scrapyfy sont classés en `authentication` (`401`, `403`),
`request` (`400`, `404`, `415`, `422`), `size` (`413`), `quota` (`429`) ou
`temporary` (`408`, `425`, `500`, `502`, `503`, `504`). Un statut refusé devient
`ProviderRejected` avec `retry_after_seconds` facultatif, sans rejouer le corps
ni changer de fournisseur.

Les paramètres de budget lus par Stream dans le YAML sont
`translation_max_concurrent_requests`, `translation_max_units_per_window` et
`translation_budget_window_seconds`. Un quota nul signifie « sans quota » ; la
concurrence et la fenêtre doivent être positives. Le budget en mémoire est
local au processus et partagé par identifiant de service. Le rechargement des
limites conserve les unités consommées et les réservations actives. Les fenêtres
de quota sont fixes ; un redémarrage remet les compteurs à zéro. Des définitions
de service utilisant le même compte doivent répartir son quota prudemment.
Les unités consommées ne sont pas remboursées après un échec, car le
fournisseur a pu déjà les compter. La traduction URL réserve zéro unité, car la
taille du document n'est pas connue avant l'appel fournisseur.

L'exécution retourne `TranslationMetrics` avec `batches`, `attempts`, `retries`,
`detection_attempts`, `blocks`, `units` et `elapsed_ms`. Les métriques ne
contiennent aucun texte source, texte traduit, credential ni secret
fournisseur. Les journaux de tentative exposent compteurs, durées et statuts,
y compris lors d'un échec, sans erreur brute.

La détection dédiée reçoit aussi `translation_detection_text`, le même
échantillon sous forme de texte brut. LibreTranslate utilise cette chaîne pour
`q` ; Azure utilise le bloc unique de `translation_blocks_json`. LibreTranslate
emploie la détection dédiée même sur un lot unique car son YAML actuel ne
restitue pas la langue détectée par `/translate`. Un texte traduit vide ou des
langues détectées contradictoires dans un lot invalident la réponse.

## 4. Query URL optionnelle `translate_url`

Paramètres normalisés : `translation_document_url`,
`translation_source_language`, `translation_target_language`.
La sortie normalisée contient `content` et `content_type` pour
`inline_document`. Le contenu complet est validé par l’adaptateur du format du
module `subtitles` d’`arachnea-stream`.

Sans capacité URL, le document est récupéré par le moteur, découpé en blocs
abstraits, puis traduit par `translate_texts`. Une URL privée, une URL
applicative ou des secrets de proxy ne sont jamais transmis au fournisseur.
Aucun repli facturable implicite n'a lieu après l'échec d'une traduction URL.

## 5. Sélection et exclusions

La politique est `auto` avec fournisseur préféré facultatif, ou `constrained`
avec un service imposé. La sélection :

1. filtre les services activés, configurés, avec métadonnées fraîches et budget
   disponible ;
2. vérifie le couple source/cible, la capacité utile et les restrictions de
   contexte/balisage ;
3. simule les lots sans appel fournisseur ;
4. classe un fournisseur préféré compatible, puis l'ordre administrateur, le
   nombre estimé de lots et l'identifiant stable.

Raisons normalisées d'exclusion : `unsupported_language_pair`,
`unsupported_capability`, `size_limit`, `budget_exhausted`,
`metadata_unavailable`, `source_language_required`.

Le fournisseur est figé pour tout le document avant le premier appel. Aucune
bascule vers un autre fournisseur après un appel de traduction en échec.

## 6. Planification et limites

La planification respecte simultanément :

- `max_blocks_per_request` (ou 1 si `accepts_multiple_blocks: false`) ;
- `max_units_per_block` ;
- `max_units_per_request` ;
- `max_request_bytes`, mesurés par la construction réelle du corps POST Scrapyfy
  après résolution des paramètres et credentials, sans appel réseau.

`plan_batches` reste une estimation JSON normalisée pour la couche documentaire.
`plan_batches_with_measurement` et `ScrapyfyTranslationBridge::plan_texts`
utilisent les octets du corps réel. `select_text_translation_bridge` dans Stream
emploie ces plans et la disponibilité effective des budgets pour sélectionner le
service, conserve les limites locales à l'exécution et garde le même fournisseur
pour tous les lots.

Le découpage d’une cue trop longue reste dans le module `subtitles`
d’`arachnea-stream`, qui ne doit couper ni Unicode ni balisage. Translation
refuse un bloc trop grand au lieu de le tronquer.

Bornes locales par défaut : 50 blocs, 10 000 unités par bloc, 50 000 unités par
requête et 65 536 octets sérialisés. Les plafonds effectifs sont les plus
restrictifs entre fournisseur et configuration locale.

La planification et le budget sont deux contrôles distincts : la planification
refuse un lot trop grand sans appel fournisseur, tandis que la réservation de
budget s'applique à chaque tentative réelle, reprises comprises. Un quota propre
au fournisseur reste extérieur à Arachnea ; le budget configuré doit donc rester
sous le quota complet du compte lorsque d'autres clients partagent le même
compte.

## 7. Pipeline documentaire WebVTT

Le module `subtitles` d’`arachnea-stream` parse et sérialise des documents
WebVTT complets sans dépendance à un fournisseur ni au player.

- L’entrée doit être en UTF-8 valide. Un BOM initial est accepté et les fins de
  ligne CRLF/CR sont normalisées en LF.
- La ligne de signature, les métadonnées d’en-tête facultatives, les
  identifiants de cues, les lignes de timing brutes (paramètres compris), les
  lignes de payload et les blocs `NOTE`, `STYLE` et `REGION` sont conservés en
  sortie.
- Les timestamps sont validés au format `mm:ss.mmm` ou `hh:mm:ss.mmm` ; la fin
  doit être strictement postérieure au début. La traduction ne réécrit jamais
  les données temporelles.
- Seul le texte visible des cues est envoyé. Les balises WebVTT et les
  timestamps internes restent dans leur cue et servent de frontières de
  segments, afin qu’un fournisseur ne puisse ni modifier ni supprimer le
  balisage. Les entités sont décodées pour la traduction, puis le texte retourné
  est rééchappé ; une traduction ne peut pas introduire de nouvelle balise ni
  terminer un bloc de cue.
- Une cue est l’unité logique initiale : ses lignes sont traduites ensemble
  lorsqu’aucun balisage protégé ne la découpe. Un texte trop long est scindé aux
  frontières de phrases, puis de mots, puis en dernier recours entre caractères
  Unicode. L’espacement retiré entre les fragments est conservé et restauré à la
  reconstruction ; aucun caractère Unicode ni jeton de balisage n’est coupé.
- Chaque segment reçoit un identifiant de bloc abstrait stable et une
  correspondance cue/segment vérifiable. La reconstruction exige exactement les
  identifiants, le nombre et l’ordre prévus ; un segment manquant, en trop,
  réordonné ou vide fait échouer le document au lieu de publier un fichier
  partiel.
- Les plafonds locaux sont appliqués avant traduction : 4 Mio décompressés,
  10 000 cues et 500 000 unités traduisibles par défaut. Ce sont des valeurs
  locales, distinctes des limites fournisseur.

SRT reste hors du chemin initial et n’est jamais servi comme du WebVTT brut.

## 8. Cache navigateur et validateurs

`TranslationRevision` décrit les langues source demandée et cible, le service,
le hash YAML, les révisions de configuration et du pipeline, ainsi qu’une
empreinte opaque des options influant sur le résultat. Le
`SubtitleTranslationValidator` de Stream ajoute le validateur source, les
empreintes opaques de source et de périmètre d’accès, le format et les révisions
du parseur et du pipeline de sous-titres. L’ETag composite est faible, versionné,
encodé en JSON/base64 compatible URL et limité à 4096 octets. Son contenu reste
lisible : ne jamais y inclure de credentials, URL signées ou autres secrets. Le
catalogue expose `service_yaml_hash`, optionnel ; son absence empêche de créer
un validateur fiable.

Le validateur source contient l’ETag HTTP amont ou un hash des octets source,
accompagné éventuellement de `Last-Modified`. Le port de récupération accepte ce
validateur et distingue une source inchangée d’un document complet ; l’appelant
compare les octets si la source ne fournit pas de validateur exploitable. Un tag
client ne prouve ni les droits ni la fraîcheur : il faut vérifier le contexte
autorisé puis revalider la source. Le navigateur conserve les fichiers traduits ;
ces contrats ne fournissent pas de store documentaire serveur. Si le navigateur
perd son fichier, une nouvelle traduction est nécessaire. L’implémentation de la
récupération, du transport conditionnel et du cache navigateur reste prévue aux
lots suivants.

## 9. Contrat backend prévu

| Commande | Rôle |
|---|---|
| `get_translation_services` | Catalogue public des services utilisables, capacités et langues ; aucun secret. |
| `prepare_subtitle_translation` | Vérifie les droits et candidats, puis produit un descripteur et un jeton sans récupérer ni traduire le document. |
| `GET <public-path>/subtitle_translation/<token>.vtt` | Retourne le fichier WebVTT et son ETag composite, ou `304` après revalidation de la source et du contexte. |
| `HEAD` sur le même chemin | Répond `405` sans récupération ni traduction. |

Le jeton est opaque, stable pendant sa durée de vie, sans URL source ni
credential. L'accès est revérifié au téléchargement et un jeton d'un utilisateur
ne donne pas accès aux sous-titres d'un autre. Les erreurs ne sont jamais
servies comme un fichier VTT.

### Statuts HTTP

| Situation | Statut |
|---|---|
| Descripteur invalide, format ou document invalide | `400 Bad Request` |
| Accès refusé | `403 Forbidden` |
| Jeton expiré | `410 Gone` |
| Document, cues ou unités au-delà des bornes | `413 Payload Too Large` |
| `HEAD` | `405 Method Not Allowed` |
| Source indisponible ou récupération échouée | `502 Bad Gateway` |
| Couple, capacité ou taille refusés | `422 Unprocessable Entity` |
| Quota ou budget indisponible | `429 Too Many Requests` |
| Métadonnées indisponibles ou incident temporaire | `503 Service Unavailable` |
| État interne en échec | `500 Internal Server Error` |

`Cache-Control: private, no-cache` est proposé pour conserver le corps tout en
revalidant avec `If-None-Match`. Une réponse `304` est vide et conserve l'ETag.

## 10. Hors périmètre du socle

SRT reste derrière une extension explicite. Les sous-titres HLS/DASH intégrés,
le direct, TTML/ASS, la transcription audio, les iframes sans piste externe, les
modes URL avec tâche différée, l'upload de documents, un fournisseur supplémentaire
et le regroupement linguistique entre cues demandent une analyse séparée.
