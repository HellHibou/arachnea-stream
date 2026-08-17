# Analyse : cycle de vie du browser `chaser-cf` — échec des appels suivants

Date : 2026-08-17

## Symptôme observé

Premier appel sur une source Cloudflare : succès. Appels suivants :

```text
ERROR arachnea_scrapyfy::scrapyfy::scraper_agregator: source query failed
  error_code=ARACHNEA_E... operation="get_players" source=papadustream-v2
  error=chaser-cf failure: Failed to initialize browser:
  Browser process exited with status ExitStatus(ExitStatus(0))
  before websocket URL could be resolved, stderr: BrowserStderr("")

WARN chaser_cf::core: ChaserCF dropped without explicit shutdown().
     Call shutdown() for clean resource release.
WARN chaser_oxide::browser: Browser was not closed manually,
     it will be killed automatically in the background
ERROR chaser_oxide::handler: WS Connection error:
     Ws(Io(Os { code: 10054, kind: ConnectionReset, ... }))
```

La séquence des logs détruit l'ordre naïf : le second appel échoue au lancement du
browser, puis les warnings de drop correspondent au premier appel dont le
processus Chrome est abandonné en arrière-plan.

## Cause racine

Trois comportements se combinent pour produire l'échec.

### 1. `user-data-dir` fixe partagé par toutes les instances Chrome

`chaser-oxide` (`browser/config.rs`) utilise un répertoire **constant** quand aucun
`user_data_dir` n'est configuré :

```rust
builder.arg(Arg::value(
    "user-data-dir",
    std::env::temp_dir().join("chromiumoxide-runner").display(),
));
```

`chaser-cf` 0.2.1 n'expose aucun moyen de configurer ce répertoire :

- `ChaserConfig` (`core/config.rs`) n'a pas de champ `user_data_dir`.
- `extra_args` ne peut pas le remplacer : `ArgsBuilder` (`browser/argument.rs`)
  **concatène les valeurs d'une même clé avec des virgules** (`join(",")`), donc un
  `--user-data-dir` personnalisé produirait une valeur invalide écrasée par le
  chemin par défaut ajouté ensuite par `BrowserConfig::launch()`.

Conséquence : deux instances Chrome lancées par chaser-cf/chaser-oxide partagent le
même profil. Chrome refuse alors de démarrer une seconde instance sur un profil
verrouillé : il délègue vers l'instance existante puis **sort immédiatement avec le
code 0** sans afficher `DevTools listening on ws://...`. C'est exactement le message
`Browser process exited with status ExitStatus(ExitStatus(0))` observé.

### 2. `BrowserManager::shutdown()` ne ferme pas réellement Chrome

Dans `chaser-cf` 0.2.1 (`core/browser.rs`) :

```rust
pub async fn shutdown(self) {
    self.healthy.store(false, Ordering::SeqCst);
    // ... ne ferme PAS Browser
}
```

`ChaserCF::shutdown()` (`core/mod.rs`) appelle bien ce `shutdown()`, mais il ne
ferme pas le processus Chrome. La fermeture effective ne se produit qu'au `Drop`
de la structure `Browser` de chaser-oxide (`browser/mod.rs`) :

```rust
impl Drop for Browser {
    fn drop(&mut self) {
        // ...
        tracing::warn!(
            "Browser was not closed manually, it will be killed automatically in the background"
        );
    }
}
```

Le `Child` a `kill_on_drop` : le kill est demandé mais **asynchrone**, sans attente.
Le processus Chrome reste donc vivant un court instant après le drop.

### 3. L'engine et sa façade `ChaserCF` sont recréés entre les appels

`ChaserCfEngine` stocke la façade dans un cache lazy :

```rust
chaser: Arc<RwLock<Option<Arc<ChaserCF>>>>,
```

