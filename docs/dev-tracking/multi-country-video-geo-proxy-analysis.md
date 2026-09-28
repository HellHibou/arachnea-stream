# Analyse — proxy géolocalisé multi-pays par vidéo

## Contexte

Le proxy dynamique et le système de scraping ne prennent actuellement en
charge qu'un seul code pays pour une requête géolocalisée. Cette limite ne
correspond pas aux plateformes légales dont la disponibilité varie selon la
vidéo : France TV, M6 Play, Arte, TF1+ et TV5MONDE+ peuvent fournir plusieurs
pays de disponibilité pour un même contenu.

L'objectif est donc de faire remonter une liste **optionnelle**, définie au
niveau de chaque vidéo, depuis les réponses des plateformes jusqu'au proxy. Si
une liste est présente, le proxy doit pouvoir sélectionner un proxy utilisable
dans l'un quelconque des pays indiqués. Si elle est absente, le comportement
actuel sans contrainte de géolocalisation doit être conservé.

Le terme *lecteur* désigne ici la configuration de lecture retournée dans les
champs techniques `players` et `resolver`.

## État d'implémentation au 28 septembre 2026

Le contrat multi-pays est implémenté de bout en bout. Il reste à configurer les
listes réellement émises par chaque service à partir de métadonnées de droits
vérifiées, vidéo par vidéo. Aucun pays ne doit être déduit du seul nom d'une
plateforme, de la langue du catalogue ou d'un paramètre technique de lecture.

### Chaîne multi-pays réalisée

- Le proxy dynamique accepte une liste ordonnée de pays, tente les pools dans
  cet ordre et conserve le chargement à la demande, le cache négatif et les
  erreurs par pays.
- `proxy_countries` est la forme canonique sur les contrats backend et
  frontend. `proxy_country` reste accepté comme repli à un seul élément ; si
  les deux sont présents, `proxy_countries` est prioritaire.
- Le frontend normalise les codes ISO alpha-2 valides en majuscules, supprime
  les doublons sans modifier l'ordre et appelle `get_stream` avec
  `proxy_countries` ainsi qu'avec le premier pays dans `proxy_country` pendant
  la transition.
- Les résolveurs spécifiques concernés propagent la liste vers leurs requêtes
  de négociation, leurs URLs média proxifiées et, lorsque nécessaire, leurs
  requêtes DRM différées.
- Scrapyfy accepte les listes statiques et l'expansion d'une liste JSON
  d'exécution avec `http.proxy_countries: ["{proxy_countries}"]`. Les listes
  sont normalisées et dédupliquées avant l'appel HTTP.
- Une liste absente, vide ou invalide ne déclenche volontairement aucun proxy
  géolocalisé implicite.

### Validation exécutée

| Vérification | Résultat |
|---|---|
| `git diff --check` | Réussi lors de l'implémentation. |
| `rustfmt --check` sur les fichiers Rust modifiés | Réussi. |
| `cargo check -p arachnea-proxy -p arachnea-scrapyfy -p arachnea-stream` | Réussi. |
| Tests ciblés Scrapyfy pour listes JSON d'exécution, listes statiques et repli historique | Réussis. |
| `cargo test -p arachnea-proxy` | Réussi. |
| `npm run type-check` et `npm run lint:css` du frontend | Réussis. |
| Suite Scrapyfy complète | Trois échecs préexistants liés au répertoire de données global et à l'empoisonnement de `Once`. |
| Suite Stream complète | Deux échecs préexistants de fixtures `anime-sama` : `Missing link in load_home section`. |

## État actuel

### Proxy dynamique

Le modèle proxy était mono-pays sur toute la chaîne. Il transporte désormais
une liste ordonnée normalisée, avec compatibilité de lecture de l'ancienne
forme à un pays :

- `ProxyLoadRequest`, `ProxyInventory` et le routeur dynamique reçoivent et
  sélectionnent une liste priorisée de pays.
- L'inventaire conserve un chargement et un cache négatif distincts pour
  chaque pays, puis retient le premier candidat utilisable dans l'ordre de la
  liste demandée.
