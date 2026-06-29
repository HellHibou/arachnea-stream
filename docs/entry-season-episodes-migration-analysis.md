# Analyse : episodes embarques dans `seasons` pour `get_entry`

## Objectif

Le contrat `get_entry` doit cesser d'exposer les episodes au niveau racine et les rattacher aux saisons.

La forme cible est :

```json
{
  "seasons": [
    {
      "label": "Saison 15",
      "link": null,
      "episodes": []
    }
  ]
}
```

Une saison peut donc etre resolue de deux facons :

- `seasons[].episodes[]` est present : le front utilise directement ces episodes embarques.
- `seasons[].link` est present : le front conserve le chargement dynamique actuel via `get_season`.

Il n'y aura pas de retro-compatibilite : le front ne doit plus lire `season`, `episode` ou `episodes` a la racine de `get_entry`, et les YAML doivent tous produire le nouveau contrat.

## Etat actuel

### Backend YAML

Les sources utilisent plusieurs formes pour les episodes et les saisons dans `get_entry`.

| Source | Etat actuel dans `get_entry` | Migration attendue |
|---|---|---|
| `francetv.yaml` | `season[]` contient deja des episodes, mais sous `episode[]` singulier. Les saisons viennent de `/collections/*[type=playlist_video]`. | Renommer le groupe racine en `seasons[]` et le champ enfant en `episodes[]`. Garder le filtre `playlist_video`. |
| `rtbf-auvio-be.yaml` | Les sous-requetes de programme produisent `season[]` et injectent des `episode[]` dans chaque saison. | Renommer le groupe racine en `seasons[]` et le champ enfant en `episodes[]`. |
| `rtlplay-be.yaml` | `season[]` expose des `link`; `episode[]` top-level contient les episodes de la saison selectionnee dans la reponse detail. | Renommer `season[]` en `seasons[]` et supprimer ou deplacer `episode[]` top-level. Comme les saisons ont des `link`, le plus simple est de s'appuyer sur `get_season`. |
| `m6play-fr.yaml` | `season[]` expose des `link`; `get_season` renvoie `episodes[]`. | Renommer `season[]` en `seasons[]`; verifier que toutes les saisons gardent un `link`. |
| `tf1-fr.yaml` | `season[]` expose des `link`; `get_season` renvoie `episodes[]`. | Renommer `season[]` en `seasons[]`; verifier que toutes les saisons gardent un `link`. |
| `papystreaming.yaml` | `season > ...` produit des saisons implicites avec `link`. | Renommer le groupe implicite en `seasons > ...` et garder le chargement dynamique. |
| `crunchyroll.yaml` | `season > ...` produit des saisons implicites avec `link`. | Renommer le groupe implicite en `seasons > ...` et garder le chargement dynamique. |

Les endpoints `get_season` continuent a renvoyer `episodes[]`. Le changement concerne uniquement la reponse detail `get_entry`.

### Front

Le front normalise actuellement les donnees de detail dans `front/src/services/rustify.ts`.

Comportement actuel :

- `normalizeEntrySeasons()` lit `entry.season`, puis seulement `season.label` et `season.link`.
- `normalizeEntryEpisodes()` lit `entry.episode` au niveau racine.
- `EntrySeason` contient seulement `id`, `label`, `link`.
- `entryDetailsData.loadSeasonPage()` charge les episodes uniquement si `season.link` existe.
- `entryEpisodeSelection.displayedEpisodes` affiche `seasonEpisodes` quand l'entree a des saisons, sinon `details.episodes`.

Consequence : une reponse `seasons[].episodes[]` est ignoree par le front actuel.

## Contrat cible

### `get_entry`

`get_entry` doit produire :

```ts
interface EntryDetailsPayload {
  seasons: EntrySeasonPayload[]
}

interface EntrySeasonPayload {
  label?: string | null
  link?: string | null
  episodes?: EntryEpisodePayload[]
}
```

Regles :

