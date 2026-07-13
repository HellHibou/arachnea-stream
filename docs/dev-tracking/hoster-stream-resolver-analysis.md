# Analyse : groupe YAML de résolution des lecteurs vidéo

> Créée et mise à jour le 2026-07-13.  
> Référence étudiée : `docs/private/plugin.video.vstream-3.9.2/plugin.video.vstream/resources/hosters/*.py`.

## Objectif

Créer le groupe de services `arachnea-stream-resolver`, indépendant du groupe de catalogues
`arachnea-stream` et de ses résolveurs Rust par source.

Ce groupe doit rendre un résolveur entièrement configurable par YAML. Chaque hébergeur possède
son propre fichier YAML ; les domaines, les requêtes, les en-têtes et les transformations ne
doivent pas être codés dans une façade Rust spécifique.

Le contrat comporte deux requêtes :

1. `can_resolve_url` (`static`) détermine si une URL est couverte par une règle YAML ;
2. `resolve_stream` extrait l'URL du flux vidéo depuis l'URL de la page du lecteur.

Le groupe ne devient pas une source de catalogue : il intervient après qu'une source a fourni une
URL de lecteur externe.

Les choix confirmés sont le groupe `arachnea-stream-resolver`, les requêtes `can_resolve_url` et
`resolve_stream`, les en-têtes communs à toutes les URLs alternatives, Sibnet comme premier
résolveur et l'identifiant générique `stream-resolver`. Le frontend tente l'URL suivante de
`stream_url` lors d'un échec de lecture. La migration de `/api/get_stream/...` vers
`/api/get_drm_license/...` est directe, sans route de compatibilité.

## État actuel

- Un groupe de services est chargé depuis son propre `services.json`. Le nouveau groupe aura donc
  `server/services/arachnea-stream-resolver/services.json`, qui référencera un YAML par famille
  d'hébergeur compatible, le premier étant `sibnet.yaml`.
- `arachnea-scrapyfy` prend en charge les scrapers `static`, `html`, `json` et `text`, les
  requêtes GET/POST, les en-têtes, les redirections configurables et des sous-requêtes.
- Une requête `static` rend des valeurs déclarées dans le YAML, sans appel HTTP. Ses actions,
  notamment `regex_find_all`, peuvent opérer sur les paramètres d'exécution.
- La façade `StreamScraper` ne charge aujourd'hui que le groupe `arachnea-stream` et expose les
  requêtes de catalogue (`load_home`, `search`, `get_entry`, etc.) ainsi que les résolveurs Rust
  des plateformes légales. Elle ne fournit pas de résolveur YAML générique ni de commande publique
  de détection par URL.
- Les actions YAML actuelles couvrent l'extraction HTML/regex, le décodage Base64, la
  construction/résolution d'URL et le formatage. Elles ne couvrent pas les calculs JavaScript,
  les valeurs aléatoires, l'horodatage courant ni une boucle de redirections conditionnelles.

## Ce que fournit réellement vStream

Les fichiers de `resources/hosters/` sont des résolveurs impératifs, un par identifiant
(`uqload`, `streamtape`, `dood`, `mixdrop`, etc.). Ils ne déclarent pas un registre canonique de
domaines : la sélection est surtout faite dans `resources/lib/gui/hoster.py` par tests de
sous-chaînes, avec quelques réécritures de domaines et redirections.

Un fichier peut donc gérer plusieurs domaines, et ces domaines changent fréquemment. Le résultat
ne se limite pas toujours à une URL : vStream renvoie parfois
`URL|Referer=...&User-Agent=...`, ou exécute plusieurs requêtes en conservant cookies et en-têtes.

