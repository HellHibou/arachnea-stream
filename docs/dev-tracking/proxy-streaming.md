# Analyse Complète et Définitive : Refonte du Proxy HTTP vers un Modèle Streaming

## 1. Diagnostic de l'état actuel

### 1.1 Architecture en place

Le proxy fonctionne sur **3 couches** qui communiquent via des structures bufferisées :

| Couche | Fichier | Rôle |
|--------|---------|------|
| **Transport/Réseau** | `client.rs` | Connexion TCP/TLS via `ArachneaProxyCore`, lecture complète de la réponse en mémoire |
| **Routage/Métier** | `proxy_service.rs` | Construction de la requête, application des `post_actions`, réécriture des headers |
| **Sortie HTTP** | `mod.rs` | Conversion de `ControlerStreamOutput` en réponse Warp |

### 1.2 Goulots d'étranglement identifiés

**Dans `client.rs` :**
```rust
// Ligne critique : lecture TOUTE la réponse en mémoire
let mut bytes = Vec::new();
stream.read_to_end(&mut bytes).await?;
```
- `send_request_and_read_response` lit le flux TCP/TLS jusqu'à EOF
- `decode_chunked_body` travaille sur un `&[u8]` complet
- `parse_http_response` reçoit un `Vec<u8>` complet et en extrait headers + corps

**Dans `proxy_service.rs` :**
```rust
let proxy_response = match client.request_proxied(proxy_request).await { ... };
// proxy_response.body est un Vec<u8> complet
let body = if method == "HEAD" { Vec::new() } else { proxy_response.body };
```

**Dans `mod.rs` :**
```rust
// call_and_reply_stream reçoit un Vec<u8> et le passe directement à warp
let response = builder.body(body).expect("...");
```

**Dans la structure `ControlerStreamOutput` :**
```rust
pub struct ControlerStreamOutput {
    pub status: u16,
    pub body: Vec<u8>,           // <-- Le goulot principal
    pub content_type: String,
    pub headers: HashMap<String, String>,
}
```

### 1.3 Conséquences

- **Latence (TTFB)** : Le client final ne reçoit rien tant que le proxy n'a pas fini de télécharger la réponse complète
- **Mémoire (RAM)** : Un fichier de 2 Go en transit consomme 2 Go de RAM sur le proxy
- **Scalabilité** : Impossible de servir plusieurs gros téléchargements simultanés sans saturer la RAM

---

## 2. Découverte majeure : Unification des flux déjà résolue

### Le type `ProxyStream`

Le core expose déjà un type unifié : **`ProxyStream`**. Ce n'est pas un `enum`, mais un wrapper par **trait object** :

```rust
pub struct ProxyStream {
    inner: Pin<Box<dyn AsyncReadWrite + Send + 'static>>,
    metadata: ConnectMetadata,
}
```

### Conséquences sur la refactorisation

