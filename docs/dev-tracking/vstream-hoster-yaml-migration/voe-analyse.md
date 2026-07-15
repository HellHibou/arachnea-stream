# Analyse : migration de `voe.py` vers résolveur YAML

> Créée le 2026-07-15.
> Source : `docs/private/plugin.video.vstream-3.9.2/plugin.video.vstream/resources/hosters/voe.py`
> URL de test : `https://ellenpoliticalfollow.com/e/hqaji6plklmn`
> Résolveur : VOE (voe.sx, voe-sx.com et domaines miroirs)

## État actuel

- Le fichier `voe.py` est marqué « ⬜ À qualifier — extraction/domaines à relever » dans le tableau de migration (ligne `voe.py`, p.424 du document de migration).
- Le tag « ✅ Signature Voe » dans la colonne Sonde `/e`/`/v` confirme que le fallback dynamique reconnaît ce hoster, sans pour autant le résoudre.
- Aucun code Rust existant pour VOE dans `server/` (vérifié par grep).
- Aucun YAML `voe.yaml` ou équivalent dans `services.json` ou `services/arachnea-stream-resolver/`.

## Analyse du protocole vStream — `voe.py`

### Algorithme complet (lignes 12-67)

```
1. GET {url} avec User-Agent: Mozilla/5.0 ...
   ↓
2. [REDIRECTION CONDITIONNELLE]
   Si la réponse HTML contient "const currentUrl" :
   → extraire window.location.href = '<url>'
   → GET <url> avec même UA (remplace l'URL courante)
   ↓
3. [EXTRACTION CODE + JS PATH]
   Pattern : json">["([^"]+)"]</script>\s*<script\s*src="([^"]+)
   → groupe 1 : code   (string encodé en base64-like après transformations)
   → groupe 2 : js_path (ex: /assets/js/main.abc123.js)
   ↓
4. [SOUS-REQUÊTE JS]
   url2 = host + js_path
   GET url2 avec UA
   ↓
5. [EXTRACTION LUT]
   Pattern : (\[(?:'\W{2}'[,\]]){1,9})
   → capture : array JS littéral comme ['a%','b^','c$',]
   ↓
6. [DÉCODAGE] voe_decode(code, luts)
```

### `voe_decode` en détail (lignes 71-87)

```python
def voe_decode(ct, luts):
    # Φ1 : Parser la LUT
    lut = [''.join([('\\' + x) if x in '.*+?^${}()|[]\\'
                    else x for x in i])
           for i in luts[2:-2].split("','")]
    #   → ['a\\%', 'b\\^', 'c\\$'] (caractères regex échappés)

    # Φ2 : ROT13 modifié sur le code
    txt = ''
    for i in ct:
        x = ord(i)
        if 64 < x < 91:     # A-Z (65-90)
            x = (x - 52) % 26 + 65
        elif 96 < x < 123:  # a-z (97-122)
            x = (x - 84) % 26 + 97
        txt += chr(x)

    # Φ3 : Supprimer chaque motif LUT du texte
    for i in lut:
        txt = re.sub(i, '', txt)

    # Φ4 : Premier base64 decode
    ct = base64.b64decode(txt)

    # Φ5 : Byte shift -3
    # Φ6 : Reverse
    txt = ''.join([chr(i - 3) for i in ct])
    txt = txt[::-1]

    # Φ7 : Second base64 decode
    txt = base64.b64decode(txt)

    # Φ8 : JSON parse
    return json.loads(txt)
```

### Transformation pas à pas

| Étape | Opération | Entrée | Sortie |
|---|---|---|---|
| Φ1 | Parse LUT | `['a%','b^','c$',]` | `['a\\%', 'b\\^', 'c\\$']` |
| Φ2 | ROT13 modifié | `"Uryyb%Jbeyq^"` | `"Hello%World^"` |
| Φ3 | Suppression LUT | `"Hello%World^"` | `"HelloWorld"` |
| Φ4 | base64_decode | `"SGVsbG9Xb3JsZA=="` | `[72, 101, 108, 108, 111, 87, 111, 114, 108, 100]` |
| Φ5 | bytes -3 | `[69, 98, 105, 105, 108, 84, 108, 111, 105, 97]` |
| Φ6 | reverse | `[97, 105, 111, 108, 84, 108, 105, 105, 98, 69]` |
| Φ7 | base64_decode | `"hWlsV"` | → texte après second décodage |
| Φ8 | json.loads | `'{"file":"https://..."}'` | `{file: "https://..."}` |

### Sortie finale du hoster (ligne 62-63)

```python
file = s.get('file') or s.get('source')
api_call = file + '|User-Agent=' + UA
# → "https://cdn...mp4|User-Agent=Mozilla/5.0..."
```