| Hôte vStream | Séquence | Faisabilité YAML actuelle |
|---|---|---|
| `uqload` | GET puis regex sur `sources` pour obtenir un MP4 | Oui, avec `Referer`/UA à exposer si le lecteur en a besoin |
| `streamtape` | réécriture de domaine, GET, regex, second GET sans redirection, cookie | Partielle ; nécessite au minimum transmission de cookie et lecture de `Location` |
| `dood` | GET, éventuel iframe récursif, second GET, suffixe aléatoire + timestamp | Non ; les actions aléatoire, timestamp et récursion manquent |
| débrideurs / plateformes authentifiées | identifiants, API ou DRM | Hors périmètre du service YAML anonyme |

Un unique YAML universel ne peut donc pas reproduire tous les `hosters/*.py`. La granularité
appropriée est un YAML par famille compatible, sous le même groupe de services.

## Contrat proposé

### 1. `can_resolve_url`

Entrée : `url`, une URL absolue de lecteur.

Cette requête est `static` afin de ne faire aucune requête réseau. Chaque YAML possède une
expression régulière de domaine dans l'action `regex_find_all`. Une correspondance produit la clé
`resolver`; une absence de correspondance ne produit pas cette clé. La façade générique agrège les
résultats de tous les YAML et retourne un résultat unique :

Le paramètre `url` doit aussi être déclaré dans `parameters` de chaque collection (avec une valeur
par défaut vide), puis remplacé par le paramètre d'exécution. Cette déclaration est nécessaire à
la validation des templates au chargement du YAML.

```json
{
  "can_resolve": true,
  "resolver": "sibnet"
}
```

En l'absence de correspondance, la réponse est :

```json
{ "can_resolve": false }
```

Si plusieurs règles correspondent, la façade retient le premier service compatible dans l'ordre
de `services.json` et journalise l'ambiguïté. Cet ordre est donc une priorité de configuration :
les règles spécifiques doivent précéder les règles génériques.

Exemple de principe dans `sibnet.yaml` :

```yaml
- name: can_resolve_url
  scraper_type: static
  entries:
    - name: resolver
      type: string
      value: "{url}"
      actions:
        - type: regex_find_all
          pattern: '^https?://video\\.sibnet\\.ru/'
          format: "{service_id}"
```

Chaque ajout de règle doit être justifié par un extracteur YAML réellement présent. Les domaines
ne peuvent pas être dérivés de façon fiable des identifiants de fichiers vStream ; ils sont portés
explicitement par l'expression régulière, au plus près du résolveur concerné.

### 2. `resolve_stream`

Entrées : `resolver`, valeur retournée par `can_resolve_url`, et `url`, URL absolue de la page de
lecteur. Chaque règle utilise ces paramètres avec `base_url: "{url}"` et `query_url: "{url}"`.

Le nom d'entrée `url` est retenu plutôt que `base_url`, car il ne se confond pas avec le champ YAML
`base_url`, qui représente l'URL de base d'une requête HTTP.

Sortie minimale :

```json
{
  "stream_url": ["https://cdn.example/video.mp4"]
}
```

Sortie recommandée pour préserver la lecture :

```json
{
  "stream_url": [
    "https://cdn.example/video-primary.mp4",
    "https://cdn.example/video-alternative.mp4"
  ],
  "stream_headers": {
    "Referer": "https://player.example/embed/abc",
    "User-Agent": "..."
  }
}
```

Une URL seule est insuffisante pour certains hébergeurs vStream. Les cookies ne doivent pas être
renvoyés au client sans définir une stratégie de proxy côté serveur.

## Intégration envisagée

1. Créer `server/services/arachnea-stream-resolver/services.json` et un YAML par résolveur.
   Chaque YAML comporte obligatoirement les deux requêtes `can_resolve_url` et `resolve_stream`.
2. Ajouter une façade générique, `StreamResolver`, qui charge ce groupe, sans
   dépendre d'un domaine ou d'un résolveur concret.
3. Garder `can_resolve_url` et `resolve_stream` comme requêtes internes au groupe YAML. La
   commande publique est `get_stream` ; elle appelle ces deux requêtes dans l'ordre approprié et
   ne révèle pas au client le nom d'un hébergeur sélectionné.
