# Spécification des contrats de traduction Arachnea

Ce document décrit le contrat générique de traduction porté par
`arachnea-translation`, le contrat documentaire porté par
`arachnea-subtitles` et leur intégration prévue avec Stream. Le schéma YAML
général reste décrit dans [arachnea-scrapyfy](arachnea-scrapyfy-fr.md) ; ce
document ne décrit que les requêtes et sorties normalisées propres à la
traduction.

Statut : lot 1 implémenté (contrats, crates et pont générique). Les lots 2 à 10
ajoutent la configuration, le parseur WebVTT, l'orchestration, le transport et
l'interface utilisateur.

## 1. Séparation des couches

| Couche | Responsabilités | Interdictions |
|---|---|---|
| `arachnea-translation` | Langues à deux lettres, blocs abstraits, capacités et limites, métadonnées, sélection, planification, identité de cache, pont Scrapyfy. | Aucune connaissance de WebVTT, des cues, du player ou des pistes vidéo. Aucun fournisseur Rust spécifique. |
| `arachnea-subtitles` | Descripteurs de piste source, descripteur persistant, jeton, récupération autorisée et cache documentaire injectés, identité de cache du document, statuts HTTP. | Aucune dépendance vers Stream ni Scrapyfy ; aucun accès réseau non autorisé. |
| `arachnea-stream` | Droits, résolution des liens de proxy, catalogues, assemblage REST/Tauri, transport du fichier. | Aucun parsing de sous-titres ni mapping fournisseur. |
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

`text_translation` contient `query`, `accepts_multiple_blocks`, `size_unit`,
`max_blocks_per_request`, `max_units_per_block`, `max_units_per_request`,
`max_request_bytes`, `context_support`, `markup_support` et
`preserves_request_order`. `accepts_multiple_blocks: false` limite chaque
requête à un bloc. `preserves_request_order: true` permet à un fournisseur de
retourner les lignes dans l'ordre de la requête sans identifiants de blocs ; le
pont les associe alors par position tout en vérifiant la cardinalité.

`url_translation` contient `query`, `accepted_formats`, `max_document_bytes`,
`max_document_units` et `result_mode`. Seul `result_mode: inline_document` est
défini par le socle. Une simple page HTML n'est pas une traduction de
sous-titres ; un endpoint d'upload n'est pas une capacité URL.

`size_unit` accepte `utf8_bytes`, `unicode_code_points` ou une unité inconnue
conservée telle quelle. Une unité inconnue est mesurée prudemment en octets
UTF-8.

### Détection de la source

`automatic: true` indique que la traduction accepte une source non fournie.
`returns_detected_language: true` indique que la réponse expose la langue
effective. `query` désigne une query de détection séparée facultative.

Une entrée normalisée `source_language: null` demande la détection ; `auto`
n'est pas un code de langue et n'apparaît jamais dans `languages`. Le pont
transmet `translation_source_language = "auto"` au YAML, qui l'adapte à l'API
(omission, valeur `auto` ou autre). Avec des couples restreints et une source
inconnue, une détection préalable est requise pour établir l'éligibilité.

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

## 4. Query URL optionnelle `translate_url`

Paramètres normalisés : `translation_document_url`,
`translation_source_language`, `translation_target_language`.
La sortie normalisée contient `content` et `content_type` pour
`inline_document`. Le contenu complet est validé par l'adaptateur du format dans
`arachnea-subtitles`.

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
- `max_request_bytes`, mesurés sur la construction JSON normalisée, sans appel
  réseau.

Le découpage d'une cue trop longue reste dans `arachnea-subtitles`, qui ne doit
couper ni Unicode ni balisage. Translation refuse un bloc trop grand au lieu de
le tronquer.

Bornes locales par défaut : 50 blocs, 10 000 unités par bloc, 50 000 unités par
requête et 65 536 octets sérialisés. Les plafonds effectifs sont les plus
restrictifs entre fournisseur et configuration locale.

## 7. Cache

L'identité générique comprend : empreinte du contenu source, langues source et
cible effectives, service, endpoint/modèle, options linguistiques, révision de
configuration YAML/actions, révision du pipeline Translation et périmètre
d'accès. Aucun credential n'y figure. Une source inconnue reste `auto` ; aucun
code n'est inventé.

`arachnea-subtitles` ajoute l'empreinte du document, le format, la révision du
parseur WebVTT et la révision du pipeline documentaire. L'ETag du document final
est une empreinte forte de ses octets, entre guillemets ; l'ETag distant ne
valide que la source.

Le cache générique combine persistance backend et validation HTTP. L'ETag seul
n'empêche pas une retraduction après perte de l'état backend.

## 8. Contrat backend prévu

| Commande | Rôle |
|---|---|
| `get_translation_services` | Catalogue public des services utilisables, capacités et langues ; aucun secret. |
| `prepare_subtitle_translation` | Vérifie les droits et candidats, puis produit un descripteur et un jeton sans récupérer ni traduire le document. |
| `GET <public-path>/subtitle_translation/<token>.vtt` | Retourne le fichier WebVTT complet, son ETag et sa politique de cache. |
| `HEAD` sur le même chemin | Sert uniquement les métadonnées d'un résultat existant. Sans résultat, répond `404` sans récupération ni traduction. |

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
| `HEAD` sans résultat existant | `404 Not Found` |
| Source indisponible ou récupération échouée | `502 Bad Gateway` |
| Couple, capacité ou taille refusés | `422 Unprocessable Entity` |
| Quota ou budget indisponible | `429 Too Many Requests` |
| Métadonnées indisponibles ou incident temporaire | `503 Service Unavailable` |
| Cache ou état interne en échec | `500 Internal Server Error` |

`Cache-Control: private, no-cache` est proposé pour conserver le corps tout en
revalidant avec `If-None-Match`. Une réponse `304` est vide et conserve l'ETag.

## 9. Hors périmètre du socle

SRT reste derrière une extension explicite. Les sous-titres HLS/DASH intégrés,
le direct, TTML/ASS, la transcription audio, les iframes sans piste externe, les
modes URL avec tâche différée, l'upload de documents, un fournisseur supplémentaire
et le regroupement linguistique entre cues demandent une analyse séparée.
