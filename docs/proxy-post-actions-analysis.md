# Analyse du système d'actions post-réponse du proxy

## État actuel

Le proxy HTTP public est implémenté dans `server/crates/arachnea-proxy/src/core/http/proxy_service.rs`.

Le format d'URL supporte déjà un segment optionnel `opts_<BASE64URL_JSON>` :

```text
/<api>/[opts_<base64url-json>/]<protocol>://<host>/<path>
```

Aujourd'hui, `opts` accepte uniquement les champs suivants :

- `headers`
- `cookies`
- `proxy`

Le champ réellement utilisé pour le routage pays est `headers`. `proxied_url` peut encoder le header `Arachnea-Proxy-Country`, puis `handle_proxy_http` le convertit en `ClientContext` grâce aux `ParameterDefinition` du proxy core. Cela permet au handler `country` existant de choisir l'egress réseau.

Le nouveau système d'actions post-réponse doit suivre le même principe : les actions voyagent comme des headers proxy internes et non comme un champ `actions` séparé dans `opts`.

La réponse HTTP est construite dans `server/crates/arachnea-proxy/src/core/http/client.rs` :

1. `request_proxied` ouvre le flux via `ArachneaProxyCore`.
2. La réponse brute est lue entièrement.
3. `parse_http_response` parse le status code, les headers et le body.
4. Si la réponse est en `transfer-encoding: chunked`, le body est décodé et les headers `transfer-encoding` / `content-length` sont retirés.
5. Le point d'extension demandé est déjà signalé par ce commentaire :

```rust
////////////////// Post actions (status_code, headers, body) ///////////////////////
```

Ensuite, `handle_proxy_http` applique encore des traitements de niveau service :

- réécriture du header `Location` pour les redirections ;
- suppression des headers non transférables ;
- extraction de `content-type` dans le champ dédié `ControlerStreamOutput::content_type` ;
- suppression du body pour les requêtes `HEAD`.

Point important : `ControlerStreamOutput` ne définit pas explicitement `Content-Length`. Les backends REST/Tauri construisent une réponse avec le body final et les headers transmis. Si un `content-length` upstream est conservé alors que le body est modifié, il peut devenir faux.

## Problème à résoudre

Le mécanisme actuel ne sait gérer qu'une action conceptuelle de type `country`, et cette action agit avant la requête, sur le routage réseau.

Il faut ajouter un système d'actions post-réponse, capable de recevoir :

- `status_code`
- `headers`
- `body`
- un ou plusieurs paramètres propres à l'action

Le proxy doit accepter plusieurs actions, y compris plusieurs actions du même type. Elles doivent être exécutées selon l'option commune `order` quand elle est fournie.

L'ordre n'est pas déduit de la position des headers. Chaque action peut porter une option commune `order`, optionnelle, qui définit son ordre d'exécution. Les actions sans `order` restent valides et sont exécutées après les actions ordonnées, dans leur ordre de découverte.

La première action à fournir est `ReplaceAll` :

- elle ne s'applique qu'aux réponses textuelles ;
- elle reçoit une expression régulière en paramètre ;
- elle remplace toutes les occurrences trouvées ;
- elle remet le body sous forme binaire après transformation ;
- elle corrige `content-length` après l'exécution des actions.

La fonction `proxied_url` doit aussi permettre de passer des paramètres optionnels pour ajouter ces actions dans l'URL proxifiée.

## Impact attendu

Ce changement modifie le contrat public du proxy HTTP contrôleur :

- les headers proxy internes doivent accepter des actions post-réponse, par exemple `Arachnea-Proxy-ReplaceAll` ;
- `ProxiedHttpRequest` doit transporter ces actions jusqu'au client HTTP core ;
- `parse_http_response` ou un équivalent doit appliquer les actions au moment où les headers et le body sont déjà disponibles ;
- `proxied_url` doit générer des URLs enrichies sans casser les usages existants.

Les appels actuels de `proxied_url` doivent rester compatibles :

- sans actions, l'URL générée doit rester identique au comportement actuel ;
- avec `country`, le routage pays doit continuer à utiliser le header `Arachnea-Proxy-Country` ;
- avec actions, le segment `opts_` doit contenir les headers proxy d'action en plus des headers/cookies éventuels.