- Le transport HTTP et les URLs créées par `proxied_url_with_countries`
  transportent l'ensemble du contexte de pays.

### Chargement des sources proxy

`ScrapyfyProxyDataProvider` dans
`server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs` conserve une
résolution par pays :

1. reçoit une demande de chargement pour un pays ;
2. exécute `list_proxies_for_country` avec `{ country }` ;
3. résout éventuellement les pays manquants des IP ;
4. filtre les résultats sur ce seul pays.

Les sources proxy YAML sont donc elles aussi invoquées une fois par pays. Cela
peut être conservé : le changement doit orchestrer plusieurs chargements
indépendants, puis agréger leurs candidats dans l'inventaire.

### Scraping et lecteurs

`ScraperHttpConfig`, le contrat de lecture, `GetStreamRequest` et le chemin
générique `scraper-query` acceptent désormais `proxy_countries`. Les YAML
peuvent écrire `resolver > proxy > countries`, et les documents
`docs/specifications/arachnea-scrapyfy-*.md` et
`docs/specifications/arachnea-stream-*.md` ont été mis à jour. Les formes
historiques `country` et `proxy_country` restent des replis compatibles.

## Exigence fonctionnelle cible

Chaque vidéo peut exposer une liste facultative de pays compatibles avec sa
lecture. Cette liste doit être transportée dans le lecteur :

```yaml
players:
  - resolver:
      kind: example-video
      target_id: example-id
      proxy:
        countries:
          - FR
          - BE
          - CH
```

Règles attendues :

- `resolver.proxy.countries` est optionnel ;
- chaque valeur est un code ISO 3166-1 alpha-2 ;
- les valeurs sont normalisées en majuscules, les entrées vides ou invalides
  sont ignorées et les doublons sont supprimés en préservant l'ordre ;
- une liste non vide demande une sortie proxy géolocalisée ;
- une liste absente ou vide ne déclenche aucun proxy géolocalisé implicite ;
- les pays décrivent la disponibilité de la vidéo, pas le pays du catalogue,
  de l'interface ou d'une requête API auxiliaire.

Le flux complet devient :

```text
Réponse catalogue / détail / entitlement de la plateforme
  -> extraction YAML des pays de disponibilité de la vidéo
  -> players[].resolver.proxy.countries
  -> normalisation frontend (proxyCountries: string[])
  -> get_stream(proxy_countries: string[])
  -> résolveur spécifique ou scraper-query
  -> configuration HTTP ou URL proxy avec la liste
  -> inventaire proxy : chargement et sélection parmi les pays demandés
```

## Choix de sélection proxy recommandé

La liste est une liste de pays admissibles, pas une obligation de posséder un
proxy dans tous les pays.

L'ordre extrait ou défini par la plateforme doit être conservé comme ordre de
préférence :

1. rechercher puis sélectionner un proxy exploitable pour le premier pays ;
2. en cas d'absence de candidat, de cache négatif, d'échec de chargement ou de
   proxy non éligible, essayer le pays suivant ;
3. retourner une erreur seulement lorsqu'aucun pays de la liste ne produit de
   proxy exploitable.

Ce comportement permet au proxy de prendre en compte les proxies de tous les
pays fournis, tout en évitant qu'un pays secondaire soit choisi aléatoirement
alors que le pays prioritaire est disponible.

Les caches négatifs, les chargements concurrents et la persistance restent
indexés par pays. Une requête multi-pays doit donc pouvoir ignorer un pays en
cache négatif sans empêcher l'essai des autres pays.

## Contrat mis en œuvre

### Proxy

L'API interne doit accepter plusieurs pays, par exemple :

- `ProxyLoadRequest { countries: Vec<String> }` ;
- une méthode `ProxyInventory::select_any_for_destination(countries, ...)` ;
- un marqueur de pool multi-pays ou une représentation de paramètres capable
  de conserver la liste jusqu'à la résolution asynchrone ;
- un en-tête HTTP dédié, par exemple `Arachnea-Proxy-Countries`, encodé de
  manière non ambiguë.