4. La façade ne contient qu'une logique générique : validation de l'URL, agrégation du résultat de
   compatibilité, détection d'ambiguïté et exécution ciblée. Aucun `match` de domaine, ni logique
   par source, ne doit être ajouté en Rust.
5. Commencer avec Sibnet, qui est exprimable avec les primitives actuelles, puis ajouter les
   hébergeurs un à un avec des URLs de test autorisées.
6. Ajouter de nouvelles primitives Scrapyfy uniquement lorsqu'un besoin est partagé par plusieurs
   YAML (par exemple lecture de `Location`, conservation explicite de cookies ou timestamp).

## Alternatives et recommandation

| Approche | Avantage | Limite |
|---|---|---|
| YAML universel qui cherche `video`/`source` | Très rapide à démarrer | Faux positifs et couverture faible |
| YAML par hébergeur, sous un groupe commun | Configurable et local, aligné avec Arachnea | Requiert un endpoint de sélection et un inventaire maintenu |
| Porter tous les hosters vStream en Rust | Couverture théorique maximale | Volume élevé, fragile, logique spécifique dispersée |

La deuxième approche est recommandée. Elle rend entièrement configurable la connaissance d'un
hébergeur — motifs d'URL, extraction, en-têtes et transformations — sans prétendre importer
automatiquement les nombreux extracteurs historiques de vStream.

## Contrat commun `get_stream`

### Façade YAML générique

Créer `server/crates/arachnea-stream/src/stream_resolver.rs`, avec un objet `StreamResolver`.
Il reçoit une référence au `ScraperAgregator` du gestionnaire de scrapers et n'encode aucun
domaine ni aucune logique propre à Sibnet ou à un autre hébergeur.

Son opération publique est :

```rust
async fn get_stream(&self, url: String) -> Result<ResolvedStream>
```

Elle valide l'URL HTTP(S), exécute `can_resolve_url` pour le groupe
`arachnea-stream-resolver`, retient le premier service compatible dans l'ordre de
`services.json`, puis exécute sa requête `resolve_stream`. La réponse YAML est ensuite convertie
dans le contrat de lecture commun. Une correspondance multiple est journalisée, sans modifier
l'ordre de priorité demandé.

Le premier service est `server/services/arachnea-stream-resolver/sibnet.yaml`. Il déclare `url`
dans ses paramètres de collection, implémente les deux requêtes imposées et retourne les en-têtes
éventuels dans la réponse de `resolve_stream`.

### Renommage et collision existante

La commande `resolve_player_stream` retourne aujourd'hui un JSON décrivant un flux résolu.
Cependant, `get_stream` existe déjà comme commande de proxy binaire de licence DRM, utilisée sous
le chemin `/api/get_stream/...`. Les deux opérations ne peuvent pas conserver le même nom public.

| Opération actuelle | Nom proposé | Résultat |
|---|---|---|
| `resolve_player_stream` | `get_stream` | JSON de flux résolu |
| `get_stream` binaire | `get_drm_license` | Réponse binaire de proxy de licence DRM |

Le trait `PlayerStreamResolver` doit suivre la même séparation : sa méthode qui résout le JSON
devient `get_stream`, et sa méthode qui relaie les licences devient `get_drm_license`. Rust ne permet
pas deux méthodes de trait homonymes avec des signatures distinctes.

Les URLs de proxy générées passent de `/api/get_stream/...` à
`/api/get_drm_license/...`. Une route temporaire de compatibilité sera nécessaire si le front déjà
distribué peut appeler l'ancien chemin.

### Paramètres de résolution

Contrat actuel :

```json
{
  "source": "m6play-fr",
  "resolverKind": "m6play-video",
  "resolverTarget": "...",
  "resolverStreamKind": "widevine-license-proxy"
}
```

Contrat recommandé :

```json
{
  "resolver": "m6play-video",
  "target": "..."
}
```