Le changement est plus large qu'un correctif local parce qu'il introduit une nouvelle structure d'extension dans `arachnea-proxy/src/core/http/`. Conformément aux règles du dépôt, il faut confirmer l'approche avant implémentation.

## Approche recommandée

### 1. Ajouter un module d'actions HTTP post-réponse

Créer un sous-module dédié dans `server/crates/arachnea-proxy/src/core/http/`, par exemple :

```text
server/crates/arachnea-proxy/src/core/http/actions/
  mod.rs
  replace_all.rs
```

Le fichier `mod.rs` contiendrait le trait commun, les types partagés et le dispatch générique. Chaque action concrète serait dans son propre fichier, comme demandé.

### 2. Définir un trait commun

Proposition de contrat :

```rust
pub trait ProxyHttpPostAction {
    fn post_apply(
        &self,
        status_code: u16,
        headers: &mut HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<Vec<u8>>;
}
```

Le body est pris par valeur et retourné par valeur pour rendre l'ordre des transformations explicite et éviter les mutations ambiguës. Les headers restent mutables, car certaines actions futures pourraient ajuster un header en fonction du body.

Une alternative serait de passer `body: &mut Vec<u8>` et de retourner `Result<()>`. C'est plus direct, mais moins pratique si une action veut reconstruire entièrement le body à partir d'une conversion texte.

### 3. Représenter les actions avec des headers proxy

Les actions doivent utiliser un mécanisme proche de `country` : un header proxy interne par action.

Format HTTP cible :

```text
Arachnea-Proxy-ReplaceAll: {"pattern":"...","replacement":"..."}
```

Dans une URL générée par `proxied_url`, ce header est transporté via le segment `opts_`, dans `headers`, comme le fait déjà `Arachnea-Proxy-Country` :

```json
{
  "headers": [
    ["Arachnea-Proxy-Country", "FR"],
    ["Arachnea-Proxy-ReplaceAll", "{\"pattern\":\"https://origin.example\",\"replacement\":\"/api/proxy/https://origin.example\"}"]
  ]
}
```

La valeur du header d'action est un JSON. Cela évite une grammaire maison fragile avec les RegEx, les `;`, les `=`, les espaces, les backslashes et les caractères non ASCII. Le parseur doit désérialiser ce JSON avant de construire l'action.

Point important : le proxy doit accepter plusieurs actions du même type. HTTP permet les headers répétés, mais le code actuel convertit les headers en `HashMap<String, String>` dans plusieurs couches (`ControlerStreamInput`, `ProxyHttpOpts.headers`, `ProxiedHttpRequest.headers`). Une `HashMap` écrase naturellement les doublons.

Pour préserver les doublons, il faut ajouter un transport interne dédié pour les headers d'action, par exemple :

```rust
pub struct ProxyHttpActionHeader {
    pub name: String,
    pub value: String,
}
```

`handle_proxy_http` extrairait les headers dont le nom commence par `Arachnea-Proxy-` et qui correspondent à une action connue. Pour `opts`, comme JSON ne permet pas de doublons fiables dans un objet, `proxied_url` devra fournir un encodage qui préserve les répétitions.

Comme on garde `headers` pour rester aligné avec `country`, le format retenu est un tableau de paires :

```json
{
  "headers": [
    ["Arachnea-Proxy-Country", "FR"],
    ["Arachnea-Proxy-ReplaceAll", "{\"pattern\":\"first\",\"replacement\":\"one\"}"],
    ["Arachnea-Proxy-ReplaceAll", "{\"pattern\":\"second\",\"replacement\":\"two\"}"]
  ]
}
```

Le format objet peut rester supporté pour compatibilité avec les URLs existantes, mais `proxied_url` doit générer le format tableau dès qu'il ajoute des actions. Les headers `Arachnea-Proxy-*` sont internes au proxy : ils doivent être consommés par le proxy et ne jamais être forwardés vers le serveur upstream.

Format retenu :

```text
Arachnea-Proxy-ReplaceAll: {"pattern":"one","replacement":"first"}
```

Avec l'option commune `order` :

```text
Arachnea-Proxy-ReplaceAll: {"order":10,"pattern":"one","replacement":"first"}
```

`order` est commun à toutes les actions. Il doit être optionnel. Recommandation de tri :

1. actions avec `order`, triées par valeur croissante ;
2. en cas d'égalité, conserver l'ordre de découverte ;
3. actions sans `order`, après les actions ordonnées, dans l'ordre de découverte.