Les méthodes mono-pays existantes doivent rester comme façades vers la nouvelle
API avec une liste d'un élément afin de limiter les régressions dans les
appels Rust existants.

### Scrapyfy et requêtes HTTP

`ScraperHttpConfig` est complété par `proxy_countries`, avec repli de lecture
depuis `proxy_country`. La liste est interpolable par les templates YAML sous
forme JSON et n'est pas confondue avec une chaîne CSV brute.

Les requêtes de sources de proxys peuvent rester mono-pays. Le provider doit
simplement les appeler pour chaque pays demandé et ne conserver que les
enregistrements associés aux pays demandés.

### Backend stream et frontend

Le contrat de lecteur a évolué de :

```text
resolver.proxy.country -> proxyCountry?: string -> proxy_country?: string
```

vers :

```text
resolver.proxy.countries -> proxyCountries?: string[] -> proxy_countries?: string[]
```

Les types frontend concernés comprennent notamment :

- `front/public-app/src/types/entry.ts` ;
- `front/public-app/src/types/home.ts` ;
- `front/public-app/src/services/rustify.ts`.

`GetStreamRequest` et `resolve_scraper_query_stream` acceptent cette liste. Les
URLs média générées par les résolveurs spécifiques transmettent la même liste
que leurs requêtes de négociation lorsque celles-ci sont géolocalisées.

## Compatibilité

Le champ historique `resolver.proxy.country`, la propriété frontend
`proxyCountry`, le paramètre `proxy_country` et l'en-tête
`Arachnea-Proxy-Country` doivent être lus pendant la transition comme une liste
à un élément.

Les nouvelles données produites par les YAML doivent utiliser
`resolver.proxy.countries`. Lorsque les formes simple et multiple sont toutes
deux présentes, la forme multiple doit être prioritaire afin de conserver une
sémantique déterministe.

La documentation doit indiquer explicitement cette période de compatibilité et
la règle de priorité.

## Sources à adapter

### Tableau de décision par service

| Service | Lecteurs/résolveur | État de propagation technique | Source de droits observée | Règle de pays actuellement justifiée | Action restante |
|---|---|---|---|---|---|
| France TV | `francetv-video`, `francetv-live` | Prête : la liste atteint K7, le jeton de manifeste, l'URL média proxifiée et la licence Widevine différée. | Aucune dans les réponses catalogue consommées ; les droits doivent être relevés dans K7 ou une réponse player. | Aucune. Ne pas injecter `FR` dans les lecteurs. | Capturer le même `si_id` avec plusieurs sorties pays et relever le schéma K7, les erreurs et les URLs/manifestes. |
| M6 Play | `m6play-video` | Complète pour les zones connues : le YAML extrait `/clips/0/areas/*/zone_id`, construit `resolver.proxy.countries` et le résolveur propage la liste vers la négociation et le média proxifié. | `areas` au niveau de la vidéo, vérifié sur des payloads réels. | `areas=11` → `[AD, FR, GP, GF, MQ, YT, MC, NC, PF, RE, BL, MF, PM, TF, WF]` ; `areas=34` → absence de proxy. | Confirmer toute nouvelle valeur avant de l'ajouter au mapping. |
| TF1+ | Résolveur TF1+ | Prête : les négociations, URLs média et DRM reçoivent le contexte demandé. | Non relevée dans une fixture exploitable. | Aucune. | Capturer le nœud GraphQL vidéo et le média-info pour des contenus aux droits différents. |
| TV5MONDE+ | Résolveur TV5MONDE+ | Prête : le contexte demandé est propagé à la négociation, au média et au DRM. | Non relevée dans un détail d'asset/entitlement exploitable. | Aucune. | Capturer le détail d'asset et l'entitlement `/play`, notamment pour des contenus disponibles dans plusieurs pays. |
| Arte | `scraper-query` | Prête : Scrapyfy accepte `http.proxy_countries: ["{proxy_countries}"]` et les URLs de manifestes peuvent conserver ce contexte. | Non relevée dans les réponses EMAC utilisées. | Aucune, sauf le live qui portait historiquement `FR` et doit être revu avec des données. | Relever les champs de droits/territoires de programme et de player config. |
| RTBF Auvio | Résolveur RTBF Auvio | Propagation multi-pays ajoutée au résolveur affecté. | Non analysée dans cette phase. | Aucune. | Analyser les réponses de lecture et les éventuels géoblocages avant d'émettre une liste. |
| RTL Play | Résolveur RTL Play | Propagation multi-pays ajoutée au résolveur affecté. | Non analysée dans cette phase. | Aucune. | Analyser les réponses de lecture et les éventuels géoblocages avant d'émettre une liste. |
| Antenne Réunion | Résolveur Antenne Réunion | Propagation multi-pays ajoutée au résolveur affecté. | Non analysée dans cette phase. | Aucune. | Analyser les réponses de lecture et les éventuels géoblocages avant d'émettre une liste. |