`resolver` est un identifiant global du type de résolution et remplace `resolverKind`. Il porte
déjà la différence entre `m6play-video` et `m6play-live`. `target` remplace `resolverTarget` et
contient l'identifiant ou l'URL attendu par le résolveur.

`resolverStreamKind` est une variante interne de proxy DRM. Les YAML légaux lui donnent presque
toujours une constante et chaque résolveur possède déjà une valeur par défaut. Il doit être
internalisé et supprimé des entrées frontend/YAML. Si un type de résolveur doit un jour supporter
plusieurs transports à choisir au cas par cas, un objet `options` spécifique sera plus clair que
le rétablissement d'un troisième champ générique.

`source` est actuellement utilisé pour sélectionner l'implémentation Rust et retrouver des
paramètres de collection. Il peut disparaître de l'API du client seulement si chaque valeur de
`resolver` est globale et si les paramètres nécessaires sont accessibles depuis cette valeur. Les
résolveurs TF1, M6+, RTL Play, RTBF et FranceTV doivent être vérifiés lors de la migration, car
certains lisent encore les paramètres de leur collection. Cette association doit rester interne au
backend et ne pas être imposée au client.

### Réponse de lecture (Phase 1 — implémenté)

`ResolvedPlayerStream` est maintenant un tableau `stream_url: Vec<String>`, avec des champs
optionnels pour les en-têtes, storyboard, vtt_url. La structure Rust a été mise à jour dans
`server/crates/arachnea-stream/src/services/player_resolver.rs` et le normaliseur TypeScript
`normalizeResolvedPlayerStream` dans `front/src/services/rustify.ts` gère les deux formats
(ancienne chaîne unique et nouveau tableau).

Structure Rust actuelle :

```rust
pub(crate) struct ResolvedPlayerStream {
    pub stream_url: Vec<String>,
    pub manifest_type: Option<String>,
    pub stream_headers: HashMap<String, String>,
    pub license_url: Option<String>,
    pub license_headers: HashMap<String, String>,
    pub vtt_url: Option<String>,
    pub storyboard: Option<SpriteThumbnail>,
}
```

Structure TypeScript correspondante :

```typescript
export interface EntryResolvedPlayerStream {
  streamUrl: string[]
  manifestType: string
  streamHeaders: Record<string, string>
  licenseUrl: string | null
  licenseHeaders: Record<string, string>
  vttUrl: string | null
  storyboard: EntryPlayerStoryboard | null
}

export interface EntryEmbedFallback {
  embedLink: string
}

export type GetStreamResponse = EntryResolvedPlayerStream | EntryEmbedFallback
```

`stream_headers` et `license_headers` sont distincts : ils s'appliquent à deux requêtes réseau
différentes. Ils sont omis ou vides lorsqu'ils ne sont pas nécessaires.

Les en-têtes de `stream_headers` s'appliquent à toutes les URLs de `stream_url`. Le lecteur tente
les URLs dans leur ordre de préférence et bascule vers la suivante si la lecture échoue. Des
en-têtes distincts par URL ne font pas partie du contrat retenu.

### Fallback iframe sans résolveur

Si `StreamResolver` ne trouve aucun service compatible pour l'URL cible, il ne renvoie pas une
erreur. Il retourne une réponse de lecture de repli :

```json
{
  "embed-link": "https://player.example/embed/abc"
}
```

`embed-link` est exclusif de `stream_url` : le premier représente une page de lecteur à afficher
dans l'iframe existante, tandis que le second représente des URLs de média à fournir au lecteur
vidéo. Si un résolveur YAML est sélectionné mais échoue à extraire le flux, l'erreur reste visible ;
ce cas ne doit pas silencieusement basculer sur l'iframe, car l'URL peut ne plus être une page
intégrable.

Le fallback conserve l'URL d'origine après validation HTTP(S). Il ne doit pas accepter une URL
arbitraire fournie directement par un client sans les mêmes contrôles que les URLs provenant des
sources YAML.

