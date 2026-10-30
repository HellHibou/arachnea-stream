# Spécification de rédaction YAML arachnea-stream-hoster

## 1. Responsabilité et prérequis

Ce groupe reconnaît les pages de lecteurs vidéo externes et extrait les flux
lisibles. Il ne fournit ni catalogue, ni recherche, ni saisons, ni listes
d'épisodes. Le YAML du catalogue émet un descripteur de lecteur ; ce groupe
résout sa cible.

Consulter [la spécification YAML commune](arachnea-scrapyfy-fr.md) pour les champs
de collection, HTTP, entrées, actions, sous-requêtes, prétraitements et
post-traitements. Consulter [la spécification stream](arachnea-stream-fr.md) pour
les descripteurs de lecteurs et les métadonnées publiques de lecture. Ces documents
contiennent le contrat de rédaction ; aucune recherche dans les sources n'est
nécessaire.

Placer une collection par hébergeur dans `server/services/arachnea-stream-hoster/`.
Inscrire son fichier dans le `services.json` du dossier :

```json
[{"path":"example-hoster.yaml","enabled":true}]
```

Ajouter au tableau existant sans remplacer les autres hébergeurs. L'ordre du
manifeste définit la priorité de résolution. Les ID de collection doivent être
uniques dans le groupe. Une collection doit avoir `resolve_stream` et au moins
une reconnaissance utilisable pour être sélectionnée. Chaque reconnaissance est
facultative individuellement ; les omettre toutes les deux rend la collection
inaccessible par le résolveur générique.

## 2. Appelant et entrées des opérations

Un catalogue peut produire ce descripteur JSON de lecteur avec des entrées YAML :

```json
{"name":"Example host","resolver":"stream-resolver",
 "target":"https://player.example.org/embed/123"}
```

La forme objet historique équivalente utilise `resolver > kind: stream-resolver`
et `resolver > target_id: <URL du lecteur>`. La cible doit utiliser HTTP(S).

| Requête | Type conseillé | Entrées | Sortie utile obligatoire |
|---|---|---|---|
| `can_resolve_url` | `static` | `url`, `origine` | Scalaire `resolver` non vide si l'URL correspond |
| `can_resolve_html` | `static` ou `html` | `url`, `origine`, `html` | Scalaire `resolver` non vide si le document correspond |
| `resolve_stream` | `html`, `json` ou `text` | `url`, `origine` ; `html` seulement pendant le repli HTML | Une ligne contenant `stream_url` ou un `error_message` terminal |

`url` est la cible originale pour la reconnaissance et l'extraction directes,
puis l'URL finale après redirections pour la reconnaissance et l'extraction HTML.
`origine` s'écrit volontairement ainsi : schéma, nom d'hôte et port facultatif,
sans chemin final. `html` est le document récupéré, pas une URL. Les paramètres
communs `service_*` et `{source}` sont aussi disponibles. Ne pas supposer la
présence de paramètres de recherche ou pagination du catalogue. L'application
fournit son chemin de proxy public lorsqu'il est configuré ; ne pas coder en dur
son nom d'hôte ou son port.

La reconnaissance vérifie **seulement qu'une ligne contient une valeur `resolver`
non vide**. Cette valeur ne choisit pas un autre service ; émettre `{service_id}`
pour la clarté. Une regex sans correspondance ne doit produire aucune valeur,
pas la chaîne `false`, qui est non vide et provoquerait une reconnaissance.
Reconnaître un nom d'hôte précis ou une signature distinctive du document.
Ne pas reconnaître toute page HTML contenant une balise générique `<video>`.

## 3. Ordre de résolution et échecs

1. Parcourir les hébergeurs actifs dans l'ordre du manifeste. Exécuter
   `can_resolve_url` ; une reconnaissance absente ou négative saute l'extraction
   directe pour cet hébergeur.
2. En cas de reconnaissance positive, exécuter `resolve_stream` avec une requête
   réseau. Seule la première ligne de sortie est consommée. Un flux utilisable
   est retourné immédiatement. Une ligne absente ou un flux vide fait continuer
   avec l'hébergeur suivant. Un échec technique direct n'arrête pas seul la recherche.
3. Si aucune extraction directe ne réussit, récupérer la page pour la détection
   HTML partagée. Exiger un statut HTTP de réussite, `Content-Type: text/html`
   (les paramètres comme charset sont acceptés) et un corps d'au plus 1 Mio
   (1 048 576 octets). La limite de redirections HTTP est 16. Les affectations
   littérales simples `window.location.href = "..."` sont suivies avec des
   récupérations HTTP bornées ; cette détection n'exécute pas de JavaScript arbitraire.
