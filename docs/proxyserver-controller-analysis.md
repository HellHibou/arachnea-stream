# Proxy HTTP via `ProxyServer` et `ControlerService`

## Objectif

Ajouter une capacité d'appel HTTP proxifiee utilisable depuis le navigateur via le controller.

Le besoin cible est :

- recevoir une requete navigateur sur une URL locale ;
- recuperer la methode HTTP, les headers, les cookies et le body ;
- appliquer des surcharges optionnelles fournies par le front ;
- appeler l'URL distante via le proxy Arachnea ;
- renvoyer au navigateur le status HTTP, les headers de reponse et le body binaire ;
- conserver un comportement aussi transparent que possible pour les redirections.

La solution retenue est une exposition via `register_stream_function_with_state`, pas via `register_result_function_with_state`.

## Etat d'implementation

Implementation commencee et corrigee dans les fichiers suivants :

- `server/crates/arachnea-core/src/controler/mod.rs`
- `server/crates/arachnea-core/src/controler/rest/mod.rs`
- `server/crates/arachnea-core/src/controler/tauri/mod.rs`
- `server/crates/arachnea-proxy/src/core/http/client.rs`
- `server/crates/arachnea-proxy/src/core/http/mod.rs`
- `server/crates/arachnea-proxy/src/core/http/proxy_service.rs`
- `server/crates/arachnea-proxy/src/server/server.rs`
- `server/crates/arachnea-stream/src/main.rs`
- `server/crates/arachnea-stream/src/stream_scraper.rs`

Ce qui est implemente :

- `ControlerStreamInput` porte maintenant `method` et `headers`.
- `ControlerStreamOutput` porte maintenant `status`.
- Le backend REST capture methode, headers, query et body pour les routes stream.
- Le backend REST accepte les methodes `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`.
- Le backend Tauri transmet methode et headers aux handlers stream.
- REST et Tauri appliquent le status de `ControlerStreamOutput`.
- REST et Tauri suppriment le body des reponses `HEAD`.
- `arachnea-proxy::core::http` expose `ProxiedHttpRequest`, `ProxiedHttpResponse` et `SimpleHttpClient::request_proxied`.
- `arachnea-proxy::core::http::proxy_service` contient le handler controller, deplace depuis `arachnea-stream` et renomme depuis `proxy_http.rs`.
- `ProxyServer::request_http` expose une facade directe pour executer une requete HTTP via le proxy.
- `request_proxied` supporte `http` et `https`, avec TLS client pour les destinations HTTPS.
- `request_proxied` ne suit pas automatiquement les redirections.
- `request_proxied` parse status, headers et body.
- `request_proxied` decode les reponses `Transfer-Encoding: chunked`.
- Le handler `proxy_http` parse `/proxy_http/<target_url_base64url>`.
- Le handler `proxy_http` parse `opts` en base64url sans padding.
- Le handler `proxy_http` applique la limite de `8 KB` sur l'URL locale complete.
- Le handler `proxy_http` fusionne headers/cookies entrants et options front.
- Le handler `proxy_http` filtre les headers non transferables avant l'appel distant et avant la reponse locale.
- Le handler `proxy_http` refuse les schemas autres que `http` et `https`.
- Le handler `proxy_http` reecrit les headers `Location` de redirection vers la route proxy locale.
- Le handler `proxy_http` conserve `opts` encode lors des redirections.
- Le handler `proxy_http` derive `content_type` depuis le header distant et evite de dupliquer `content-type` dans `headers`.
- Les erreurs de parsing/validation sont transformees en reponses stream avec les status prevus.
- Le module `proxy_service` est compile via la feature optionnelle `controller-service`, activee par `arachnea-stream`.

Corrections importantes ajoutees pendant la verification :

- correction de la variable de registration dans `main.rs` (`proxy_core_for_http`) ;
- correction du format de la ligne HTTP en absolute-form ;
- correction de l'envoi de body pour eviter d'envoyer un body sur `GET` et `HEAD` ;
- ajout de TLS pour les URLs `https://` ;
- conservation de `opts` sous sa forme encodee dans les redirections ;
- transformation d'une `Location` invalide en `502 Bad Gateway`.