L'en-tête `User-Agent` est passé via le pipe pour le proxy, pas comme en-tête HTTP standard. Dans le contexte YAML, cela deviendrait une configuration `stream_headers` ou `proxy_headers`.

## Contraintes de migration

### Ce qui est EXPRIMABLE en YAML + actions existantes

| Opération | Action existante | Remarque |
|---|---|---|
| GET page | `get_response_body` (via `scraper_type: html`) | OK |
| Regex extraction (code, js_path) | `regex_find_all` | OK |
| Construction URL (host + path) | `format_text` ou `resolve_url` | OK |
| GET sous-requête JS | `sub_queries` (entry-level) | OK |
| Regex extraction LUT | `regex_find_all` dans sub-query | OK |
| Base64 decode | `base64_decode` | OK |
| JSON parse + field extract | `extract_field` | OK |
| Proxy resolve | `resolve_url { proxy: true }` | OK — avec `User-Agent` en `proxy_headers` |

### Ce qui NÉCESSITE de nouvelles actions génériques

| Opération | Action proposée | Générique ? |
|---|---|---|
| ROT13 modifié sur A-Z, a-z, décalage configurable | `rot13 { shift: 26 }` | Oui — Caesar configurable |
| Supprimer occurrences de motifs (re.sub) | `regex_remove_all { patterns: [...] }` | Oui — proche de `replace_text` mais avec regex |
| Soustraire constante à chaque byte | `bytes_shift { value: -3 }` | Oui — shift générique |
| Inverser byte array | `reverse` | Oui — inverser string/bytes |

### Le vrai gap architectural

Les motifs LUT sont **dynamiques** — extraits d'une sous-requête HTTP (le JS). Impossible de les écrire en dur dans le YAML.

**Problème** : dans le modèle entry + sub_query actuel, la sous-requête remplace la valeur de l'entrée parente mais les actions pipeline d'UNE entrée ne peuvent pas référencer les valeurs d'UNE AUTRE entrée.

```
Entry "code"    → get_response_body → regex_find_all(code_pattern)
                   ↑ actions pipeline — peut appliquer rot13, base64, etc.
                   MAIS n'a pas accès aux motifs LUT de l'autre entrée

Entry "js_url"  → regex_find_all(js_pattern)
                   → sub_query → fetch JS → regex_find_all(lut_pattern)
                   ↓ résultat remplace js_url par lut[]
```

## Appareillage mécanique disponible

### Mécanismes Scrapyfy existants

#### `sub_queries` sur une entry

**Fichier** : `server/crates/arachnea-scrapyfy/src/scrapyfy/scraper/query_executor.rs` (lignes 1137-1392)

Quand une entry possède `sub_queries` et que `request_pointer` est absent (ou vaut le sentinel), **la valeur de l'entry devient l'URL à fetch**. Le résultat de la sous-requête **remplace** la valeur de l'entry parente.

```yaml
# Schéma d'une entry avec sub_query
entries:
  - name: js_url
    selector: "script[src]"               # ou extraction via actions
    actions:
      - type: regex_find_all
        pattern: 'json">\["[^"]+"\]</script>\s*<script\s*src="([^"]+)"'
        format: "{host}{1}"
    sub_queries:
      - scraper_type: html
        row_selector: "html"
        entries:
          - name: lut
            type: string
            actions:
              - type: get_response_body
              - type: regex_find_all
                pattern: "(\[(?:'(?:\W{2})'[,,\]]){1,9})"
                format: "{1}"
        # Le résultat { lut: "['a%','b^']" } remplace js_url
```

#### Post-processes existants

**Fichier** : `server/crates/arachnea-scrapyfy/src/scrapyfy/post_processes/mod.rs`

| Post-process | Utilité pour VOE |
|---|---|
| `ComputeItemsField` | Évalue une expression mathématique avec variables depuis d'autres champs — NE convient PAS pour du texte |
| `ExtractRegexItems` | Extrait des items depuis un champ texte via regex |
| `SetNestedFields` | Copie/champs statiques dans des items imbriqués |
| `FetchRegexItemsFromItems` | Fetch HTTP par item + extraction regex |

Aucun ne permet de combiner deux champs scalaires dans un pipeline d'actions.

#### Actions disponibles (23 actions)

Liste complète dans `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/mod.rs` (ligne 53-267).

Pertinentes pour VOE :
- `get_response_body` — body brut
- `regex_find_all` — extraction par regex
- `base64_decode` — Base64 standard
- `resolve_url` — résolution + proxy
- `format_text` — template string
- `replace_text` — substitution string
- `extract_field` — extraction champ JSON
- `unpack_packer` — Dean Edwards Packer (NON compatible VOE)

### Actions à créer

#### `rot13`

