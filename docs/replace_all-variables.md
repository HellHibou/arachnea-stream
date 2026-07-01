# ReplaceAll : Substitution de variables dans les templates de remplacement

## Problème

L'action `ReplaceAll` (post-réponse) permet le remplacement par expression
régulière dans les corps de réponses HTTP textuels, mais le `replacement` est
statique. Dans un contexte proxy, il est souvent utile d'injecter dynamiquement
l'URL cible ou ses composants dans le texte de remplacement — par exemple pour
réécrire des chemins relatifs en URLs absolues du proxy.

## Variables demandées

| Variable | Description | Exemple |
|---|---|---|
| `{proxy}` | Chemin relatif du proxy depuis `entry_point` | `/api/proxy` |
| `{base_url}` | URL de base complète de la cible proxifiée | `https://host.com:8080` |
| `{path}` | Partie chemin après le `base_url` | `/mon/chemin.jpg` |
| `{path_base}` | Chemin sans le dernier segment (fichier) | `/mon/` |

## État actuel

- **`ProxyHttpPostAction::post_apply`** (trait, `actions/mod.rs:180`) ne prend
  que `status_code`, `headers`, `body` — aucun contexte de requête.
- **`apply_post_actions`** (`actions/mod.rs:232`), **`parse_http_response`**
  (`client.rs:337`) et **`post_apply`** sur `ReplaceAllAction` n'ont pas accès
  à l'URL.
- L'URL cible complète est disponible dans `ProxiedHttpRequest.url` à
  l'intérieur de `request_proxied` (`client.rs:127`) mais n'est **pas**
  transmise à `parse_http_response`.

## Modifications proposées

### 1. Créer une structure de contexte

Introduire une petite structure partagée pour porter les informations
nécessaires à la résolution des variables sans coupler toutes les actions à
l'URL complète.

```rust
/// Contexte passé aux actions post-réponse pour la substitution de variables.
pub struct PostActionContext {
    /// Point d'entrée / chemin de base du proxy,
    /// ex. "http://127.0.0.1:8080/api/proxy".
    pub entry_point: String,
    /// URL cible complète, ex. "https://host.com:8080/mon/chemin.jpg".
    pub target_url: String,
}
```

Emplacement : `actions/mod.rs` à côté de la définition du trait.

### 2. Propager le contexte dans la chaîne d'appels

```
proxy_service.rs (handle_proxy_http)
  └─ target_url_str     ← construite depuis le chemin proxy parsé
  └─ input.entry_point  ← depuis ControlerStreamInput
       │
       ▼  assembler PostActionContext
       │
client.rs (request_proxied)
  └─ a request.url (= target_url_str)
       │
       ▼
client.rs (parse_http_response)
  └─ nouveau paramètre : action_context: &PostActionContext
       │
       ▼
actions/mod.rs (apply_post_actions)
  └─ nouveau paramètre : action_context: &PostActionContext
       │
       ▼  transmettre à chaque action
       │
ReplaceAllAction::post_apply
  └─ reçoit action_context, résout {proxy}, {base_url}, {path}, {path_base}
```

#### Modifications par fichier

| Fichier | Changement |
|---|---|
| `actions/mod.rs` | Ajouter la struct `PostActionContext`. Ajouter le param `action_context` au trait `post_apply`, à `apply_post_actions`, et à `build_action` (si nécessaire). |
| `client.rs` | Ajouter le param `action_context` à `parse_http_response`. Le construire dans `request_proxied` depuis `request.url`. Le transmettre à `parse_http_response`. |
| `proxy_service.rs` | Construire `PostActionContext` depuis `target_url_str` et `input.entry_point`. Le passer à `request_proxied` (ou le faire transiter par `ProxiedHttpRequest`). |
| `replace_all.rs` | Utiliser `action_context` pour résoudre les variables dans `post_apply`. |

### 3. Résolution des variables dans ReplaceAllAction::post_apply

Dans `post_apply`, après le décodage du corps mais avant l'application de la
regex :

```rust
fn resolve_variables(&self, context: &PostActionContext) -> String {
    let entry_point = &context.entry_point;
    let target_url = &context.target_url;

    let base_url = target_url
        .trim_end_matches('/')
        .to_string();

    let path = Url::parse(target_url)
        .ok()
        .map(|u| u.path().to_string())
        .unwrap_or_default();

    let path_base = match path.rfind('/') {
        Some(idx) => path[..=idx].to_string(),
        None => String::new(),
    };

    self.replacement
        .replace("{proxy}", entry_point)
        .replace("{base_url}", &base_url)
        .replace("{path}", &path)
        .replace("{path_base}", &path_base)
}
```

### 4. Dépendance URL

La crate `url` est déjà une dépendance de `arachnea-proxy` (utilisée dans
`client.rs` et `proxy_service.rs`), donc aucune nouvelle crate n'est nécessaire
pour le parsing d'URL.

## Rétrocompatibilité

- Tous les appelants existants de `post_apply`, `apply_post_actions` et
  `parse_http_response` doivent être mis à jour pour passer le nouveau
  paramètre de contexte.
- Lorsqu'aucun service proxy n'est impliqué (ex. utilisation directe du
  client), un `PostActionContext` vide peut être fourni — les variables dans la
  chaîne de remplacement resteront simplement non résolues (pas de crash, pas
  de substitution).
- L'action `ReplaceAll` reste pleinement fonctionnelle sans utiliser les
  variables.

## Plan de test

1. Étendre les tests unitaires existants avec un remplacement contenant
   `{path}` et vérifier que la substitution a lieu.
2. Ajouter un test avec du contenu binaire pour s'assurer que le chemin de
   résolution des variables n'interfère pas avec le contenu non textuel.
3. Ajouter un test avec des variables non résolubles (contexte vide) pour
   confirmer l'absence de crash.
4. Les tests existants (`test_replace_all_text`, `test_replace_all_no_match`,
   etc.) continuent de passer sans modification.

## Questions résolues

- **RemoveHeader et les actions futures doivent-elles aussi recevoir le contexte ?**
  Oui, le trait ajoute le paramètre pour toutes les actions afin de rester
  générique. Les actions qui n'en ont pas besoin ignorent simplement le
  paramètre.
- **Le contexte doit-il être `&PostActionContext` ou
  `Option<&PostActionContext>` ?**
  `&PostActionContext` est retenu — les tests unitaires et les chemins non-proxy
  peuvent passer une valeur par défaut.
- **Faut-il dériver `Default` pour `PostActionContext` ?**
  Oui, pour le rendre ergonomique dans les tests et les chemins d'appel sans
  contexte proxy.

```rust
#[derive(Default)]
pub struct PostActionContext {
    pub entry_point: String,
    pub target_url: String,
}
```