Validation effectuee :

- `cargo check -p arachnea-stream`
- `cargo check -p arachnea-proxy --features server`
- `cargo check -p arachnea-proxy --features controller-service`
- `git diff --check`

## Etat actuel

### `ProxyServer`

Fichier : `server/crates/arachnea-proxy/src/server/server.rs`

`ProxyServer` demarre les listeners proxy HTTP, SOCKS et HTTPS. L'implementation ajoute aussi `ProxyServer::request_http`, une facade pour executer une requete HTTP applicative via `ArachneaProxyCore`.

### `ControlerService`

Fichier : `server/crates/arachnea-core/src/controler/mod.rs`

Le controller expose deux familles de handlers :

- `register_result_function_with_state` pour les APIs JSON ;
- `register_stream_function_with_state` pour les flux binaires.

Le besoin correspond au second contrat, car il faut transporter un body binaire, des headers HTTP et un status HTTP.

Avant cette implementation, `ControlerStreamInput` contenait :

```rust
pub struct ControlerStreamInput {
    pub path: String,
    pub query: String,
    pub body: Vec<u8>,
}
```

Avant cette implementation, `ControlerStreamOutput` contenait :

```rust
pub struct ControlerStreamOutput {
    pub body: Vec<u8>,
    pub content_type: String,
    pub headers: HashMap<String, String>,
}
```

Les champs suivants ont ete ajoutes par l'implementation :

- les headers entrants dans `ControlerStreamInput` ;
- la methode HTTP entrante dans `ControlerStreamInput` ;
- le status HTTP dans `ControlerStreamOutput`.

## Contrat cible

### Entree stream

Evolution cible :

```rust
pub struct ControlerStreamInput {
    pub path: String,
    pub query: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
```

`headers: HashMap<String, String>` suffit pour l'implementation initiale. Le support multi-valeurs reste hors perimetre tant qu'un cas concret ne l'exige pas.

### Sortie stream

Evolution cible :

```rust
pub struct ControlerStreamOutput {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
    pub headers: HashMap<String, String>,
}
```

Les backends REST et Tauri doivent utiliser `status` lors de la construction de la reponse HTTP.

### Options fournies par le front

Les surcharges optionnelles sont fournies par le front dans `opts`.

Structure autorisee :

```json
{
  "headers": {},
  "cookies": {},
  "proxy": {}
}
```

Decisions retenues :

- `headers: Option<HashMap<String, String>>` suffit ;
- `cookies: Option<HashMap<String, String>>` suffit ;
- les champs autorises dans `opts` sont uniquement `headers`, `cookies` et `proxy` ;
- `proxy` est reserve pour une deuxieme phase et ne definit pas encore de cles supportees ;
- les headers et cookies fournis par le front sont autorises sans whitelist metier ;
- les champs inconnus dans `opts` doivent etre refuses avec `404 Not Found`.

### Priorite de construction

Ordre retenu pour construire la requete sortante :

1. partir des headers/cookies entrants depuis le navigateur ;
2. retirer les headers non transferables ;
3. appliquer les headers optionnels fournis par le front ;
4. appliquer les cookies optionnels fournis par le front.

Les surcharges front ont donc priorite sur la requete navigateur.

## Route locale proposee

Format retenu :

```text
/<entrypoint_root>/<entrypoint_api>/proxy_http/<target_url_base64url>?opts=<options_json_base64url>
```

Exemple avec la configuration REST par defaut :

```text
/api/proxy_http/aHR0cHM6Ly9zZXJ2aWNlLmV4YW1wbGUvdmlkZW8vbmV4dC5tcGQ?opts=eyJoZWFkZXJzIjp7InJlZmVyZXIiOiJodHRwczovL3NlcnZpY2UuZXhhbXBsZS8ifSwiY29va2llcyI6eyJzZXNzaW9uIjoiYWJjMTIzIn19
```

`target_url_base64url` contient l'URL distante absolue encodee en base64url sans padding.

