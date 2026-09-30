# Plan de migration vers le contrat proxy multi-pays unique

Date : 29 septembre 2026

## Objectif

Supprimer le contrat historique mono-pays et conserver une seule représentation
du routage géographique par proxy dans l'ensemble du dépôt.

Le portage doit être réalisé de bout en bout : tous les producteurs,
consommateurs, configurations, exemples et documents présents dans le dépôt
doivent être convertis vers le contrat pluriel. Il ne suffit pas de cesser
d'accepter les anciens champs sans migrer leurs appelants.

Cette migration doit également corriger le traitement du manifeste M6+ afin que
l'action exécutée lors d'une redirection HTTP 302 cible le même en-tête que celui
présent dans les options de l'URL proxy.

## Contrat final unique

| Niveau | Contrat conservé |
| --- | --- |
| En-tête HTTP | `Arachnea-Proxy-Countries` |
| Paramètre proxy interne | `countries` |
| Valeur transportée | Tableau JSON ordonné, par exemple `["FR"]` ou `["FR", "DE"]` |
| Configuration Scrapyfy | `proxy_countries` |
| Paramètre `get_stream` | `proxy_countries` |
| Modèle frontend | `proxyCountries` |
| Descripteur YAML | `resolver.proxy.countries` |
| Template runtime YAML | `{proxy_countries}` |

Les formes suivantes doivent être supprimées du code actif :

- `Arachnea-Proxy-Country` ;
- le paramètre proxy interne `country` ;
- `proxy_country` ;
- `proxyCountry` ;
- `resolver.proxy.country` ;
- `{proxy_country}` ;
- les constantes, champs et helpers réservés à la forme singulière.

Un pays unique doit être représenté par une liste à un élément, par exemple
`["FR"]`.

## Principes de migration

1. Porter les producteurs internes avant de retirer les anciens symboles.
2. Maintenir le dépôt compilable aux principaux points intermédiaires.
3. Ne pas introduire d'alias silencieux entre les anciens et nouveaux contrats.
4. Préserver l'ordre des pays comme ordre de préférence de routage.
5. Normaliser les codes ISO alpha-2 en majuscules et supprimer les doublons.
6. Ne pas modifier manuellement les fichiers frontend générés sous `front/dist/`.
7. Préserver les modifications locales déjà présentes dans le dépôt.

## Phase 1 — Porter les producteurs vers le contrat pluriel

Cette phase peut être réalisée alors que les symboles historiques existent
encore temporairement. Elle limite les ruptures de compilation intermédiaires.

### 1.1 Frontend

Fichiers principaux :

- `front/public-app/src/services/rustify.ts` ;
- `front/public-app/src/types/entry.ts` ;
- `front/public-app/src/types/home.ts`.

Modifications :

- supprimer `proxyCountry` des interfaces TypeScript ;
- ne lire que `resolver.proxy.countries` ;
- supprimer les fallbacks suivants :
  - `resolver.proxy.country` ;
  - `resolverProxyCountry` ;
  - `resolverProxy.country` ;
- simplifier `normalizeProxyCountries` pour ne recevoir qu'une liste ;
- envoyer uniquement le champ `proxy_countries` à `get_stream` ;
- ne plus envoyer le premier pays comme `proxy_country` historique.

L'appel final doit avoir cette forme :

```ts
const response = await call_api<unknown>('get_stream', {
  resolver: player.resolver.kind,
  target: player.resolver.targetId,
  source: player.resolver.source,
  proxy_countries: player.resolver.proxyCountries,
  proxy_rewrite_manifest_urls: player.resolver.proxyRewriteManifestUrls,
})
```

Les fichiers générés sous `front/dist/` ne doivent pas être modifiés
manuellement.

### 1.2 API `get_stream`

Fichiers principaux :

- `server/crates/arachnea-stream/src/stream_scraper.rs` ;
- `server/crates/arachnea-stream/src/reloadable_stream_scraper.rs`.

Modifications :

- retirer `GetStreamRequest.proxy_country` et son alias `proxyCountry` ;
- conserver uniquement le champ pluriel :

```rust
#[serde(default, alias = "proxyCountries")]
proxy_countries: Vec<String>,
```

- retirer le paramètre `proxy_country: Option<String>` de `get_stream` ;
- simplifier `normalize_proxy_countries` pour normaliser uniquement la liste
  reçue ;
- supprimer la fusion actuelle entre `proxy_countries` et `proxy_country` ;
- adapter l'enregistrement de la fonction `get_stream` dans le scraper
  rechargeable.

