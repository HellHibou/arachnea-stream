# Chaînes des sources de streaming

Analyse du 20 novembre 2026, en attente de validation avant implémentation.

## État actuel

- **Contrat.** `get_entry` renvoie les métadonnées d’une fiche, dont `web-link` pour l’ouverture chez le fournisseur. La [façade Stream](../../server/crates/arachnea-stream/src/stream_scraper/catalog.rs) transmet les données extraites par le scraper. `get_category` accepte déjà des descripteurs associant l’identifiant d’une source à ses paramètres de requête.
- **Odysee.** Les cartes de chaînes utilisent déjà `get_category` avec `query_url`, `category_kind`, `channel_id`, `channel_url` et `category_label`. La page de chaîne gère déjà la couverture, le logo et les onglets de contenu. Le `get_entry` vidéo effectue une recherche LBRY par identifiant de publication. Le SDK LBRY documente `signing_channel` et peut renvoyer un objet réduit à l’identifiant quand la chaîne ne peut pas être résolue. La forme effectivement servie par le point d’accès utilisé par Arachnea reste à contrôler. [Scraper Odysee](../../server/services/arachnea-stream/social-stream/odysee.yaml) · [Encodeur JSON du SDK LBRY](https://raw.githubusercontent.com/lbryio/lbry-sdk/master/lbry/extras/daemon/json_response_encoder.py)
- **M6Play.** `load_home` fournit déjà quatre catégories basées sur les services `m6replay`, `w9replay`, `6terreplay` et `gulli`. Leur `get_category` produit des rails à partir des dossiers. Seul l’accueil déclare actuellement une collection de bannières, alimentée par les programmes de `m6replay`. Le `get_entry` part de l’identifiant du programme et le YAML actuel n’extrait aucune attribution de chaîne sur cette fiche. Le site officiel affiche les quatre logos. [Scraper M6Play](../../server/services/arachnea-stream/legal-stream/m6play-fr.yaml) · [Page officielle des chaînes M6+](https://www.m6.fr/m6plus/direct-m6-f_999)
- **Interface.** `EntryDetailsPosterPanel.vue` contient les actions de la fiche, dont le bouton d’ouverture chez le fournisseur. La navigation vers une collection est déjà centralisée et conserve le descripteur de catégorie dans la route. `CategoryView.vue` réutilise `HomeCatalog`, qui sait charger un carrousel différé lorsque le catalogue expose `banners.link`. RTBF Auvio utilise déjà ce mécanisme dans son propre `get_category`. [Panneau de fiche](../../front/public-app/src/components/entry-details/EntryDetailsPosterPanel.vue) · [Navigation média](../../front/public-app/src/services/mediaRouting.ts) · [Vue catégorie](../../front/public-app/src/views/CategoryView.vue) · [Scraper RTBF Auvio](../../server/services/arachnea-stream/legal-stream/rtbf-auvio-be.yaml)

## Contrat proposé

Ajouter un objet facultatif `channel` à la racine de la réponse `get_entry`. Il représente la chaîne à laquelle le fournisseur rattache cette fiche et contient les éléments suivants.

| Champ | Rôle |
| --- | --- |
| `channel.name` | Nom affiché de la chaîne. |
| `channel.logo` | URL du logo. Si le fournisseur ne fournit aucune image ou si elle ne se charge pas, le nom reste visible et le lien reste utilisable. |
| `channel.request` | Paramètres existants de la catégorie, dont `query_url` et les paramètres propres au fournisseur. Le client utilise l’identifiant de la source de la fiche pour construire le descripteur envoyé à `get_category`. |

Le scraper omet `channel` si la chaîne ne peut pas être identifiée ou si sa catégorie ne peut pas être construite. Le client masque aussi le lien si le nom ou la destination est inutilisable. Ce contrat est additif et les réponses actuelles restent valides. La chaîne est accessible depuis la fiche, y compris quand elle a été ouverte depuis la recherche, une bannière ou un lien direct.

## Implémentation recommandée

### Odysee

1. Partir des métadonnées de la chaîne signataire du résultat de `claim_search`. Vérifier sur le point d’accès configuré la présence de l’identifiant, de l’URL de chaîne, de `value.title` et de `value.thumbnail.url`. Utiliser le nom de chaîne LBRY comme repli du titre. Si le résultat qualifie explicitement la signature d’invalide, omettre la relation.
2. Émettre les mêmes paramètres que les cartes de chaîne existantes, avec `category_kind: channel` et `category_label` renseigné par le nom affiché. Conserver l’URI de la chaîne, qui permet à `get_category` de charger son habillage et ses onglets. Une résolution complémentaire ciblée dans le YAML se justifie seulement si la réponse principale fournit une référence exploitable avec des métadonnées insuffisantes. Elle reste facultative pour l’ouverture de la fiche.

### M6Play

1. Identifier dans les réponses de programme le champ qui attribue le contenu à une chaîne, en vérifiant des exemples issus des quatre catalogues et un programme sans attribution claire. Les codes du direct (`M6`, `W9`, `6T`, `gulli`) et les clés du replay servent de références de correspondance. Retenir la chaîne principale si l’API la désigne. Omettre le lien si plusieurs chaînes restent possibles.
2. Centraliser dans le YAML la correspondance entre clé de replay, libellé, logo et URL de catégorie. Réutiliser les destinations actuelles `/{service}/folders?limit=999&offset=0` pour M6, W9, 6ter et Gulli. Valider une URL d’image autonome par chaîne. Les logos repérés sur le site officiel renvoient à un sprite SVG commun, dont l’URL seule ne suffit pas pour l’affichage d’un logo dans une balise `img`.
3. Dans `get_category`, exposer `banners.link` pour les quatre catégories de chaîne. Le lien reprend la forme utilisée par l’accueil M6Play, `/{service}/programs?limit={max_banners}&offset=0&csa=6&sort=last_video_publication&with=layout`. Vérifier que chaque service renvoie des programmes avec des visuels utilisables par `get_banners`. Limiter cette construction aux quatre services connus et prévoir qu’elle fonctionne aussi depuis les descripteurs actuels, qui fournissent déjà l’URL des dossiers. Le carrousel représente des programmes de la chaîne, et son chargement indépendant permet d’afficher les rails même si les bannières sont indisponibles.

### Frontend public

- Normaliser `channel` dans `services/rustify/entry/details.ts`, typer les données dans `types/entry.ts` et conserver tous les paramètres du descripteur pendant la navigation.
- Transmettre la chaîne depuis `ProgramEntryDetails.vue` via `EntryDetails.vue`. Afficher dans `EntryDetailsPosterPanel.vue` un lien interne avec le logo et le nom, immédiatement sous le groupe contenant le bouton d’ouverture chez le fournisseur. Réutiliser la destination de collection existante avec un lien accessible au clavier et un logo décoratif lorsque le nom est visible.
- Réutiliser le carrousel de catégorie et son chargement différé pour M6Play. Le périmètre proposé couvre les fiches issues de `get_entry` et les catégories de chaîne.

## Impact et choix

Le passage par `get_category` réutilise la route, le partage de liens, les onglets Odysee et la pagination existante. Une opération `get_channel` dédiée impliquerait un nouveau contrat et un nouveau parcours côté client. Elle aurait un intérêt si une future chaîne devait exposer des fonctionnalités que la page de catégorie ne sait pas représenter. Pour le besoin actuel, l’extraction et la mise en forme restent dans les YAML, avec les actions génériques du scraper et les composants Vue existants.

À l’implémentation, documenter `channel` dans les spécifications Stream FR et EN, puis aligner le README de l’application publique et le changelog.

## Validation prévue

- Odysee avec et sans attribution de chaîne. Vérifier le lien sous le bouton source, le repli sans logo, l’arrivée sur la même chaîne que depuis une carte de catalogue et le fonctionnement des onglets et de la pagination.
- M6Play avec un programme de chacune des quatre chaînes, un programme dont la chaîne est indéterminée et une ouverture directe de fiche. Vérifier que les bannières appartiennent à la chaîne visitée, ouvrent les fiches attendues et laissent les rails accessibles en cas d’échec de chargement. Vérifier aussi l’accès aux mêmes bannières depuis les catégories déjà affichées sur l’accueil M6Play.
- Adapter les assertions affectées du [contrat M6Play existant](../../server/crates/arachnea-stream/tests/fixtures/services/arachnea-stream/legal-stream/m6play-fr.yaml), exécuter la [suite d’intégration Stream](../specifications/arachnea-stream-source-checks-fr.md) ciblée sur `m6play-fr` et lancer `npm --prefix front/public-app run type-check`. Vérifier Odysee dans l’application, la suite de contrats actuelle ne contenant pas de contrat pour cette source.

Les vérifications de payloads du middleware M6Play et du point d’accès Odysee restent à faire. Les sources publiques consultées confirment la structure LBRY et les visuels M6+, tandis que les chemins exacts à exploiter et la couverture des bannières par service dépendent des réponses utilisées par les scrapers.