Une ligne marquée « aucune » signifie qu'aucune contrainte ne doit être
fabriquée : le lecteur laisse `resolver.proxy.countries` absent jusqu'à ce que
la plateforme fournisse une donnée de droits suffisamment précise.

### France TV

`server/services/arachnea-stream/legal-stream/francetv.yaml` construit des
lecteurs à partir de `si_id`. Le résolveur
`server/crates/arachnea-stream/src/services/francetv_resolver.rs` :

- envoie `country_code`, configuré à `FR` dans le YAML, à l'API K7
  `https://k7.ftven.fr/videos/{si_id}` ;
- construit un `ScraperHttpConfig` avec les `proxy_countries` reçus ;
- emploie ce client pour K7, pour la signature du manifeste et pour le jeton
  Widevine ;
- crée une URL média avec `proxied_url_with_countries` ;
- sauvegarde les mêmes pays dans le contexte de licence différée et les
  réemploie lors du `POST` Widevine.

Les trois emplacements YAML qui créent un lecteur France TV — cartes de
programme, détail de contenu et direct — ne renseignent encore que `kind` et
`target_id`. Ils n'émettent donc pas `resolver.proxy.countries`.

#### Analyse des données France TV disponibles

Le 28 septembre 2026, la réponse publique
`https://api-mobile.yatta.francetv.fr/generic/directs?platform=apps` a été
inspectée. Elle fournit des éléments de catalogue avec `si_id` (par exemple
`0b50c747-116b-4749-9243-abc087177ac9`) et les métadonnées éditoriales
attendues. Une recherche récursive des clés contenant `geo`, `country`,
`territor`, `right`, `restrict`, `avail` ou `block` n'a trouvé que
`ads_blocked`; aucun champ de territoire, de pays ou de droits vidéo n'est
présent dans les éléments consultés. Cette réponse ne peut donc pas déterminer
une liste de proxy par vidéo.

Le dépôt confirme cette séparation : le YAML de catalogue ne consomme que les
objets Yatta et transmet le `si_id`, tandis que le résolveur demande ensuite la
réponse K7 avec un `country_code`. L'implémentation de référence locale
Catch-up TV & More adopte le même modèle : elle envoie le pays géolocalisé du
client à K7, avec un repli sur `FR`, puis exploite `video.url`, `video.format`,
`video.drm` et `video.token`. Elle ne fournit pas de liste de territoires de
droits. Cela démontre que `country_code` est une entrée de négociation ; cela
ne démontre pas que `FR` soit autorisé pour toutes les vidéos.

#### Conclusion France TV et plan de relevé

La propagation multi-pays France TV est terminée, mais la décision par vidéo
ne l'est pas. À ce stade, laisser `resolver.proxy.countries` absent est le seul
comportement fondé sur les données disponibles. Ajouter systématiquement
`[FR]`, ou dériver une liste du paramètre `country_code`, créerait une règle de
droits non vérifiée.

Pour finaliser le mapping, capturer et versionner des fixtures anonymisées pour
au moins :

1. un replay sans DRM, un replay DRM et un direct ;
2. un contenu supposé accessible et un contenu géobloqué ;
3. chaque même `si_id` interrogé avec au moins `FR`, `BE` et `US`, via des
   sorties proxy effectivement situées dans ces pays ;