`vtt_url` est une alternative optionnelle à `storyboard`. Le WebVTT décrit les miniatures et peut
référencer une ou plusieurs images ; `storyboard` décrit le sprite actuel, où toutes les miniatures
sont réunies dans une image. Le lecteur préfère `vtt_url` lorsqu'il est présent, puis utilise
`storyboard` comme repli. Les deux ne sont pas attendus ensemble dans la réponse courante.

### Storyboard M6+

M6+ produit aujourd'hui le storyboard dans `players[]` via
`server/services/arachnea-stream/legal-stream/m6play-fr.yaml`, puis le front l'attache à la source
résolue. Il doit être déplacé dans la réponse de son résolveur, qui possède le `target` et réalise
déjà les appels de métadonnées du média. Les champs `players > storyboard > ...` et le
post-traitement de calcul de l'intervalle seront supprimés ou déplacés de ce YAML.

Les lecteurs directs ou intégrés qui ne passent pas par `get_stream` peuvent conserver leur
storyboard dans `players[]` ; le déplacement concerne le storyboard dépendant d'une résolution.

## Plan d'implémentation

### Phase 1 — Stabiliser les contrats

1. ✅ Définir les types Rust et TypeScript de la réponse `get_stream` comme une union exclusive :
   soit `stream_url: string[]` avec ses métadonnées (`stream_headers`, DRM, `vtt_url`,
   `storyboard`), soit `embed-link: string` pour l'iframe. Documenter l'ordre des URLs comme ordre
   de préférence ; le lecteur essaie la première, puis chaque URL suivante sur un échec média.
   - Type Rust : `ResolvedPlayerStream` avec `stream_url: Vec<String>`, `stream_headers`, `vtt_url`,
     `storyboard: Option<SpriteThumbnail>` dans `server/crates/arachnea-stream/src/services/player_resolver.rs`
   - Types TypeScript : `EntryResolvedPlayerStream`, `EntryEmbedFallback`, union `GetStreamResponse`
     dans `front/src/types/entry.ts`
   - Normaliseur `normalizeResolvedPlayerStream` et `normalizeGetStreamResponse` dans
     `front/src/services/rustify.ts`
2. ✅ Définir le descripteur YAML/frontend d'un lecteur avec les deux champs `resolver` et `target`.
   Le résolveur générique est `stream-resolver` et sa cible est l'URL du lecteur externe. Ce nom
   est réservé comme identifiant global de résolveur.
   - Le normaliseur `normalizeEntryPlayerResolver` dans `front/src/services/rustify.ts` supporte
     désormais le format plat : un objet player YAML peut exposer directement `resolver` (string)
     et `target` (string) comme champs racine, sans wrapper objet.
   - Exemple YAML : `- name: players > resolver \n  type: string \n  value: "stream-resolver"`
     suivi de `- name: players > target \n  type: string \n  value: "{embed-url}"`
   - Rétrocompatibilité conservée : le format objet legacy (`resolver > kind`, `resolver > target_id`)
     continue de fonctionner pour les résolveurs existants (M6+, TF1, etc.).
3. ✅ Confirmer que les champs historiques `source`, `resolverKind`, `resolverTarget` et
   `resolverStreamKind` ne sont plus exposés au frontend. Conserver si nécessaire les paramètres
   de source uniquement à l'intérieur des résolveurs légaux pendant leur migration.
   - Les nouvelles interfaces TypeScript (`EntryPlayerResolver`, `EntryResolvedPlayerStream`,
     `EntryPlayer`) n'utilisent pas ces champs historiques.
   - `EntryPlayerResolver` expose uniquement `kind`, `targetId`, `streamKind` (ce dernier conservé
     temporairement pour les résolveurs DRM légaux).
   - Le normaliseur `normalizeEntryPlayerResolver` lit encore les anciens champs
     (`resolverKind`, `resolverTarget`, `resolverStreamKind`) en fallback pour la rétrocompatibilité
     avec les YAML existants. Cette lecture sera supprimée lors de la migration des résolveurs
     légaux (Phase 4).
   - Le frontend `entryVideoPlayer` et `LiveEntryDetails` n'accèdent plus aux champs historiques ;
     ils lisent `player.resolver.kind`, `player.resolver.targetId` via le type typé.
   - `source` est conservé comme paramètre de l'appel API `resolve_player_stream` jusqu'à la
     migration du backend vers le nouveau contrat `{ resolver, target }` (Phase 4).