4. Parcourir les requêtes `can_resolve_html` dans l'ordre du manifeste, avec l'URL
   finale et le HTML récupéré. La première reconnaissance positive choisit
   l'extracteur. Pour une requête HTML, déclarer `input_html: "{html}"` dans la
   reconnaissance et l'extraction si elles doivent réutiliser le corps. Sans ce
   champ, une requête HTML peut récupérer à nouveau la page. Les requêtes JSON/text
   ne prennent pas en charge ce champ de réutilisation HTML.
5. Si l'extracteur choisi par HTML échoue ou ne produit aucun flux, retourner une
   erreur ; ne pas essayer le service reconnu suivant. La reconnaissance doit
   être précise.
6. Si la récupération partagée réussit sans reconnaissance HTML, retourner un
   repli iframe contenant `embed-link` et `web-link`, tous deux égaux à la cible
   originale. Les erreurs de récupération partagée (statut, type, taille, limite
   de redirections ou transport) retournent des erreurs, pas ce repli.

Un `error_message` non vide sur la première ligne d'extraction est prioritaire
sur `stream_url` dans les deux phases et arrête la résolution. L'utiliser pour
une erreur métier confirmée, telle qu'une vidéo supprimée. Une valeur vide ou
absente n'a pas d'effet terminal. Ne pas produire une erreur permanente parce
qu'un sélecteur facultatif n'a rien trouvé.

## 4. Schéma du flux extrait

Ces champs appartiennent à la première ligne de `resolve_stream`, avant que
l'application enveloppe le résultat et applique le proxy. Utiliser exactement
les noms avec soulignements indiqués.

| Champ | Type de sortie YAML | Obligatoire / signification |
|---|---|---|
| `stream_url` | `string[]` | Obligatoire pour réussir ; URL absolues ordonnées, première préférée ; espaces périphériques et valeurs vides retirés |
| `manifest_type` | `string` | Facultatif ; par exemple `m3u8`, `mpd`, `mp4` |
| `error_message` | `string` | Erreur métier terminale facultative, prioritaire sur le flux |
| `stream_headers` | `object` | Dictionnaire facultatif d'en-têtes amont avec valeurs de type chaîne non vide |
| `title` | `string` | Titre facultatif |
| `image/title > link` | `string` | URL facultative de l'image du titre |
| `license_url` | `string` | Point d'entrée facultatif de licence pour un parcours DRM déjà pris en charge |
| `license_headers` | `object` | En-têtes facultatifs de requête DRM, valeurs de type chaîne |
| `storyboard_vtt_url` | `string` | URL WebVTT facultative de miniatures lisible par le navigateur |
| `storyboard` | `object` | Métadonnées facultatives de sprite, ci-dessous |
| `subtitles` | `object[]` | Pistes WebVTT externes ordonnées facultatives, ci-dessous |

`stream-url` et `manifest-type` sont des alias de compatibilité ; préférer
`stream_url` et `manifest_type` dans les nouveaux YAML. Déclarer seulement
`embed-link` ne produit pas de flux extrait. Ce convertisseur YAML ne remplit
pas `chapters`.

Utiliser `resolve_url` pour rendre les résultats relatifs absolus. Les en-têtes
média sont internes : ils sont intégrés aux requêtes proxy et omis du JSON public.
Le convertisseur ajoute `Referer: <URL source courante>` si la clé exacte
`Referer` est absente. La casse des noms de ce dictionnaire est préservée ; écrire
`Referer` de façon cohérente. `request_headers` configure la requête de la page
lecteur ; `stream_headers` configure les requêtes média ultérieures. Leurs portées
sont différentes.

L'application applique le proxy aux URL média lorsque son point d'entrée public
est configuré. Utiliser `resolve_url` avec `proxy: true` pour les images,
sous-titres ou miniatures qui nécessitent le proxy, avec leurs `proxy_headers`.
Pour les redirections et remplacements spécifiques, `resolve_url` accepte aussi
`proxy_follow_redirects` et `proxy_replace_all` ; leur syntaxe complète figure
dans la spécification commune. Examiner les URL produites sans ajouter un second
préfixe proxy manuellement.

### Métadonnées de sprite

| Enfant de `storyboard` | Type | Règles |
|---|---|---|
| `url` | `string` | URL non vide obligatoire ; `link` est un alias ; peut contenir le littéral `{index}` |
| `columns`, `rows` | `number` | Entiers positifs obligatoires |
| `width`, `height` | `number` | Dimensions positives facultatives d'une miniature ; déduites par le lecteur si absentes |
| `first_page_index` | `number` | Entier non signé facultatif ; le lecteur utilise 0 par défaut |
| `interval` | `number` | Secondes positives facultatives entre les miniatures |