```rust
struct Rot13 {
    /// Caesar shift applied to letters (default 13).
    /// VOE uses 26 = standard ROT13 on both upper and lower case.
    shift: Option<u8>,
    /// Character ranges to transform. VOE: A-Z (65-90), a-z (97-122).
    /// Default includes both.
    ranges: Option<Vec<(u8, u8)>>,
}
```

Pipeline :
```
"Uryyb%Jbeyq^" → rot13 { shift: 26 } → "Hello%World^"
```

#### `regex_remove_all`

Deux modes :
- **Mode statique** : `patterns` en dur dans le YAML
- **Mode référencé** : `source_field` pointe vers un autre champ du même item

```rust
struct RegexRemoveAll {
    /// Inline patterns (static mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patterns: Vec<String>,
    /// Read patterns from a sibling field (dynamic mode).
    /// Needed for VOE LUT which comes from sub-query result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_field: Option<String>,
}
```

Le mode `source_field` nécessite que l'action ait accès au `ScraperDataNode` courant (tous les champs de l'item), et pas seulement la valeur courante du pipeline.

#### `bytes_shift`

```rust
struct BytesShift {
    /// Value to add to each byte (negative for subtraction).
    value: i8,
}
```

Pipeline :
```
[72, 101, 108, 108, 111] → bytes_shift { value: -3 } → [69, 98, 105, 105, 108]
```

#### `reverse`

```rust
struct Reverse;
```

Pipeline :
```
"hello" → reverse → "olleh"
[69, 98, 105] → reverse → [105, 98, 69]
```

## Architecture de la sous-requête LUT

### Problème

Les motifs LUT sont extraits du JS sous forme de string JS array : `['a%','b^','c$',]`.

Pour les passer à `regex_remove_all`, il faut :
1. Extraire le string brut `"['a%','b^','c$',]"`
2. Le parser en Vec\<String\>
3. Le passer comme `patterns` à l'action

### Solutions envisagées

#### Solution A — Nouveau mode `source_field` sur `regex_remove_all`

L'action reçoit un nom de champ (ex: `lut`), lit ce champ dans l'item courant, parse comme array JS, et applique les suppressions.

```yaml
entries:
  - name: code
    type: string
    selector: "html"   # ou body
    actions:
      - type: get_response_body
      - type: regex_find_all
        pattern: 'json">\["([^"]+)"\]</script>'
        format: "{1}"

  - name: js_url
    type: string
    actions:
      - type: get_response_body
      - type: regex_find_all
        pattern: '<script\s*src="([^"]+)"'
        format: "{host}{1}"
    sub_queries:
      - scraper_type: html
        row_selector: "html"
        entries:
          - name: lut
            type: string
            actions:
              - type: get_response_body
              - type: regex_find_all
                pattern: (\[(?:'\W{2}'[,\]]){1,9})
                format: "{1}"

post_process:
  # Appliquer le décodage complet sur le champ `code` en utilisant `lut` comme source de motifs
  - type: compute_stream
    source: code
    lut_field: lut
    actions:
      - type: rot13
        shift: 26
      - type: regex_remove_all
        source_field: lut
      - type: base64_decode
      - type: bytes_shift
        value: -3
      - type: reverse
      - type: base64_decode
      - type: extract_field
        path: "/file"
      - type: resolve_url
        proxy: true
        proxy_headers:
          User-Agent: "Mozilla/5.0 ..."
```