- `seasons[]` est la seule liste de saisons acceptee dans `get_entry`.
- `seasons[].episodes[]` contient les episodes deja disponibles dans la reponse detail.
- `seasons[].link` indique que les episodes doivent etre charges avec `get_season`.
- Une saison peut avoir les deux champs, mais le front doit preferer `episodes[]` quand il est non vide.
- `label` est optionnel quand il n'y a qu'une seule saison.
- Si une seule saison existe et que son label est absent ou vide, le front masque la liste des saisons et affiche directement les episodes.
- Si plusieurs saisons existent, chaque saison doit avoir un label exploitable.

### `get_season`

`get_season` garde le contrat actuel :

```json
{
  "current_page": 1,
  "have_more": false,
  "episodes": []
}
```

Ce contrat reste necessaire pour les sources paginees ou chargees dynamiquement.

## Impact front

### Types

`front/src/types/entry.ts` doit evoluer.

Proposition :

```ts
export interface EntrySeason {
  id: string
  label: string | null
  link: string | null
  episodes: EntryEpisode[]
}

export interface EntryDetails {
  seasons: EntrySeason[]
}
```

`EntryDetails.episodes` peut etre supprime si aucun autre flux ne doit exposer des episodes hors saison. Comme il n'y a pas de retro-compatibilite, c'est preferable pour rendre le contrat clair.

### Normalisation

`normalizeEntrySeasons()` devient responsable de normaliser les episodes embarques :

- lire `entry.seasons`;
- lire `season.episodes`;
- normaliser chaque episode via `normalizeEntryEpisode(..., season.label)`;
- conserver `season.link` pour les sources dynamiques;
- accepter `label = null` uniquement si la liste finale contient une seule saison.

`normalizeEntryEpisodes()` doit etre supprime ou ne plus etre utilise par `normalizeEntryDetails()`.

### Chargement des saisons

`entryDetailsData.loadSeasonPage()` doit choisir entre donnees embarquees et chargement dynamique :

1. Si `season.episodes.length > 0`, remplir `seasonEpisodes` avec ces episodes, sans appeler `getSeasonEpisodes`.
2. Sinon, si `season.link` existe, conserver le flux actuel `getSeasonEpisodes(...)`.
3. Sinon, afficher un etat vide.

Les champs de pagination doivent etre ajustes :

- saison embarquee : `currentSeasonPage = 1`, `hasMoreSeasonEpisodes = false`;
- saison dynamique : comportement actuel.

### Selection automatique

Au chargement d'une entree :

- si une seule saison existe et qu'elle contient des episodes embarques, elle doit etre selectionnee automatiquement;
- si une seule saison existe sans label, la section de choix des saisons doit etre masquee;
- si plusieurs saisons existent, conserver le select de saisons;
- si la premiere saison a des episodes embarques, elle peut etre chargee sans requete reseau;
- si elle a seulement un `link`, garder le chargement dynamique actuel.

### Presentation

`entryDetailsCatalogPresentation` et `EntryDetailsCatalogSection.vue` doivent distinguer :

- `hasMultipleSeasons`: afficher la liste/select des saisons;
- `hasSingleImplicitSeason`: masquer la liste des saisons, mais afficher les episodes;
- `displayedEpisodeItems`: continuer a provenir de `displayedEpisodes`.

La condition actuelle `hasSeasons = details.seasons.length > 0` est trop grossiere, car elle force le mode "saisons" meme quand il y a une seule saison sans label.

### Navigation episode suivant

`ProgramEntryDetails.vue` cherche actuellement les saisons suivantes via `season.link`. Ce code doit aussi savoir inspecter `season.episodes`.

Regles proposees :

- pour une saison embarquee, chercher le premier episode jouable dans `season.episodes`;
- pour une saison dynamique, utiliser `getSeasonEpisodes`;
- `hasLaterSeason` doit retourner vrai si une saison suivante a un `link` ou des episodes embarques jouables.

## Impact YAML

### Convention de nommage

Dans `get_entry`, le champ canonique doit etre `seasons[].episodes[]`, avec les deux groupes au pluriel.

Anciennes formes a supprimer :

- `episode` au niveau racine;
- `episodes` au niveau racine;
- `season` au niveau racine;
- `season[].episode` au singulier.

### FranceTV

Etat recent :