### 4. Transporter les actions jusqu'au client core

Ajouter un champ à `ProxiedHttpRequest` :

```rust
pub post_actions: Vec<ProxyHttpPostActionConfig>,
```

`handle_proxy_http` le remplit en parsant les headers proxy d'action, par exemple `Arachnea-Proxy-ReplaceAll`. Les erreurs de parsing ou de validation d'action doivent remonter jusqu'à la route proxy et produire une réponse `502` avec un message explicite.

`SimpleHttpClient::request_proxied` transmet ensuite ces actions à la phase de parsing/application. Le plus petit changement consiste à remplacer :

```rust
parse_http_response(&response_bytes)
```

par :

```rust
parse_http_response(&response_bytes, &request.post_actions)
```

### 5. Appliquer les actions au point d'extension prévu

Dans `parse_http_response`, après le décodage chunked et au niveau du commentaire existant :

1. trier les actions selon `order`, puis les exécuter dans cet ordre ;
2. laisser chaque action recevoir `status_code`, `headers` et `body` ;
3. après toutes les actions, supprimer `transfer-encoding` si présent ;
4. écrire `content-length` avec la taille du body final ;
5. retourner `ProxiedHttpResponse`.

La correction de `content-length` doit être globale, après toutes les actions, pas dans chaque action. Cela évite les erreurs quand plusieurs transformations s'enchaînent.

### 6. Implémenter `ReplaceAll`

`ReplaceAll` devrait :

1. vérifier que la réponse est textuelle ;
2. déterminer l'encodage du body à partir de la réponse ;
3. compiler le RegEx fourni ;
4. appliquer `replace_all` ;
5. réencoder le texte transformé dans l'encodage de sortie retenu ;
6. retourner les bytes finaux.

Détection textuelle recommandée :

- `content-type` commence par `text/` ;
- ou `content-type` contient un type connu textuel : `application/json`, `application/javascript`, `application/xml`, `application/xhtml+xml`, `image/svg+xml`, `application/vnd.apple.mpegurl`, `application/dash+xml`.

Si le type n'est pas textuel, l'action doit laisser le body inchangé.

La stratégie d'encodage doit tenir compte du serveur upstream. Proposition :

1. Lire le `charset` du header `content-type`, par exemple `text/html; charset=ISO-8859-1`.
2. Si le charset est connu, décoder le body avec cet encodage.
3. Si aucun charset n'est fourni, tenter UTF-8.
4. Si UTF-8 échoue, tenter une détection/fallback compatible anciens serveurs, avec au minimum Windows-1252 puis ISO-8859-1.
5. Réencoder dans l'encodage réellement utilisé pour décoder, sauf si l'encodage n'est pas supporté en sortie.

Approche recommandée : ajouter `encoding_rs`, qui couvre UTF-8, Windows-1252, ISO-8859-* et les encodages web courants. Cela colle mieux au comportement des navigateurs que de supposer UTF-8.

Si aucun décodage fiable n'est possible, l'action doit laisser le body inchangé et tracer un debug/warn plutôt que renvoyer une erreur. Certains serveurs anciens annoncent mal leur encodage ; un proxy de média doit éviter de transformer une réponse incertaine en échec dur.

### 7. Ajouter la dépendance `regex`

`arachnea-proxy` n'a pas actuellement de dépendance `regex`. Il faudra l'ajouter dans `server/Cargo.toml` ou directement dans `server/crates/arachnea-proxy/Cargo.toml`, selon le style workspace existant.

Le pattern doit être compilé à l'exécution depuis les paramètres de l'action. Une erreur de RegEx invalide doit produire une erreur de proxy claire.

La syntaxe à documenter est celle de la crate Rust `regex`. Les fonctionnalités PCRE non supportées par cette crate, comme certaines formes de lookaround ou backreferences, ne doivent pas être promises.

### 8. Étendre `proxied_url`

La signature actuelle est :

```rust
pub fn proxied_url(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    country: Option<&str>,
) -> String
```

Changer directement la signature pour accepter les paramètres optionnels d'actions :

```rust
pub fn proxied_url(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    country: Option<&str>,
    actions: &[ProxyHttpPostActionConfig],
) -> String
```

Tous les appels existants devront être mis à jour avec `&[]` lorsqu'ils n'ont pas d'action. Cette option rend le changement explicite et évite d'introduire une fonction parallèle.