`opts` contient le JSON d'options encode en base64url sans padding.

La taille totale de l'URL locale est limitee a `8 KB`. Au-dela, le handler doit refuser la requete avec un status client explicite, idealement `414 URI Too Long`.

## Encodage

Regle pour `target_url_base64url` :

1. obtenir une URL distante absolue ;
2. encoder cette URL en UTF-8 ;
3. encoder les octets en base64url sans padding ;
4. placer le resultat dans le segment de chemin apres `proxy_http`.

Seuls les schemas `http` et `https` sont autorises pour l'URL distante.

Regle pour `opts` :

1. construire un JSON compact ;
2. verifier que seuls les champs autorises sont presents ;
3. encoder ce JSON en UTF-8 ;
4. encoder les octets en base64url sans padding ;
5. placer le resultat dans le parametre query `opts`.

Le champ `proxy` peut etre present pour reserver le format, mais son contenu n'est pas interprete pendant la premiere phase.

Pourquoi ce format :

- l'URL distante reste une valeur opaque ;
- les query strings distantes ne se melangent pas avec la query string locale ;
- les fragments et caracteres reserves ne cassent pas la route locale ;
- le front peut construire directement l'URL proxifiee ;
- le backend n'a pas besoin d'un contexte memoire opaque de type `ctx`.

Attention : `opts` est dans l'URL. Il peut donc apparaitre dans l'historique navigateur, les logs locaux ou un header `Referer`. Ce format convient aux parametres que le front accepte d'exposer dans une URL locale. Les secrets tres sensibles necessiteront un autre mecanisme.

## Headers et cookies

### Headers entrants

Les backends doivent capturer les headers de la requete entrante :

- REST : via Warp avant l'appel au stream handler ;
- Tauri : via l'objet request du custom URI scheme.

### Headers filtres

Liste initiale des headers a ne pas forwarder tels quels :

- `connection`
- `content-length`
- `transfer-encoding`
- `host`
- `proxy-authorization`
- `proxy-connection`

`content-length` et `transfer-encoding` sont filtres car ils decrivent le corps et le transport sur une connexion HTTP precise. Apres reconstruction de la requete ou de la reponse, ces valeurs peuvent devenir fausses. La librairie HTTP doit les recalculer a partir du body reellement envoye.

### Headers et cookies fournis par le front

Les headers et cookies fournis dans `opts` sont autorises sans restriction metier. Cela signifie qu'il n'y a pas de whitelist applicative des noms autorises.

Deux controles techniques restent necessaires :

- la validation syntaxique HTTP ;
- le filtrage des headers non transferables listes plus haut.

Validation syntaxique minimale :

- noms de headers syntaxiquement valides ;
- valeurs de headers representables en HTTP ;
- noms de cookies syntaxiquement valides ;
- valeurs de cookies sans caracteres interdits par le format `Cookie`.

Les cookies fournis par le front doivent etre fusionnes avec les cookies entrants. Un cookie fourni dans `opts.cookies` remplace un cookie entrant de meme nom.

## Methodes HTTP

Le proxy stream doit prendre en charge :

- `GET`
- `POST`
- `PUT`
- `PATCH`
- `DELETE`
- `HEAD`
- `OPTIONS`

### Impact REST

Le backend REST stream accepte actuellement seulement `GET` et `POST`.

Il faudra faire evoluer `register_stream_function` pour router les methodes listees ci-dessus vers le meme stream handler.

Comportement attendu :

- `GET` : pas de body requis ;
- `POST`, `PUT`, `PATCH` : body entrant transmis au proxy ;
- `DELETE` : body possible mais optionnel, il faut le transmettre si present ;
- `HEAD` : appeler la cible en `HEAD` et renvoyer uniquement status + headers, sans body ;
- `OPTIONS` : transmettre la methode a la cible et renvoyer la reponse distante. Le controller ne traite pas les preflights CORS localement dans cette phase.

Point important : Warp devra extraire le body pour les methodes qui peuvent en porter un, sans casser `GET`, `HEAD` et `OPTIONS`.

### Impact Tauri