| Aspect | Avant (hypothèse initiale) | Après (avec ProxyStream) |
|--------|---------------------------|-------------------------|
| Type de flux à retourner | Besoin de créer un `enum` ou boxer dynamiquement | **Déjà résolu** : `ProxyStream` |
| Gestion TLS dans `client.rs` | `TlsStream` concret, types différents | Ré-envelopper avec `ProxyStream::new(tls_stream, metadata)` |
| Complexité | Moyenne (unification manuelle) | **Faible** (réutiliser l'existant) |

### Correction à apporter dans `client.rs`

Le code actuel casse temporairement l'unification :

```rust
// Code actuel (problématique pour le streaming)
let response_bytes = if is_https && stream.target_form == HttpRequestTargetForm::OriginForm {
    let mut tls_stream = crate::core::transport::tls::client_tls(
        stream.stream, host, Duration::from_secs(10), true
    ).await?;
    send_request_and_read_response(&mut tls_stream, ...).await?
} else {
    let mut raw_stream = stream.stream;
    send_request_and_read_response(&mut raw_stream, ...).await?
};
```

**Solution** : Ré-envelopper le `TlsStream` dans un `ProxyStream` après le handshake :

```rust
// Code corrigé pour le streaming
let mut unified_stream = if is_https && stream.target_form == HttpRequestTargetForm::OriginForm {
    let tls_stream = crate::core::transport::tls::client_tls(
        stream.stream, host, Duration::from_secs(10), true
    ).await?;
    ProxyStream::new(tls_stream, stream.metadata) // Ré-enveloppement
} else {
    stream // Déjà un ProxyStream
};

// Maintenant on a un seul type : ProxyStream
// On peut retourner le flux après lecture des headers
```

---

## 3. Clarification sur les post-actions

### 3.1 Structure réelle de `ProxyHttpPostActionConfig`

`ProxyHttpPostActionConfig` n'est **pas un enum** mais une **struct** avec un champ `action: String` :

```rust
pub struct ProxyHttpPostActionConfig {
    pub action: String,        // Nom de l'action, ex: "ReplaceAll"
    pub order: Option<i32>,    // Ordre d'exécution
    pub params: HashMap<String, String>, // Paramètres spécifiques
}
```

### 3.2 Inventaire exact des post-actions

| Action | Modifie le corps ? | Modifie les headers ? | Notes |
|--------|-------------------|----------------------|-------|
| `"ReplaceAll"` | Oui (conditionnel) | Non | Seule post-action implémentée |
| *(autre valeur)* | N/A | N/A | Erreur à l'exécution |

### 3.3 Détail de "ReplaceAll"

`ReplaceAll` modifie le corps **uniquement si** toutes ces conditions sont remplies :
- Corps non vide
- `Content-Type` textuel (ex: `text/html`, `application/json`, `image/svg+xml`)
- `Content-Encoding` absent, vide, ou `identity`
- Décodage charset réussi

Sinon, le corps est retourné inchangé (no-op). **Mais même en cas de no-op, l'action exige le buffer complet** car elle doit évaluer les conditions.

### 3.4 Distinction avec les redirect actions

`RemoveHeader` n'est **pas** une post-action sur la réponse upstream. C'est une **redirect action** (`ProxyHttpRedirectActionConfig::RemoveHeader`) qui modifie l'URL proxy encodée lors d'une redirection 302. Elle n'impacte pas le corps HTTP ni les headers de réponse.

### 3.5 Règle de bascule buffer/stream

La règle se simplifie considérablement :

```rust
fn post_actions_require_body_buffering(actions: &[ProxyHttpPostActionConfig]) -> bool {
    actions.iter().any(|a| a.action == "ReplaceAll")
    // Toute future action modifiant le corps → ajouter ici
}
```

| Situation | Mode requis |
|-----------|-------------|
| `post_actions` vide | **Stream possible** |
| `post_actions` contient `"ReplaceAll"` | **Buffer obligatoire** |
| `post_actions` contient une action inconnue | Erreur à l'exécution (pas de stream) |
| `RemoveHeader` présent (redirect action) | **Stream possible** — n'impacte pas `post_actions` |

---

## 4. Stratégie de refonte : Mode Hybride

Puisque `"ReplaceAll"` nécessite de modifier le corps, **on ne peut pas tout passer en streaming pur**. La stratégie retenue est un **mode hybride avec basculement dynamique** :

| Condition | Mode utilisé | Conséquence |
|-----------|-------------|-------------|
| `post_actions` est vide | **Streaming** | Économie de RAM, TTFB faible, compression préservée |
| `post_actions` contient `"ReplaceAll"` | **Buffer** (comportement actuel) | Modification possible du corps, mais consommation RAM |

---

## 5. Modifications nécessaires couche par couche

### 5.1 Couche 1 : Le contrat de données (`ControlerStreamOutput`)

**Fichier concerné** : Module définissant `ControlerStreamOutput` (probablement `arachnea_core::controler`)

**Modification requise** :

1. Créer un `enum ResponseBody` polymorphe :
   ```rust
   pub enum ResponseBody {
       /// Corps entièrement chargé en mémoire (pour post_actions modifiant le corps)
       Buffered(Vec<u8>),
       /// Corps en flux continu (pour les réponses non modifiées)
       Streamed(Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>),
   }
   ```

2. Remplacer le champ `body: Vec<u8>` par `body: ResponseBody` dans `ControlerStreamOutput`

3. Dépendances à ajouter : `bytes`, `futures` (déjà présentes dans le `Cargo.toml`)

**Impact sur `proxy_service.rs`** :
- La fonction `stream_error` doit retourner `ResponseBody::Buffered(message.into().into_bytes())`

**Impact sur `mod.rs`** :
- La fonction `call_and_reply_stream` doit matcher sur `ResponseBody` pour construire la réponse Warp :
  - `Buffered(bytes)` → `warp::hyper::Body::from(bytes)`
  - `Streamed(stream)` → `warp::hyper::Body::wrap_stream(stream)`

### 5.2 Couche 2 : Le client HTTP (`client.rs`)

C'est le cœur de la refactorisation. Plusieurs sous-problèmes à résoudre :

#### 5.2.1 Ré-envelopper TLS dans `ProxyStream`

**État actuel** : Le code crée un `TlsStream` concret et le passe à `send_request_and_read_response`.

**Modification requise** :
```rust
let mut unified_stream = if is_https && stream.target_form == HttpRequestTargetForm::OriginForm {
    let tls_stream = crate::core::transport::tls::client_tls(
        stream.stream, host, Duration::from_secs(10), true
    ).await?;
    ProxyStream::new(tls_stream, stream.metadata) // Ré-enveloppement
} else {
    stream // Déjà un ProxyStream
};
```

#### 5.2.2 Séparation lecture des headers / lecture du corps

**État actuel** : `send_request_and_read_response` fait un `read_to_end` qui mélange headers et corps dans un seul `Vec<u8>`.

**Modification requise** :

Créer une fonction `send_request_and_read_headers` qui :
1. Écrit la requête sur le flux
2. Lit octet par octet (ou via un `BufReader`) jusqu'à trouver `\r\n\r\n`
3. Parse les headers (status line + headers HTTP)
4. **Retourne le `ProxyStream` non consommé** (il contient encore le corps)

**Fonction existante à réutiliser** : `read_headers_only` fait déjà la lecture jusqu'à `\r\n\r\n`. Il faut l'adapter pour qu'elle retourne à la fois les headers parsés ET le flux restant.

#### 5.2.3 Décodeur HTTP en streaming

**État actuel** : `decode_chunked_body` prend un `&[u8]` complet et le décode.

**Modification requise** : Créer un décodeur qui travaille en streaming, c'est-à-dire qui décode les chunks au fur et à mesure qu'ils arrivent du réseau.

**Solution retenue** : Utiliser `tokio-util` avec la feature `codec`.

**Dépendance à ajouter** :
```toml
tokio-util = { version = "0.7", features = ["codec"] }
```

**Implémentation** :
- Créer `HttpChunkDecoder` qui implémente `tokio_util::codec::Decoder`
- Utiliser `FramedRead::new(stream, decoder)` pour transformer le flux en `Stream<Item = Result<BytesMut>>`
- Gérer les 3 cas HTTP/1.1 :
  1. `Transfer-Encoding: chunked` → décodage incrémental des chunks
  2. `Content-Length: N` → lecture d'exactement N octets, puis fin du stream
  3. `Connection: close` (sans length ni chunked) → lecture jusqu'à EOF

#### 5.2.4 Scinder `request_proxied`

**Modification requise** :

1. Garder `request_proxied` (ou renommer en `request_proxied_buffered`) pour le mode buffer
2. Créer `request_proxied_streaming` qui retourne `(status, headers, Stream)` sans consommer le corps
3. Le choix entre les deux se fait dans `proxy_service.rs` selon les `post_actions`

### 5.3 Couche 3 : Le routeur (`proxy_service.rs`)

#### 5.3.1 Règle de bascule Buffer vs Stream

**État actuel** :
```rust
if post_actions_require_identity_encoding(&post_actions) {
    remove_header_variants(&mut headers, "accept-encoding");
    headers.insert("Accept-Encoding".to_string(), "identity".to_string());
}
```
Cette fonction force l'encodage `identity` si des `post_actions` sont présentes. C'est correct pour le mode buffer, mais trop restrictif pour le streaming.

**Modification requise** :

Créer une fonction plus fine : `post_actions_require_body_buffering(actions: &[ProxyHttpPostActionConfig]) -> bool`

```rust
fn post_actions_require_body_buffering(actions: &[ProxyHttpPostActionConfig]) -> bool {
    actions.iter().any(|a| a.action == "ReplaceAll")
}
```

Cette fonction retourne `true` uniquement si au moins une action est `"ReplaceAll"`.

**Conséquence importante sur la compression** :
- Si mode buffer → forcer `Accept-Encoding: identity` (comme aujourd'hui) pour pouvoir lire/modifier le texte
- Si mode stream → **ne pas toucher** à `Accept-Encoding`. Le serveur cible peut envoyer du `gzip`/`brotli`, et le proxy le transmet tel quel au client (économie de bande passante)

#### 5.3.2 Construction du `ControlerStreamOutput`

**Modification requise** :
```rust
let body = if method == "HEAD" {
    ResponseBody::Buffered(Vec::new())
} else if post_actions_require_body_buffering(&post_actions) {
    // Mode buffer : appeler request_proxied_buffered, appliquer post_actions
    ResponseBody::Buffered(modified_body)
} else {
    // Mode stream : appeler request_proxied_streaming
    ResponseBody::Streamed(body_stream)
};
```

### 5.4 Couche 4 : La sortie Warp (`mod.rs`)

**État actuel** :
```rust
let response = builder.body(body).expect("...");
```
`body` est un `Vec<u8>`, Warp le convertit en `hyper::Body` automatiquement.

**Modification requise** :
```rust
let body = match output.body {
    ResponseBody::Buffered(bytes) => warp::hyper::Body::from(bytes),
    ResponseBody::Streamed(stream) => warp::hyper::Body::wrap_stream(stream),
};
let response = builder.body(body).expect("...");
```

**Point de vigilance pour HEAD** :
Le code actuel fait `if is_head { body.clear(); }`. Avec un stream, on ne peut pas "clear". Il faut gérer HEAD **avant** de lancer le téléchargement côté `client.rs` (en ne lisant que les headers, comme le fait déjà `read_headers_only`).

---

## 6. Gestion des erreurs en mode streaming

### 6.1 Comportement attendu

- **Mode buffer** : Si le serveur coupe la connexion au milieu, on renvoie une 502 au client
- **Mode stream** : Le client a déjà reçu les headers (status 200) et le début du corps. Si le serveur coupe, le stream retourne une erreur `std::io::Error`, ce qui coupe la connexion côté client. Le client verra une réponse incomplète (pas de 502 possible à ce stade). C'est le comportement standard d'un proxy streaming.

### 6.2 Logging des erreurs

**Impératif** : Toute erreur survenant en cours de stream doit être loguée en **WARNING** avec le maximum de détails pour faciliter le diagnostic.

**Implémentation requise** :

Dans le `Stream` retourné par `request_proxied_streaming`, mapper les erreurs pour logger avant de les propager :

```rust
let mapped_stream = stream.map(|result| {
    match result {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            tracing::warn!(
                error = %e,
                error_kind = ?e.kind(),
                target_url = %target_url,
                method = %method,
                status = status_code,
                "Stream error during proxy response body transfer"
            );
            Err(e)
        }
    }
});
```

**Détails à logger** :
- Message d'erreur complet (`%e`)
- Type d'erreur IO (`?e.kind()`)
- URL cible
- Méthode HTTP
- Status code de la réponse
- Tout autre contexte pertinent (headers, taille des données déjà transmises si disponible)

---

## 7. Compression en mode streaming

### 7.1 Comportement confirmé

**Décision** : Laisser le serveur cible compresser (gzip/brotli) quand il n'y a pas de `post_actions` modifiant le corps.

### 7.2 Implémentation

- **Mode buffer** (`post_actions` contient `"ReplaceAll"`) :
  - Forcer `Accept-Encoding: identity` dans la requête sortante
  - Le serveur cible envoie le corps non compressé
  - Le proxy peut lire/modifier le texte
  - Le proxy renvoie le corps modifié au client (non compressé, ou avec `Content-Length` mis à jour)

- **Mode stream** (`post_actions` vide) :
  - **Ne pas toucher** à `Accept-Encoding` dans la requête sortante
  - Le serveur cible peut envoyer du `gzip`/`brotli`/`deflate`
  - Le proxy transmet le corps compressé tel quel au client
  - **Économie de bande passante** entre le serveur cible et le proxy, et entre le proxy et le client
  - Les headers `Content-Encoding`, `Content-Length`, `ETag`, etc. sont transmis tels quels

### 7.3 Conséquence sur les headers

En mode stream, il ne faut **pas** supprimer les headers de validation (`Content-Length`, `ETag`, `Content-MD5`, `Digest`) car le corps n'est pas modifié. Ces headers doivent être transmis intégralement au client final.

---

## 8. Points de vigilance et risques

### 8.1 Gestion des erreurs en cours de stream

- **Mode buffer** : Si le serveur coupe la connexion au milieu, on renvoie une 502 au client
- **Mode stream** : Le client a déjà reçu les headers (status 200) et le début du corps. Si le serveur coupe, le stream retourne une erreur `std::io::Error`, ce qui coupe la connexion côté client. Le client verra une réponse incomplète (pas de 502 possible à ce stade). C'est le comportement standard d'un proxy streaming.
- **Logging** : Toute erreur en mode stream doit être loguée en WARNING avec détails complets

### 8.2 Headers de validation

- **Mode buffer** : Le code supprime `Content-Length`, `ETag`, `Content-MD5`, `Digest` si le corps est modifié
- **Mode stream** : Ces headers sont transmis tels quels (le corps n'est pas modifié, la compression est préservée)

### 8.3 Méthode HEAD

- En mode HEAD, le serveur cible n'envoie pas de corps
- Le code actuel gère cela via `headers_only: true` dans `ProxiedHttpRequest`, qui appelle `read_headers_only`
- Il faut conserver ce comportement : en mode HEAD, on ne crée pas de stream, on retourne `ResponseBody::Buffered(Vec::new())`

### 8.4 Redirections (3xx)

- Le code réécrit le header `Location` pour les redirections
- Cela ne concerne que les headers, pas le corps
- Les redirections peuvent donc fonctionner en mode stream sans problème

### 8.5 Cookies et headers non-transferables

- La logique de filtrage des headers (`filter_non_transferable`, `remove_header_variants`) ne change pas
- Elle s'applique aux headers de la requête sortante et de la réponse entrante, indépendamment du mode (buffer ou stream)

---

## 9. Résumé des actions pour la phase de développement

| Étape | Fichier | Action | Complexité | Dépendance |
|-------|---------|--------|------------|------------|
| 1 | `Cargo.toml` | Ajouter `tokio-util = { version = "0.7", features = ["codec"] }` | Faible | - |
| 2 | Module `controler` | Créer `enum ResponseBody`, modifier `ControlerStreamOutput` | Faible | - |
| 3 | Module `actions` | Créer `post_actions_require_body_buffering()` | **Faible** (seulement vérifier `action == "ReplaceAll"`) | - |
| 4 | `client.rs` | Ré-envelopper TLS dans `ProxyStream` | **Faible** (grâce à la découverte) | - |
| 5 | `client.rs` | Créer `HttpChunkDecoder` via `tokio-util::codec::Decoder` | Élevée | `tokio-util` |
| 6 | `client.rs` | Séparer lecture headers / corps | Moyenne | - |
| 7 | `client.rs` | Créer `request_proxied_streaming()` avec logging WARNING des erreurs | Moyenne | - |
| 8 | `proxy_service.rs` | Implémenter l'arbitrage Stream vs Buffer | **Faible** (règle simplifiée) | - |
| 9 | `proxy_service.rs` | Adapter la construction de `ControlerStreamOutput` | Moyenne | - |
| 10 | `mod.rs` | Adapter `call_and_reply_stream` pour `ResponseBody` | Faible | - |

---

## 10. Synthèse finale

### Points clés validés

1. **Unification des flux** : `ProxyStream` existe déjà, pas besoin de créer un `enum`
2. **Post-actions** : Seule `"ReplaceAll"` existe et modifie le corps → règle de bascule simplifiée
3. **Compression** : Mode stream préserve la compression du serveur cible (économie de bande passante)
4. **Erreurs** : Logging WARNING détaillé pour toute erreur en cours de stream
5. **Redirect actions** : `RemoveHeader` n'impacte pas le mode (stream possible)

### Bénéfices attendus

- **Réduction drastique de la RAM** pour les gros fichiers (vidéos, downloads)
- **TTFB amélioré** : le client reçoit les premiers octets immédiatement
- **Bande passante économisée** grâce à la préservation de la compression
- **Scalabilité** : possibilité de servir plusieurs gros fichiers simultanément
- **Diagnostic facilité** : logs WARNING détaillés pour les erreurs de stream

---

## 11. Plan de développement par étapes (étapes)

Chaque étape représente une unité de travail réalisable en une demi-journée à une journée, avec des jalons clairs et testables.

---

### Étape 1 : Ajout de `ResponseBody` dans `arachnea-core`
- **Objectif :** Modifier le contrat de données sans casser la compilation.
- **Tâches :**
  - Ajouter les dépendances `bytes` et `futures` à `arachnea-core/Cargo.toml`.
  - Définir l'enum `ResponseBody` avec les variantes `Buffered(Vec<u8>)` et `Streamed(Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>)`.
  - Remplacer le champ `body: Vec<u8>` de `ControlerStreamOutput` par `body: ResponseBody`.
  - Adapter tous les sites de construction de `ControlerStreamOutput` pour utiliser `ResponseBody::Buffered(body)` (comportement inchangé).
  - Vérifier que le projet compile toujours (`cargo check --workspace`).
- **Statut : ✅ Terminé**
  - `arachnea-core/Cargo.toml` : ajout de `bytes` et `futures` workspace deps
  - `arachnea-core/src/controler/mod.rs` : définition de `ResponseBody`, mise à jour du champ `body` dans `ControlerStreamOutput`
  - `proxy_service.rs` : `stream_error` et `handle_proxy_http` → `ResponseBody::Buffered(...)`
  - `stream_scraper.rs` : `get_drm_license` → `ResponseBody::Buffered(response.body)`
  - `rest/mod.rs` : extraction `Vec<u8>` depuis `ResponseBody::Buffered` dans `call_and_reply_stream`
  - `tauri/mod.rs` : extraction `Vec<u8>` depuis `ResponseBody::Buffered` dans le handler Tauri
  - `cargo check --workspace` : succès (9.62s)

---

### Étape 2 : Préparation du client HTTP – unification des flux
- **Objectif :** Uniformiser le retour du flux réseau via `ProxyStream`.
- **Tâches :**
  - Dans `client.rs`, après l'établissement de la connexion TLS, envelopper le flux avec `ProxyStream::new(tls_stream, metadata)` (au lieu de laisser un type concret).
  - S'assurer que `send_request_and_read_response` accepte désormais un `&mut ProxyStream` (déjà générique, donc compatible).
  - Tester avec une requête HTTP simple (mode bufferisé) pour valider que le comportement est inchangé.
- **Statut : ✅ Terminé**
  - `client.rs` : Ajout de `ProxyStream` dans l'import, clonage du metadata avant le move de `stream.stream`, création d'un `ProxyStream` unifié après TLS via `ProxyStream::new(tls_stream, metadata)`, appel unique à `send_request_and_read_response(&mut unified_stream, ...)`
  - `cargo check --workspace` : succès

---

### Étape 3 : Refactoring de la lecture des en-têtes
- **Objectif :** Séparer la lecture des en-têtes de celle du corps.
- **Tâches :**
  - Extraire la logique de lecture des en-têtes (`read_headers_only`) dans une fonction dédiée qui retourne `(status, headers, rest_of_stream)` ou un état.
  - Modifier `send_request_and_read_response` pour ne lire que les en-têtes, puis retourner un objet contenant le flux sous-jacent pour la lecture du corps.
  - Conserver la lecture complète du corps avec `read_to_end` en attendant les décodeurs streaming.
- **Statut : ✅ Terminé**
  - `client.rs` : Nouvelle fonction `parse_response_head(raw_header_bytes)` qui parse status + headers depuis les bytes d'en-têtes
  - `client.rs` : Nouvelle fonction `send_request_and_read_head(stream, ...)` qui écrit la requête, lit les headers via `read_headers_only`, et retourne `(status, headers)`
  - `client.rs` : Nouvelle fonction `process_response_body(body, headers, ...)` qui gère le décodage chunked et les post-actions
  - `client.rs` : `request_proxied` utilise désormais `send_request_and_read_head` + `read_to_end` sur le stream pour récupérer le body séparément
  - `client.rs` : `send_request_and_read_response` et `parse_http_response` supprimés (code mort)
  - `cargo check --workspace` : succès, 0 warnings

---

### Étape 4 : Implémentation des décodeurs streaming (base)
- **Objectif :** Créer les wrappers `AsyncRead` pour les trois cas de corps.
- **Tâches :**
  - Créer `ChunkedBodyReader` : lit les chunks selon `Transfer-Encoding: chunked` (taille hex, données, \r\n) jusqu'au chunk de taille 0.
  - Créer `ContentLengthBodyReader` : lit exactement N octets puis retourne `Ok(0)` (EOF).
  - Créer `UntilEofBodyReader` : lit jusqu'à la fermeture du flux (fallback).
  - Tester unitairement chaque reader avec des données simulées.
- **Statut : ✅ Terminé**
  - `body_readers.rs` : Nouveau module avec les trois readers
    - `ContentLengthBodyReader<S>` :: lit exactement N octets, délègue au stream via un buffer local
    - `UntilEofBodyReader<S>` :: lit jusqu'à EOF, simple délégation
    - `ChunkedBodyReader<R>` :: machine à états (ChunkSize → ChunkData → ChunkCrLf → Done) avec buffer interne
  - `http/mod.rs` : ajout de `pub mod body_readers`
  - 10 tests unitaires : tous passent (content-length, until-eof, chunked : single/multiple/empty/extension/large)

---

### Étape 5 : Intégration du streaming dans le client HTTP
- **Objectif :** Remplacer `read_to_end` par l'utilisation des décodeurs.
- **Tâches :**
  - Analyser les en-têtes de réponse pour choisir le bon reader.
  - Construire le reader approprié à partir du flux restant (après les en-têtes).
  - Modifier `ProxiedHttpResponse` pour contenir `body: ResponseBody::Streamed(reader)`.
  - Conserver un mode `headers_only` pour `HEAD` (pas de lecture de corps).
  - Vérifier que les tests d'intégration HTTP passent toujours (avec des réponses de petite taille).

---

### Étape 6 : Logique de bascule buffer / stream dans `proxy_service`
- **Objectif :** Choisir entre `Buffered` ou `Streamed` selon les post-actions.
- **Tâches :**
  - Renommer `post_actions_require_identity_encoding` en `post_actions_require_body_buffering` dans `actions/mod.rs`.
  - Généraliser la fonction pour qu'elle prenne en compte toute action future (en plus de `ReplaceAll`).
  - Dans `handle_proxy_http`, appeler cette fonction. Si `true`, collecter le stream en `Vec<u8>` (via `try_collect` ou boucle) et construire `ResponseBody::Buffered`. Sinon, passer le stream directement.
  - Vérifier que le comportement avec `ReplaceAll` reste inchangé.

---

### Étape 7 : Adaptation du backend REST (Warp)
- **Objectif :** Permettre le streaming vers le client REST.
- **Tâches :**
  - Modifier `call_and_reply_stream` dans `rest/mod.rs` :
    - Si `ResponseBody::Buffered`, utiliser `warp::hyper::Body::from(bytes)`.
    - Si `ResponseBody::Streamed`, utiliser `warp::hyper::Body::wrap_stream(stream)`.
  - Supprimer l'ancien code qui manipulait directement `Vec<u8>`.
  - Tester avec une route proxy renvoyant un fichier volumineux (vérifier la mémoire et le TTFB).

---

### Étape 8 : Adaptation du backend Tauri (fallback buffer)
- **Objectif :** Conserver le fonctionnement existant (bufferisé) en attendant le streaming natif.
- **Tâches :**
  - Modifier le handler Tauri dans `tauri/mod.rs` pour extraire le `Vec<u8>` du `ResponseBody::Buffered` (et paniquer ou logger une erreur si `Streamed` est rencontré, ce qui ne devrait pas arriver car Tauri utilisera toujours le fallback).
  - Ajouter un commentaire `TODO: utiliser tauri-plugin-http pour le streaming`.
  - Vérifier que les commandes Tauri continuent de fonctionner.

---

### Étape 9 : Nettoyage et suppression du code mort
- **Objectif :** Éliminer les anciennes fonctions de décodage bufferisé.
- **Tâches :**
  - Supprimer `decode_chunked_body` et les fonctions auxiliaires associées (elles ne sont plus utilisées).
  - Supprimer les imports inutiles.
  - Vérifier que tout compile et que les tests passent.

---

### Étape 10 : Mise à jour des tests et intégration continue
- **Objectif :** Assurer la couverture des nouveaux cas.
- **Tâches :**
  - Adapter les tests unitaires de `client.rs` pour utiliser les nouveaux readers.
  - Mettre à jour les tests d'intégration (`post_action_integration.rs`) pour valider le basculement buffer/stream.
  - Ajouter un test avec une réponse chunked de grande taille (simulée).
  - Lancer `cargo test --workspace` et corriger les régressions.

---

### Étape 11 : Validation en environnement réel et documentation
- **Objectif :** Valider les gains en mémoire et en latence.
- **Tâches :**
  - Déployer sur un environnement de test avec des fichiers de différentes tailles.
  - Mesurer la consommation mémoire avant / après.
  - Mesurer le TTFB.
  - Documenter les nouvelles limites et le comportement (notamment le fallback Tauri).
  - Mettre à jour le guide d'administration.

---

**Ordre imposé :** chaque étape dépend des précédentes, mais les étapes 8 (Tauri) et 7 (REST) peuvent être menées en parallèle après l'étape 6. L'étape 11 est finale.