### 9. Encodage combiné des headers et actions

Quand `country` et `actions` sont présents ensemble, `proxied_url` doit générer un seul segment `opts_` contenant les headers proxy internes.

Le parseur peut continuer à accepter l'objet actuel pour compatibilité, mais `proxied_url` doit utiliser le format tableau de paires dès qu'il y a au moins une action. Cela évite de créer deux comportements selon le nombre d'actions et garantit que les doublons restent possibles :

```json
{
  "headers": [
    ["Arachnea-Proxy-Country", "FR"],
    ["Arachnea-Proxy-ReplaceAll", "{\"order\":10,\"pattern\":\"one\",\"replacement\":\"first\"}"],
    ["Arachnea-Proxy-ReplaceAll", "{\"order\":20,\"pattern\":\"two\",\"replacement\":\"second\"}"]
  ]
}
```

Il ne faut pas créer plusieurs segments `opts_`, car le parser actuel n'en lit qu'un seul en première position. Le même segment doit transporter les headers applicatifs, le pays et les actions.

## Points d'attention

### Compression upstream

Le client actuel ne décode que `transfer-encoding: chunked`. Il ne décompresse pas `content-encoding: gzip`, `br` ou `deflate`.

Pour que `ReplaceAll` fonctionne correctement, il faut soit :

- éviter d'envoyer `Accept-Encoding` vers l'upstream dans les requêtes proxifiées ;
- soit ajouter une gestion de décompression/recompression, ce qui est nettement plus large.

Approche recommandée pour cette première étape : filtrer ou forcer `Accept-Encoding: identity` pour les requêtes qui ont des post-actions textuelles. Cela limite la portée et évite de manipuler des bodies compressés.

### Validation des actions

Une action invalide doit faire échouer la requête proxifiée avec une réponse `502` côté route proxy et un message d'erreur explicite. Cela concerne notamment :

- JSON invalide dans `Arachnea-Proxy-ReplaceAll` ;
- champ `pattern` manquant ou vide ;
- champ `replacement` manquant ;
- type JSON invalide pour `order`, `pattern` ou `replacement` ;
- RegEx invalide.

Il n'y a pas de limite supplémentaire spécifique aux actions à ajouter dans cette première version. La limite existante sur la taille complète de l'URL proxifiée reste la protection retenue.

### Headers liés au body

Après modification du body, certains headers peuvent devenir faux :

- `content-length`
- `etag`
- `content-md5`
- `digest`

Décision retenue : quand au moins une action modifie réellement le body, supprimer `etag`, `content-md5` et `digest`, puis recalculer `content-length` sur le body final.

### `content-type` dans `ControlerStreamOutput`

`handle_proxy_http` retire `content-type` des headers et le place dans `content_type`. Les actions exécutées dans `client.rs` verront encore le header original, ce qui est le bon moment pour déterminer si la réponse est textuelle.

### Requêtes `HEAD`

`request_proxied` lit uniquement les headers pour `HEAD`, puis `parse_http_response` reçoit un body vide. Les actions post-réponse ne devraient pas modifier un body `HEAD`. La correction de `content-length` doit être prudente : pour `HEAD`, le `content-length` upstream peut décrire la réponse d'un `GET`, pas le body transmis. Comme `parse_http_response` ne connaît pas la méthode actuellement, il faudrait soit :

- passer la méthode ou un booléen `headers_only` à l'application des actions ;
- soit ne pas corriger `content-length` dans le client pour `HEAD` et laisser le handler final vider le body.

Approche recommandée : transmettre `headers_only` à `parse_http_response` et ne pas appliquer les actions body pour `HEAD`.

## Plan d'implémentation proposé

