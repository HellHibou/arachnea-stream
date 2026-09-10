# Analyse — Résolveur de flux générique piloté par une query YAML

**Date** : 10 septembre 2026  
**Statut** : implémentée le 10 septembre 2026
**Cas initial** : Antenne Réunion, bande-annonce d'un asset AlphaNetworks

---

## 1. Problème

Les resolvers légaux actuels (`rtbf-auvio-video`, `m6play-video`,
`antennereunion-video`, etc.) sont implémentés en Rust et retournent un
`ResolvedPlayerStream` au moment de l'appel `get_stream`.

Ce modèle est indispensable lorsque la lecture implique une authentification,
une négociation DRM ou un protocole non exprimable en YAML. Certaines
plates-formes exposent cependant une API de lecture éphémère entièrement
descriptible dans Scrapyfy : requête HTTP, extraction de l'URL signée, type de
manifeste et éventuels en-têtes.

### Cas Antenne Réunion vérifié live

Pour l'asset `26034` (« Chasseurs de dragons »), `POST /proxy/assets` expose :

```json
{
  "directMetadata": {
    "AD_LITRAILE": [
      { "idMedia": "76553", "audio": "fra", "sub": "non" },
      { "idMedia": "76554", "audio": "fra", "sub": "non" }
    ]
  }
}
```

Le site officiel appelle ensuite :

```text
POST /proxy/readTrailer
idMedia=76553
```

La réponse contient `result.url`, une URL HLS (`.m3u8`) signée et valide pour
une durée très courte (~10 secondes observées). Elle ne peut pas être retournée
au chargement de `get_entry` : elle doit être obtenue au clic, via `get_stream`.

---

## 2. État actuel de l'architecture

### Contrat public de lecture

Le frontend appelle actuellement :

```json
{
  "resolver": "rtbf-auvio-video",
  "target": "<asset-id>"
}
```

`GetStreamRequest` ne porte que `resolver` et `target`. Les resolvers Rust sont
sélectionnés globalement par leur `resolver_id`, puis invoqués par
`StreamScraper::get_stream`.

### Exécution de query déjà disponible

`ScraperAgregator::execute_query_async` sait déjà exécuter une query nommée
pour une source spécifique du groupe `arachnea-stream`, avec des paramètres
runtime et `CacheType::NoCache`. C'est le même mécanisme que `StreamResolver`
utilise pour les hosters YAML (`resolve_stream`).

Il n'est donc pas nécessaire d'ajouter un nouveau scraper ou une action
spécifique à Antenne Réunion : il faut relier proprement `get_stream` à cette
capacité existante.

---

## 3. Proposition recommandée

### 3.1 Nouveau resolver global : `scraper-query`

Ajouter une branche générique dans `StreamScraper::get_stream`, avant le
routage vers les resolvers Rust spécifiques :

```text
resolver == "scraper-query"
  → exécuter la query YAML explicitement désignée par le player
  → convertir stream_url / manifest_type en ResolvedPlayerStream
```

Ce resolver ne doit pas implémenter `PlayerStreamResolver` tel quel : cette
trait exige un `source_id()` statique, tandis que la source est dynamique
(`antennereunion-fr` aujourd'hui, d'autres services demain). Il peut être une
petite structure générique dans `src/services/` ou une fonction dédiée du
module `player_resolver`, appelée par `StreamScraper`.

Les resolvers Rust existants restent inchangés pour DRM, authentification et
protocoles non exprimables en YAML.

### 3.2 Extension du descripteur de resolver

Le transport actuel `{ resolver, target }` ne peut pas transmettre le nom de
query ni la source de façon sûre. La proposition ajoute deux champs optionnels :

```json
{
  "resolver": "scraper-query",
  "target": "76553",
  "source": "antennereunion-fr",
  "query": "get_stream_trailer"
}
```

Dans les descripteurs YAML objet, le même contrat est :

```yaml
resolver:
  kind: scraper-query
  target_id: <id media>
  source: antennereunion-fr
  query: get_stream_trailer
```

Le frontend doit préserver `source` et `query` lors de la normalisation d'un
player, puis les transmettre dans `get_stream`. Les champs sont optionnels pour
préserver tous les resolvers existants.

### 3.3 Paramètres fournis à la query

Pour une invocation `scraper-query`, le backend construit au minimum :

| Paramètre runtime | Valeur |
|---|---|
| `stream_target` | valeur du `target` du player, ex. `76553` |
| `query_url` | même valeur, pour cohérence avec les queries existantes |
| paramètres de service | `base_url`, `language_id`, clés d'identité, etc. |

La convention recommandée dans les YAML est **`{stream_target}`**. Ne pas
surcharger un identifiant métier existant tel que `{asset_id}` ou `{id}`.

### 3.4 Résultat YAML standardisé

La query de flux doit retourner la forme déjà utilisée par le resolver YAML de
hosters :

```yaml
- name: stream_url
  type: string[]
  pointer: /url
  select: all
- name: manifest_type
  type: string
  select: first
  actions:
    - type: format_text
      argument: m3u8
```