4. la réponse K7 complète, le résultat du jeton de manifeste, le manifeste et,
   pour Widevine, le statut et le corps d'erreur non sensible de la licence.

Le relevé devra comparer statut HTTP, structure JSON, présence de `video`,
URL/format/DRM/token, codes et messages d'erreur, et toute clé de droits ou de
territoire. Une liste ne pourra être émise dans le YAML que si la réponse
identifie explicitement les territoires autorisés, ou si la matrice de réponses
par pays établit une règle stable et documentée. Les caches de manifeste et de
licence devront alors être vérifiés pour leur isolation par liste de pays.

### M6 Play

`server/services/arachnea-stream/legal-stream/m6play-fr.yaml` demande déjà
`with=links,subcats,rights` dans `get_entry`, mais n'extrait aucun champ de
`rights`. Les joueurs des épisodes sont construits avec le résolveur
`m6play-video`.

Le résolveur
`server/crates/arachnea-stream/src/services/m6play_resolver.rs` propage la
liste reçue vers la négociation et les URLs de manifeste ; il ne doit plus
imposer un pays indépendamment du lecteur.

La géolocalisation M6 Play est portée par `areas` au niveau de la vidéo. Les
valeurs actuellement connues sont :

| Valeur `areas` | Règle de proxy |
|---|---|
| `34` | Ne pas demander de proxy géolocalisé. |
| `11` | Demander, dans l'ordre, `resolver.proxy.countries: [AD, FR, GP, GF, MQ, YT, MC, NC, PF, RE, BL, MF, PM, TF, WF]`. |

Le YAML extrait désormais `/clips/0/areas/*/zone_id` dans les lecteurs
`m6play-video`. Une action partagée mappe `11` vers la liste ordonnée de pays,
puis la divise en `resolver.proxy.countries`. La valeur `34`, les valeurs
absentes et les valeurs inconnues sont supprimées du résultat : elles ne
produisent donc aucune contrainte géographique implicite. Toute nouvelle valeur
`areas` doit être confirmée avant d'être ajoutée au mapping.

### Arte

`server/services/arachnea-stream/legal-stream/arte-fr.yaml` crée des lecteurs
`scraper-query` pour les programmes, épisodes, trailers et clips. Seul le
lecteur live déclare aujourd'hui `resolver > proxy > country: FR`.

La query `resolve_stream` peut désormais utiliser
`http.proxy_countries: ["{proxy_countries}"]`. La liste est propagée dans les
URLs de manifestes, y compris lors de la réécriture des playlists HLS enfant.

Il faut identifier les champs de disponibilité par territoire dans les réponses
EMAC/programme ou dans le player config afin de renseigner chaque lecteur, pas
seulement le direct.

### TF1+

`server/services/arachnea-stream/legal-stream/tf1-fr.yaml` construit les
lecteurs depuis les nœuds `video` GraphQL. Les droits territoriaux ne sont pas
encore extraits.

`server/crates/arachnea-stream/src/services/tf1_resolver.rs` propage le
contexte multi-pays aux requêtes de résolution, aux URLs média et aux requêtes
DRM différées.

Il faut relever le champ de disponibilité par pays dans les nœuds vidéo
GraphQL, l'écrire dans `resolver.proxy.countries`, l'utiliser pour les appels
de négociation et pour le manifeste, la licence et les ressources associées
lorsqu'elles nécessitent le même contexte géographique.

### TV5MONDE+

`server/services/arachnea-stream/legal-stream/tv5mondeplus-fr.yaml` possède
`allowed_country: US`, mais ce paramètre est utilisé pour les endpoints de
catalogue. Il ne doit pas être interprété comme le pays de disponibilité de
l'asset.

Les lecteurs `tv5mondeplus-video` sont créés depuis les assets, tandis que
`server/crates/arachnea-stream/src/services/tv5mondeplus_resolver.rs` effectue
une authentification anonyme puis une requête entitlement. Le contexte
multi-pays demandé est maintenant propagé à ces appels, au média et au DRM.

