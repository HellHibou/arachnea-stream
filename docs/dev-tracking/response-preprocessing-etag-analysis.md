# Analyse : pré-traitement de réponse avant parsing et ETag

> Créé le 2026-08-24  
> Portée proposée : `arachnea-scrapyfy`, documentation du schéma YAML et `papadustream-v2.yaml`  
> Statut : analyse — aucune implémentation dans ce document

## 1. Contexte

La validation conditionnelle actuelle construit le fragment de fallback `C:`
sur le corps texte brut de la requête racine. Cette opération se produit dans
`fetch_responses_with_validation`, après la réponse HTTP finale (redirections
et éventuel moteur Cloudflare déjà appliqués) et avant la conversion vers HTML,
JSON ou texte.

Ce comportement est cohérent avec le document fourni au parseur, mais il est
trop sensible aux blocs techniques qui ne participent pas à l'extraction.
PapaDuStream en est un exemple : deux réponses identiques pour les lecteurs
utiles diffèrent par des jetons Cloudflare injectés dans un lien
`/cdn-cgi/content` et un script `window.__CF$cv$params`. Aucun ETag distant ni
`Last-Modified` n'est fourni. Le hash brut change donc à chaque appel et
empêche un `304`.

## 2. État actuel et limites

Le flux simplifié actuel est :

```text
réponse HTTP finale
  -> lecture du texte brut
  -> fragment ETag E: ou C: basé sur ce texte
  -> conversion HTML / JSON / texte
  -> extraction des champs
  -> post_process sur les données extraites
```

`post_process` agit sur les `ScraperDataNode` extraits ; il est donc trop tard
pour influencer le parsing ou le fragment ETag. Les actions de champs agissent
elles aussi sur des valeurs déjà extraites. Aucune couche ne transforme le
corps entier entre le téléchargement et le parsing.

Les sous-requêtes n'alimentent pas aujourd'hui le fragment de validation de la
requête racine. La proposition porte d'abord sur chaque réponse traitée, avec
l'application effective de l'ETag limitée au root comme aujourd'hui.

## 3. Objectif

Introduire un pré-traitement optionnel, indépendant de `scraper_type`, qui :

1. reçoit le corps texte final retourné par le client HTTP ;
2. applique une liste ordonnée de pré-actions propres à la réponse ;
3. utilise le résultat pour le fallback `C:` et la comparaison de hash ;
4. parse ce même résultat pour les scrapers HTML, JSON et texte.

Le résultat est qu'un élément explicitement supprimé par configuration ne peut
ni modifier l'ETag fallback ni influencer l'extraction.

## 4. Proposition de schéma YAML

Ajouter le champ optionnel suivant aux requêtes racines et sous-requêtes :

```yaml
pre_process:
  - type: remove_text_blocks
    start: '<script data-volatile="true">'
    end: '</script>'
```

Le nom `pre_process` distingue clairement cette phase de
`post_process`, qui reste appliquée aux données extraites.

### 4.1 Première pré-action : `remove_text_blocks`

Contrat proposé :

| Champ | Type | Rôle |
|---|---|---|
| `type` | littéral | `remove_text_blocks` |
| `start` | chaîne non vide | Délimiteur de début du bloc à supprimer. |
| `end` | chaîne non vide | Délimiteur de fin du bloc à supprimer. |

L'action traite toutes les occurrences, dans l'ordre du texte. Pour éviter une
suppression imprévisible :

- chaque recherche du `end` commence après le `start` trouvé ;
- un `start` sans `end` produit une erreur contextualisée ;
- aucune interprétation regex n'est implicite ; les délimiteurs sont des
  chaînes littérales ;
- une action absente (`start` non trouvé) est un no-op.

Une action regex distincte pourra être envisagée plus tard si nécessaire, mais
ne doit pas être introduite pour ce premier cas : des bornes littérales rendent
le YAML plus lisible et limitent le risque de regex coûteuse sur des réponses
volumineuses.

## 5. Point d'insertion technique

La modification doit être centralisée dans
`scrapy/query_executor.rs`, au niveau de la réponse texte obtenue avant le
`match query.scraper_type()` :

```text
send_for_request(...)
  -> response.text()
  -> apply_pre_processes(text)                [nouveau]
  -> fragment_from_response(..., text)
  -> comparaison du hash entrant avec text
  -> serde_json::from_str(text) / HTML / Text
```

Le même chemin est requis pour :

- le fetch conditionnel de la requête racine ;
- le fetch normal de requêtes et de sous-requêtes ;
- `page_navigate`, appliqué au HTML rendu avant le calcul de son hash.

La logique doit rester unique : dupliquer le pré-traitement dans les branches
HTML et JSON finirait par faire diverger le contenu hashé et le contenu parsé.

Chaque requête possède sa propre liste `pre_process`, y compris une
sous-requête. Les pré-actions d'une requête parente ne sont pas héritées ni
réappliquées à ses sous-requêtes : chaque corps HTTP est transformé uniquement
avec la configuration de la requête qui l'a récupéré.