- Tant que `ArachneaHttpClient` vit, l'engine et son `ChaserCF` vivent aussi.
- Quand un `ArachneaHttpClient` est reconstruit (nouveau `HttpClient` par
  `HttpClient::configured()` avec une config de transport différente, invalidation
  du cache proxy, ou recréation d'un query client), l'engine est droppé :
  - `ChaserCF` droppé → warning `ChaserCF dropped without explicit shutdown()`;
  - `Browser` droppé → kill asynchrone de Chrome → `WS Connection error: 10054`;
  - au prochain appel, un nouveau `ChaserCF` lance un nouveau Chrome sur le même
    `user-data-dir` fixe, encore verrouillé par l'ancien processus en cours de
    destruction → échec `ExitStatus(0)`.

Cas déclencheur concret rencontré (papadustream `get_players`) : les sous-queries
`execution: page_click` ouvrent une `ChaserCfPageSession`, qui crée un
`BrowserManager` dédié par session. Chaque session fermée (`close()`) appelle un
`shutdown()` qui ne ferme pas Chrome, puis abondonne le processus. Une session
suivante (ou un solve `solve_waf_session` de la façade) relance Chrome immédiatement
sur le même profil verrouillé.

## Impacts

- Échec intermittent des requêtes Cloudflare à partir du deuxième appel.
- Fuite de processus Chrome en arrière-plan pendant le kill asynchrone.
- Aucune garantie qu'une `ChaserCfPageSession` ou un solve de façade puisse être
  suivi d'un autre sans délai de nettoyage.

## Pointeurs de code

| Fichier | Point |
| --- | --- |
| `chaser-oxide` `browser/config.rs:393-403` | `user-data-dir` fixe `chromiumoxide-runner` |
| `chaser-oxide` `browser/argument.rs:15-40` | Concaténation des args empêchant la surcharge du `user-data-dir` |
| `chaser-oxide` `browser/mod.rs:519-537` | `Drop` de `Browser` : kill asynchrone |
| `chaser-cf` `core/browser.rs:274-280` | `BrowserManager::shutdown()` ne ferme pas Chrome |
| `chaser-cf` `core/mod.rs:104-112` | `ChaserCF::shutdown()` délègue au no-op |
| `arachnea-http` `engine/chaser_cf.rs:117-125` | Cache lazy de la façade `ChaserCF` |
| `arachnea-http` `engine/chaser_cf.rs:419-497` | `ChaserCfPageSession` ouvre un `BrowserManager` dédié |
| `arachnea-scrapyfy` `http_client.rs:517-546` | `configured()` peut créer un nouveau `HttpClient` donc un nouvel engine |

## Solutions possibles

### A. Centraliser un unique browser chaser-cf au niveau processus (recommandé)

Faire vivre un `Arc<ChaserCF>` **partagé par tout le processus** (ou au moins par
toutes les instances `ChaserCfEngine`), comme le fait déjà
`global_cookie_cache()` pour les cookies. Le `ChaserCF` et son `BrowserManager`
ne seraient alors plus jamais droppés pendant la durée de vie du process, évitant
le kill asynchrone et le relancement sur un profil verrouillé.

- **Avantage** : corrige le problème pour tous les chemins (solve de façade et
  sessions de page), aucune dépendance externe à changer.
- **Inconvénient** : un unique profil Chrome pour tout le process ; il faut
  maintenir un état global dans `arachnea-http`.

### B. Fermer proprement Chrome avant le drop

Implémenter un `Drop` réel sur les structures qui possèdent un `Browser` :
appeler la fermeture CDP (`Browser::close()`) avant de relâcher le processus ou,
à défaut, attendre le `kill` synchrone dans `BrowserManager::shutdown()`.

- **Avantage** : corrige la fuite de processus.
- **Inconvénient** : nécessite de patcher `chaser-cf`/`chaser-oxide` (dépendances
  externes) ou de reproduire la logique côté `arachnea-http` ; le `user-data-dir`
  fixe reste un risque de verrouillage résiduel entre une fermeture propre et le
  prochain lancement.

### C. Isoler le `user-data-dir` par instance

Permettre un `user_data_dir` unique par `ChaserConfig` (nouvelle option dans
`chaser-cf`, et `chaser-oxide` doit cesser de concaténer la clé
`user-data-dir`). Chaque instance Chrome utilise alors un profil privé.

- **Avantage** : élimine le verrouillage de profil à la racine.
- **Inconvénient** : patch de dépendances externes ; n'évite pas le kill asynchrone
  et la recréation d'engine.

### D. Mutualiser les sessions `ChaserCfPageSession`

Éviter de créer un `BrowserManager` par session de page : ouvrir toutes les pages
sur le browser unique de la façade (`ChaserCF`), plutôt qu'un browser dédié par
`open_browser_page_session`.

- **Avantage** : un seul Chrome, une seule profil.
- **Inconvénient** : refonte de `open_browser_page_session` et de la
  `ChaserCfPageSession` ; dépend de la solution A pour être pleinement efficace.

## Recommandation retenue et implémentée

La solution A + B (façade `ChaserCF` partagée + fermeture propre) s'est révélée
**insuffisante** : elle partageait chaque type de browser séparément, mais la
façade `ChaserCF` (cas 1, `solve_waf_session`) et le `BrowserManager` des
sessions de page (cas 2, `open_browser_page_session`) lançaient toujours **deux
Chrome distincts** sur le même `user-data-dir` fixe `chromiumoxide-runner`.
Quand le cas 1 avait lancé son Chrome, le cas 2 tentait d'en lancer un second
sur le même profil verrouillé → Chrome délègue et sort avec `ExitStatus(0)`.

La solution finale implémentée dans `chaser_cf.rs` :

1. **Un seul `BrowserManager` process-wide** (`GLOBAL_BROWSER`) partagé par
   toutes les instances `ChaserCfEngine` et toutes les sessions de page.
2. **`solve_waf_session` réimplémenté directement** sur ce browser partagé au
   lieu de passer par la façade `ChaserCF` : navigation, attente passive de
   `cf_clearance` (6 s), clic du challenge Turnstile via CDP shadow-root
   (`GetDocumentParams` + `GetBoxModelParams` + courbe de curseur), extraction
   des cookies et du user-agent.
3. **`fetch_page_source` réimplémenté** sur le même browser partagé.
4. **`ChaserCfPageSession`** ouvre ses pages sur le même browser partagé et ne
   ferme plus le browser à la fermeture de session.

Résultat : exactement **un seul Chrome** pour les deux cas, plus aucun conflit
de verrouillage `user-data-dir` entre un solve WAF et une session de page.