### 1.3 Configurations Scrapyfy et YAML

Fichiers identifiés :

- `server/services/arachnea-stream/legal-stream/antennereunion-fr.yaml` ;
- `server/services/arachnea-stream/legal-stream/ln24-be.yaml`.

Une configuration statique mono-pays :

```yaml
proxy_country: BE
```

doit devenir :

```yaml
proxy_countries: [BE]
```

Un template runtime mono-pays :

```yaml
proxy_country: "{proxy_country}"
```

doit devenir :

```yaml
proxy_countries: ["{proxy_countries}"]
```

Tous les paramètres alimentant ces templates doivent eux-mêmes fournir une
liste JSON ordonnée.

## Phase 2 — Simplifier `ScraperHttpConfig`

Fichiers principaux :

- `server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs` ;
- `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` ;
- les résolveurs de stream utilisant la configuration HTTP.

Modifications :

- supprimer le champ `proxy_country: Option<String>` ;
- supprimer le builder `proxy_country(...)` ;
- supprimer `proxy_country_hint()` ;
- supprimer le fallback mono-pays de `proxy_countries_hint()` ;
- conserver uniquement `proxy_countries: Vec<String>` ;
- conserver uniquement le builder `proxy_countries(...)` ;
- mettre à jour la fusion parent/enfant pour ne traiter que la liste ;
- interpoler uniquement les éléments de `proxy_countries` ;
- supprimer les tests de compatibilité singulière devenus obsolètes ;
- remplacer les appelants mono-pays par une liste à un élément.

Par exemple :

```rust
ScraperHttpConfig::default().proxy_country(TF1_PROXY_COUNTRY)
```

doit devenir :

```rust
ScraperHttpConfig::default().proxy_countries(vec![TF1_PROXY_COUNTRY.to_string()])
```

Dans `scraper_agregator.rs`, l'activation de l'observation dynamique doit être
basée uniquement sur la liste de pays et l'affinité :

```rust
if !http_config.proxy_countries.is_empty() || http_config.proxy_affinity.is_some()
```

## Phase 3 — Rendre le proxy core exclusivement pluriel

Fichiers principaux :

- `server/crates/arachnea-proxy/src/core/parameters.rs` ;
- `server/crates/arachnea-proxy/src/core/mod.rs` ;
- `server/crates/arachnea-proxy/src/core/tests/core_tests.rs` ;
- `server/crates/arachnea-proxy/src/server/handlers/http.rs` ;
- `server/crates/arachnea-proxy/src/server/handlers/socks.rs` ;
- `server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs`.

### 3.1 Constantes

Supprimer :

```rust
PROXY_HEADER_PARAMETER_COUNTRY
PROXY_PARAMETER_COUNTRY
```

Conserver uniquement :

```rust
PROXY_HEADER_PARAMETER_COUNTRIES
PROXY_PARAMETER_COUNTRIES
```

### 3.2 Définition par défaut

Remplacer conceptuellement :

```rust
default_country_parameter_definition()
```

par :

```rust
default_countries_parameter_definition()
```

La définition doit utiliser exclusivement :

```rust
ParameterDefinition::new(
    PROXY_HEADER_PARAMETER_COUNTRIES,
    PROXY_PARAMETER_COUNTRIES,
    false,
)
```

`ParameterRegistry::with_defaults()` doit enregistrer uniquement cette
définition.

### 3.3 Handler statique

`CountryRoutingProxyHandler` peut conserver son nom : il représente toujours un
routage par pays, même si sa valeur d'entrée est une liste.

Son comportement doit devenir :

1. lire le tableau JSON `countries` ;
2. normaliser et dédupliquer les codes ;
3. parcourir les pays dans leur ordre de déclaration ;
4. sélectionner la première route statique configurée ;
5. ne sélectionner aucune route si aucun pays n'est reconnu.

Ainsi, la valeur suivante :

```json
["FR", "DE"]
```

sélectionne la route FR si elle existe, sinon la route DE.

### 3.4 Handler dynamique

`DynamicCountryRoutingProxyHandler` accepte déjà la liste plurielle. Il faut :

- supprimer sa lecture de secours du paramètre `country` ;
- ne déclarer que `countries` dans `parameter_definitions()` ;
- retirer toute branche de compatibilité singulière ;
- conserver l'ordre de préférence des pays.

### 3.5 Configuration Scrapyfy du proxy core

Dans `proxy_provider.rs`, remplacer :

```rust
parameter_name: Some(PROXY_PARAMETER_COUNTRY.to_string()),
http_header: Some(PROXY_HEADER_PARAMETER_COUNTRY.to_string()),
```