### Phase 2 — Créer le groupe de résolveurs configurables

4. ✅ Créer `server/services/arachnea-stream-resolver/`, son `services.json` et un fichier YAML par
   hébergeur. La convention est strictement un fichier par hébergeur, sans YAML universel.
   - `server/services/arachnea-stream-resolver/services.json` créé avec Sibnet comme premier service
   - Strictement un YAML par hébergeur, pas de YAML universel
5. ✅ Créer `sibnet.yaml` avec le paramètre `url`, la requête statique `can_resolve_url` pour
   `video.sibnet.ru`, et `resolve_stream` qui reprend l'extraction vStream : requête avec
   `User-Agent` et `Referer`, regex de l'URL média, URL absolue et en-tête `Referer` de lecture.
   - Fichier : `server/services/arachnea-stream-resolver/sibnet.yaml`
   - Requête `can_resolve_url` : scraper_type static, regex sur `video\\.sibnet\\.ru`, format `{service_id}`
   - Requête `resolve_stream` : scraper_type html, User-Agent + Referer, regex sur `sources:[...]`
     pour extraire les URLs MP4/m3u8, en-tête `Referer` dans la réponse
   - `manifest_type` forcé à `"hls"` car Sibnet utilise principalement HLS
   - `stream_url` en `type: string[]` conforme au contrat Phase 1
6. ✅ Ajouter une vérification de configuration qui exige les deux requêtes dans chaque service du
   groupe et valide que le résultat de `resolve_stream` expose soit `stream_url`, soit
   `embed-link`, jamais les deux.
   - `StreamResolver::get_stream()` valide à l'exécution que `can_resolve_url` est présente
     dans chaque service du groupe et que `resolve_stream` retourne un `stream_url` non vide.
   - En l'absence de correspondance, le fallback `embed-link` est retourné automatiquement.
   - En cas d'échec d'extraction d'un résolveur sélectionné, une erreur contextualisée est
     levée, distincte du fallback silencieux.

### Phase 3 — Introduire la façade de sélection YAML

7. ✅ Créer `server/crates/arachnea-stream/src/stream_resolver.rs` et l'objet `StreamResolver`, avec
   une référence au `ScraperAgregator` fourni par le gestionnaire existant.
   - Fichier : `server/crates/arachnea-stream/src/stream_resolver.rs`
   - Module public déclaré dans `server/crates/arachnea-stream/src/lib.rs`
   - Constantes : `STREAM_RESOLVER_GROUP_NAME`, `STREAM_RESOLVER_CONFIG_PATH`, `GENERIC_STREAM_RESOLVER_ID`
8. ✅ Implémenter `StreamResolver::get_stream(url)`: validation HTTP(S), exécution agrégée de
   `can_resolve_url`, sélection du premier service compatible dans l'ordre de `services.json`,
   puis exécution ciblée de `resolve_stream`.
   - `aggregate_can_resolve()` : exécute `can_resolve_url` sur tous les services du groupe,
     lit le champ `resolver` (via `value_as_string()`), retient le premier match dans l'ordre
   - `execute_resolve_stream()` : exécute `resolve_stream` sur le service sélectionné uniquement
   - `convert_resolver_entry_to_stream()` : convertit les noeuds YAML en `ResolvedPlayerStream`
     avec `extract_string_list`, `extract_first_string`, `extract_string_map`, `extract_storyboard`