Il faut identifier les territoires disponibles dans le détail d'asset ou dans
la réponse entitlement, les extraire au niveau du lecteur et fournir cette
liste au résolveur pour les appels qui déterminent ou consomment la lecture.

## Données API à relever avant l'implémentation YAML finale

Le dépôt ne contient pas de captures JSON représentatives des cinq APIs. Les
JSON Pointers exacts ne peuvent donc pas être déduits de manière fiable à ce
stade. Il faut collecter, pour une vidéo disponible dans plusieurs pays et une
vidéo plus restreinte, les réponses suivantes :

| Plateforme | Réponse à relever | Donnée recherchée |
|---|---|---|
| France TV | détail de contenu et réponse K7/player | codes de disponibilité/territoires de la vidéo |
| M6 Play | `get_entry` avec `rights`, liste d'épisodes/clips et payload vidéo | nouvelles valeurs possibles de `areas` ; `34` signifie sans proxy et `11` correspond à `AD, FR, GP, GF, MQ, YT, MC, NC, PF, RE, BL, MF, PM, TF, WF` |
| Arte | EMAC programme/épisode et player config | territoires ou contraintes de lecture par programme |
| TF1+ | nœud `video` GraphQL et médiainfo | disponibilité pays ou droits de diffusion |
| TV5MONDE+ | détail asset et entitlement `/play` | pays/territoires autorisés pour l'asset |

Les pointeurs YAML ne doivent être ajoutés qu'après vérification de ces payloads
réels. Les données absentes, invalides ou ambiguës doivent aboutir à l'absence
de `resolver.proxy.countries`, pas à un pays fixe inventé.

## Fichiers susceptibles d'être modifiés lors de l'implémentation

### Backend proxy et scraping

- `server/crates/arachnea-proxy/src/core/proxy_record.rs`
- `server/crates/arachnea-proxy/src/core/proxy_inventory.rs`
- `server/crates/arachnea-proxy/src/core/parameters.rs`
- `server/crates/arachnea-proxy/src/core/http/proxy_service.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs`
- `server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs`

### Backend streaming

- `server/crates/arachnea-stream/src/stream_scraper.rs`
- `server/crates/arachnea-stream/src/stream_resolver.rs`
- les résolveurs France TV, M6 Play, TF1+ et TV5MONDE+ ;
- les YAML des cinq plateformes concernées.

### Frontend et documentation

- `front/public-app/src/types/entry.ts`
- `front/public-app/src/types/home.ts`
- `front/public-app/src/services/rustify.ts`
- `docs/specifications/arachnea-scrapyfy-en.md`
- `docs/specifications/arachnea-scrapyfy-fr.md`
- `docs/specifications/arachnea-stream-en.md`
- `docs/specifications/arachnea-stream-fr.md`
- documentation `arachnea-proxy` pertinente ;
- `docs/TODO.md` et `CHANGELOG.md`.

## Validation attendue

Sans introduire une nouvelle infrastructure de test, adapter les tests existants
touchés pour vérifier :

1. normalisation, déduplication et rejet des codes pays invalides ;
2. compatibilité avec un seul pays historique ;
3. sélection ordonnée avec repli entre plusieurs pays ;
4. chargements, caches négatifs et erreurs isolés par pays ;
5. propagation inchangée de la liste depuis le YAML, via le frontend et
   `get_stream`, jusqu'à la configuration ou l'URL proxy ;
6. conservation du comportement sans proxy géolocalisé lorsqu'aucune liste
   vidéo n'est fournie ;
7. désérialisation des cinq YAML et des payloads de test disponibles.

Les vérifications pertinentes après implémentation seront au minimum :

```sh
cd /Users/jdecker/Downloads/Arachnea/server
cargo test -p arachnea-proxy -p arachnea-scrapyfy -p arachnea-stream
```

## Décision à confirmer avant l'implémentation

L'architecture et le contrat multi-pays sont déterminés par cette analyse. Les
listes de pays effectivement extraites pour chaque plateforme restent à
finaliser après inspection de réponses API réelles, car les noms et structures
des champs de droits ne sont pas présents dans le dépôt.