**Inconvénient** : nécessite un nouveau mécanisme (post-process avec pipeline d'actions référençant un champ source).

#### Solution B — Fusionner tout dans un post-process `ComputeItemsField` étendu

Étendre `ComputeItemsField` pour supporter des actions (pas seulement des maths) et des variables de type texte.

**Inconvénient** : dévie du rôle initial (maths) et complexifie le post-process.

#### Solution C — Une seule action `voe_decode` (rejetée par le user)

Monolithique mais simple. Ne correspond pas à l'approche « primitives réutilisables ».

#### Solution D — Actions avec slot `input_field`

Toute action Scrapyfy pourrait optionnellement lire son entrée depuis un champ nommé plutôt que depuis la valeur courante du pipeline :

```yaml
- type: regex_remove_all
  input_field: lut    # ← optionnel : lire depuis ce champ au lieu de la valeur courante
```

**Avantage** : mécanisme générique qui profite à toutes les actions.
**Inconvénient** : changement architectural dans le dispatch des actions (modification de `ScraperAction::apply`).

## Actions recommandées

### Priorité 1 — Actions atomiques (indépendantes, sans dépendance entre champs)

| Action | Fichier | Complexité |
|---|---|---|
| `rot13` | `actions/rot13.rs` | Faible — transformation string |
| `bytes_shift` | `actions/bytes_shift.rs` | Faible — `u8::wrapping_add` |
| `reverse` | `actions/reverse.rs` | Très faible — `str::chars().rev()` |
| `regex_remove_all` (mode statique) | `actions/regex_remove_all.rs` | Moyenne — similaire à `replace_text` |

### Priorité 2 — Résolution du gap dynamique

Choix à faire entre :
- **Mode `source_field`** sur `regex_remove_all` (minimal, spécifique)
- **Slot `input_field`** optionnel sur toutes les actions (générique, plus de code)

### Priorité 3 — Intégration VOE

1. Créer les 4 actions atomiques + les ajouter à `ScraperAction` et au dispatch
2. Documenter dans les specs Scrapyfy française et anglaise
3. Créer `voe.yaml` dans `services/arachnea-stream-resolver/`
4. Ajouter l'entrée dans `services.json`
5. Tester avec l'URL autorisée

## YAML cible (esquisse, solution à valider)

```yaml
id: voe-resolver
title: VOE hoster
description:
  en: "Resolver for voe.sx and mirror domains"
http:
  mode: auto
  headers:
    User-Agent: "Mozilla/5.0 (Windows NT 6.1; Win64; x64; rv:66.0) Gecko/20100101 Firefox/66.0"

parameters:
  - name: url
    value: ""
    description: "URL of the VOE embed page"

queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{url}"
        actions:
          - type: regex_find_all
            pattern: '^https?://(?:www\.)?(?:voe|voesx|ellenpoliticalfollow)\.(?:sx|com|ru)/e/'
            format: "{service_id}"

  - name: resolve_stream
    scraper_type: html
    base_url: "{url}"
    query_url: "{url}"
    row_selector: "html"
    entries:
      # --- encoded code from the page ---
      - name: code
        type: string
        actions:
          - type: get_response_body
          - type: regex_find_all
            pattern: 'json">\["([^"]+)"\]</script>\s*<script\s*src="([^"]+)"'
            format: "{1}"
      # --- JS url → sub-query → LUT patterns ---
      - name: js_url
        type: string
        actions:
          - type: get_response_body
          - type: regex_find_all
            pattern: 'json">\["([^"]+)"\]</script>\s*<script\s*src="([^"]+)"'
            format: "{host}{2}"
        sub_queries:
          - scraper_type: html
            row_selector: "html"
            entries:
              - name: lut
                type: string
                actions:
                  - type: get_response_body
                  - type: regex_find_all
                    pattern: (\[(?:'\W{2}'[,\]]){1,9})
                    format: "{1}"
    # --- post-query action pipeline (mécanisme à créer) ---
    # Équivalent conceptuel — le mécanisme exact dépend de la solution choisie
    post_process:
      - type: apply_actions_to_field
        field: code
        actions:
          - type: rot13
          - type: regex_remove_all
            source_field: lut
          - type: base64_decode
          - type: bytes_shift
            value: -3
          - type: reverse
          - type: regex_find_all
            pattern: '(?:"file"|"source")\s*:\s*"([^"]+)"'
            format: "{1}"
      # --- stream_url entry ---
      - name: stream_url
        type: string[]
        actions:
          - type: extract_field
            path: "/result"
          - type: resolve_url
            proxy: true
            proxy_headers:
              User-Agent: "Mozilla/5.0 (Windows NT 6.1; Win64; x64; rv:66.0) Gecko/20100101 Firefox/66.0"
      - name: manifest_type
        type: string
        value: mp4
```

## Points à trancher

1. **Mode `source_field` sur `regex_remove_all`** ou **slot `input_field` générique** ?
2. Où placer la pipeline de décodage final ?
   - Nouveau post-process `apply_actions_to_field`
   - Extension de `ComputeItemsField`
   - `ExtractRegexItems` amélioré
3. Le `request_actions` de la sub-query peut-il construire l'URL JS ? (`{host}{js_path}` avec `format_text`)
4. La sous-requête doit-elle être `html` (avec `row_selector: "html"`) ou plutôt `json` pour un texte brut ?
   → Le JS n'est pas du HTML structuré — utiliser `scraper_type: html` avec `row_selector: "html"` fonctionne car le body est traité comme un seul élément texte.

## Références

- Résolveur Packer de référence : `unpack_packer.rs` dans `server/crates/arachnea-scrapyfy/src/scrapyfy/actions/`
- Dispatch des actions : `mod.rs` dans le même dossier (ligne 289-363, méthode `apply`)
- Sub-query entry-level : `query_executor.rs` (ligne 1137-1392)
- `SubQueryCommon` : `scraper/config.rs` (ligne 245-323)
- `HtmlScraperSubQueryRaw` : `scraper_html/config.rs` (ligne 258-288)
- `EntrySubQueryRaw` : `scraper_json/config.rs` (ligne 96-193)