par :

```rust
parameter_name: Some(PROXY_PARAMETER_COUNTRIES.to_string()),
http_header: Some(PROXY_HEADER_PARAMETER_COUNTRIES.to_string()),
```

La documentation associée doit préciser que la valeur est un tableau JSON
ordonné.

## Phase 4 — Unifier les helpers d'URL proxy

Fichier principal :

- `server/crates/arachnea-proxy/src/core/http/proxy_service.rs`.

Le fichier expose encore plusieurs helpers prenant un pays unique :

- `proxied_url` ;
- `proxied_url_with_options` ;
- `proxied_url_with_insecure_tls` ;
- le constructeur interne recevant `country: Option<&str>`.

Le portage doit :

1. migrer leurs appelants vers une liste ;
2. faire du constructeur pluriel le chemin canonique ;
3. retirer les paramètres `country: Option<&str>` ;
4. supprimer la génération de `Arachnea-Proxy-Country` ;
5. générer exclusivement un tableau JSON sous
   `Arachnea-Proxy-Countries` ;
6. centraliser la normalisation, la déduplication, la sérialisation JSON,
   l'ajout des actions, les options de redirection et les options TLS.

Les appelants identifiés incluent notamment :

- `server/crates/arachnea-stream/src/services/antennereunion_resolver.rs` ;
- `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/resolve_url.rs`.

Le résultat ne doit plus comporter deux chemins parallèles de construction des
options selon qu'un ou plusieurs pays sont fournis.

## Phase 5 — Corriger spécifiquement M6+

Fichier :

- `server/crates/arachnea-stream/src/services/m6play_resolver.rs`.

Remplacer l'import singulier :

```rust
use arachnea_proxy::PROXY_HEADER_PARAMETER_COUNTRY;
```

par :

```rust
use arachnea_proxy::PROXY_HEADER_PARAMETER_COUNTRIES;
```

Puis remplacer :

```rust
RemoveHeader::on_http302([
    PROXY_HEADER_PARAMETER_COUNTRY,
    REMOVE_HEADER_ACTION_HEADER,
])
```

par :

```rust
RemoveHeader::on_http302([
    PROXY_HEADER_PARAMETER_COUNTRIES,
    REMOVE_HEADER_ACTION_HEADER,
])
```

Cette modification corrige l'incohérence actuelle : le manifeste est demandé
avec `Arachnea-Proxy-Countries`, mais l'action de redirection tente de retirer
un autre en-tête.

Les tests de `proxy_service.rs` utilisant littéralement
`Arachnea-Proxy-Country` doivent être convertis vers
`Arachnea-Proxy-Countries` avec une valeur JSON, par exemple :

```rust
(
    "Arachnea-Proxy-Countries".to_string(),
    r#"["FR"]"#.to_string(),
)
```

## Phase 6 — Porter les exemples et configurations proxy

Fichiers :

- `server/crates/arachnea-proxy/config-sample/parameters-country-routing.toml` ;
- `server/crates/arachnea-proxy/config-sample/routing-country.toml` ;
- `server/crates/arachnea-proxy/examples/country_routing_parameters.rs`.

Exemple HTTP final :

```text
Arachnea-Proxy-Countries: ["US"]
```

Configuration finale :

```toml
parameter_name = "countries"
http_header = "Arachnea-Proxy-Countries"
```

Pour SOCKS, la valeur doit être transmise sous forme JSON encodée dans le nom
d'utilisateur. L'encodage nécessaire doit être documenté correctement au lieu
de conserver l'ancien exemple ambigu `country=US`.

L'exemple Rust doit sérialiser la liste :

```rust
let countries = serde_json::to_string(&vec![country])?;
```

puis utiliser `PROXY_HEADER_PARAMETER_COUNTRIES`.

## Phase 7 — Documentation et annonce de rupture

Fichiers à mettre à jour :

- `docs/specifications/arachnea-scrapyfy-en.md` ;
- `docs/specifications/arachnea-scrapyfy-fr.md` ;
- `docs/specifications/arachnea-stream-en.md` ;
- `docs/specifications/arachnea-stream-fr.md` ;
- `docs/specifications/arachnea-proxies-en.md` ;
- `docs/specifications/arachnea-proxies-fr.md` ;
- `server/crates/arachnea-proxy/README.md` ;
- `docs/dev-tracking/multi-country-video-geo-proxy-analysis.md` ;
- `CHANGELOG.md` ;
- `docs/TODO.md` si les éléments suivis sont affectés.

La documentation doit :