```yaml
- name: season
  pointer: /collections/*[type=playlist_video]
  entries:
    - name: label
      pointer: /label
    - name: episode
      pointer: /items/*[type=integrale]
```

Forme cible :

```yaml
- name: seasons
  type: object[]
  pointer: /collections/*[type=playlist_video]
  select: all
  entries:
    - name: label
      type: string
      select: first
      pointer: /label
    - name: episodes
      type: object[]
      pointer: /items/*[type=integrale]
      select: all
      entries: *episode_entries
```

Le filtre `type=playlist_video` est important : une entree de collection correspond a une saison. Les collections comme `playlist_to_discover` ne doivent pas alimenter les saisons.

### RTBF Auvio

`rtbf_program_episode_sub_query` doit produire `seasons[]`, puis `episodes[]` dans chaque saison au lieu de `season[]` et `episode[]`.

Cette source est un bon cas de reference pour le mode hybride : elle construit des saisons via sous-requetes, avec episodes embarques par saison.

### RTL Play

`get_entry` produit actuellement :

- `season[]` avec `link`;
- `episode[]` top-level pour la saison selectionnee.

Avec le nouveau contrat, deux options existent :

1. Supprimer `episode[]` top-level et laisser `get_season` charger la saison selectionnee.
2. Deplacer ces episodes dans la saison correspondante sous `seasons[].episodes[]`.

L'option 1 est plus simple et coherente avec les saisons dynamiques, car chaque saison RTL a deja un `link`.

### Sources avec saisons dynamiques uniquement

`m6play-fr.yaml`, `tf1-fr.yaml`, `papystreaming.yaml` et `crunchyroll.yaml` peuvent rester sur des saisons avec `link`, sans episodes embarques, tant que le front sait continuer a appeler `get_season`.

## Trajectoire de migration

1. Mettre a jour le front.
   - Etendre `EntrySeason` avec `episodes`.
   - Supprimer la lecture de `entry.episode`/`entry.episodes` dans `get_entry`.
   - Faire charger une saison depuis `season.episodes` avant de regarder `season.link`.
   - Masquer la liste des saisons quand il n'y a qu'une seule saison sans label.

2. Migrer les YAML `get_entry`.
   - FranceTV : `season[].episode` -> `seasons[].episodes`.
   - RTBF : sous-requetes `season[].episode` -> `seasons[].episodes`.
   - RTL Play : `season[]` -> `seasons[]`, puis supprimer `episode` top-level ou le rattacher a `seasons[].episodes`.
   - Verifier qu'aucun `get_entry` ne produit encore `season`, `episode` ou `episodes` a la racine.

3. Valider les contrats.
   - Chercher dans `server/services/*.yaml` les champs `name: episode` et `name: episodes` sous `get_entry`.
   - Verifier que les seuls `episodes` restants hors `seasons[]` sont dans `get_season`.
   - Lancer `cargo test config_yaml_to_json --lib -p arachnea-scrapyfy`.
   - Lancer les checks TypeScript/front existants si disponibles.

## Points d'attention

- Les donnees embarquees doivent etre normalisees exactement comme les episodes de `get_season`, sinon les lecteurs, images et labels divergeront.
- Une saison sans `label` est acceptable uniquement si elle est seule. Avec plusieurs saisons, le front a besoin d'un libelle pour le select.
- Le mode eager (`episodes[]`) doit desactiver le bouton "charger plus".
- La navigation episode suivant doit fonctionner entre saisons eager et dynamiques.
- Les bookmarks stockent `groupId` et `selectedItemId`; `groupId` doit rester l'id de saison genere par le front, meme pour une saison implicite.
- Les sources qui exposent simultanement `episodes[]` et `link` doivent preferer les episodes embarques pour eviter une requete inutile.

## Resultat attendu

Apres migration, `get_entry` aura un seul modele mental :

- une entree contient des saisons;
- une saison contient directement des episodes ou un lien pour les charger;
- le front ne connait plus d'episodes racine dans une page detail.

Cela aligne FranceTV avec la structure de sa source (`playlist_video` = saison), garde le chargement dynamique pour les sources paginees, et retire l'ambiguite entre `episode`, `episodes`, `season` et `seasons`.