Une grille obligatoire absente, une URL vide ou une grille/dimension nulle fait
ignorer le storyboard. Préserver le littéral `{index}` pour le lecteur ; ce
n'est pas un paramètre du service. Sans `interval`, le lecteur le déduit de la
durée et de la grille. `storyboard_vtt_url` est prioritaire ; les métadonnées de
sprite servent de repli si le chargement ou l'analyse VTT échoue. Le rendu VTT
actuel prend en charge un sprite unique avec des cellules uniformes.

### Sous-titres externes

Chaque élément `subtitles` contient `link` obligatoire et `lang` et `label`
facultatifs, tous de type chaîne. Utiliser une URL HTTP(S) ou de proxy applicatif
servant du WebVTT lisible par le navigateur. Les liens vides ou invalides sont
ignorés. Les valeurs facultatives vides sont omises ; les codes de langue sont
préservés. Une liste vide est omise du JSON public. Appliquer le proxy aux pistes
qui exigent des en-têtes ou ne sont pas directement accessibles au navigateur.
Aucune piste externe n'est activée par défaut.

## 5. Exemple HTML complet avec reconnaissance d'URL

Ce fichier complet illustre l'extraction d'un hébergeur HTML hypothétique.
Adapter ses sélecteurs et son domaine au fournisseur réel ; ce n'est pas une
source en ligne.

```yaml
id: example-hoster
title: Example hoster
description:
  en: "Extracts HTML5 media from the example host."
http:
  mode: direct
queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{url}"
        actions:
          - type: regex_find_all
            pattern: '^https?://player\.example\.org/embed/'
            format: "{service_id}"

  - name: resolve_stream
    scraper_type: html
    base_url: "{url}"
    query_url: "{url}"
    row_selector: html
    request_headers:
      - name: Referer
        actions:
          - type: format_text
            argument: "{url}"
    entries:
      - name: stream_url
        type: string[]
        selector: "video source[src]"
        actions:
          - type: get_attribut
            argument: src
          - type: resolve_url
      - name: manifest_type
        type: string
        select: first
        actions:
          - type: format_text
            argument: mp4
      - name: title
        type: string
        select: first
        selector: title
        actions:
          - type: get_text
      - name: stream_headers > Referer
        type: string
        select: first
        actions:
          - type: format_text
            argument: "{url}"
```

## 6. Ajouter la reconnaissance du contenu sans autre récupération du lecteur

Ajouter cette requête à la liste `queries` du fichier précédent :

```yaml
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: 'data-player-engine="example-hoster"'
            format: "{service_id}"
```

Ajouter aussi `input_html: "{html}"` à la requête HTML `resolve_stream`, ainsi
que cette valeur par défaut de collection avant `queries` :

```yaml
parameters:
  - name: html
    value: ""
```

En mode direct, aucune entrée `html` n'est fournie ; la valeur vide conserve le
GET ordinaire. Sans cette valeur par défaut, l'absence de `{html}` est une erreur.
Pendant le repli HTML, le corps non vide fourni remplace le défaut et le GET.
Pour reconnaître le contenu avec des sélecteurs, utiliser une requête HTML avec
`base_url: "{url}"`, `query_url: "{url}"`, `input_html: "{html}"`, un
`row_selector` précis et un champ `resolver` construit avec `format_text`.

### Exemples de services existants

Ces exemples facultatifs illustrent des modèles propres aux fournisseurs. Le
contrat complet reste dans ce guide et la spécification commune ; ne pas copier
une URL, un identifiant ou un sélecteur sans vérifier la réponse de la cible.

- [sibnet.yaml](../../server/services/arachnea-stream-hoster/sibnet.yaml) — Reconnaissance d’URL et extraction HTML du flux.
- [voe.yaml](../../server/services/arachnea-stream-hoster/voe.yaml) — Reconnaissance du contenu et réutilisation du HTML fourni.

## 7. Liste de contrôle de rédaction

1. Confirmer que la cible est une page de lecteur, pas un catalogue ou un manifeste.
2. Inscrire l'hébergeur et vérifier activation et priorité.
3. Reconnaître précisément ; vérifier aussi une URL et un document sans correspondance.
4. Pour la reconnaissance du contenu, réutiliser explicitement le HTML et tenir
   compte de l'URL finale après redirections.
5. Extraire une ligne unique avec toutes les URL média candidates dans `stream_url`.
6. Séparer les en-têtes de page et de média. Vérifier l'accès au manifeste, aux
   segments, sous-titres et sprites selon les cas.
7. Vérifier qu'une vidéo supprimée produit une erreur utile sans transformer
   l'absence ordinaire d'un champ facultatif en erreur terminale.
8. Valider séparément le chargement YAML, le JSON extrait et la lecture réelle.
   Une validation syntaxique ou de schéma ne prouve pas qu'un hébergeur fonctionne.