- présenter `proxy_countries` comme unique contrat ;
- retirer les mentions de compatibilité, de fallback historique et de
  transition ;
- préciser que même un pays unique est une liste ;
- expliquer l'ordre de préférence ;
- annoncer que `Arachnea-Proxy-Country` et `proxy_country` ne sont plus
  acceptés ;
- fournir un tableau de migration pour les intégrations externes.

Exemples de migration :

```text
Arachnea-Proxy-Country: FR
```

devient :

```text
Arachnea-Proxy-Countries: ["FR"]
```

```yaml
proxy_country: FR
```

devient :

```yaml
proxy_countries: [FR]
```

```json
{
  "proxy_country": "FR"
}
```

devient :

```json
{
  "proxy_countries": ["FR"]
}
```

La section `Unreleased` du changelog doit signaler explicitement le changement
de contrat. Les anciennes entrées de versions publiées ne doivent pas être
réécrites si elles décrivent fidèlement le comportement historique.

## Phase 8 — Validation

### 8.1 Vérifications Rust intermédiaires

Après chaque phase backend, exécuter au minimum :

```bash
cargo check -p arachnea-proxy
cargo check -p arachnea-scrapyfy
cargo check -p arachnea-stream
```

Puis exécuter les tests existants concernés :

```bash
cargo test -p arachnea-proxy
cargo test -p arachnea-scrapyfy
cargo test -p arachnea-stream
```

Aucune nouvelle infrastructure de test ne doit être ajoutée. Les tests
existants qui utilisent le contrat singulier doivent être adaptés au contrat
pluriel.

### 8.2 Vérifications frontend

Depuis `front/public-app` :

```bash
npm run type-check
npm run build
```

### 8.3 Recherche de résidus

Une recherche finale doit confirmer l'absence des anciens contrats dans les
sources actives :

```bash
grep -RInE \
  'Arachnea-Proxy-Country|PROXY_HEADER_PARAMETER_COUNTRY|PROXY_PARAMETER_COUNTRY|proxy_country|proxyCountry' \
  server front/public-app/src docs
```

Les occurrences historiques dans d'anciennes entrées du changelog peuvent
rester si elles décrivent fidèlement des versions passées. En revanche, la
section `Unreleased`, les spécifications et le code actif ne doivent plus
présenter le singulier comme supporté.

### 8.4 Validation fonctionnelle M6+

Reproduire l'appel suivant avec les logs de debug activés :

```text
/api/proxy/opts_*/https://lbcdn.6cloud.fr/...mpd
```

Vérifier que :

1. les options initiales contiennent uniquement
   `Arachnea-Proxy-Countries` ;
2. la sélection géographique utilise la liste dans l'ordre ;
3. le HTTP 302 vers Bedrock applique l'action attendue ;
4. aucun `Arachnea-Proxy-Country` n'est généré ;
5. aucun fallback interne vers `proxy_country` n'existe encore ;
6. la réponse obtenue correspond au comportement géographique attendu ;
7. une absence de proxy géographique ne soit pas masquée par un contrat
   singulier résiduel.

## Ordre recommandé des commits

1. Porter le frontend, l'API stream et les YAML vers `proxy_countries`.
2. Porter `ScraperHttpConfig` et tous ses appelants.
3. Basculer le proxy core vers `countries` uniquement.
4. Unifier les helpers d'URL proxy et corriger M6+.
5. Porter les exemples, la documentation et le changelog.
6. Effectuer le nettoyage final et les validations globales.

Cet ordre évite de supprimer les anciennes constantes avant que leurs appelants
aient été migrés, tout en garantissant leur disparition complète à la fin du
portage.

## Critères d'acceptation

La migration est terminée lorsque :

- le code actif ne déclare plus `Arachnea-Proxy-Country` ;
- le proxy core ne déclare plus de paramètre interne `country` ;
- Scrapyfy n'expose plus `proxy_country` ;
- l'API `get_stream` n'accepte plus `proxy_country` ou `proxyCountry` ;
- le frontend ne lit et n'émet plus `proxyCountry` ;
- les YAML actifs utilisent exclusivement `proxy_countries` ;
- les URLs proxy transportent une liste JSON sous
  `Arachnea-Proxy-Countries` ;
- l'action M6+ exécutée sur HTTP 302 cible
  `Arachnea-Proxy-Countries` ;
- les trois crates Rust compilent et leurs tests existants passent ;
- le frontend passe le contrôle TypeScript et la compilation Vite ;
- les spécifications et le changelog décrivent le contrat final sans fallback
  historique.