Le backend Tauri doit remplir `ControlerStreamInput.method` avec la methode de la requete custom URI scheme.

Il doit aussi appliquer le status HTTP de `ControlerStreamOutput`. Pour `HEAD`, il doit construire une reponse sans body meme si le handler retourne par erreur des octets.

### Impact redirections

La methode ne doit pas etre encodee dans l'URL locale. Le navigateur applique deja les regles HTTP de redirection :

- `303` repart normalement en `GET` ;
- `307` et `308` preservent la methode et le body ;
- `301` et `302` ont des comportements historiques variables selon les navigateurs.

La route proxy doit utiliser la methode effectivement recue apres redirection. C'est la raison principale pour laquelle `PUT`, `PATCH`, `DELETE`, `HEAD` et `OPTIONS` doivent etre supportees au niveau du controller.

## Redirections HTTP

Decision retenue :

- ne pas suivre automatiquement les redirections cote proxy ;
- renvoyer le status `3xx` au navigateur ;
- reecrire le header `Location` vers une URL locale proxy ;
- conserver les options front necessaires dans `opts`.

Regle de reecriture :

1. lire le header `Location` distant ;
2. si la valeur est relative, la resoudre contre l'URL distante courante ;
3. obtenir une URL distante absolue ;
4. encoder cette URL dans `target_url_base64url` ;
5. recopier ou ajuster `opts` ;
6. renvoyer `Location: /api/proxy_http/<target_url_base64url>?opts=<options_json_base64url>`.

Cas d'erreur :

- si une reponse `3xx` n'a pas de header `Location`, renvoyer le `3xx` tel quel ;
- si `Location` est present mais invalide ou impossible a resoudre, renvoyer `502 Bad Gateway`.

Exemple :

```text
Location distant:
../next.mpd?token=a&quality=hd

URL distante courante:
https://service.example/live/path/current.mpd

URL resolue:
https://service.example/live/next.mpd?token=a&quality=hd

Location renvoye au navigateur:
/api/proxy_http/aHR0cHM6Ly9zZXJ2aWNlLmV4YW1wbGUvbGl2ZS9uZXh0Lm1wZD90b2tlbj1hJnF1YWxpdHk9aGQ?opts=eyJoZWFkZXJzIjp7InJlZmVyZXIiOiJodHRwczovL3NlcnZpY2UuZXhhbXBsZS8ifX0
```

Pour `HEAD`, la redirection doit aussi etre renvoyee sans body.

## Content-Type

`ControlerStreamOutput` garde un champ `content_type` pour rester proche du contrat actuel.

Regle retenue :

- `content_type` est derive du header distant `content-type` quand il existe ;
- si le header distant ne contient pas `content-type`, utiliser `application/octet-stream` par defaut ;
- ne pas dupliquer `content-type` dans `headers` lorsque `content_type` est renseigne ;
- le backend REST/Tauri ecrit le header HTTP `Content-Type` a partir de `content_type`.

Cela evite une ambiguite entre `ControlerStreamOutput.content_type` et `ControlerStreamOutput.headers["content-type"]`.

## Erreurs de parsing et validation

Statuses retenus :

- URL locale complete superieure a `8 KB` : `414 URI Too Long` ;
- base64url invalide pour `target_url_base64url` ou `opts` : `400 Bad Request` ;
- JSON `opts` invalide : `400 Bad Request` ;
- champ inconnu dans `opts` : `404 Not Found` ;
- URL cible invalide : `400 Bad Request` ;
- schema d'URL cible autre que `http` ou `https` : `400 Bad Request` ;
- header ou cookie invalide : `400 Bad Request` ;
- `Location` de redirection present mais invalide : `502 Bad Gateway`.

## Fonction proxifiee cote Rust

La fonction a ajouter doit rester typable et reutilisable par le stream handler.

Forme conceptuelle :

```rust
pub struct ProxiedHttpRequest {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub struct ProxiedHttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
```

La fonction doit :

- construire la requete HTTP sortante ;
- utiliser le proxy Arachnea pour le transport ;
- ne pas suivre les redirections automatiquement ;
- retourner le status, les headers et le body ;
- traiter `HEAD` comme une reponse sans body.