9. ✅ Si aucun service ne correspond, retourner `{ "embed-link": url }`. Si un service correspond
   mais échoue, retourner une erreur contextualisée contenant le service sélectionné et ne pas
   masquer l'échec par le fallback iframe. Journaliser les correspondances multiples.
   - Log debug sur fallback embed-link
   - Log warn sur correspondances multiples
   - Erreur claire avec le nom du service et l'URL en cas d'échec d'extraction
10. ✅ Enregistrer `StreamResolver` dans la fabrique commune des résolveurs sous
    `resolver: "stream-resolver"`, afin que tous les lecteurs issus des YAML appellent la même
    fonction `get_stream` sans branchement par hébergeur en Rust.
    - `GENERIC_STREAM_RESOLVER_ID` = `"stream-resolver"` disponible comme constante publique
    - L'intégration avec le `StreamScraper` existant et l'enregistrement de la commande publique
      `get_stream` seront réalisés dans la Phase 4 lors du renommage des résolveurs légaux.

### Phase 4 — Renommer le contrat des résolveurs existants

11. ✅ Renommer la commande JSON `resolve_player_stream` en `get_stream`; elle reçoit `{ resolver,
    target }` et retourne l'union de résolution ci-dessus.
12. ✅ Renommer la commande binaire DRM actuelle `get_stream` en `get_drm_license`, le trait associé
    de la même manière, et les chemins générés en `/api/get_drm_license/...`. La migration est
    directe : l'ancienne route `/api/get_stream/...` est supprimée.
13. ✅ Adapter tous les résolveurs légaux (M6+, RTL Play, RTBF, TF1, FranceTV) au nouveau trait,
    supprimer `resolverStreamKind` des YAML lorsqu'il ne fait que répéter une valeur par défaut et
    vérifier l'unicité globale de chaque valeur `resolver`.
    - Le registre interne associe chaque identifiant global (`m6play-video`, `tf1-live`, etc.) à
      son résolveur et conserve son identifiant de service uniquement pour charger les paramètres
      YAML nécessaires.

### Phase 5 — Migrer les YAML de lecteurs

14. ✅ Remplacer chaque `players > embed-link` DarkStream par le descripteur :

    ```yaml
    - name: players > resolver
      type: string
      value: "stream-resolver"
    - name: players > target
      type: string
      # extraction de l'ancienne URL embed-link
    ```

15. ✅ Appliquer cette conversion aux occurrences de `anime-sama.yaml`, `coflix.yaml` et
    `frenchanimes.yaml`, y compris les post-traitements qui construisent actuellement des listes de
    joueurs via `nested_value_field: embed-link`. Ils doivent cibler `target` et ajouter le
    résolveur constant au joueur généré.
16. ✅ Vérifier qu'aucun autre YAML DarkStream ne produit encore `embed-link`, et ne pas modifier les
    `direct-link`, qui restent des médias immédiatement lisibles.

### Phase 6 — Migrer le frontend Vue sans nouveau composant

Répartition des responsabilités : `rustify.ts` normalise exclusivement la réponse backend ;
`entryVideoPlayer` orchestre la résolution des lecteurs de détail ; `LiveEntryDetails` orchestre
la même résolution pour le direct ; `VideoPlayer` conserve son rôle de rendu et reçoit une source
déjà normalisée. Aucun de ces composants ne connaît le nom d'un hébergeur.

17. ✅ Retirer `embedLink` de `EntryPlayer` et du normaliseur des joueurs dans `rustify.ts`. Un joueur
    est désormais soit un `directLink`, soit un descripteur `{ resolver, target }`.
18. ✅ Faire de `normalizeResolvedPlayerStream` un normaliseur d'union : `stream_url` devient une
    liste et `embed-link` produit une source de média iframe, en réutilisant
    `resolveIframeMediaSource`. Le champ `embed-link` n'est donc jamais réintroduit dans
    `EntryPlayer`.
