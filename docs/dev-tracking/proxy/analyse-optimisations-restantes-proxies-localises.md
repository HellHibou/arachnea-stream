# Optimisations restantes des appels et de la recherche de proxies localisés

Date : 20 octobre 2026.

Statut : analyse initiale suivie d’une implémentation le 20 octobre 2026. Les sections 1 à 8 conservent les constats et propositions antérieurs à cette implémentation ; la section 9 décrit le périmètre livré, les bornes retenues et les limites restantes. L’utilisateur a demandé le passage à l’implémentation. Les nouvelles pondérations de sélection, validateurs HTTP et réutilisations de connexions restent des études distinctes.

## 1. Objectif et sources

Réduire le temps jusqu’à la première réponse **acceptée par la destination métier**, le nombre d’échanges réseau inutiles et le coût local de recherche. Une sélection rapide d’un proxy simplement admissible ne suffit pas.

Cette analyse complète [le suivi historique](analyse-optimisation-proxies-localises.md), particulièrement ses sections 6 à 10, 19 à 22, 27, 28 et 30 à 35. Les anciennes descriptions et cases ouvertes doivent être interprétées à la lumière des livraisons ultérieures. Elle complète aussi la section [Server/Proxy du TODO](../../TODO.md#serverproxy) et tient compte du [listener partagé livré le 20 octobre](analyse-serveur-proxy-loopback-unique.md#12-réalisation-du-20-octobre-2026).

Les constats de fonctionnement ci-dessous reposent sur le code présent. La liste de travaux terminés fournie par l’utilisateur est intégrée au bilan des acquis : elle complète les sections datées du suivi, notamment pour le découpage du crate, le repli direct et le regroupement SQLite. Les mesures et résultats de tests cités sont des résultats historiques, sans nouvelle mesure ni réexécution lors de cette analyse. Aucun pourcentage de gain ni débit fournisseur réel ne peut en être déduit.

## 2. Ce qui est déjà livré

| Sujet | État à conserver | Référence dans le suivi historique |
| --- | --- | --- |
| Préférence métier | Historique durable de trois endpoints par destination exacte ; restauration et filtrage par pays autorisés avant découverte | Sections 14, 18 et 35 |
| Parcours multi-pays | Classement commun sans priorité liée à l’ordre fourni ; candidats prêts utilisables avant les sondes des inconnus | Section 18 |
| Pipeline technique | Préchecks progressifs puis validations concurrentes, sans attendre la fin d’une fenêtre complète | Section 19 |
| Blocage avec persistance | Préchecks et validations progressent ensemble ; le verrou de commit détenu par un précheck ne bloque plus un pipeline qui aurait cessé de le piloter | Section 25 |
| Mutualisation | TCP par endpoint ; validation par endpoint, besoin HTTP/HTTPS et configuration ; fournisseur par pays | Sections 19, 31 et 33 |
| Isolation des piles | Sondes TCP/TLS et rafraîchissements partagés exécutés dans des tâches enfants possédées et annulables ; mutualisation, créneaux et échéances conservés | Sections 31 et 33 |
| Fournisseurs et géolocalisation | Publications progressives ; pays déjà connus publiés sans attendre les résolutions géographiques | Section 20 |
| Pays courant | Détection isolée du contexte métier : sa réponse ne clôt plus la recherche du domaine demandé ; timeout et annulation conservés | Section 23 |
| Sonde complémentaire | Option B en mode permissif ; preuves de tunnel, TLS et réponse distinctes ; circuit par cible et secours borné | Section 20 |
| Budget de recherche | 120 s, grâce de 15 s réservée à un essai métier déjà engagé ; anciens plafonds de nombre 64/32/8 supprimés | Sections 19 et 27 |
| Timeouts | Découverte : caps TCP 3 s, handshake proxy 5 s, établissement 15 s ; réutilisation : caps de phase 30 s et enveloppe d’établissement 60 s, selon moteur et configuration | Sections 30 et 35 |
| Erreurs ambiguës | Suspension effective `Other` plafonnée à 30 s, y compris pour les anciennes exclusions persistées ; refus explicites préservés | Sections 32 et 35 |
| Réconciliation des exclusions | Un succès postérieur répare et persiste les anciens `Other` de la même association ; blocages explicites, autres destinations et échecs de date égale ou plus récente préservés | Section 32 |
| Appels parallèles | Coordination de découverte par destination exacte et ensemble canonique de pays ; réveil sur succès ; transferts connus parallèles | Section 35 |
| Redirections | Preuve intermédiaire distincte de l’acceptation du contenu final | Section 35 |
| Loopback | Un listener applicatif partagé, avec contextes et routes épinglées ; cette mutualisation ne prouve pas une réutilisation des connexions entre appels | Suivi du loopback, section 12 |
| Repli direct | `fallback_direct` explicite et configurable, surcharge par requête, activation dans Stream et restitution des erreurs de disponibilité avec les données extraites | Liste de travaux terminés fournie ; changelog courant |
| Stockage SQLite | Store et vues typées distincts ; inventaire, échecs et préférences dans `proxy-inventory.sqlite3`, métadonnées par table et chemins génériques sans sous-dossier ; anciennes données non reprises et ancien reset/marqueur des services retiré | Bilan de regroupement, section 9, indiqué dans la liste fournie ; changelog courant |
| Logs de recherche | Compteurs corrélés distinguant listes, préchecks, validations et essais métier ; capacités prouvées et reprises d’origine visibles | Section 24 |
| Routes REST | Arbre Warp équilibré pour supprimer la profondeur linéaire, avec ordre d’enregistrement et ACL conservés | Section 29 |
| Lots initiaux | Lots 0 à 6 et recette locale du lot 7 terminés ; la qualification réelle demeure distincte | Sections 11 et 21 |

Le découpage structurel est également terminé, selon les points fournis et l’organisation présente du crate :

- neuf blocs de tests externalisés avec modules, accès privés et conditions de compilation conservés ; inclusion des tests de cycle de vie maintenue ;
- inventaire séparé en façade et neuf sous-modules privés, sans changement des chemins publics ni de la concurrence ;
- core et sondes séparés chacun en neuf fichiers, avec état des pools distinct de leur sélection et échanges de sondes distincts des transports sortants ;
- tests du core et de l’inventaire répartis par domaine, fixtures communes réservées aux tests et types de configuration séparés avec leurs implémentations, sans changement des noms de tests, assertions, API et attributs Serde ;
- service HTTP séparé en façade et cinq sous-modules privés, API et handler conservés. Le bilan fourni indique qu’aucun fichier Rust du crate ne dépasse désormais 1 000 lignes.

Ces réalisations correspondent aux sections 10 à 13 et 15 du suivi de découpage mentionné dans la liste fournie. Elles constituent la base du présent travail ; une nouvelle réorganisation des fichiers ou extraction des tests n’est pas proposée comme optimisation restante.

Il serait donc incorrect de proposer à nouveau la persistance, la suppression des fenêtres bloquantes, la découverte progressive ou la coordination initiale des appels comme travaux restant intégralement à faire.

La section 28 reste pertinente pour les sondes annulées, la suspension de découverte pendant un essai, les diagnostics permissifs et le rafraîchissement. Ses constats sur les délais et le cooldown de 24 h ont depuis reçu des correctifs. La section 34 décrit elle aussi un état antérieur à la livraison 35.

Les chiffres de validation fournis (254 tests distincts pour les lots initiaux et 234 tests pour le regroupement SQLite) sont des bilans propres à ces livraisons. Ils ne s’additionnent pas aux suites ultérieures et ne constituent pas une validation nouvelle de l’exploration continue proposée ici. La recette vidéo réelle, l’authentification 6play réelle et la lecture média parallèle demeurent ouvertes selon la liste fournie. L’atomicité métier multi-tables reste hors du périmètre du regroupement SQLite livré.

## 3. Priorité principale : conserver l’avancement entre les essais métier

### État actuel et problème

Dans [`selection.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/selection.rs), `select_cached_any` retourne dès qu’un pipeline produit un candidat. Le flux des autres validations est alors détruit. `select_any_with_search` retourne à son tour et abandonne ses sélecteurs et abonnements fournisseur.

Dans [`probing.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/probing.rs), le registre conserve des références faibles : lorsqu’une opération perd son dernier abonné, son `JoinSet` est détruit et demande l’annulation de la tâche enfant. Un autre appel encore abonné peut la maintenir ; l’annulation n’est donc pas systématique à l’échelle de l’inventaire.

Les résultats terminés sont conservés et dédupliqués. Les réservations incomplètes sont libérées, ce qui permet de recommencer la phase à la sélection suivante mais ne reprend pas un handshake interrompu. Pendant l’essai métier, cette recherche ne conserve plus son propre pipeline actif pour préparer le repli. Ce point est distinct du blocage corrigé en section 25 et de l’isolation des piles livrée en sections 31/33 : ces correctifs sont acquis, mais ne conservent pas un abonné après le retour de la sélection.

La section 28 documente 96 démarrages pour 82 endpoints et 81 terminaisons, dont un endpoint démarré neuf fois. Ces observations historiques étayent le gaspillage possible ; elles ne quantifient pas son ampleur après tous les correctifs ultérieurs.

### Orientation retenue : sondes conservées puis réserve après acceptation

L’utilisateur retient une préparation en deux temps. Pendant l’essai métier, conserver et faire progresser les sondes déjà engagées pour éviter leur annulation et leur recommencement. Après acceptation, rendre la réponse disponible au client sans attendre la préparation, puis compléter si nécessaire une petite réserve de candidats en arrière-plan.

| Phase | Travail prévu | Bénéfice |
| --- | --- | --- |
| Recherche et essai métier en cours | Garder les abonnements aux sondes engagées ; conserver leurs résultats pour une éventuelle sélection suivante | Moins de travail perdu et repli du même appel potentiellement plus rapide |
| Réponse acceptée et rendue disponible au client | Préserver le proxy préféré ; confier au runtime une préparation bornée si la réserve est insuffisante | Repli plus rapide pour les appels futurs si le préféré devient indisponible |
| Réserve suffisante, échéance de préparation ou arrêt du runtime | Arrêter le travail spéculatif propre à cette préparation | Coût borné et absence de découverte permanente |

La préparation après acceptation ne réduit pas le temps de la recherche initiale qui vient de réussir. Elle complète la continuité des sondes engagées, sans imposer une alimentation continue de nouveaux candidats pendant chaque essai métier. Après un échec rejouable, la recherche active peut naturellement lancer les travaux nécessaires dans son budget.

L’acceptation correspond à la décision du handler après les en-têtes ou le préfixe requis. Pour un flux média, il ne faut pas attendre la fin du corps. La réponse ou le flux doit être rendu disponible sans attendre la réserve ; aucun sondage ni commit supplémentaire de préparation n’est ajouté sur ce chemin. Les commits déjà requis pour le succès métier gardent leur contrat.

### Réserve et déclenchement

Point de départ proposé : **deux endpoints de secours distincts**, en plus du préféré. Ce nombre est un réglage à mesurer, pas une valeur déjà livrée. La réserve conserve des records et leurs preuves techniques, sans maintenir des tunnels ou connexions ouverts uniquement pour anticiper un repli.

Un candidat prêt est techniquement admissible selon le mode demandé ; il n’est pas nécessairement accepté par le domaine métier. En mode permissif, une capacité inconnue reste inconnue. La préparation ne fabrique ni préférence ni preuve de redirection, et ne rejoue aucune authentification, aucun POST ni requête utilisateur en arrière-plan.

La coordination de préparation doit distinguer la destination exacte, l’ensemble canonique de pays, le besoin HTTP/HTTPS et l’identité de configuration pertinente. Plusieurs appels compatibles rejoignent une même préparation ; les opérations techniques continuent à être mutualisées à leur portée actuelle. Chaque utilisation revalide pays, exclusions, fraîcheur et configuration.

Ne pas lancer une tâche pour chaque réponse, manifeste ou segment : consulter d’abord la réserve et le travail déjà en cours. Ne compléter qu’un déficit de candidats suffisamment récents. Réutiliser d’abord l’inventaire ; une réponse réussie ne déclenche pas systématiquement un téléchargement fournisseur. Une éventuelle consultation respecte les règles de rafraîchissement de la section 4.

### Propriété, échéances et annulation

Cette orientation remplace, pour la préparation après acceptation seulement, l’ancienne exigence « aucun travail de découverte après la fin de l’appel ». Les règles historiques restent la description du comportement livré ; cette exception est une décision de conception explicite, pas un changement déjà effectué.

Avant acceptation, la session appartient à l’appel : abandon, résultat terminal sans rejeu ou échéance technique libèrent son travail. Après acceptation, une préparation autorisée appartient au gestionnaire de tâches du runtime/inventaire, avec une durée maximale propre, une borne globale de préparations et une capacité totale de réserve bornée. Les valeurs de ces bornes restent à fixer. Il ne s’agit ni d’une boucle périodique ni d’un thread dédié par réponse.

Le budget métier reste 120/135 s ; la préparation ne le prolonge pas et ne réutilise pas son contexte d’acceptation. Elle peut vivre après la fin de ce budget grâce à son échéance technique indépendante. Elle transporte uniquement les paramètres de routage et de validation nécessaires, sans corps, cookies ou identifiants utilisateur de l’appel terminé.

Le transfert de propriété des abonnements doit éviter une annulation puis un redémarrage lors de l’acceptation. Les tâches restent possédées, supervisées et annulables ; arrêt du runtime, configuration remplacée ou préparation devenue obsolète libèrent ses abonnements. Une opération partagée encore utile à une recherche active conserve ses autres abonnés. Les frontières de tâches introduites contre les débordements de pile restent nécessaires. Aucun échec artificiel n’est persisté à l’annulation.

### Effets attendus sur les performances

| Dimension | Effet attendu et limite |
| --- | --- |
| Réponse courante | Pas d’attente volontaire de préparation ; une contention indirecte sur CPU, réseau ou stockage reste à mesurer |
| Appels suivants | Réduction possible du délai de repli ; pas de gain garanti si le préféré reste disponible ou si les candidats de réserve sont refusés par l’origine |
| Réseau | Sondes supplémentaires, parfois inutilisées ; bornes de réserve, déduplication et fraîcheur limitent ce coût |
| CPU et mémoire | Coût du TLS, des tâches et des records ; une tâche asynchrone convient aux attentes réseau sans thread dédié par préparation |
| Créneaux et SQLite | Risque de retarder recherches et transferts actifs ; les préparations doivent céder la priorité et ne pas occuper tout le budget partagé |

La préparation partage les plafonds réseau existants et nécessite une limite plus restrictive de travail spéculatif. La priorité ne doit pas reposer uniquement sur l’ordre d’attente d’un sémaphore commun : l’admission doit éviter qu’une réserve occupe tous les créneaux utiles aux appels actifs. Elle ne tient aucun verrou d’inventaire ou de destination pendant un échange réseau et ne retarde pas une publication de succès métier.

Le compromis accepté est davantage de trafic technique pour réduire le coût des replis futurs. L’ampleur du gain et le bon réglage de la réserve restent à mesurer. Réduire encore les timeouts seuls ne résout pas les recommencements et risque d’abandonner des proxies utiles.

## 4. Rafraîchir selon l’utilité, en conservant le chevauchement

[`config.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/config.rs) et [`discovery.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/discovery.rs) confirment : cooldown normal de 120 s, plancher de 10 s après signal autorisant une consultation anticipée, exploration et chargement pouvant se chevaucher après 10 s ou huit validations infructueuses. L’épuisement des sélecteurs peut aussi déclencher le chargement.

Ces seuils répondent au problème initial d’un cache long à épuiser. Il ne faut donc pas remplacer ce mécanisme par « toujours tester toute la liste avant de rafraîchir ».

Améliorations à étudier :

- distinguer le résultat d’une consultation : annulée, échouée, liste vide, identique ou enrichie ; actuellement la date de tentative est enregistrée avant l’appel fournisseur et ne décrit pas à elle seule sa complétude ;
- suivre le nombre de candidats encore exploitables, en cours et réellement nouveaux, pour éviter un téléchargement sans utilité tout en autorisant le remplacement rapide d’un mauvais cache ;
- conserver, par source et pays, les validateurs HTTP disponibles et utiliser des requêtes conditionnelles si le transport et la source les prennent en charge ; un `304` épargne le corps, mais reste un appel réseau ;
- comparer les identités endpoint/déclaration des lots reçus pour mesurer les listes inchangées. Cette comparaison ne doit pas effacer des métadonnées nouvelles ni les preuves et exclusions existantes.

L’absence de changement n’est pas une preuve que les endpoints sont bons ; le vieillissement maximal et les reprises après erreur doivent rester définis. Le support effectif des validateurs HTTP n’a pas été vérifié sur les sources externes. Ces extensions doivent rester génériques dans le transport, l’agrégateur ou la configuration, sans traitement métier propre à une source dans la façade de service.

## 5. Diagnostics permissifs et ordre des candidats

Dans [`results.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_probe/results.rs), `apply_result` peut retourner `None` malgré une erreur de sonde si le repli permissif autorise l’essai. Les observations contextualisées et les traces DEBUG conservent déjà une partie des faits ; le résumé INFO de validation peut néanmoins afficher `error=None` avec `capability_proven=false`.

Première étape : exposer séparément admissibilité, preuve de capacité, résultat issu du cache ou d’un nouvel échange, et cause contextualisée par protocole/cible/phase. Cela permet de choisir ensuite un classement fondé sur des mesures plutôt que sur une assimilation entre compteur « éligible » et succès HTTPS.

Le classement actuel préfère déjà les capacités prouvées, puis la fraîcheur, la latence et les échecs. L’amélioration éventuelle concerne surtout les candidats peu prouvés : distinguer la latence TCP du temps d’établissement utile et mieux départager les indications fournisseur des échanges réellement observés. Une nouvelle pondération serait un changement de sélection à confirmer.

L’option B validée reste applicable : ne pas imposer le mode strict globalement, ne pas transformer `Unknown` en preuve, ne pas invalider tous les proxies à cause d’une cible complémentaire défaillante. Une réponse métier refusée ne crée pas une panne globale du proxy.

## 6. Coûts locaux supplémentaires identifiés dans le code

Ces pistes complètent les limites réseau du suivi. Elles sont vérifiées statiquement, mais leur contribution à la latence n’est pas mesurée.

| Coût observé | Piste locale | Condition de correction |
| --- | --- | --- |
| Après échec du parcours préféré, `select_any_with_search` charge les caches persistants des pays séquentiellement à chaque sélection ; `load_cached_country` ne mémorise pas une hydratation réussie | Mutualiser les chargements concurrents et éviter les relectures sans changement pertinent | Définir invalidation et nouvelle tentative après erreur ; ne pas masquer des écritures externes attendues |
| `TypedProxyRepository::find_by_country` lit la liste puis appelle `get` pour chaque record ; `get` relit le record et son éventuelle éviction | Réutiliser les records déjà lus et étudier une lecture groupée des exclusions | Préserver le filtrage des évictions et la cohérence avec les écritures concurrentes ; ne pas supposer que chaque appel correspond à une requête SQL physique |
| `merge_provider_batch` clone l’ensemble de `inner.records` avant préparation d’un lot | Construire un instantané des seuls endpoints présents dans le lot | Conserver fusion des déclarations, santé, exclusions et preuves ; attention aux doublons et publications concurrentes |
| `select_cached_any` prépare le contexte de sonde sous verrou d’écriture et reclasse les candidats à chaque passage | Réduire les parcours et recalculs, en commençant par les copies et empreintes de configuration répétées | Revalider l’éligibilité avant emploi : les cooldowns expirent avec le temps et les observations peuvent changer sans publication fournisseur |
| Les observations de phase et les mises à jour techniques attendent des commits sérialisés | Mesurer attente et durée des commits avant d’envisager un regroupement limité | Ne pas détacher les écritures, perdre les preuves nécessaires, ni retarder silencieusement la préférence durable |

Fichiers : [`persistence.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/persistence.rs), [`proxy_repository.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_repository.rs), [`discovery.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/discovery.rs), [`selection.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/selection.rs) et [`observations.rs`](../../../server/crates/arachnea-proxy/src/core/proxy_inventory/observations.rs).

Il n’est pas nécessaire de commencer par une refonte du stockage : les tables inventaire, échecs et préférences partagent déjà le fichier SQLite applicatif selon le changelog courant. Ce partage physique ne supprime pas les garanties ni les coûts des commits métier distincts.

## 7. Appels successifs, média et loopback

La coordination de la section 35 vise surtout les recherches sans preuve métier connue. Elle ne doit pas sérialiser les transferts déjà acceptés ni fusionner cookies, corps ou réponses de plusieurs appels.

Le listener partagé livré le 20 octobre réduit les créations de sockets d’écoute et centralise leur cycle de vie. Il ne résout pas à lui seul le coût des nouvelles connexions TCP/tunnel/TLS entre appels contextuels. La réutilisation de connexions reste une étude distincte : elle doit préserver le token exact, la destination, la route et le budget de chaque appel. Un cache de client indexé uniquement par adresse du listener partagé serait insuffisant.

Pour le média, mesurer manifestes et segments séparément. Une ressource acceptée ne prouve pas l’accès à tous les domaines suivants. Un budget de recherche conforme à 120/135 s peut néanmoins dépasser le délai acceptable du lecteur ; cela demande une recette du lecteur et du déploiement, pas une augmentation générale du budget.

Le suivi historique excluait le repli direct automatique du chantier. Le code a depuis reçu une politique explicite `fallback_direct`, désactivée par défaut selon le changelog. Cette analyse ne propose ni de l’activer ni de modifier les politiques de routage.

## 8. Ordre de travail recommandé et critères de validation

1. **Établir une référence actuelle.** Mesurer délai jusqu’à acceptation, provenance, sondes réellement démarrées/terminées/annulées/recommencées, abonnés d’un travail partagé, attente des créneaux et des commits, durée des listes et nombre de nouveaux endpoints utiles. Ne pas confondre réservations et exécutions réseau.
2. **Livrer l’orientation retenue en section 3** : conserver les sondes engagées pendant l’essai, rendre la réponse acceptée disponible immédiatement, puis compléter une réserve bornée sous la responsabilité du runtime. Fixer durée, capacités, fraîcheur et admission prioritaire avant implémentation ; comparer le coût et les replis avec et sans préparation après acceptation.
3. **Qualifier et ajuster le rafraîchissement**, avec suivi des annulations et de l’utilité des sources, sans perdre le chevauchement cache/fournisseur.
4. **Traiter les coûts locaux mesurés**, en commençant par le clonage global et les lectures répétées ; ajuster le classement permissif seulement avec des diagnostics suffisants.
5. **Qualifier les appels média réels** et étudier séparément la réutilisation des connexions si leur coût domine encore.

Scénarios de comparaison : préférence en mémoire, restauration après redémarrage réel, cache sans préférence, démarrage sans cache, échec du premier candidat, source lente ou inchangée, appels parallèles de pays compatibles et différents, abandon du conducteur, et média prolongé. Ajouter : réponse rendue disponible avant fin de préparation ; rafale de segments ne créant qu’une préparation compatible ; réserve réutilisée ou expirée ; repli avec réserve ; saturation par recherches actives ; changement de configuration et arrêt du runtime. Mesurer aussi les candidats préparés réellement utilisés, le trafic spéculatif, les commits supplémentaires et les délais des transferts actifs. Comparer les distributions de latence et le volume réseau, pas seulement une exécution réussie.

Critères communs : moins de travail recommencé à conditions comparables ; pas de hausse incontrôlée des sondes ; maintien des limites TCP 8/16, validations 2/4 et fournisseurs 2/2 pour les recherches et des plafonds partagés incluant la réserve ; géolocalisation bornée à 20 IPs par chargement pays et quatre résolutions partagées. Aucun nouveau travail de recherche n’est lancé pour l’appel après 120 s. Après acceptation, seul le travail de réserve explicitement transféré au runtime peut continuer sous son échéance et ses bornes propres. Un abandon avant acceptation ne déclenche pas de préparation autonome. Les annulations ne produisent aucune dégradation artificielle.

La file borne le travail préparé, jamais le nombre total d’essais. Une seule tentative métier reste active par appel. Les règles de rejeu POST, les contrôles TLS, les pays autorisés, les exclusions par destination et l’ordre des succès persistés restent applicables.

Les qualifications encore ouvertes en section 22 du suivi restent nécessaires : HTTPS propre à `ProxyProbe` et secours, charge et annulations, processus réellement redémarré, POST avec effet serveur, média long, horizon réel 120/135 s et pannes processus/disque. Les résultats locaux historiques ne valent pas validation de ces scénarios.

Aucun test n’est créé ni exécuté pour ce complément documentaire. Les éventuelles adaptations des tests existants accompagneront l’implémentation ; de nouveaux tests nécessitent une demande explicite conformément à `AGENTS.md`.


## 9. Implémentation du 20 octobre 2026

### Cycle de vie livré

Les sélections associées à une destination conservent les abonnements TCP et validation déjà engagés dans un propriétaire partagé entre contexte d’appel et token de sélection. Un worker retenu garde son créneau local et sa réservation ; il ne possède pas son propre superviseur. La chute du dernier propriétaire, l’annulation de sélection ou l’échéance technique annulent les abonnements, sans résultat d’échec artificiel. Les opérations techniques hors sélection métier gardent l’annulation au départ de leur dernier abonné.

Cette continuité porte sur les phases engagées : une phase TCP terminée est enregistrée et sa validation pourra commencer lors du prochain parcours actif. Elle ne continue pas à alimenter de nouveaux pipelines pendant l’essai métier et ne maintient pas les téléchargements fournisseur abandonnés par la sélection.

Après le commit métier déjà requis, `record_accepted` peut transférer les workers retenus au superviseur de l’inventaire, sans attente d’une sonde ou d’un commit de réserve. Le transfert prolonge leur seule échéance technique, sans annulation/redémarrage. La préparation termine ces phases puis recherche au plus deux secours distincts du proxy accepté, avec les paramètres suivants :

| Borne | Valeur livrée |
| --- | --- |
| Secours par préparation | `InventoryConfig::reserve_size`, défaut 2, plafonné à 2 ; 0 désactive la préparation après acceptation |
| Durée après acceptation | `InventoryConfig::reserve_timeout`, défaut 15 s |
| Préparations simultanées | 2 par inventaire ; même destination exacte/pays canoniques/besoin HTTP-HTTPS/configuration de sonde : une seule préparation |
| Nouveau travail spéculatif | 1 pipeline par préparation ; TCP admis seulement avec plus de 8 créneaux partagés libres, validation avec plus de 2 |
| Fraîcheur | `probe_ttl` existant, défaut 600 s ; pays, configuration, santé et exclusions revérifiés |
| Stockage de réserve | Records ordinaires de l’inventaire, aucun cache de tunnels/connexions ; cible totale de quatre secours pour les deux préparations actives |
| Fournisseurs | Aucun téléchargement déclenché par la préparation ; inventaire uniquement |

La consultation immédiate d’une réserve suffisante ou d’une préparation compatible évite une tâche supplémentaire par segment/réponse. Une tentative terminée ne peut pas être relancée pour la même clé avant son échéance de préparation initiale ; au plus 32 clés sont mémorisées. Un déficit impossible à remplir ne déclenche donc pas une nouvelle tâche à chaque segment. Sans admission, les abonnements propres à la sélection acceptée sont libérés. Les phases engagées avant acceptation finissent sous leurs plafonds existants ; la marge d’admission réduite s’applique aux nouvelles phases spéculatives. Le superviseur possède les tâches, ses workers ne le retiennent pas ; sa destruction les annule. Une nouvelle instance d’inventaire/configuration possède son propre superviseur. Les budgets métier 120/135 s, les règles de POST, les préférences durables, TLS et `fallback_direct` sont conservés.

### Coûts locaux et observabilité

- Les instantanés des lots fournisseur ne copient plus l’ensemble de l’inventaire : seuls les endpoints reçus sont copiés, avec la fusion et les protections concurrentes existantes.
- Les lectures pays du repository réutilisent les records déjà lus et filtrent les évictions sous le verrou de commit. Les exclusions restent lues individuellement ; aucune nouvelle atomicité multi-tables n’est revendiquée.
- Deux imports pays peuvent progresser ensemble. Ils restent relus lors des sélections suivantes : aucune mémorisation susceptible de masquer une écriture externe n’a été introduite.
- Les erreurs observées des sondes sont journalisées avec phase, protocole, cible, portée et cause, indépendamment de l’admissibilité permissive ; cache et nouvel échange sont distingués. Les traces DEBUG différencient abonnements, démarrages/fins/annulations du travail partagé et attente/durée des commits d’inventaire.

Les comparaisons de latence réelle, l’utilité des listes, leurs validateurs HTTP, l’hydratation persistante mutualisée avec invalidation, la réduction des recalculs de sélection, une autre pondération et la réutilisation des connexions ne sont pas livrées par ce changement. La qualification média et les scénarios réels de la section 8 restent ouverts. Aucun gain chiffré n’est déduit des tests locaux.

### Validation

Validation locale : `cargo test -p arachnea-proxy --all-features --lib --offline` réussit avec **128 tests existants, aucun échec**. `cargo check -p arachnea-proxy --no-default-features --features core --offline`, le contrôle de formatage du crate et `git diff --check` réussissent également. Le scénario existant de découverte partagée est adapté pour vérifier la survie d’une sonde au retour de sélection, l’acceptation sans attente de sa terminaison, le transfert hors du propriétaire d’appel et la libération des créneaux à l’échéance de réserve. Aucun nouveau test ni infrastructure n’est créé. Les tests utilisant des sockets locales nécessitent une exécution hors du bac à sable qui interdit leur ouverture.