## Checklist de developpement

- [x] Ajouter `method` et `headers` a `ControlerStreamInput`.
- [x] Ajouter `status` a `ControlerStreamOutput`.
- [x] Adapter le backend REST pour capturer headers, methode et status.
- [x] Adapter le backend REST pour accepter `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`.
- [x] Adapter le backend Tauri pour remplir headers, methode et status.
- [x] Ajouter le parsing de route `/proxy_http/<target_url_base64url>`.
- [x] Ajouter le parsing optionnel de `opts`.
- [x] Appliquer la limite de `8 KB` sur l'URL locale complete.
- [x] Fusionner headers/cookies entrants et options front.
- [x] Filtrer les headers non transferables.
- [x] Refuser les schemas autres que `http` et `https`.
- [x] Ajouter la fonction de requete HTTP proxifiee.
- [x] Ajouter `ProxyServer::request_http`.
- [x] Deplacer le handler controller de `arachnea-stream/src/proxy_http.rs` vers `arachnea-proxy/src/core/http/proxy_service.rs`.
- [x] Gater le handler controller avec la feature `controller-service`.
- [x] Gerer les destinations `https://` avec TLS.
- [x] Reecrire les `Location` de redirection vers la route proxy locale.
- [x] Normaliser `content-type` via `ControlerStreamOutput.content_type`.
- [x] Verifier par compilation les cas controller et proxy server.
- [ ] Ajouter des tests automatises dedies aux cas `HEAD`, `OPTIONS`, redirections relatives et URL cible avec query string.
- [ ] Tester manuellement une URL `http://` et une URL `https://` via `/api/proxy_http/...`.

## Risques et points d'attention

- `opts` peut exposer des informations dans l'URL locale.
- Le support `HEAD` doit eviter tout body en sortie.
- Les redirections `307` et `308` peuvent reutiliser methode et body ; le controller doit donc accepter les methodes non `GET` apres redirection.
- Les headers multi-valeurs ne sont pas couverts par l'implementation initiale.
- `Set-Cookie` en reponse peut etre limite par le choix `HashMap<String, String>` si plusieurs valeurs doivent etre renvoyees.
- La limite `8 KB` d'URL totale peut etre atteinte vite si le front met trop de headers/cookies dans `opts`.
- `proxy` est reserve dans `opts`, mais son schema fonctionnel reste a definir en phase 2.

## Points a specifier avant ou pendant le developpement

Ces points ne bloquent pas la conception actuelle, mais doivent etre traites avant une exposition large.

### Limites de body

Il faut definir une limite maximale pour les bodies entrants et sortants, ou confirmer que le flux sera bufferise en memoire pour la premiere phase.

Point d'attention : le contrat actuel utilise `Vec<u8>`, donc les reponses volumineuses sont chargees en memoire.

### Timeouts

La fonction HTTP proxifiee doit definir des timeouts explicites :

- timeout de connexion ;
- timeout de lecture ;
- timeout total de requete.

Sans timeout, un appel distant bloque peut immobiliser une route controller.

### Reseau cible autorise

Les schemas sont limites a `http` et `https`, mais il reste a decider si les URLs vers reseaux locaux ou adresses privees doivent etre autorisees.

Point d'attention : si l'URL cible vient du front, cela peut ouvrir un risque SSRF local. Le proxy Arachnea peut volontairement devoir atteindre certains reseaux, mais ce choix doit etre explicite.

### Headers de reponse multi-valeurs

Le contrat initial garde `HashMap<String, String>`. Cela peut limiter certains headers de reponse comme plusieurs `Set-Cookie`.

Decision actuelle : accepter cette limite pour la premiere phase. Si des services ont besoin de plusieurs `Set-Cookie`, il faudra faire evoluer `ControlerStreamOutput.headers` vers une representation multi-valeurs.

### Taille de `opts`

La limite retenue est `8 KB` pour l'URL complete. Il faudra verifier que le front n'encode pas des headers/cookies trop volumineux dans `opts`.