### 5.1 Cas particulier : `input_html`

`input_html` n'est pas une nouvelle réponse HTTP dans l'exécuteur actuel. Il
est d'abord résolu comme un template avec les paramètres du contexte, puis est
directement emballé dans `FetchedResponse::Html`, sans passer par
`fetch_responses`. En pratique, il peut donc contenir le corps ou un fragment
issu d'une réponse HTTP récupérée une seule fois par une requête parente, puis
être consommé par plusieurs requêtes avec des hôtes ou des types de scraper
différents.

Lui appliquer automatiquement le `pre_process` de la requête qui fournit la
réponse serait incorrect : la provenance HTTP n'est plus disponible à ce
point et les consommateurs peuvent légitimement avoir des besoins distincts.
Lui appliquer celui de chaque consommateur modifierait, au contraire, le
contenu avant son parsing local sans modifier le corps ni la validation de la
requête qui l'a téléchargé.

La première implémentation doit donc **exclure `input_html`** du mécanisme,
afin de préserver cette sémantique. Une évolution dédiée devra préciser :

1. si `input_html` représente un document complet ou un fragment transporté ;
2. quel propriétaire déclare les pré-actions (producteur, consommateur, ou les
   deux) ;
3. si un hash doit être associé au document partagé et comment éviter qu'un
   pré-traitement propre à un consommateur altère la validation des autres.

Cette analyse sera menée sur les configurations qui utilisent réellement
`input_html` avant toute prise en charge.

## 6. Modèle Rust proposé

Ajouter un type dédié, séparé de `ScraperAction` :

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreProcessAction {
    RemoveTextBlocks {
        start: String,
        end: String,
    },
}
```

Les définitions unifiées de requêtes et de sous-requêtes exposeront
`pre_process: Vec<PreProcessAction>`. Le trait `ScraperQuery`
devra fournir une vue sur cette liste, comme il le fait déjà pour
`post_processes`.

Une fonction pure `apply_pre_processes(body, actions)` renverra le
texte transformé ou une erreur. Elle sera testable sans HTTP, parser HTML ou
parser JSON.

## 7. Configuration PapaDuStream proposée

La requête `get_players` devra supprimer les deux blocs techniques observés,
avant le parse HTML :

```yaml
pre_process:
  - type: remove_text_blocks
    start: '<a href="https://papadustream.boo/cdn-cgi/content?id='
    end: '</a>'
  - type: remove_text_blocks
    start: '<script>(function(){function c(){var b=a.contentDocument'
    end: '</script>'
```

Ces sélecteurs sont volontairement ancrés dans du contenu technique absent de
la liste `.player-list` et de `var dle_login_hash`, les seules zones utilisées
par cette requête. Ils devront être confirmés avec un fixture avant livraison,
car le fournisseur peut modifier son script Cloudflare.

## 8. Impact ETag

| Situation | Comportement après implémentation |
|---|---|
| ETag HTTP distant disponible | Le fragment `E:` reste prioritaire ; le pré-traitement n'altère pas sa valeur. |
| Pas d'ETag distant | Le `C:` est calculé sur le texte pré-traité. |
| `If-Modified-Since` + hash | Le hash comparé est celui du texte pré-traité. |
| Contenu volatile supprimé | N'invalide plus le `C:`. |
| Contenu utile modifié | Modifie toujours le `C:` et produit une réponse `200`. |

Le pré-traitement ne doit pas changer la sémantique des fragments `E:`, `C:`,
`N:` et `S:`. Il change uniquement l'entrée du fallback de contenu `C:`.

## 9. Validation à prévoir lors de l'implémentation

- tests unitaires de `remove_text_blocks` : une occurrence, plusieurs
  occurrences, bloc absent, fin absente, conservation des délimiteurs ;
- test d'intégration HTML : contenu pré-traité utilisé à la fois pour le hash
  et l'extraction ;
- test JSON : suppression appliquée avant `serde_json::from_str` ;
- test de non-régression : sans `pre_process`, le comportement actuel
  est strictement conservé ;
- test de non-régression : une sous-requête n'applique que son propre
  `pre_process`, jamais celui de la requête parente ;
- fixture PapaDuStream avec deux jetons Cloudflare différents et même liste de
  lecteurs, vérifiant un fragment `C:` identique après pré-traitement ;
- mise à jour des deux spécifications de schéma :
  `docs/specifications/arachnea-scrapyfy-fr.md` et
  `docs/specifications/arachnea-scrapyfy-en.md`.

## 10. Décision à confirmer avant implémentation

La solution retenue est `pre_process` avec `remove_text_blocks` littéral,
centralisé dans l'exécuteur. Un début sans fin doit produire une erreur
contextualisée. Elle répond au cas PapaDuStream sans introduire de logique
spécifique à une source dans Rust.

L'implémentation touchera les types de requêtes, leur désérialisation, le trait
unifié, l'exécuteur polymorphe, la documentation du schéma et le YAML
PapaDuStream : c'est donc une évolution structurelle de Scrapyfy qui nécessite
confirmation avant réalisation.