1. Ajouter `actions/mod.rs` et `actions/replace_all.rs` dans `arachnea-proxy/src/core/http/`.
2. Définir `ProxyHttpPostAction`, `ProxyHttpPostActionConfig`, `ProxyHttpActionHeader` et une fonction `apply_post_actions`.
3. Ajouter `regex` et `encoding_rs` aux dépendances.
4. Ajouter `post_actions: Vec<ProxyHttpPostActionConfig>` à `ProxiedHttpRequest`.
5. Étendre la désérialisation de `ProxyHttpOpts.headers` pour accepter l'objet existant ou un tableau ordonné de paires.
6. Parser les headers `Arachnea-Proxy-ReplaceAll` en actions post-réponse depuis des valeurs JSON.
7. Passer les actions parsées de `handle_proxy_http` vers `ProxiedHttpRequest`.
8. Trier les actions selon `order`, puis appeler `apply_post_actions` au commentaire prévu dans `client.rs`.
9. Corriger `content-length`, retirer `transfer-encoding`, et supprimer `etag`, `content-md5`, `digest` si le body a changé.
10. Changer directement `proxied_url` pour accepter une liste d'actions.
11. Mettre à jour la rustdoc et les appels existants.
12. Ajouter l'entrée correspondante dans `server/CHANGELOG.md` lors de l'implémentation.

## Validation

Les consignes du dépôt demandent de ne pas créer de nouveaux tests par défaut. Si l'implémentation est confirmée, il faudra au minimum exécuter les checks existants pertinents :

```sh
cargo check -p arachnea-proxy
```

Vérifications manuelles recommandées :

- une URL proxifiée sans `opts` garde le comportement actuel ;
- une URL avec `country` continue à produire le header `Arachnea-Proxy-Country` ;
- une URL avec deux actions `replace_all` les applique selon `order` ;
- deux actions du même type sont conservées et exécutées ;
- une action invalide retourne `502` avec un message explicite ;
- les headers `Arachnea-Proxy-*` ne sont pas forwardés à l'upstream ;
- une réponse `text/plain` ou `application/json` est modifiée ;
- une réponse textuelle non UTF-8, par exemple Windows-1252 ou ISO-8859-1, est décodée puis réencodée correctement ;
- une réponse binaire reste inchangée ;
- `content-length` correspond à la taille du body final ;
- `etag`, `content-md5` et `digest` disparaissent quand le body est modifié ;
- une réponse chunked est décodée puis renvoyée avec un body non chunked cohérent ;
- une requête `HEAD` ne reçoit pas de body transformé.

## Décisions retenues

- Header d'action : `Arachnea-Proxy-ReplaceAll`.
- Valeur d'action : JSON.
- Option commune d'ordre : `order`, optionnelle.
- Format `opts.headers` retenu pour les actions : tableau de paires.
- Action invalide : réponse `502` côté route proxy avec message explicite.
- Headers internes `Arachnea-Proxy-*` : jamais forwardés à l'upstream.
- Headers validateurs supprimés si le body change : `etag`, `content-md5`, `digest`.
- Limites : pas de limite supplémentaire spécifique aux actions ; la limite actuelle sur l'URL complète reste utilisée.
- RegEx : syntaxe Rust `regex`.
- Encodage texte : `charset` du `content-type`, puis UTF-8, puis fallback Windows-1252/ISO-8859-1 via `encoding_rs`.
- Compression : forcer ou filtrer `Accept-Encoding: identity` pour les requêtes avec post-actions.


# Exemples

JSON: {"headers":[["Arachnea-Proxy-ReplaceAll", "{\"pattern\":\"CATEGORY_LIST\",\"replacement\":\"CATEGORY_LIST  Hello Toto\"}"]]}
BASE64: eyJoZWFkZXJzIjpbWyJBcmFjaG5lYS1Qcm94eS1SZXBsYWNlQWxsIiwgIntcInBhdHRlcm5cIjpcIkNBVEVHT1JZX0xJU1RcIixcInJlcGxhY2VtZW50XCI6XCJDQVRFR09SWV9MSVNUICBIZWxsbyBUb3RvXCJ9Il1dfQ
URL: http://127.0.0.1:8080/api/proxy/opts_eyJoZWFkZXJzIjpbWyJBcmFjaG5lYS1Qcm94eS1SZXBsYWNlQWxsIiwgIntcInBhdHRlcm5cIjpcIkNBVEVHT1JZX0xJU1RcIixcInJlcGxhY2VtZW50XCI6XCJDQVRFR09SWV9MSVNUICBIZWxsbyBUb3RvXCJ9Il1dfQ/https://bff-service.rtbf.be/auvio/v1.23/widgets/17226


JSON: {"headers":[]}
BASE64: eyJoZWFkZXJzIjpbXX0

# Etat en cours
Sans entete Arachnea-Proxy-ReplaceAll, cela fonctionne.
Avec entete Arachnea-Proxy-ReplaceAll, on a un fichier vide en sortie (bug)