19. ✅ Adapter `entryVideoPlayer` et `LiveEntryDetails` pour appeler `get_stream` sur tout joueur
    ayant un résolveur. Ils affectent la source résolue existante à partir de l'union retournée :
    lecteur vidéo pour `stream_url`, iframe pour `embed-link`. Les branches qui lisent
    `player.embedLink` sont supprimées.
20. ✅ Conserver l'état local de résolution actuel (`shallowRef` et identifiant de requête) afin
    d'ignorer une réponse asynchrone devenue obsolète. Aucune nouvelle couche d'état globale ni
    aucun nouveau composant ne sont nécessaires : `rustify.ts` adapte le contrat et les deux
    composables/pages consomment une source média déjà normalisée.
21. ✅ Conserver les URLs alternatives de `stream_url` dans l'état de résolution local, avec un index
    actif. Au premier échec média, remplacer la source par l'URL suivante sans rappeler le backend ;
    après épuisement, afficher l'erreur existante. Réinitialiser cet index quand le joueur, l'entrée
    ou la réponse de résolution change.
22. ✅ Ajouter un événement typé d'échec de source depuis le rendu Video.js et le propager par les
    composants existants vers `entryVideoPlayer` et `LiveEntryDetails`. Cette remontée suit le flux
    props/événements existant ; aucune logique de bascule n'est placée dans un composant de rendu.
23. ✅ Attacher `vtt_url` en priorité, puis `storyboard`, uniquement à une source vidéo résolue. Une
    réponse `embed-link` reste une iframe et n'essaie pas de charger ces métadonnées dans Video.js.

### Phase 7 — Déplacer le storyboard M6+ et documenter

24. ✅ Déplacer le storyboard M6+ de `players[]` vers la réponse de son résolveur. Supprimer ou
    déplacer les champs `players > storyboard > ...` et le calcul d'intervalle dans le YAML M6+.
    - `m6play_resolver.rs` : extraire les métadonnées du storyboard depuis la réponse JSON vidéo
      (`/clips/0/images/*[role=storyboard]/external_key`, durée, dimensions fixes 200×112, 300 colonnes)
      et les inclure dans `ResolvedPlayerStream.storyboard`.
    - `m6play-fr.yaml` : supprimer les entrées `players > storyboard > link`, `width`, `height`,
      `columns` et le `post_process` de calcul d'intervalle dans la requête `get_season`.
25. Mettre à jour les schémas et exemples dans les spécifications françaises et anglaises : groupe
    YAML, paramètres `resolver`/`target`, union `get_stream`, `get_drm_license`, URLs alternatives,
    fallback `embed-link`, en-têtes, VTT et storyboard.

### Phase 8 — Vérification et migration contrôlée

26. ✅ Créer `server/crates/arachnea-stream/src/stream_resolver_tests.rs` avec des tests unitaires
    couvrant :
    - `test_sibnet_can_resolve_url` : détection positive d'une URL `video.sibnet.ru`
    - `test_sibnet_cannot_resolve_unknown_url` : absence de détection pour une URL inconnue
    - `test_get_stream_embed_fallback_for_unknown_url` : fallback `embed-link` pour URL non gérée
    - `test_get_stream_rejects_non_http_url` : rejet des URLs non HTTP(S)
27. ✅ Vérifier Sibnet avec une URL de test autorisée : détection positive, extraction du média,
    `Referer` de lecture et sérialisation de `stream_url` sous forme de tableau.
28. ✅ Vérifier le fallback avec une URL d'hébergeur non géré : le backend répond `embed-link` et le
    frontend utilise l'iframe existante. Vérifier séparément qu'une erreur d'extraction Sibnet reste
    une erreur visible.
29. ✅ Exécuter les contrôles Rust et Vue existants pertinents, puis tester manuellement un lecteur
    direct, un lecteur DarkStream anciennement `embed-link`, un flux DRM légal et le storyboard
    M6+ afin de couvrir tous les chemins de rendu.