Champs supplémentaires réutilisables : `stream_headers`, `license_url`,
`license_headers`, `storyboard_vtt_url`, `storyboard`, `chapters`,
`image/title > link` et `title`.

La conversion de réponse `stream_url` doit être mutualisée avec le resolver
YAML existant, pour éviter deux implémentations divergentes du proxy HTTP,
des manifestes et des métadonnées de lecture.

---

## 4. Sécurité et validation indispensables

Le client peut appeler publiquement `get_stream`. Il ne faut donc pas lui
laisser le pouvoir d'exécuter arbitrairement une query par son nom.

### Autorisation explicite par query

La config de query doit recevoir un opt-in, par exemple :

```yaml
- name: get_stream_trailer
  stream_resolver: true
```

Le backend doit vérifier les quatre invariants suivants avant toute exécution :

1. `resolver == "scraper-query"` ;
2. `source`, `query` et `target` ne sont pas vides ;
3. la source existe dans le groupe `arachnea-stream` ;
4. la query demandée existe et déclare `stream_resolver: true`.

Les queries administratives, de catalogue, de recherche ou de détail ne seront
ainsi jamais accessibles via l'API de stream.

### Cache et URL signées

Les queries de lecture sont exécutées avec `CacheType::NoCache`. Une URL signée
de quelques secondes ne doit jamais être servie depuis le cache client ou le
cache serveur.

### Validation de sortie

Le backend refuse une réponse sans URL HTTP(S) de `stream_url` et protège les
flux avec le proxy HTTP existant quand il est configuré. Une query peut retourner
plusieurs URLs, dans leur ordre de préférence.

---

## 5. Application Antenne Réunion proposée

Le fichier compagnon
`yaml-query-stream-resolver-antennereunion-proposal.yaml` décrit les fragments
à ajouter après l'implémentation du contrat :

1. `get_entry` expose un trailer resolver seulement si
   `AD_LITRAILE[0].idMedia` existe ;
2. `get_stream_trailer` appelle `/proxy/readTrailer` ;
3. la query récupère `/result/url` et retourne un manifeste `m3u8` ;
4. `get_stream` l'exécute au clic avec `stream_target=76553`.

Le premier média `AD_LITRAILE` est retenu car il est la variante HLS observée
pour l'asset `26034`; le second est la variante DASH. Une évolution ultérieure
peut définir deux players ou une priorité de format si nécessaire.

---

## 6. Support de trailer dans le frontend

Les détails d'entrée actuels ne reconnaissent qu'une URL directe dans
`video/trailer`. Ils ne savent pas encore demander `get_stream` pour un
descripteur de trailer.

La même extension de contrat doit donc introduire un bloc de détail distinct :

```yaml
trailer > resolver > kind
trailer > resolver > target_id
trailer > resolver > source
trailer > resolver > query
```

Le frontend normalise ce bloc en `EntryTrailerResolver` et résout son stream au
clic sur « Bande-annonce » (et à la demande pour l'arrière-plan si l'option est
active). Cela évite de mélanger la bande-annonce aux `players[]` du programme
principal.

---

## 7. Plan d'implémentation proposé

1. **Contrat frontend/backend** : ajouter `source?: string` à
   `EntryPlayerResolver` et `GetStreamRequest`; préserver les resolvers actuels.
2. **Backend Stream** : implémenter le chemin générique `scraper-query`, qui
   exécute exclusivement la query conventionnelle `resolve_stream`, avec
   `NoCache`, une seule source et des erreurs contextualisées.
4. **Conversion** : mutualiser la conversion de sortie `stream_url` en
   `ResolvedPlayerStream` avec le resolver YAML existant, proxy inclus.
5. **Trailer frontend** : ajouter un descripteur de trailer résolu à la demande
   sans modifier le comportement des `video/trailer` directs existants.
6. **Antenne Réunion** : fragments intégrés au service encore désactivé sous
   `work-in-progress`; une vérification live de l'asset `26034` reste requise
   lors de l'activation : bouton bande-annonce, appel `readTrailer`, HLS
   proxifié et renouvellement après expiration.
7. **Tests** : tests unitaires de validation/allowlist et de conversion de
   stream, plus un test live manuel du YAML Antenne Réunion.

---

## 8. Alternatives écartées

### Ajouter `antennereunion-trailer` au resolver Rust existant

Fonctionne immédiatement mais multiplie le code Rust spécifique pour chaque
API simple. À réserver à un protocole non configurable, à l'authentification ou
au DRM.

### Stocker l'URL HLS dans `video/trailer`

Incorrect : l'URL est signée et expire très vite.

### Encoder `source` et `query` dans `target`

À éviter : format fragile, ambiguïté d'échappement et validation de sécurité
moins claire. Les champs doivent rester structurés.

### Réutiliser `get_players`

`get_players` est adapté au chargement différé d'une collection de players,
mais ne fournit pas le contrat générique `ResolvedPlayerStream` ni la
résolution HLS proxifiée. Il reste complémentaire, pas un substitut à
`scraper-query`.