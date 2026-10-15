# Analyse des possibilités d’optimisation des proxies localisés

Date : 13 octobre 2026.

Statut : lots 1 à 5 intégrés ; validations et limites de recette des lots 1/2 en sections 13 à 16, lot 3 en section 17, lot 4 en section 18, lot 5 en section 19 (15 octobre 2026).

Révision : prise en compte des deux phases de test, de l’indisponibilité ou des restrictions géographiques du serveur de test, et des décisions sur la persistance et les pays autorisés.

## 1. Objectif et contraintes

L’objectif prioritaire est de trouver le plus rapidement possible un proxy accepté par la destination demandée, et non simplement un proxy accessible ou capable d’ouvrir un tunnel.

Les contraintes sont les suivantes :

- Ne pas dépendre d’un processus d’actualisation en arrière-plan.
- Conserver, pour chaque domaine, le dernier proxy accepté et le réutiliser de préférence afin d’éviter une nouvelle recherche inutile.
- Persister obligatoirement ces préférences et les restaurer après redémarrage.
- Traiter la liste des pays comme un ensemble de pays autorisés, sans aucune priorité liée à leur ordre.
- Considérer cette liste comme une connaissance préalable des origines à utiliser pour accéder au domaine. Ne jamais exclure ou suspendre tous les proxies d’un pays à partir des échecs de quelques endpoints : un serveur peut blacklister certains proxies sans refuser les autres du même pays.
- Permettre les appels simultanés à plusieurs domaines sans que leurs préférences se remplacent mutuellement.
- Privilégier ensuite les proxies déjà testés en cache.
- Tester progressivement les proxies inconnus en cache et actualiser les listes lorsque cela devient pertinent.
- Trouver un compromis entre réutilisation du cache, exploration et fraîcheur des données.
- Ne pas assimiler une panne ou une restriction géographique du serveur de test à une panne globale du proxy.

Cette analyse repose sur la lecture du code et l’exécution de tests existants. Aucun benchmark réseau n’a été réalisé : les gains évoqués sont attendus, mais non mesurés.

## 2. Synthèse

Le principal levier consiste à remplacer une sélection par lots bloquants, suivie de validations séquentielles, par une recherche progressive, bornée et annulable dès qu’un candidat utilisable est trouvé.

Le backend dispose déjà de fondations pertinentes :

- préférence indépendante par destination ;
- cache mémoire et persistant ;
- découverte à la demande ;
- mutualisation du chargement fournisseur par pays ;
- exclusions spécifiques à une destination.

Il est recommandé de conserver ces fondations et de mieux organiser les priorités, plutôt que de réécrire toute la sélection.

La révision ajoute un préalable de fiabilité : séparer la santé propre au proxy, la réussite d’un échange via une URL de test et l’acceptation par le domaine métier. Une URL inaccessible ne doit pas dégrader tous les proxies testés. La persistance des préférences et l’absence de priorité entre pays deviennent des exigences de premier niveau, et non des options à confirmer.

## 3. Périmètre du code examiné

Les principaux fichiers étudiés sont :

| Responsabilité | Fichier |
| --- | --- |
| Inventaire, cache et sélection | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_inventory.rs` |
| Tests des proxies | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_probe.rs` |
| Routage et connexion | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/core.rs` |
| Modèle des proxies | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_record.rs` |
| Cache persistant | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_repository.rs` |
| Chargement des sources | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs` |
| Agrégation des sources | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` |
| Réponses HTTP et rotation | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs` |
| Résolution géographique | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/ip_country.rs` |
| Documentation du comportement | `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/README.md` |

## 4. Fonctionnement actuel

### 4.1. Réutilisation par destination

`preferred_by_destination` mémorise le dernier proxy ayant obtenu une réponse considérée comme acceptée pour une clé composée de :

```text
pays + schéma + hôte + port
```

Deux domaines appelés en parallèle peuvent donc conserver des proxies préférés différents. La portée est celle d’une destination exacte, et non celle de tous les sous-domaines d’un domaine parent.

La préférence est consultée avant les préchecks et la découverte fournisseur. L’ancienneté du test général ne déclenche pas, à elle seule, une nouvelle validation du proxy préféré. Les vérifications d’éligibilité restent applicables : pays, santé, authentification, capacités et exclusions.

Ce comportement correspond à l’objectif : ne pas rechercher un nouveau proxy tant que celui accepté pour la destination reste éligible.

Limites actuelles :

- Les préférences ne sont pas restaurées après un redémarrage.
- Une réponse est considérée comme acceptée dès que son statut ne figure pas dans `proxy_rejection_statuses`. Cela ne garantit pas une validation fonctionnelle du contenu.

### 4.2. Parcours de sélection

Le parcours général est le suivant :

```text
proxy préféré pour la destination
→ éventuelle affinité
→ cache mémoire
→ cache persistant
→ chargement fournisseur
→ nouvelle sélection
```

La recherche dans le cache n’est pas une simple lecture. Elle peut déclencher des tests réseau sur les candidats inconnus ou dont le test a dépassé `probe_ttl`, soit 10 minutes par défaut.

Pour HTTPS, les capacités validées passent avant les simples indications fournisseur. Pour HTTP, le classement repose essentiellement sur la latence, sans séparation aussi nette entre candidats validés et inconnus.

La sélection multi-pays parcourt les pays dans l’ordre demandé. Le parcours d’un pays peut inclure des tests réseau avant que les candidats du pays suivant soient examinés.

### 4.3. Préchecks et validations

`select_cached()` effectue des préchecks TCP par fenêtres, puis valide les capacités des candidats séquentiellement.

```text
prendre une fenêtre de candidats
→ effectuer les préchecks TCP en parallèle
→ attendre toute la fenêtre
→ valider les capacités candidat par candidat
→ retourner le premier candidat utilisable
```

La configuration générale prévoit 8 préchecks simultanés. La configuration construite par Scrapyfy porte cette fenêtre à 32 candidats.

### 4.4. Actualisation fournisseur

Le chargement fournisseur :

- est déclenché à la demande ;
- est mutualisé par pays ;
- applique un délai minimal de 120 secondes entre tentatives, y compris après un échec ou une liste vide ;
- conserve les informations d’exécution des endpoints déjà connus ;
- stocke les listes sans tester immédiatement tous les endpoints.

Ces mécanismes sont à conserver. Ils ne nécessitent pas de processus périodique en arrière-plan.

Certains échecs de transport observés peuvent libérer le délai d’actualisation afin qu’une sélection ultérieure puisse rechercher des remplaçants lorsque le cache ne suffit plus.

### 4.5. Réponses de la destination

La configuration Scrapyfy teste les capacités HTTPS générales avec `https://example.com/`. Elle ne garantit donc pas que le domaine demandé acceptera le proxy.

La couche HTTP enregistre les refus configurés, invalide la sélection correspondante et abandonne son client HTTP mis en cache pour ne pas réutiliser le tunnel rejeté. Elle effectue actuellement un seul retry automatique.

### 4.6. Deux phases de test et limites de leur interprétation

La distinction fonctionnelle est :

1. **Connexion au proxy** : accessibilité de l’endpoint et, selon l’échange effectivement réalisé, transport TLS vers un proxy HTTPS, négociation du protocole et authentification.
2. **Connexion à un serveur via le proxy** : capacité d’acheminer une connexion vers une destination donnée, puis éventuellement TLS et réponse HTTP à l’URL fournie.

Dans le parcours automatique actuel, le précheck initial est TCP uniquement. Il ne valide ni le protocole du proxy ni son authentification. Ces vérifications ne sont donc pas toutes isolées de la seconde phase dans l’implémentation existante. SOCKS5 permet une négociation initiale avant la demande de connexion ; un CONNECT HTTP ou SOCKS nécessitant une destination ne peut pas, à lui seul, isoler complètement les capacités du proxy de l’état de la cible.

Points constatés dans le code :

- `failed_capability_status()` classe les timeouts, erreurs d’E/S et erreurs DNS comme résultats inconnus, mais les erreurs TLS, de protocole et `UpstreamRejected` comme indisponibles.
- Un CONNECT HTTP rejeté est représenté par `UpstreamRejected`, sans distinction structurée entre refus du proxy, restriction de destination et indisponibilité du serveur cible.
- Une réponse HTTPS 200–499 valide l’échange ; une réponse 5xx est rejetée. `destination_tls` agrège ainsi la validation TLS et le résultat HTTP, alors que ce sont deux observations différentes.
- Une réponse 403 ou 451 peut prouver que le transport fonctionne tout en indiquant que le contenu n’est pas accessible. Elle ne valide pas l’acceptation métier.
- `apply_result()` peut produire un statut global KO lorsque le résultat n’est pas utilisable ; le mode relâché possède des mécanismes de repli, mais ne suffit pas à résoudre toutes les ambiguïtés d’attribution.
- `record_destination_failure()` peut supprimer un enregistrement ancien dès un échec de destination. Il peut également promouvoir plusieurs échecs de destinations autres que `BlockedByOrigin` en KO global. Ces règles doivent être revues pour les échecs attribuables ou possiblement attribuables aux cibles.

L’ancienneté seule, un timeout vers la cible distante ou un code HTTP seul ne constituent pas une preuve suffisante de panne du proxy. Un échec de connexion à l’endpoint du proxy est une observation différente et relève de sa disponibilité globale depuis le backend.

### 4.7. Distinction validée entre exclusion de destination et éviction du proxy

La portée limitée à une destination concerne les tests nécessitant un accès à un serveur distant via le proxy et les refus métier contextualisés. Elle ne doit pas être appliquée à une impossibilité de se connecter au proxy lui-même.

| Type de test ou d’échec | Portée et conséquence |
| --- | --- |
| Connexion directe à l’endpoint du proxy, sans serveur distant via le proxy | Santé globale du proxy depuis le backend ; si l’échec de connexion est observé après le seuil de 24 heures retenu, supprimer le proxy de la liste disponible |
| Tunnel, TLS distant ou requête HTTP via le proxy | Résultat contextualisé par cible, proxy et phase ; ne pas supprimer globalement l’endpoint sur cette seule observation |
| Rejet explicite du proxy par le domaine, classifié par le handler | Exclusion pour la destination exacte ; sa durée est indépendante du seuil d’éviction globale |

**Décision acquise : après 24 heures, un proxy auquel il est impossible de se connecter doit être supprimé de la liste des proxies disponibles.** Cette vérification se fait à la demande : l’écoulement du délai ne déclenche ni test en arrière-plan ni suppression automatique d’un proxy toujours fonctionnel. Avant ce seuil, un échec de connexion rend le candidat globalement indisponible selon sa temporisation, sans le proposer aux autres domaines comme s’il était sain.

La suppression doit retirer l’endpoint de l’inventaire mémoire et persistant et nettoyer les références de préférences, faute de quoi un redémarrage pourrait le réintroduire immédiatement. Une découverte ultérieure peut le proposer de nouveau comme candidat à valider ; sa réapparition dans une liste fournisseur ne constitue pas un succès et ne doit pas contourner les protections contre les réessais immédiats.

**Décision acquise : le délai de 24 heures part de la dernière preuve positive de fonctionnement du proxy**, et non du premier échec de connexion. Une actualisation fournisseur, une tentative échouée ou une observation inconclusive ne renouvelle pas cette preuve.

Le code actuel utilise `last_validated_at`, sinon `last_checked` ; sans ces deux dates, il ne considère pas l’enregistrement comme ancien. Le futur modèle doit distinguer une preuve positive d’un simple horodatage de tentative. La représentation précise des preuves reste à concevoir ; la date de découverte ne doit pas être présentée comme une preuve de fonctionnement.

**Traitement des candidats jamais validés approuvé :** si la connexion au proxy lui-même échoue, le retirer immédiatement des candidats utilisables, conserver son état d’échec et passer au suivant. Autoriser un nouvel essai uniquement à la demande après 15 minutes, valeur initiale ajustable. Si le candidat n’a toujours aucune preuve positive plus de 24 heures après sa découverte initiale et qu’une nouvelle connexion échoue, supprimer son enregistrement de l’inventaire disponible. La découverte est uniquement une date de rétention ; un rechargement fournisseur ne renouvelle ni cette date ni la temporisation d’échec. Une connexion réussie apporte une preuve positive pour la phase effectivement réalisée, sans valider les phases distantes. Un échec limité au serveur distant ne justifie pas cette suppression globale.

Le seuil de 24 heures est indépendant du cooldown de refus de destination : le premier conditionne une éviction globale sur un échec de connexion propre au proxy, le second interdit temporairement une association proxy/destination.

**Exclusion de destination validée : 24 heures par défaut, configurables selon la politique de rejet.** Elle concerne uniquement le couple proxy/destination exacte et conserve le dernier succès dans l’historique durable tout en rendant l’association inéligible. À expiration, le candidat redevient éligible sous réserve des autres contrôles, sans test en arrière-plan ni obligation de l’essayer immédiatement. Ne pas lever l’exclusion avant son échéance lorsque tous les candidats sont exclus : rechercher d’autres candidats dans le budget, puis signaler l’absence de candidat si nécessaire. Une actualisation fournisseur ou un redémarrage n’efface pas l’exclusion.

## 5. Causes de lenteur identifiées

### 5.1. Attente de la fin des préchecks d’une fenêtre

Dans `select_cached()`, l’attente suivante impose la fin de toute la fenêtre :

```rust
let prechecked: Vec<_> = prechecks.collect().await;
```

Un candidat prêt peut attendre un endpoint lent du même lot. Le précheck TCP est borné à 3 secondes par défaut.

Cette attente peut également retarder un candidat déjà validé et frais lorsqu’il partage une fenêtre avec des candidats nécessitant un précheck.

Remplacer uniquement `buffered` par `buffer_unordered` ne suffirait pas : il faut aussi supprimer la collecte complète avant la validation.

### 5.2. Validations séquentielles des capacités

Après les préchecks, les candidats sont validés un par un. La sélection applique une limite extérieure de 8 secondes par validation complète.

Exemple théorique : quatre candidats accessibles en TCP mais inutilisables peuvent consommer environ 32 secondes avant d’atteindre le candidat suivant.

Augmenter la taille de la fenêtre TCP ne résout donc pas le coût principal des endpoints qui acceptent la connexion mais échouent ensuite.

### 5.3. Exploration excessive du cache avant actualisation

Le code peut parcourir tous les candidats sélectionnables d’un pays avant de passer au suivant, au cache persistant ou aux fournisseurs.

Un grand nombre de proxies inconnus en mémoire peut ainsi retarder un candidat déjà validé disponible ailleurs.

La politique actuelle ne sépare pas suffisamment :

1. la réutilisation d’un candidat connu ;
2. l’exploration d’une quantité raisonnable de candidats inconnus ;
3. le renouvellement des données.

### 5.4. Attente de toutes les sources fournisseur

L’agrégateur exécute les sources en parallèle avec `join_all`. Les premières listes reçues ne sont pas mises à disposition de la sélection avant la fin de toutes les sources.

Une source lente peut donc retarder les candidats d’une source rapide.

Le fournisseur peut ensuite effectuer jusqu’à 20 résolutions géographiques séquentielles pour les lignes sans pays, avant de retourner la liste filtrée. Des candidats déjà géolocalisés peuvent attendre ce travail.

### 5.5. Écart entre capacité technique et acceptation par le domaine

Un proxy peut réussir le test général puis être refusé par la destination réelle.

Comme la couche HTTP ne réalise qu’un retry automatique, plusieurs candidats techniquement fonctionnels mais refusés par l’origine peuvent empêcher de trouver un candidat accepté dans le même appel.

## 6. Stratégie recommandée

### 6.0. Préalable : classifier les observations sans accuser la mauvaise composante

#### Séparer les états selon leur portée

| Observation | Portée proposée | Ce qu’elle permet de conclure |
| --- | --- | --- |
| Connexion TCP à l’endpoint | Proxy | Endpoint accessible ou inaccessible depuis le backend |
| TLS vers le proxy HTTPS | Proxy et identité TLS du proxy | Transport sécurisé validé ou erreur de sécurité ; ne pas désactiver la validation des certificats |
| Négociation du protocole et authentification | Proxy et protocole | Protocole confirmé ou exigence d’authentification, selon les preuves disponibles |
| Réponse à la demande de tunnel | Proxy, protocole et destination | Tunnel établi, refus expliqué ou échec ambigu ; pas nécessairement une incapacité globale |
| TLS avec le serveur distant | Proxy, destination et contexte réseau | Échange TLS réussi ou échec contextualisé ; ne pas confondre avec le certificat du proxy |
| Réponse HTTP du serveur de test | URL de test, proxy et contexte géographique | Transport observé et statut de la cible ; l’acceptation dépend d’un critère distinct |
| Réponse acceptée du domaine métier | Proxy et destination métier | Validation de la préférence pour cette destination |

Pour la seconde phase, conserver séparément les faits positifs : tunnel établi, TLS valide et réponse HTTP reçue. Un échec applicatif ultérieur ne doit pas effacer ces preuves ni une réussite antérieure pour un autre domaine.

Les résultats négatifs ambigus restent **inconclusifs pour les capacités générales**. Ils peuvent limiter temporairement les essais pour la cible concernée, mais ne doivent ni marquer globalement le proxy KO ni supprimer sa préférence pour un domaine sans rapport. Un échec explicite du protocole peut être global à ce protocole ; un refus de destination ne l’est pas.

#### Cas du serveur fourni dans l’URL de test

| Situation | Interprétation | Comportement recommandé |
| --- | --- | --- |
| Serveur opérationnel et accessible | Une réussite apporte une preuve positive d’acheminement vers cette cible | Mémoriser les étapes réussies ; distinguer transport et acceptation du statut ou du contenu |
| Serveur complètement indisponible | Les timeouts ou refus via plusieurs proxies ne prouvent pas que tous les proxies sont défectueux | Préserver leur santé générale ; borner les essais et suspendre temporairement cette cible lorsque les observations convergent |
| Serveur accessible uniquement depuis certains pays | L’appel fournit déjà les pays autorisés ; l’échec d’un candidat peut être propre à cet endpoint, notamment une blacklist | Continuer avec les autres endpoints éligibles de l’ensemble autorisé ; ne jamais généraliser les échecs à un pays |
| Situation impossible à départager | Une observation distante seule ne permet pas toujours d’attribuer la panne | Retourner un résultat inconclusif contextualisé plutôt qu’une certitude de panne globale |

Une erreur TLS distante doit également rester distincte : elle peut venir du serveur ou d’une interception. Il faut refuser l’échange non sûr, sans prétendre identifier sa cause avec certitude ni contaminer automatiquement les capacités des autres destinations.

#### Éviter une cascade de tests sur une cible indisponible

Prévoir un état de santé partagé de la cible de test, alimenté uniquement par les requêtes en cours :

- conserver les succès et échecs récents avec leur phase, le proxy et le pays ;
- mutualiser les diagnostics pour éviter que chaque domaine recommence le même travail ;
- utiliser plusieurs observations indépendantes avant une suspension collective ;
- distinguer les échecs propres aux endpoints des indices d’indisponibilité de la cible ; aucune suspension ou exclusion collective par pays ;
- ouvrir un circuit temporaire après des échecs convergents, sans déclarer une panne certaine ;
- après son expiration, autoriser un essai de reprise à la demande, mutualisé, puis rouvrir ou refermer le circuit selon le résultat ;
- ne pas déclencher d’actualisation massive de proxies pour compenser une cible présumée indisponible.

La liste de pays exprime une connaissance préalable des origines autorisées, sans garantir que chaque adresse de proxy sera acceptée. Le pays reste une métadonnée de diagnostic, jamais une clé d’exclusion collective. Un serveur peut blacklister un endpoint tout en acceptant un autre du même pays. Une réussite doit pouvoir lever rapidement une suspicion de panne de cible et les observations négatives doivent avoir une durée de validité limitée.

**Politique de suspension validée, avec paramètres ajustables par itération :**

- Le seuil initial de 3 échecs convergents en 60 secondes via 3 endpoints distincts dont la connexion au proxy fonctionne est un signal de suspicion de panne, pas un verdict ni une exclusion collective. Plusieurs appels partageant une même tentative ne constituent pas plusieurs observations indépendantes. La suspicion ne suffit pas à interrompre les essais vers le domaine réel.
- Suspendre les nouveaux tests complémentaires vers la cible pendant 30 secondes, puis 60 et 120 secondes au maximum si les essais de reprise échouent encore.
- Après expiration, la prochaine demande autorise un seul test de reprise mutualisé. Aucun test périodique ni worker en arrière-plan n’est lancé.
- Une réussite vers la cible contredit la suspicion de panne totale, lève la suspension associée et remet sa progression à zéro ; elle ne lève pas les exclusions explicites d’autres endpoints.
- **Correction validée : aucune suspension par pays et aucune exclusion collective de ses proxies.** Des échecs dans un ou plusieurs pays ne suffisent pas à conclure à une panne de la cible : plusieurs endpoints peuvent être blacklistés. Continuer à considérer les autres candidats autorisés. Une suspension de l’URL complémentaire, indépendante des pays, doit reposer sur des indices spécifiques à l’indisponibilité du serveur et ne doit pas interdire les essais métier via d’autres proxies.
- Un 403 ou 451 n’est pas une absence de réponse : il peut indiquer un refus d’accès tout en confirmant le transport. Une erreur TLS, un timeout ou un refus de tunnel ne prouve pas à lui seul une panne de la cible.
- Les durées 30/60/120 secondes concernent la suspension des tests vers la cible, et non le timeout d’un proxy ou le budget total de recherche.
- Ne pas déclarer les proxies défectueux et ne pas faire attendre une recherche pour l’expiration de la suspension. Avec l’option B, les essais vers le domaine réel restent possibles dans leur budget, sous réserve des contrôles de sécurité et de la politique de retry.

**Distinction suspicion/rejet validée :** un refus classifié par le handler entraîne uniquement l’exclusion de l’endpoint pour la destination. Une réponse indiquant une indisponibilité du service constitue un indice de dégradation de la cible, sans condamnation du proxy. Un timeout ou refus de tunnel reste ambigu : plusieurs proxies peuvent être blacklistés silencieusement. Sans preuve suffisamment claire de panne, poursuivre les essais réels avec les autres candidats jusqu’au succès ou aux budgets établis. Une suspension de l’URL complémentaire sert uniquement à limiter des tests répétitifs et ne bloque pas les autres candidats pour le domaine réel. Les critères techniques d’un indice de panne doivent préserver ces règles, sans créer de verdict à partir du seul nombre de pays en échec.

Une vérification directe depuis le backend peut apporter un indice supplémentaire, mais pas un verdict : elle utilise son propre pays et son réseau. Son échec ne prouve pas une panne globale, et sa réussite ne prouve pas l’accessibilité depuis tous les pays. Elle doit rester facultative et compatible avec les règles de confidentialité et de routage.

**Décision acquise : le diagnostic direct est désactivé par défaut.** Aucune connexion automatique sans proxy ne doit être ajoutée pour vérifier la cible. Une activation explicite peut autoriser ce diagnostic en tenant compte de l’exposition de l’adresse IP du backend et de son contexte géographique. Son résultat reste un indice, jamais une preuve de disponibilité ou d’indisponibilité mondiale.

#### Quel usage conserver pour l’URL de test ?

Deux approches ont été examinées :

1. **URL obligatoire avec diagnostic contextualisé** : conserver le test, mais un résultat inconclusif ou une cible suspendue ne doit pas invalider globalement les proxies. Cette approche minimise les changements du parcours, au prix d’une étape supplémentaire et de la dépendance à cette cible.
2. **URL comme preuve complémentaire, sans en faire une porte obligatoire pour chaque domaine** : privilégier les preuves déjà en cache et la requête réelle. Un proxy dont le transport est validé mais dont l’échange distant reste inconclusif peut faire l’objet d’un essai borné vers le domaine demandé, sans transformer ses capacités inconnues en capacités validées.

**Décision acquise : option B, URL comme preuve complémentaire.** Sa réussite n’est pas un préalable obligatoire à chaque essai vers le domaine métier. Un résultat ambigu n’empêche pas un essai réel borné ; il ne transforme pas pour autant une capacité inconnue en capacité validée et ne permet pas de contourner une incompatibilité explicite ou la validation des certificats.

Un éventuel mode strict doit rester explicite : il ne peut pas sélectionner silencieusement un proxy dont les capacités exigées ne sont pas démontrées. La politique complémentaire retenue doit être distinguée de ce mode, afin de ne pas réintroduire implicitement l’URL comme préalable obligatoire dans le parcours choisi. Si un appel impose effectivement des preuves strictes indisponibles, signaler une validation inconclusive sans qualifier tous les proxies de défectueux.

**Comportement strict validé :**

- Réutiliser les candidats disposant des preuves requises et restant éligibles. Ne pas imposer le mode strict au parcours optimisé ni renforcer implicitement les appels existants ; vérifier le mapping des modes actuels lors de la conception technique.
- Lorsqu’un candidat manque de preuves, le tester une fois si sa temporisation et le budget le permettent.
- Si le résultat reste inconclusif, ne pas utiliser ce candidat pour la requête métier stricte et passer à un autre. Ne pas répéter le même test du même candidat dans la recherche.
- Ne signaler une validation inconclusive qu’après épuisement des possibilités dans le budget, lorsqu’aucun candidat ne satisfait les preuves exigées. Distinguer ce diagnostic d’un échec explicite de connexion ou de protocole lorsqu’il est établi.
- Un résultat inconclusif peut correspondre à un proxy réellement inutilisable ; il n’est ni un succès ni une preuve suffisante de défaillance globale. Mémoriser la tentative et limiter les nouveaux essais.
- Un retest ultérieur est uniquement à la demande, après temporisation, avec une portée par phase et cible. Le code actuel utilise `probe_ttl`, soit 10 minutes par défaut, pour empêcher la répétition récente d’une tentative de capacité ; l’adaptation au nouveau modèle reste à réaliser. Les suspensions de cible de 30/60/120 secondes sont distinctes et leur expiration ne remet pas à zéro la temporisation du candidat.
- Dans le parcours complémentaire non strict, un essai borné vers le domaine réel après un résultat inconclusif sur une autre URL reste autorisé. Cet essai n’est pas une répétition du même test et respecte les budgets, la sécurité et la politique de retry.
- Aucun mode ne transforme une capacité inconnue en preuve validée et aucun mode ne contourne une incompatibilité explicite, les contrôles TLS ou une exclusion active. Les capacités strictement prouvées ne garantissent pas l’acceptation métier : celle-ci est décidée par le handler à partir de la réponse réelle.

Des cibles de secours configurables peuvent compléter le diagnostic, à la demande et avec un budget global. Elles ne doivent pas devenir une série systématique de tests avant chaque requête. Leur politique et les critères configurables sont validés ci-dessous ; les URLs concrètes dépendent de la configuration déployée.

**Politique de secours validée :**

- Les URLs de secours sont facultatives et configurables ; aucune nouvelle URL externe n’est imposée.
- Si la cible complémentaire est suspendue ou donne un résultat inconclusif, autoriser une seule cible de secours par validation, sans recommencer le budget de temps et sans dépasser les limites de travail.
- Ne pas tester systématiquement toutes les URLs et ne pas créer de chaîne récursive de secours.
- Une réussite apporte une preuve de transport, pas une garantie d’acceptation du domaine métier. Les résultats et suspensions restent contextualisés par cible.
- Sans URL de secours configurée, l’essai du domaine réel reste possible conformément à l’option B, sous réserve des contrôles de sécurité et de la politique de retry.
- Les propriétés et critères d’acceptation des URLs effectivement configurées devront être documentés ; leur choix concret n’est pas arrêté ici.

Si l’URL de test correspond au domaine métier demandé, un serveur indisponible peut rendre la demande impossible : il faut alors arrêter la recherche à son budget, préserver les proxies et retourner une erreur contextualisée. Une validation de transport ne suffit pas à promettre une réponse métier.

**Configuration des URLs de test validée :**

- URLs configurables, remplaçables ou désactivables, sans nouveau service externe obligatoire. Conserver les URLs actuelles comme valeurs de compatibilité.
- Utiliser HEAD pour HTTPS par défaut, avec GET configurable lorsque la cible ne prend pas correctement en charge HEAD. Aucun fallback automatique systématique entre méthodes.
- Ne pas suivre les redirections par défaut ; une redirection apporte une observation mais ne valide pas automatiquement la destination finale.
- Réutiliser le contrat de classification validé avec une politique propre à la cible : statuts attendus et, facultativement, marqueurs d’en-têtes ou de contenu, dans les limites d’inspection définies.
- Conserver séparément les preuves positives : tunnel établi, TLS validé, réponse HTTP reçue et réponse acceptée par la politique de la cible.
- Un 403 n’efface pas les preuves de tunnel ou de TLS ; un 5xx ne condamne pas automatiquement le proxy. Contextualiser les résultats par cible, endpoint et phase.
- Une réussite sur l’URL complémentaire ne crée pas de préférence pour un autre domaine. Seule une réponse acceptée du domaine concerné établit sa préférence.

### 6.1. Étape 1 : réutiliser le proxy préféré sans découverte

Conserver le parcours rapide actuel :

- recherche par destination exacte ;
- vérification des exclusions ;
- utilisation directe ;
- renouvellement de la préférence après réponse acceptée.

Ne pas imposer d’actualisation périodique à ce parcours. Si le proxy fonctionne toujours, aucune découverte n’est nécessaire.

La persistance de la préférence est obligatoire. Une préférence restaurée doit toujours passer les contrôles d’éligibilité, mais ni le redémarrage ni l’ancienneté du test général ne doivent imposer une nouvelle découverte.

Prévoir un stockage durable de l’association entre destination exacte et endpoint du proxy, avec la date du dernier succès. La liste de pays fournie à l’appel sert uniquement de filtre d’éligibilité : elle ne doit pas faire partie de l’identité de la préférence. Lorsqu’une préférence pointe vers un pays non autorisé par un appel, cet appel l’ignore sans la supprimer.

La clé actuelle inclut le pays. **Modèle validé : une préférence principale par schéma, hôte et port, avec un historique durable borné à 3 endpoints distincts acceptés par destination**, préférence principale comprise. Chaque association conserve l’endpoint, la date du dernier succès accepté et les informations nécessaires à sa résolution dans l’inventaire. Cela permet de retrouver un candidat accepté appartenant à l’ensemble autorisé lorsqu’une autre requête utilise des pays différents.

À chaque appel, sélectionner le candidat au succès accepté le plus récent parmi ceux encore éligibles et appartenant aux pays autorisés, sans priorité de pays. Ne pas supprimer une association parce que son pays est interdit pour un appel particulier. Ne pas expirer les préférences sur leur seul âge : un refus explicite peut les rendre temporairement inéligibles, et les références aux endpoints supprimés doivent être nettoyées.

Les mises à jour sont atomiques par destination et corrélées au proxy réellement utilisé. Le succès accepté le plus récent passe en tête sans perdre les autres associations dans la limite de 3. L’ordonnancement doit empêcher qu’une écriture retardée d’une observation ancienne remplace une observation plus récente.

À la restauration, charger l’endpoint référencé et les exclusions pertinentes avant toute recherche réseau. Gérer les références orphelines et les modifications de pays. Les timestamps durables doivent être des dates persistables, pas des `Instant` propres au processus.

La création ou le remplacement de la préférence doit être écrit durablement dans le parcours de succès, sans différer cette opération à un processus en arrière-plan. Le coût peut être limité en écrivant lors du changement d’association et en bornant les écritures de timestamps répétées sans fausser l’ordre durable des succès.

**Politique d’échec de persistance validée : retourner la réponse métier réussie, conserver la préférence en mémoire et signaler clairement l’erreur d’écriture dans les logs et les métriques.** Ne pas refaire la requête métier pour réparer le stockage et ne pas masquer l’échec. Dans ce cas exceptionnel, la conservation du nouveau succès après redémarrage n’est pas garantie ; l’exigence de persistance demeure le fonctionnement nominal. Une écriture ultérieure peut être tentée lors d’une opération à la demande, sans worker de réparation en arrière-plan.

Un échec ambigu ou une indisponibilité de cible suspend l’usage de la préférence si nécessaire, mais ne détruit pas le dernier succès durable. Un refus confirmé du domaine peut rendre l’association temporairement inéligible ; un nouveau succès remplace ensuite la préférence. Les mises à jour concurrentes doivent être corrélées à la sélection réelle et ordonnées pour qu’une ancienne observation n’écrase pas un état plus récent.

### 6.2. Étape 2 : séparer les candidats par niveau de validation

Créer des catégories explicites :

| Priorité | Candidats |
| --- | --- |
| 1 | Proxy préféré pour la destination |
| 2 | Autres proxies validés, compatibles et sans exclusion |
| 3 | Proxies connus nécessitant une revalidation |
| 4 | Proxies inconnus en cache |
| 5 | Nouveaux candidats issus d’une actualisation |

Cette séparation doit exister pour HTTP comme HTTPS.

Un candidat prêt à utiliser ne doit pas attendre les préchecks de candidats inconnus.

La décision est acquise : aucune priorité entre pays. Normaliser et dédupliquer la liste, puis traiter les pays comme un ensemble autorisé.

Rassembler les candidats de tous ces pays avant de les classer par préférence de destination, preuves utilisables, latence pertinente et échecs contextualisés. L’ordre de la liste ne doit pas modifier les préférences, les budgets, la clé de mutualisation de la recherche ni l’éligibilité. Un départage stable par endpoint peut être utilisé lorsque les critères sont identiques.

Les caches persistants des pays autorisés doivent être importés avant une exploration coûteuse des inconnus en mémoire, afin qu’un candidat validé stocké pour un autre pays ne soit pas retardé. Les lectures peuvent être mutualisées et bornées.

Les actualisations des pays éligibles peuvent avancer en parallèle avec une limite de concurrence commune et une mutualisation par pays. Aucun pays ne doit attendre simplement parce qu’il apparaît plus tard dans la liste. Si tous les pays ne peuvent être lancés simultanément, l’ordonnanceur utilise l’état du cache, les délais applicables et les observations disponibles, pas l’ordre fourni.

L’objectif n’est pas de répartir équitablement les requêtes entre pays : le premier candidat approprié peut gagner. L’unique contrainte géographique est son appartenance à l’ensemble autorisé.

### 6.3. Étape 3 : tester progressivement les candidats inconnus

Remplacer les fenêtres bloquantes par un pipeline :

```text
précheck d’un candidat
→ validation de ses capacités dès qu’il est prêt
→ mise à disposition immédiate du résultat
```

Prévoir deux limites indépendantes :

- concurrence des préchecks TCP ;
- concurrence, plus faible, des validations complètes.

Valeurs initiales à mesurer, et non réglages définitifs :

- 8 préchecks TCP simultanés ;
- 2 à 4 validations complètes simultanées.

Toutes les opérations doivent appartenir à la requête. Aucun worker périodique n’est nécessaire. Lorsque la recherche trouve un candidat ou que l’appel est abandonné, les opérations devenues inutiles sont annulées.

Compromis : l’ordre strict entre candidats inconnus est remplacé par la préférence au premier résultat utilisable. Ce choix correspond à la priorité de rapidité.

### 6.4. Étape 4 : actualiser après un budget d’exploration

Ne pas attendre l’épuisement complet du cache.

Déclencher une actualisation lorsque :

- aucun candidat validé n’est disponible ;
- un budget de temps ou de candidats inconnus a été consommé ;
- la politique de délai fournisseur l’autorise.
- les observations ne désignent pas principalement une indisponibilité ou une restriction de la cible testée ; sinon, traiter cette cible avant de renouveler les proxies.

Le réglage initial proposé est de déclencher une actualisation après 10 secondes d’exploration ou 8 validations complètes terminées sans candidat utilisable, selon le premier seuil atteint. Ce seuil n’arrête pas la recherche dans le cache : il permet de commencer l’actualisation en parallèle. Les valeurs sont provisoires et seront calibrées par mesure ; voir la section 10.3.

Une fois ce seuil atteint, deux branches peuvent progresser en parallèle :

1. exploration bornée des candidats inconnus en cache ;
2. actualisation fournisseur mutualisée.

Les deux branches restent attachées à la requête et sont arrêtées lorsqu’elles ne sont plus nécessaires. Il ne s’agit pas d’une maintenance en arrière-plan.

Conserver un plancher anti-boucle pour éviter que plusieurs domaines en échec ne déclenchent des actualisations répétées ramenant les mêmes endpoints. La libération actuelle du délai après certains échecs doit être prise en compte.

### 6.5. Étape 5 : rechercher un proxy réellement accepté

La validation générale des capacités reste utile, mais la requête réelle doit décider de l’acceptation par le domaine.

Prévoir une rotation :

- bornée par un délai total ;
- bornée par un nombre de candidats ;
- sans retester le même proxy dans le même appel ;
- réservée aux refus explicitement configurés et aux erreurs pertinentes.

Une panne présumée du domaine ne doit pas être convertie en exclusions longues pour chaque proxy ni déclencher une rotation illimitée. Conserver des délais distincts : court pour l’échec ambigu d’une cible, adapté pour une restriction observée, et exclusion par endpoint pour un refus métier confirmé. Un 403 isolé ne prouve pas que tout un pays est refusé.

Pour les requêtes non idempotentes, notamment POST, ne pas élargir automatiquement les retries sans règle explicite.

**Politique de retry validée :**

| Situation | Comportement |
| --- | --- |
| Échec avant tout envoi de la requête métier, lors de la connexion au proxy, du tunnel ou du TLS | Autoriser un autre candidat dans le budget existant, si l’absence d’envoi est démontrée |
| Requête de lecture GET ou HEAD | Autoriser une nouvelle tentative après un échec pertinent ou un rejet configuré |
| POST, PATCH ou autre requête potentiellement modificatrice déjà envoyée, ou dont l’envoi est incertain | Aucun retry automatique par défaut |
| Requête explicitement déclarée rejouable dans sa configuration | Autoriser le retry si son corps peut être reproduit et si les garanties de l’opération le permettent |

Pour une opération potentiellement modificatrice, un statut de refus ne garantit pas à lui seul l’absence d’effet côté serveur. Une autorisation explicite par requête est nécessaire, plutôt qu’une déduction à partir du statut seul. Lorsque l’absence d’envoi n’est pas prouvée, considérer l’envoi comme incertain.

Les limites communes sont : au maximum 8 proxies essayés sur le domaine, tentative initiale comprise ; aucun renouvellement du budget de 120 secondes ; aucune requête métier spéculative en parallèle ; aucun nouvel essai du même proxy dans la recherche. Les plafonds sont partagés entre les couches de retry et ne se multiplient pas entre le connecteur et le client HTTP. Une erreur probablement attribuable au serveur distant ne doit pas provoquer de rotations inutiles.

Il est déconseillé de lancer systématiquement plusieurs requêtes métier en parallèle : cela peut dupliquer des effets, augmenter la charge et compliquer les sessions.

### 6.6. Classification de la réponse et handler injectable

#### Vérification du mécanisme existant

La vérification du code confirme qu’une exclusion configurable par statut existe déjà, mais aucun trait ou callback injectable de classification des réponses de proxy n’a été identifié dans les parcours examinés.

- Dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs`, `ScraperHttpConfig::proxy_rejection_statuses` contient les statuts excluants. La liste est vide par défaut et est héritée de la configuration parent lorsqu’un enfant ne fournit pas de liste non vide.
- `rotate_rejected_proxy()` compare le statut à cette liste, appelle `mark_dynamic_proxy_blocked_by_origin()`, abandonne le client HTTP mis en cache et permet un retry. `record_final_proxy_response()` enregistre également le refus de la seconde réponse, sans troisième tentative.
- `validate_accepted_proxy_response()` considère actuellement tout statut absent de la liste comme accepté. Cela inclut donc des erreurs métier ou serveur et devra évoluer selon la classification validée ci-dessous.
- Dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/proxy_service.rs`, la route HTTP de médias applique séparément la même logique à partir de `opts.proxy_rejection_statuses`. Elle peut effectuer un retry puis enregistrer le refus ou le succès.
- FranceTV et TF1 configurent explicitement 403 dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/services/francetv_resolver.rs` et `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/services/tf1_resolver.rs`, pour leurs clients et leurs URLs de médias proxifiées. Le 403 n’est donc pas une exclusion globale par défaut.
- `mark_dynamic_proxy_blocked_by_origin()` associe le refus au proxy sélectionné et à la destination exacte. Dans l’inventaire, `BlockedByOrigin` est exclu du compteur de promotion des échecs en KO global ; le cooldown de destination vaut 24 heures par défaut. Attention : la branche d’éviction des enregistrements anciens peut néanmoins supprimer globalement un endpoint, comme relevé en section 4.6.
- Le trait `ProxyHttpPostAction`, dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/actions/mod.rs`, transforme le corps et les en-têtes. Il ne renvoie pas de décision de rejet et ne constitue pas le mécanisme recherché. Les handlers de paramètres et extensions de routage ne classifient pas non plus les réponses.

Il faut parler d’**exclusion du proxy pour une destination**, et non d’exclusion du domaine lui-même. La destination reste accessible avec d’autres candidats éligibles.

#### Décisions validées sur l’acceptation

Conserver trois résultats distincts :

| Résultat | Effet sur l’inventaire |
| --- | --- |
| Accepté | Créer ou renouveler la préférence pour la destination effectivement validée |
| Rejeté pour cette destination | Enregistrer l’exclusion contextualisée du proxy ; une rotation dépend séparément de la sécurité du retry et des budgets |
| Inconclusif ou erreur métier | Ne pas établir un nouveau succès, ne pas condamner le proxy et préserver la préférence durable existante |

Par défaut, les réponses 2xx sont acceptées sauf validation configurée contraire. Les 4xx et 5xx ne provoquent pas automatiquement une exclusion. Les redirections nécessitent une attribution correcte : le succès du domaine final ne prouve pas celui de la destination initiale, et une redirection vers une authentification ou un blocage doit être explicitement classifiée.

Une validation fonctionnelle facultative peut utiliser les statuts attendus, les en-têtes et des marqueurs de contenu. Un échec de parsing ne suffit pas, à lui seul, à rejeter le proxy. Ces principes ont été validés ; le handler demandé porte la politique de rejet personnalisée.

#### Proposition de conception du handler

Définir dans `arachnea-proxy` un trait réutilisable de classification, par exemple `ProxyResponseClassifier`, injectable par paramètre ou builder au niveau du client ou de la requête. Son résultat devrait être un enum accepté/rejeté/inconclusif, plutôt qu’un booléen : « non rejeté » n’est pas synonyme de « accepté ».

Fournir un classificateur de statuts, par exemple `HttpStatusProxyResponseClassifier`, et un constructeur ou handler prêt à l’emploi pour HTTP 403. Le handler 403 renvoie un rejet sur 403 et laisse les autres réponses à la politique d’acceptation ; il ne les valide pas toutes implicitement. Son activation doit être explicite, afin de préserver les cas où 403 signale un problème d’autorisation plutôt qu’un refus de proxy.

Le contrat de réponse fourni au trait doit rester indépendant d’`ArachneaResponse` : `arachnea-http` dépend optionnellement d’`arachnea-proxy`, donc introduire la dépendance inverse créerait un cycle. Utiliser une vue générique du statut, des en-têtes, des URLs et du contexte de sélection, avec accès facultatif à un corps borné. Les adaptateurs des clients HTTP fournissent cette vue.

Le corps ne doit pas être consommé au détriment de l’appelant ni imposer le buffering intégral d’un flux média. La route de médias peut renvoyer un corps streamé ; les handlers basés sur le statut ou les en-têtes doivent fonctionner sans lecture du corps. Un handler nécessitant du contenu déclare ce besoin et sa limite ; une observation insuffisante donne un résultat inconclusif. `ArachneaResponse` est actuellement matérialisée, mais ce fonctionnement ne doit pas être imposé à tous les clients.

La classification est sans effet de bord réseau : elle ne met pas elle-même à jour l’inventaire et ne lance pas de retry. L’orchestrateur applique la décision en la corrélant au proxy réellement utilisé. Une erreur du handler ne doit pas devenir un refus du proxy.

Les listes `proxy_rejection_statuses` existantes deviennent une configuration du handler de statuts, sans supprimer leur possibilité de configuration. Définir une priorité explicite : handler personnalisé injecté, sinon handler construit à partir de la liste configurée, sinon politique prudente par défaut. Le handler personnalisé remplace la politique de rejet de la liste ; ne pas combiner silencieusement des décisions contradictoires.

Un trait Rust ne se sérialise pas dans du YAML ou dans une URL : ces interfaces sélectionnent des handlers connus via des configurations validées. Le code peut injecter un handler personnalisé. Partager la classification entre Scrapyfy et la route de médias pour éviter deux implémentations divergentes, sans installer une politique spécifique à un domaine globalement sur l’inventaire partagé.

**Contrat du handler validé :**

- Fournir la méthode, l’URL demandée, l’URL effective, les informations de redirection disponibles, le statut, les en-têtes et le contexte corrélé au proxy réellement utilisé et à la destination.
- Ne lire aucun corps par défaut : le handler HTTP 403 utilise uniquement le statut.
- Un handler personnalisé peut demander un extrait du corps limité initialement à 64 Kio, avec un délai de lecture borné par le budget restant. Cette inspection ne doit pas imposer le chargement intégral d’un flux média.
- Restituer les octets inspectés dans le corps transmis à l’appelant, sans consommation destructive ni modification de la réponse.
- Si l’extrait est tronqué, indisponible ou insuffisant, retourner `Inconclusive`, sauf si les faits observés permettent déjà une décision. L’absence d’un marqueur dans un extrait incomplet ne démontre pas son absence dans la réponse complète.
- Appliquer la priorité handler personnalisé injecté, sinon handler des statuts configurés, sinon règles prudentes par défaut. La composition de plusieurs handlers est explicite, jamais implicite.
- Le handler classe sans effet de bord : aucune requête réseau, aucun retry et aucune mutation de l’inventaire. Une erreur est signalée et traitée comme inconclusive, jamais comme un rejet du proxy.

Le contrat fonctionnel est validé ; les signatures Rust et les adaptateurs restent à concevoir. Aucun trait ni handler n’a été ajouté au backend pendant cette analyse.

### 6.7. Fournisseur progressif

À plus long terme, permettre à la sélection de consommer les résultats fournisseur au fur et à mesure, plutôt que d’attendre toutes les sources.

Les candidats déjà géolocalisés devraient pouvoir avancer sans attendre la résolution géographique des autres lignes. Les résolutions manquantes peuvent être bornées et réalisées à la demande dans le parcours de recherche.

Cette évolution est plus structurante que la suppression des fenêtres bloquantes, car l’interface fournisseur actuelle retourne une liste complète.

## 7. Concurrence et cohérence

### 7.1. Mutualiser les opérations techniques, isoler les décisions de domaine

Conserver les portées suivantes :

| État ou opération | Portée |
| --- | --- |
| Chargement fournisseur | Pays |
| Validation des capacités techniques | Endpoint du proxy |
| Résultat d’un échange distant de test | Cible, endpoint, protocole et contexte géographique |
| Diagnostic partagé de la cible | Cible, avec observations contextualisées par pays |
| Proxy préféré | Destination exacte |
| Refus de l’origine | Destination exacte et proxy |

Les validations possèdent déjà un verrou par endpoint. Les préchecks TCP sont effectués avant ce verrou et peuvent donc être dupliqués par des appels concurrents.

Le pays des observations du diagnostic partagé sert à l’explication et à la mesure, pas à créer une interdiction collective. Aucun état de suspension de l’ensemble des proxies d’un pays ne doit être ajouté.

Si plusieurs URLs de test sont utilisées, le verrou d’endpoint peut toujours protéger les échanges techniques, mais une preuve relative à une cible ne doit pas être réutilisée comme preuve d’acceptation d’une autre. Les timestamps actuels par phase `tcp/http/https` ne suffisent pas à représenter des résultats distants dépendant de la cible ou d’une configuration modifiée.

Ils devraient rejoindre la même opération technique en cours, sans bloquer les endpoints sans rapport.

### 7.2. Corréler chaque réponse au proxy réellement utilisé

Le suivi HTTP actuel associe une destination à une sélection dans une map.

Ce mécanisme convient aux domaines distincts, mais plusieurs connexions concurrentes vers la même destination peuvent remplacer l’association. Une réponse pourrait alors être attribuée à une sélection plus récente.

Avant d’augmenter la concurrence, transporter un identifiant de sélection ou des métadonnées de connexion jusqu’au retour HTTP est recommandé.

### 7.3. Distinguer annulation et échec

Les timestamps de tentative sont enregistrés avant les validations et limitent les nouveaux essais.

Avec des validations concurrentes, distinguer :

- échec ou résultat inconclusif ;
- opération encore en cours ;
- annulation parce qu’un autre candidat a réussi.

Un candidat annulé parce qu’un autre a gagné ne doit pas être inutilement écarté pendant 10 minutes.

La mutualisation doit aussi préciser le comportement lorsque plusieurs requêtes attendent la même opération : l’abandon d’un appel ne doit pas compromettre sans raison le travail encore nécessaire aux autres.

## 8. Ordre d’implémentation proposé

| Priorité | Changement | Impact attendu |
| --- | --- | --- |
| P0 | Séparer santé du proxy, preuves de transport et résultat de la cible ; contextualiser les erreurs | Éviter de dégrader le cache à cause d’une cible indisponible ou géorestreinte |
| P1 | Persister et restaurer les préférences, avec écriture durable des changements | Respecter la réutilisation obligatoire après redémarrage |
| P1 | Sélectionner dans l’ensemble des pays autorisés sans ordre de priorité | Éviter les attentes dues au parcours pays par pays |
| P1 | Sortir les candidats validés des fenêtres de préchecks | Réutilisation plus immédiate |
| P1 | Supprimer l’attente de fin de fenêtre | Réduction des attentes liées aux endpoints lents |
| P1 | Valider quelques candidats en parallèle avec arrêt anticipé | Réduction du cumul des timeouts |
| P2 | Introduire un budget d’exploration avant actualisation | Meilleur compromis cache et fraîcheur |
| P2 | Mutualiser complètement les tests et gérer les annulations | Moins de travail dupliqué |
| P2 | Corréler précisément sélection et réponse, puis borner la rotation | Recherche fiable d’un proxy accepté |
| P2 | Ajouter un diagnostic partagé et un circuit temporaire pour la cible, alimentés à la demande | Éviter les cascades de tests et les actualisations inutiles |
| P3 | Rendre le fournisseur progressif et la géolocalisation non bloquante | Accélération de la découverte à froid |
| P3 | Réduire les écritures répétées sans différer les changements de préférence | Limiter le coût de persistance sous charge |

La recommandation révisée est de traiter d’abord la classification des résultats distants, puis la persistance des préférences et la sélection dans l’ensemble autorisé. Supprimer ensuite les fenêtres bloquantes et borner les validations concurrentes, avant d’introduire le budget cache/actualisation et le diagnostic partagé de cible.

Les prérequis de cohérence restent applicables : la gestion des annulations doit accompagner les validations concurrentes. Si la concurrence est étendue aux connexions réelles ou aux requêtes HTTP, la corrélation précise des réponses doit être traitée au préalable.

## 9. Mesure et validation

### 9.1. Indicateur principal

Mesurer le temps jusqu’à la première réponse acceptée par la destination, plutôt que le seul temps de sélection technique.

Décomposer ce temps en :

- réutilisation de préférence ;
- lecture et sélection dans les caches ;
- préchecks TCP ;
- validation des capacités ;
- chargement fournisseur ;
- connexion réelle ;
- rotations après refus ou échec.

Suivre également le nombre de candidats essayés, les taux de réutilisation, les opérations mutualisées et les actualisations ne produisant aucun nouvel endpoint utile.

Ajouter les indicateurs suivants : préférences restaurées après redémarrage, échecs d’écriture durable, phase exacte des erreurs, échecs par cible et pays, suspensions de cible, résultats inconclusifs et nombre de proxies dégradés globalement. Une panne de cible ne doit pas provoquer une hausse massive de ce dernier indicateur.

### 9.2. Scénarios à vérifier lors de l’implémentation

Cette liste décrit les validations attendues, sans créer de nouveaux tests à ce stade :

- Serveur de test accessible : les preuves de transport et l’acceptation sont enregistrées séparément.
- Serveur de test complètement indisponible : les proxies et leurs préférences pour les autres domaines restent utilisables ; la recherche respecte son budget.
- Serveur de test accessible depuis un seul des pays autorisés : le candidat de ce pays peut gagner sans pénaliser globalement ceux des autres pays.
- Échec d’un endpoint dans un pays : il ne suffit pas à exclure tous les proxies de ce pays.
- Plusieurs proxies blacklistés dans le même pays : les autres proxies de ce pays restent candidats ; aucune suspension collective ni modification de l’ensemble de pays autorisés.
- Échecs via plusieurs pays : ne pas conclure automatiquement à une panne totale ; distinguer les rejets par endpoint des indices d’indisponibilité du serveur.
- CONNECT rejeté, erreur TLS distante, 403, 451, 5xx et timeout : chaque résultat conserve sa phase et sa portée sans effacer les étapes réussies.
- Même ensemble de pays donné dans des ordres différents : aucune priorité, identité de recherche ou admissibilité ne dépend de cet ordre ; le gagnant peut varier avec les délais réseau, pas avec une préférence géographique implicite.
- Redémarrage : retrouver le dernier proxy accepté et l’essayer avant découverte, sous réserve de son éligibilité.
- Restriction de pays différente à l’appel suivant : ignorer une préférence non autorisée sans la supprimer.
- Panne présumée d’un domaine métier : préserver le dernier succès durable et ne pas explorer sans limite de nouveaux proxies.
- Appels parallèles sur des domaines différents et sur une même destination : aucune attribution de réponse au mauvais proxy ni écrasement incohérent des préférences.
- Annulation ou abandon d’un appel : pas de worker détaché, pas d’échec artificiel pour les candidats annulés et respect des autres appels partageant une opération.

### 9.3. Tests existants exécutés pendant l’analyse initiale

Commande exécutée depuis `/Users/jdecker/Downloads/arachnea/server` :

```sh
CARGO_NET_OFFLINE=true cargo test -p arachnea-proxy --lib --features persistence proxy_inventory::tests -- --test-threads=4
```

Résultat : **14 tests réussis, aucun échec**.

Ces tests couvrent notamment :

- la préférence par destination ;
- le respect des critères d’éligibilité lors de la réutilisation ;
- les exclusions spécifiques à une origine ;
- la restauration des refus depuis la persistance ;
- les caches multi-pays ;
- le chargement fournisseur mutualisé ;
- le délai entre actualisations ;
- la conservation des données runtime lors d’un rechargement.

Aucun nouveau test n’a été créé. L’exécution de ces tests ne constitue pas une mesure de performance réseau ni une validation des optimisations proposées, qui ne sont pas encore implémentées.

Lors de la vérification du mécanisme d’exclusion et du besoin de handler, les tests existants ont également été exécutés avec `CARGO_NET_OFFLINE=true cargo test -p arachnea-proxy --lib --features persistence blocked_by_origin -- --test-threads=4` depuis `/Users/jdecker/Downloads/arachnea/server`. Résultat : **2 tests réussis**, couvrant la portée par destination et la restauration des refus depuis la persistance. Ils ne valident pas un handler injectable, qui n’existe pas encore.

## 10. Décisions acquises et paramètres restant à définir

### 10.1. Décisions acquises

- Les préférences sont obligatoirement persistées et restaurées après redémarrage.
- Les échecs de connexion au proxy lui-même sont globaux, contrairement aux échecs vers un serveur distant via le proxy. Le seuil de 24 heures part de la dernière preuve positive de fonctionnement ; après ce seuil, une impossibilité observée de se connecter au proxy entraîne son retrait de la liste disponible à la demande, sans processus en arrière-plan. Les tentatives échouées, résultats inconclusifs et actualisations fournisseur ne renouvellent pas ce délai.
- Un rejet explicite pour une destination entraîne une exclusion de 24 heures par défaut, configurable selon la politique. L’association reste dans l’historique durable mais inéligible, sans levée anticipée automatique ; ni les actualisations fournisseur ni le redémarrage n’effacent l’exclusion. Sa durée est distincte du seuil d’éviction globale.
- Le modèle durable conserve jusqu’à 3 endpoints acceptés par destination exacte, sans expiration fondée sur l’âge, et sélectionne le succès le plus récent éligible dans les pays autorisés. Les mises à jour sont atomiques et corrélées aux sélections réelles. En cas d’erreur de persistance, préserver la réponse métier et la préférence mémoire, signaler l’échec dans les logs et métriques et ne pas rejouer la requête ; la restauration du nouveau succès n’est alors pas garantie.
- La liste des pays constitue un ensemble autorisé, sans priorité entre pays ni effet de son ordre sur la sélection.
- La liste de pays traduit une connaissance préalable des origines à utiliser. Aucune exclusion ou suspension collective par pays n’est autorisée à partir d’échecs de quelques proxies ; les blacklists sont évaluées par endpoint et destination. Le diagnostic d’une panne du serveur doit être distinct et ne découle pas du seul nombre de pays en échec.
- Aucun processus d’actualisation en arrière-plan ne doit être nécessaire.
- Le serveur de test peut être accessible, indisponible ou géorestreint : un échec de la seconde phase ne suffit pas à condamner globalement le proxy.
- Le budget couvre le parcours jusqu’à la première réponse acceptée du domaine : attente des ressources partagées, caches, tests, fournisseurs, connexion réelle et rotations. Il ne s’agit pas du seul temps de sélection technique.
- Une recherche à froid peut viser environ 2 minutes, en contrepartie de la réutilisation du proxy préféré lors des appels suivants.
- Le délai est une cible avec prolongation bornée, et non un arrêt inconditionnel à la seconde exacte.
- Les limites de candidats et de concurrence seront ajustées par itération. Le nombre d’appels simultanés n’est pas encore connu ; les protections doivent donc être partagées par l’inventaire, pas seulement appliquées à chaque appel.
- L’option B est retenue : l’URL de test fournit une preuve complémentaire et ne constitue pas un préalable obligatoire à l’essai du domaine réel. Un résultat ambigu autorise un essai borné sans inventer une preuve de capacité ni contourner les contrôles de sécurité.
- Le mode strict est explicite et ne bascule pas silencieusement vers le parcours complémentaire. Un candidat inconclusif n’est pas utilisé pour la requête stricte ni retesté immédiatement ; la recherche passe à d’autres candidats et ne retourne une validation inconclusive que si aucun ne satisfait les exigences dans le budget. Les retests sont temporisés et uniquement à la demande.
- Les durées de suspension de la cible complémentaire restent 30, 60 puis 120 secondes, avec reprise mutualisée uniquement à la demande, sans attente imposée aux essais métier. Le seuil de 3 échecs en 60 secondes signale seulement une suspicion. Les refus sont évalués par endpoint et destination ; les échecs ambigus ne prouvent pas une panne totale. Sans preuve suffisamment claire de panne, les essais réels continuent jusqu’au succès ou aux budgets établis. Aucune suspension par pays ni exclusion collective des proxies n’est autorisée.
- Les URLs de secours sont facultatives et configurables, avec une seule cible de secours par validation en cas de suspension ou de résultat inconclusif, dans les budgets existants. Sans secours, l’option B reste applicable.
- Le diagnostic direct sans proxy est désactivé par défaut et nécessite une activation explicite. Son résultat ne constitue jamais un verdict de disponibilité mondiale.
- La politique de retry est acquise : nouvel essai possible avant envoi démontré, ou pour une lecture GET/HEAD après un échec pertinent ; aucun retry automatique par défaut des opérations potentiellement modificatrices après envoi ou envoi incertain, sauf configuration explicite et garanties de rejeu. Les essais respectent le plafond commun de 8 proxies et le budget total sans remise à zéro.
- La classification distingue réponse acceptée, rejet explicite du proxy pour la destination et résultat inconclusif ou erreur métier. Les succès 2xx sont acceptés par défaut sauf règle contraire ; une erreur de parsing ou un 4xx/5xx non classifié ne suffit pas à exclure le proxy. Les redirections et validations fonctionnelles restent contextualisées.
- Un handler injectable de classification des réponses est demandé, avec un handler HTTP 403 réutilisable dans `arachnea-proxy`. Le mécanisme existant est une liste de statuts, pas un trait injectable ; la conception détaillée proposée est décrite en section 6.6.
- Le contrat du handler est validé : métadonnées de requête et réponse, contexte de sélection, aucun corps lu par défaut, extrait facultatif de 64 Kio maximum restitué à l’appelant, priorité explicite des politiques et classification sans effets de bord. Un contenu insuffisant ou une erreur de handler reste inconclusif.
- Les URLs de test sont configurables et facultatives, avec les valeurs actuelles conservées pour compatibilité, HEAD par défaut pour HTTPS et GET configurable, sans suivi de redirection par défaut ni fallback de méthode systématique. Leur classification conserve séparément les preuves de transport et l’acceptation de la cible ; elle ne crée aucune préférence pour un domaine différent.
- Un candidat jamais validé dont la connexion échoue est immédiatement mis à l’écart, avec état d’échec conservé et nouvel essai à la demande après 15 minutes. Après 24 heures depuis sa découverte initiale, sans preuve positive et avec un nouvel échec de connexion, il est supprimé de l’inventaire disponible. Une actualisation fournisseur ne remet pas ces délais à zéro et la découverte n’est pas une preuve positive.

### 10.2. Paramètres restant à définir

#### Choix fonctionnels encore ouverts

Aucun choix fonctionnel bloquant n’est encore ouvert dans le périmètre discuté. Les règles et valeurs initiales sont suffisantes pour préparer une conception technique et un plan d’implémentation. Les précisions ci-dessous ne doivent pas réouvrir les décisions validées ; si la lecture technique révèle un conflit ou une impossibilité, le présenter explicitement avant de modifier le contrat.

#### Détails de conception technique, sans réouverture des décisions acquises

- Mapping des modes existants et représentation des preuves et temporisations par phase et cible.
- Représentation des indices d’indisponibilité de la cible, sans assimiler les timeouts ou blacklists d’endpoints à une panne certaine ; la poursuite des essais métier en cas de doute est validée.
- Représentation de la dernière preuve positive, de la découverte initiale et des tentatives, sans les confondre ; comportement des candidats jamais validés déjà approuvé.
- Propagation de la preuve d’absence d’envoi et configuration explicite des requêtes rejouables.
- Signatures et adaptateurs du handler validé, restitution des extraits de corps et interfaces sérialisables YAML/URL.
- Schéma durable des préférences et ordonnancement des écritures atomiques ; historique de 3 endpoints et politique d’échec déjà validés.
- Alignement des timeouts du client, de l’API et des intermédiaires sur le budget de recherche.
- Coordination et annulation des opérations mutualisées : l’abandon d’un appel ne doit pas interrompre une opération encore nécessaire à un autre, sans laisser de tâche détachée inutile.
- Conservation bornée des états d’échec après éviction et redécouverte, pour éviter qu’un fournisseur ne réintroduise immédiatement un endpoint comme neuf ; définir les structures et leur nettoyage à la demande.
- Attribution correcte des réponses et redirections à l’endpoint et à la destination réellement observés, y compris sur des connexions réutilisées.

#### Réglages à ajuster par mesure et itération

- Budgets d’exploration, plafonds de candidats, marge de prolongation et limites de concurrence proposés en section 10.3.
- Temporisations des résultats inconclusifs, délai initial de 15 minutes pour les nouveaux endpoints injoignables et calibration de la suspension de cible déjà validée.
- Limite initiale de 64 Kio pour l’inspection facultative et coût de la classification sous charge.

Les choix déjà validés — persistance, pays sans priorité, option B, suspension progressive, secours facultatif, retries, acceptation, contrat du handler et mode strict explicite — ne figurent plus comme décisions fonctionnelles ouvertes.

Les décisions acquises ne sont plus à remettre en confirmation. Les modalités des changements structurants devront être validées avant implémentation. Les README, spécifications, éléments de suivi concernés et le changelog devront alors être mis à jour avec le comportement effectivement retenu.

### 10.3. Budget de recherche : décisions et proposition initiale de réglage

#### Périmètre et échéances

La cible retenue est de **120 secondes jusqu’à la première réponse acceptée** lors d’une recherche à froid. C’est un plafond cible, pas une durée à consommer : retourner immédiatement dès que la réponse est acceptée. Un appel disposant d’un proxy préféré doit rester sur le parcours rapide.

Pour concrétiser la prolongation bornée, le réglage initial proposé est **15 secondes supplémentaires au maximum**, soit une limite absolue de **135 secondes** pour ce parcours. Cette marge est une proposition technique, pas une durée explicitement fixée par l’utilisateur.

À 120 secondes :

- ne plus démarrer de nouveau précheck, test complet, chargement fournisseur ou nouvelle rotation ;
- prolonger seulement si une connexion ou une requête réelle vers le domaine demandé est déjà engagée sur un candidat sélectionné ;
- ne pas prolonger pour attendre une liste fournisseur ou un simple précheck encore en cours ;
- arrêter dès le succès, l’échec de cette tentative finale ou l’échéance absolue, sans repartir dans une recherche.

Chaque opération est limitée par son propre timeout et par le temps restant jusqu’à l’échéance applicable. Les retries et actualisations ne remettent jamais le compteur à zéro. L’abandon de l’appel annule son travail devenu inutile. Aucun travail détaché ne sert à terminer la recherche après son expiration.

La réponse acceptée désigne ici la décision HTTP permettant de valider le proxy, pas le téléchargement intégral d’un flux ou d’un corps volumineux. Les délais de transfert restent distincts. Il faudra aligner les timeouts du client, de l’API et des intermédiaires pour qu’ils n’interrompent pas la demande avant cette échéance ; leurs configurations n’ont pas encore été vérifiées dans cette analyse.

#### Répartition du travail

Ne pas découper les 120 secondes en tranches fixes à consommer successivement. Appliquer les priorités de cache, puis permettre à l’exploration et à l’actualisation de se chevaucher :

| Paramètre | Valeur initiale proposée | Rôle |
| --- | --- | --- |
| Cible totale | 120 secondes, décision acquise | Budget jusqu’à la première réponse acceptée |
| Prolongation maximale | 15 secondes | Terminer uniquement une tentative réelle déjà engagée |
| Exploration avant actualisation | 10 secondes ou 8 validations complètes infructueuses | Commencer la découverte sans épuiser tout le cache |
| Endpoints soumis à un précheck | 64 au maximum par recherche | Borner le travail TCP, cache et découvertes confondus |
| Candidats soumis à une validation complète | 32 au maximum par recherche | Borner les tests coûteux, indépendamment des préchecks |
| Proxies essayés sur le domaine métier | 8 au maximum par recherche | Borner les rotations ; ne pas autoriser des retries non sûrs |

Ces plafonds de travail sont des points de départ conservateurs, non des valeurs issues d’un benchmark. Ils sont indépendants : un endpoint peut contribuer aux trois compteurs sans constituer trois proxies différents. Les lectures du cache ne consomment pas un compteur de test réseau. Les tentatives sont comptées à leur démarrage, y compris celles qui échouent ou sont annulées ; une tentative mutualisée n’est exécutée qu’une fois et compte pour les appels qui l’utilisent. Les plafonds incluent toute la recherche, pas une remise à zéro après chaque chargement fournisseur.

Les 8 essais métier sont un plafond, pas une obligation : les règles d’idempotence, une cible présumée indisponible ou le délai total peuvent arrêter le parcours plus tôt. Un échec ambigu ne justifie pas automatiquement une nouvelle tentative. Une fois les plafonds atteints, ne pas lancer d’autres candidats ; une tentative réelle déjà engagée peut seulement terminer dans le délai restant.

#### Concurrence initiale et appels simultanés

Valeurs provisoires proposées, à coordonner avec les autres paramètres de recherche :

| Opération | Maximum par recherche | Maximum partagé par inventaire |
| --- | --- | --- |
| Préchecks TCP | 8 | 16 |
| Validations complètes | 2 | 4 |
| Actualisations fournisseur par pays | 2 | 2 |
| Tentatives métier | 1 à la fois | À coordonner avec les limites existantes du client HTTP |

Les budgets partagés empêchent la multiplication automatique des connexions avec le nombre d’appels. L’attente d’un créneau consomme le délai total. L’ordonnancement doit éviter qu’une recherche monopolise tous les créneaux ; aucun pays ne bénéficie de l’ordre de la liste fournie.

La découverte et les tests techniques sont mutualisés aux portées définies en section 7. Pour plusieurs appels vers la même destination, envisager une coordination de la découverte afin qu’un appel puisse réutiliser le succès d’un autre. Ne pas fusionner ni rejouer leurs requêtes métier : cookies, authentification, méthode et corps peuvent différer. Les ensembles de pays autorisés restent respectés pour chaque appel.

#### Ajustement par itération

Mesurer les temps jusqu’au succès, les percentiles de latence, les plafonds atteints, l’attente des créneaux, le nombre de rotations et la réutilisation effective après redémarrage. Les limites ne doivent pas être augmentées automatiquement sur la seule base d’un serveur de test indisponible.

Si les recherches échouent souvent alors qu’il reste des candidats utiles, ajuster les plafonds et les délais de phases. Si l’attente des créneaux domine, revoir la concurrence partagée en fonction des ressources et de la charge observées. Le délai de 2 minutes ne dispense pas de retourner rapidement les succès ni les erreurs dont la poursuite ne peut raisonnablement améliorer le résultat.

## 11. Plan d’implémentation

### 11.1. Périmètre et principes d’exécution

Ce plan décrit les changements à réaliser. Le lot 0 d’inspection et de conception est réalisé et documenté en section 12. Les lots 1 à 5 ont été intégrés après confirmation de leur conception ; leurs limites de recette sont suivies en sections 13 à 19. Les lots 6 et 7 restent à implémenter. Les lots structurants doivent être présentés avant leur implémentation conformément aux règles du dépôt.

Principes communs :

- Procéder par lots compilables, avec des changements locaux et les validations pertinentes après chaque lot.
- Réutiliser Tokio, Futures, les abstractions HTTP et les stores typés déjà présents ; ne pas introduire de bibliothèque ni d’infrastructure de tests supplémentaire par défaut.
- Garder les décisions de sélection et d’inventaire dans `arachnea-proxy`, la configuration YAML et les fournisseurs dans `arachnea-scrapyfy`, et l’instanciation des stores dans l’application.
- Garder `arachnea-proxy` indépendant d’`arachnea-http` et des services de `arachnea-stream`.
- Ne pas ajouter de maintenance, de probes, de reprise de cible ni de réparation de persistance en arrière-plan.
- Mettre à jour les tests existants affectés. La création de nouveaux tests nécessite une demande explicite ; les scénarios non couverts devront être signalés et vérifiés par une procédure manuelle reproductible tant que cette demande n’a pas été faite.
- Ne pas refactorer les routes ou services sans rapport avec les proxies localisés.
- Mettre à jour la documentation au fil des lots, pas uniquement en fin de chantier.

### 11.2. Constats techniques à intégrer au plan

La lecture complémentaire confirme les points suivants :

1. `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/mod.rs` définit `PROXY_CONNECT_ESTABLISHMENT_TIMEOUT` à 120 secondes. Ce budget concerne actuellement l’établissement de CONNECT, pas toute la recherche jusqu’à l’acceptation HTTP. Il ne peut pas devenir automatiquement un nouveau délai de 120 secondes à chaque rotation.
2. `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs` configure un timeout ordinaire de 25 secondes et ajoute actuellement le budget d’établissement pour les requêtes géoproxifiées. `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/config.rs` possède un timeout par défaut de 30 secondes. Il faut transmettre une échéance commune, pas additionner ou remettre à zéro des budgets à chaque couche.
3. `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/connect.rs` expose des métadonnées de connexion, mais pas encore le jeton complet permettant de corréler chaque réponse à une sélection dynamique précise.
4. Les stores d’inventaire sont créés dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/stream_scraper.rs`, puis injectés dans l’agrégateur et le fournisseur. Un store de préférences devra emprunter ce même chemin d’injection ; le backend partagé ne doit pas choisir un chemin d’application.
5. Les abstractions de `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-core/src/persistence/typed_store.rs` permettent des entités typées avec clés et champs JSON. Une seule entité par destination contenant au maximum 3 associations est une approche adaptée pour éviter un remplacement partiel de l’historique. Les garanties effectives de migration et d’atomicité doivent être vérifiées avant de fixer les écritures.
6. `ArachneaResponse` est actuellement matérialisée, alors que la route de médias peut streamer son corps. Le budget jusqu’à une classification HTTP ne doit pas être confondu avec le téléchargement intégral d’un contenu. La classification à partir des en-têtes et d’un extrait borné doit être intégrée au point où ces observations sont disponibles.
7. Les fonctions de normalisation, les marqueurs `dynamic-country`, certains commentaires et les tests actuels parlent encore de pays ordonnés. Il faudra supprimer l’influence de l’ordre dans tout le parcours géolocalisé, pas seulement dans une boucle de sélection.

### 11.3. Ordre des lots et dépendances

| Lot | Livrable | Dépendances |
| --- | --- | --- |
| 0 | Contrats techniques, périmètre des erreurs et état initial des validations | Aucune |
| 1 | Observations par phase, preuve positive et attribution fiable de la sélection | Lot 0 |
| 2 | Préférences durables, exclusions et éviction correcte | Lot 1 |
| 3 | Handler partagé et classification des réponses HTTP | Lot 1 ; lot 2 pour les effets durables |
| 4 | Sélection dans l’ensemble autorisé et parcours rapide du cache | Lots 1 et 2 |
| 5 | Échéance de bout en bout, retries, pipeline borné et annulation | Lots 1, 3 et 4 |
| 6 | Actualisation progressive, probes complémentaires et diagnostic de cible | Lots 1, 3 et 5 |
| 7 | Vérifications transversales, documentation finale et mesure | Tous les lots précédents |

L’ordre conseillé est 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7. Une séparation en sous-lots est possible pour conserver des revues petites, mais les contrats dépendants doivent être livrés ensemble avant activation. Ne pas accélérer les validations concurrentes avant de fiabiliser la classification et la corrélation.

### 11.4. Lot 0 — Figer la conception technique et l’état initial

**Objectif :** préciser les signatures et les invariants avant les modifications transversales.

**État : terminé pour l’analyse et les validations initiales.** Les contrats proposés ne sont pas encore implémentés ; les résultats et limites sont détaillés en section 12.

Suivi des tâches :

- [x] Définir les types proposés de preuve, phase, résultat inconclusif, rejet de destination, budget et jeton de sélection à partir des décisions de la section 10 — sections 12.2 à 12.5.
- [x] Vérifier la propagation des métadonnées dans les transports, le connecteur Rquest, les moteurs HTTP et les réponses streamées ou matérialisées ; identifier les adaptations nécessaires — sections 12.3 et 12.4.
- [x] Examiner les migrations des stores existants et l’instanciation mémoire, fichier et SQLite ; documenter les garanties et limites sans modifier les abstractions génériques — section 12.6.
- [x] Relever les timeouts de l’API, du client frontend et des configurations de déploiement présentes ; identifier les limites externes inconnues — section 12.7.
- [x] Exécuter les validations initiales pertinentes et distinguer les défauts préexistants des changements futurs — section 12.8 : 146 tests réussis, compilation applicative réussie, défaut de features Scrapyfy et différences de formatage documentés.
- [x] Consigner les contrats publics proposés, les adaptateurs à modifier et les points techniques à traiter avant l’implémentation — sections 12.3 à 12.9 et suivi dans `/Users/jdecker/Downloads/arachnea/docs/TODO.md`.

**Livrable produit :** contrats et frontières techniques proposés, état initial des validations et limites documentées. Leur implémentation et les corrections identifiées appartiennent aux lots suivants ; elles ne sont pas cochées comme réalisées.

### 11.5. Lot 1 — Séparer les observations et corréler les réponses

**État : code intégré et compilable ; recette réseau/concurrence exhaustive non terminée.** Le suivi ci-dessous distingue les livrables de code des vérifications encore nécessaires ; une case cochée ne signifie pas que tous les scénarios réseau sont couverts.

**Fichiers principaux existants :**

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_record.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_probe.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_inventory.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/connect.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/core.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/connectors/rquest.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/client.rs`

Suivi des livrables :

- [x] Ajouter les types d’observation séparant endpoint, protocole/authentification, TLS du proxy, tunnel, TLS distant et réponse HTTP, avec stockage borné. La négociation SOCKS reste couplée au tunnel dans les helpers existants ; un tunnel échoué ne produit pas de preuve indépendante d’authentification.
- [x] Conserver les faits positifs par contexte lorsqu’une observation ultérieure échoue ; ne plus produire une indisponibilité TLS simplement à cause d’un statut HTTP 5xx.
- [x] Ajouter la dernière preuve positive et la découverte initiale, distinctes de la dernière tentative, avec champs compatibles avec les anciens enregistrements et conservation lors des fusions fournisseur.
- [x] Contextualiser les résultats par cible, phase et configuration de probe ; invalider la réutilisation des agrégats et tentatives distantes après changement de configuration.
- [x] Intégrer sélection explicite, route épinglée et jeton dans les retours Rquest, Scrapyfy et médias, avec isolation des clients réutilisables et attribution de la réponse finale après redirection.
- [x] Qualifier l’envoi par `NotSent`, `PossiblySent` et `ResponseReceived`, sans déduire une absence d’envoi d’une erreur Rquest opaque.
- [x] Préserver les contrôles TLS/DNS et supprimer les promotions KO/évictions fondées uniquement sur les échecs distants.
- [x] Exécuter les compilations et suites existantes pertinentes ; documenter les échecs Scrapyfy et les différences de formatage global (section 13.1).
- [x] Actualiser les README, le TODO, le changelog et le suivi du lot.
- [x] Ajouter et exécuter les tests locaux de réponses concurrentes sur une même origine, reconnexions épinglées, redirection, cible HTTP 503 et changement de configuration — section 15.
- [x] Vérifier la réutilisation d’une même socket Rquest et le 503 HTTPS après tunnel/TLS valide sur le parcours métier — test local en section 16.
- [ ] Vérifier spécifiquement le 503 HTTPS dans `ProxyProbe` : le test métier Rquest ne remplace pas ce parcours, dont les racines de confiance sont actuellement fixes.
- [ ] Valider exhaustivement les critères de sortie ci-dessous ; la compilation et les suites existantes ne remplacent pas cette recette.

**Critères de sortie :** un échec distant ne devient pas une panne globale du proxy ; les retours HTTP ne peuvent pas modifier l’état d’un autre endpoint concurrent ; aucun timestamp d’échec ne renouvelle la preuve positive.

### 11.6. Lot 2 — Persister les préférences et appliquer les règles d’éviction

**État : code intégré après confirmation ; suites existantes adaptées et recette exhaustive encore ouverte.** Les cases ci-dessous suivent les livrables de code, non une validation exhaustive de chaque scénario.

**Fichiers existants à adapter :**

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_repository.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_inventory.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/mod.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/stream_scraper.rs`

**Modules nouveaux proposés, noms à confirmer au lot 0 :**

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_preference.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_preference_repository.rs`

Travail prévu :

- [x] Définir une entité par schéma/hôte/port, contenant au maximum 3 endpoints distincts et leurs dates de succès acceptés, sans expiration automatique.
- [x] Injecter son repository et son store depuis l’application, avec implémentation mémoire pour les constructeurs compatibles. Adapter aussi les snapshots et remplacements d’agrégateur afin de conserver le partage des stores.
- [x] Charger les préférences et leurs endpoints avant découverte, paresseusement à la première demande avec mutualisation. Vérifier les exclusions et pays actuels avant réutilisation.
- [x] Sérialiser les mises à jour concurrentes par destination et empêcher une écriture tardive d’un ancien succès d’écraser un succès plus récent. Ne pas maintenir un verrou global de données pendant l’I/O.
- [x] Persister les exclusions de destination et leur échéance configurable, avec conservation de l’historique et sans levée anticipée implicite.
- [x] Limiter l’éviction globale à l’échec de connexion au proxy et au seuil défini : 24 heures depuis la dernière preuve positive, ou depuis la découverte pour un endpoint jamais validé. Mettre à l’écart ce dernier pendant 15 minutes après un échec.
- [x] Retirer l’éviction globale actuellement possible sur la seule base d’un échec distant ancien et la promotion des erreurs ambiguës de cible en KO global — préalable intégré au lot 1, règles complètes d’éviction intégrées au lot 2.
- [x] Nettoyer les références orphelines et conserver des états d’échec bornés pour empêcher une redécouverte immédiate de remettre à zéro les temporisations : store séparé, 1 024 endpoints maximum, 48 heures depuis éviction, nettoyage à la demande.
- [x] Sur échec d’écriture de préférence, préserver la réponse métier et l’état mémoire, contextualiser le log et exposer le diagnostic via `preference_persistence_failures()`. Ne pas ajouter de worker de réparation.
- [x] Pour les anciens records, ne pas inventer de preuve positive à partir de `last_checked`. Les anciens succès d’origine non persistés ne peuvent pas être reconstruits : commencer l’historique durable à partir des nouvelles observations fiables.
- [x] Compléter les tests locaux réseau/concurrence, l’échec d’écriture avec log capturé, les reprises et les frontières de commit simulées — sections 15 et 16. Les coupures réelles processus/disque restent hors couverture.

**Critères de sortie :** après redémarrage, un endpoint accepté et éligible est réutilisé sans consultation fournisseur ; l’ordre des pays ne change pas le choix ; une exclusion survit au redémarrage ; un échec de stockage ne provoque pas un rejeu métier.

### 11.7. Lot 3 — Introduire le handler de réponse commun

**État : intégré après confirmation de la conception ; détails et limites en section 17.** Les cases ci-dessous suivent les livrables de code, non une recette exhaustive ; les scénarios encore non couverts restent ouverts en §11.12 et §17.3.

**Module nouveau proposé :** `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/response_classifier.rs`.

**Adaptateurs principaux :**

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/mod.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/client.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/proxy_service.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/http_client.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/client.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/services/francetv_resolver.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/services/tf1_resolver.rs`

Travail prévu :

- [x] Implémenter la vue indépendante de réponse, le trait de classification et les trois résultats validés.
- [x] Fournir la politique prudente par défaut, le handler de statuts configurables et le handler 403 réutilisable.
- [x] Préserver la configuration `proxy_rejection_statuses` comme adaptateur vers le handler. Permettre l’injection par le code sans mettre un objet trait dans une structure YAML sérialisable.
- [x] Définir les configurations de handlers connus pour YAML et URLs, et leur validation ; conserver la priorité explicite entre injection et liste de statuts.
- [x] Remplacer les comparaisons dupliquées dans Scrapyfy et la route de médias par la même classification, avec application des effets sur le jeton de sélection du lot 1.
- [x] Classifier avant toute transformation de réponse pouvant masquer un blocage ; ne pas confondre les post-actions avec le handler.
- [x] Ajouter l’inspection facultative de 64 Kio, avec restitution exacte des octets dans le flux, sans attente du corps intégral. Une observation insuffisante reste inconclusive.
- [x] Réutiliser le point d’observation HTTP disponible avant matérialisation complète ; s’il manque dans un moteur, adapter sa frontière minimale ou signaler explicitement la limitation plutôt que laisser le budget couvrir tout un flux.
- [x] Conserver l’activation explicite du rejet 403 pour FranceTV et TF1, tant pour leurs clients que leurs URLs de médias.

**Critères de sortie :** 2xx accepté selon politique, 403 excluant uniquement si activé, 4xx/5xx non classifié sans exclusion automatique, pas de succès attribué au mauvais domaine après redirection, aucune altération du corps inspecté.

### 11.8. Lot 4 — Sélectionner dans un ensemble de pays, cache d’abord

**Fichiers principaux :**

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_inventory.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/parameters.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/core.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/proxy_service.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs`

Travail prévu :

- [x] Canonicaliser l’ensemble autorisé : normalisation, déduplication et représentation stable indépendante de l’ordre pour les marqueurs et clés de mutualisation.
- [x] Regrouper les candidats des pays autorisés avant classement ; ne pas épuiser les inconnus d’un pays avant de voir un candidat validé d’un autre.
- [x] Charger le cache persistant pertinent avant l’exploration coûteuse, sans bloquer inutilement une préférence déjà restaurée et utilisable.
- [x] Appliquer les catégories : historique accepté pour la destination, capacités pertinentes validées, revalidation nécessaire, inconnus puis découvertes.
- [x] Harmoniser ce classement pour HTTP et HTTPS ; la latence ne doit pas faire passer un inconnu devant un candidat prêt.
- [x] Conserver toutes les exclusions, contrôles d’authentification, certificats et contraintes de pays sur le parcours rapide.
- [x] Auditer les handlers statiques qui consomment le même paramètre afin qu’ils n’introduisent pas une priorité fondée sur l’ordre fourni. Ne pas changer l’ordre d’un pool statique sans rapport avec ce paramètre.

**Critères de sortie :** permutations d’un même ensemble sans priorité géographique ; autres endpoints d’un pays toujours éligibles après des refus ; préférence non autorisée ignorée mais conservée ; réutilisation sans précheck de lot ni découverte inutile.

### 11.9. Lot 5 — Unifier budget, retries et pipeline concurrent

**Module nouveau proposé :** `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_search.rs`, contenant le contexte de recherche et ses limites, sans logique spécifique aux sources.

**Intégrations principales :** inventaire, core, connecteurs, clients HTTP et route de médias déjà listés ; `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/server/handlers/http.rs` pour le budget CONNECT ; `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/config.rs` pour l’adaptation des timeouts.

Travail prévu :

- [x] Créer une échéance monotone commune et des compteurs de travail par recherche : cible 120 secondes, marge initiale 15 secondes, 64 préchecks, 32 validations complètes et 8 endpoints métier.
- [x] Distinguer la fin de recherche de la durée du transfert ; après acceptation, un flux établi n’est pas coupé par l’échéance de découverte.
- [x] À 120 secondes, cesser de lancer des travaux. N’autoriser la marge que pour une tentative réelle déjà engagée et ne pas lancer une nouvelle rotation durant cette marge.
- [x] Transmettre le contexte aux retries de connexion et HTTP afin que les budgets ne se multiplient pas entre les couches. Réduire chaque timeout au temps restant.
- [x] Appliquer la politique de rejeu validée et conserver les corps nécessaires au rejeu autorisé ; un envoi incertain d’une opération modificatrice interdit le retry automatique.
- [x] Remplacer `collect()` sur la fenêtre entière par la consommation progressive des résultats et lancer la validation dès qu’un candidat est prêt.
- [x] Borner la concurrence par recherche et par inventaire avec les limites initiales de la section 10.3. Inclure l’attente des créneaux dans le budget.
- [x] Mutualiser les préchecks et validations aux bonnes portées ; distinguer tentative en cours, échec, résultat inconclusif et annulation parce qu’un autre candidat a gagné.
- [x] Gérer les attentes partagées sans laisser de tâches détachées. L’abandon d’un appel retire son intérêt, mais ne coupe pas une opération encore attendue par un autre ; après disparition du dernier intérêt, annuler le travail.
- [x] Ne pas fusionner les requêtes métier, leurs corps, cookies ou identités ; coordonner uniquement la découverte et les preuves techniques partageables.

**Critères de sortie :** aucune attente de tout un lot, succès rendu dès disponibilité, plafond total respecté entre retries, absence de rejeu non sûr, annulation sans pollution du cache ni arrêt d’un autre appel.

### 11.10. Lot 6 — Actualisation progressive et tests complémentaires

**Fichiers principaux :**

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_record.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_inventory.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_probe.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs`

Travail prévu :

- Déclencher une actualisation après 10 secondes d’exploration ou 8 validations infructueuses, dans le contexte de recherche existant et sans abandonner les candidats du cache.
- Mutualiser les chargements par pays et maintenir le cooldown fournisseur avec un plancher anti-boucle, même lorsque des échecs observés permettent une nouvelle consultation.
- Démarrer les pays éligibles selon l’état du cache et les créneaux, jamais selon leur position dans la liste.
- Ajouter une interface progressive du fournisseur qui publie des lots au fil des sources, tout en gardant un adaptateur compatible avec l’interface retournant une liste. Ne pas attendre `join_all` pour commencer à utiliser la première liste pertinente.
- Isoler ce mode progressif pour la découverte des proxies ; conserver les contrats d’agrégation et l’ordre de présentation des résultats des autres scrapers.
- Dédupliquer avant les opérations coûteuses et fusionner sans écraser les dates de succès, exclusions, découvertes initiales ou tentatives récentes.
- Mettre à disposition les candidats déjà géolocalisés sans attendre toutes les résolutions manquantes ; borner et mutualiser ces dernières, sans inventer un pays.
- Configurer les méthodes, URLs et politiques de test validées : HEAD HTTPS par défaut, GET explicite, pas de redirection suivie par défaut, une seule cible de secours et preuves séparées.
- Appliquer l’option B et le mode strict explicite sans créer de faux succès ni laisser un résultat inconclusif imposer une exclusion générale.
- Enregistrer la suspicion de panne après 3 échecs convergents en 60 secondes. Séparer refus individuels, observations ambigües et indices d’indisponibilité de service ; ne créer aucun circuit par pays.
- Suspendre uniquement le travail complémentaire répétitif selon la politique 30/60/120 secondes, avec reprise à la demande et succès levant la suspicion. Ne pas empêcher l’exploration métier d’autres endpoints ni arrêter sur la seule base de timeouts multiples.

**Critères de sortie :** premières sources utilisables sans attendre les plus lentes ; actualisation sans boucle ; cibles indisponibles sans dégradation massive du cache ; blacklist de plusieurs endpoints sans exclusion de leurs pays ; aucun travail autonome après la recherche.

### 11.11. Lot 7 — Vérification de bout en bout et mesure

Scénarios de recette, à réaliser avec les moyens existants et sans nouvelles infrastructures par défaut :

1. Recherche à froid, succès, nouvel appel sur la même destination, puis redémarrage : préférence retrouvée et fournisseur non consulté inutilement.
2. Deux domaines simultanés et plusieurs connexions sur un même domaine : retours attribués aux bons proxies, historiques atomiques et budgets non multipliés.
3. Même ensemble de pays dans plusieurs ordres, puis ensemble différent : mêmes règles d’éligibilité, aucune suppression d’une préférence simplement non autorisée pour un appel.
4. Proxy injoignable récent, proxy injoignable plus de 24 heures après preuve positive, nouveau proxy jamais validé et redécouverte fournisseur : temporisations et éviction conformes.
5. Serveur de test indisponible ou plusieurs proxies blacklistés : autres candidats disponibles, preuves positives préservées, aucune exclusion collective.
6. Handler 403, handler de contenu 200 bloqué, extrait insuffisant, erreur du handler et corps média streamé : classification correcte et octets restitués.
7. GET rejouable, POST dont l’envoi est incertain, redirection inter-domaines, abonnement à une opération partagée puis annulation : sécurité et corrélation conservées.
8. Recherche dépassant 120 secondes : aucun nouveau candidat, marge seulement pour la tentative réelle engagée, arrêt à la limite absolue ; transfert accepté non interrompu par ce budget.
9. Échec de stockage des préférences : réponse métier préservée, log contextualisé, aucun rejeu ni worker de réparation.

Mesurer le temps jusqu’à l’acceptation, le parcours rapide après redémarrage, l’attente des créneaux, les opérations techniques effectuées, les plafonds atteints et les écritures durables. Utiliser les logs/statistiques existants ; un export de métriques supplémentaire n’est pas inclus dans ce chantier.

Les valeurs initiales ne seront augmentées qu’en présence d’observations utiles, pas pour masquer une cible indisponible. Établir un bilan avant d’ajuster plusieurs paramètres simultanément.

### 11.12. Commandes de validation prévues

Les commandes ci-dessous sont à exécuter depuis `/Users/jdecker/Downloads/arachnea/server` au moment des lots concernés. Elles ne sont pas présentées comme déjà exécutées dans le cadre de la rédaction du plan.

```sh
cargo fmt --all -- --check
cargo check -p arachnea-proxy --no-default-features --features core
cargo check -p arachnea-proxy --features persistence,controller-service,rquest,arachnea-dns
cargo check -p arachnea-http --features arachnea-proxy
cargo check -p arachnea-scrapyfy --features arachnea-proxy,sqlite-persistence
cargo check -p arachnea-stream
cargo test -p arachnea-proxy --lib --features persistence proxy_inventory::tests
cargo test -p arachnea-proxy --lib --features persistence proxy_repository::tests
cargo test -p arachnea-proxy --lib --features persistence,controller-service
cargo test -p arachnea-proxy --test post_action_integration --features controller-service
cargo test -p arachnea-http --lib --features arachnea-proxy
cargo test -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence
```

Utiliser `CARGO_NET_OFFLINE=true` si toutes les dépendances sont présentes. Les builds applicatifs ou moteurs navigateur peuvent dépendre de ressources et outils locaux ; documenter tout blocage et son effet sur la couverture. Commencer par les filtres pertinents, puis élargir aux suites des crates affectées avant clôture. Ne pas prétendre que les tests existants couvrent automatiquement les nouveaux contrats.

### 11.13. Documentation, suivi et critères de clôture

Chaque lot met à jour les documents concernés lorsqu’il change leur contrat :

- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/README.md`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/README.md`
- `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/README.md`
- `/Users/jdecker/Downloads/arachnea/docs/specifications/arachnea-scrapyfy-fr.md`
- `/Users/jdecker/Downloads/arachnea/docs/specifications/arachnea-scrapyfy-en.md`
- `/Users/jdecker/Downloads/arachnea/docs/specifications/arachnea-proxies-fr.md`
- `/Users/jdecker/Downloads/arachnea/docs/specifications/arachnea-proxies-en.md`
- `/Users/jdecker/Downloads/arachnea/docs/TODO.md`
- `/Users/jdecker/Downloads/arachnea/CHANGELOG.md`

Ne modifier les YAML de services que lorsqu’une nouvelle configuration ou une migration l’exige ; conserver les activations existantes des sources. Mettre à jour les exemples de pays ordonnés et les contrats de rejet, de persistance et de retry devenus obsolètes.

Les entrées de changelog suivent la structure release → module souligné → Added/Changed/Fixed, avec les modules concernés tels que `arachnea-proxy`, `arachnea-http`, `arachnea-scrapyfy` et `arachnea-stream`. Ne pas réécrire les anciennes entrées. Le présent ajout de plan ne prétend pas avoir livré les fonctionnalités ; le suivi et le changelog seront actualisés lors des changements effectivement réalisés.

Clôture attendue :

- Tous les contrats validés de la section 10 sont implémentés ou une limitation explicite a été approuvée.
- Les préférences et exclusions survivent au redémarrage ; aucune priorité de pays ni exclusion collective ne subsiste.
- Les budgets et retries sont communs aux couches, avec classification fiable et absence de travail en arrière-plan.
- Les flux inspectés ne sont pas altérés et les transferts acceptés restent indépendants du budget de recherche.
- Les validations pertinentes ont été exécutées, les lacunes et blocages sont documentés, et les fichiers édités ont été relus.
- Le document de suivi indique pour chaque lot son état réel et les résultats de validation, sans confondre planification et réalisation.

## 12. Résultats du lot 0 — Conception technique et état initial

### 12.1. État et périmètre

**Lot 0 réalisé : inspection locale, propositions de contrats et validations initiales.** Cette section décrit l’état au moment du lot 0, avant toute implémentation des contrats Rust proposés. Les implémentations ultérieures des lots 1 et 2, confirmées avant modification, sont suivies en sections 13 et 14 ; les lots 3 à 7 restent à réaliser.

L’inspection a porté sur les records et repositories, les stores typés, SQLite et fichier, le core de sélection, le listener CONNECT, le connecteur Rquest loopback, les interfaces des moteurs HTTP, les réponses matérialisées, les clients Scrapyfy et les appels frontend/API du dépôt. Aucun benchmark réseau ni test vers des fournisseurs publics n’a été effectué.

### 12.2. Contrats proposés pour les observations

Les noms suivants sont des propositions techniques, pas des symboles existants. Le stockage reste indépendant des réponses et moteurs HTTP.

| Type proposé | Contenu et rôle |
| --- | --- |
| `ProxyObservationPhase` | `EndpointConnect`, `ProxyProtocol`, `ProxyTls`, `Tunnel`, `DestinationTls`, `DestinationHttp` |
| `ProxyObservationOutcome` | `Succeeded`, `Failed` avec cause et portée explicites, `Inconclusive` ; annulation gérée séparément sans faux échec |
| `ProxyProbeContext` | Endpoint, phase, cible optionnelle, méthode et identifiant de configuration de probe |
| `ProxyObservation` | Contexte, résultat, date UTC de fin, durée et informations bornées utiles au diagnostic |
| `ProxyRequestDelivery` | `NotSent`, `PossiblySent`, `ResponseReceived` ; `NotSent` seulement sur preuve de la couche de transport |
| `ProxyHealthEvidence` | Première découverte, dernière preuve positive, prochaine éligibilité globale ; champs optionnels pour migration |

Invariants :

- Une ouverture TCP réussie vers l’endpoint est une preuve de joignabilité du proxy, pas de capacité à transporter une requête. Un échange réellement réussi via cet endpoint apporte également une preuve positive de fonctionnement.
- Le seuil global de 24 heures utilise la preuve positive la plus récente observée par le backend, non l’indication fournisseur, le début du probe ou la dernière tentative.
- Une erreur au tunnel ou au serveur distant ne permet pas à elle seule de produire `EndpointConnect: Failed`. Les transports doivent exposer la phase réellement échouée.
- La réussite TLS ne disparaît pas lorsqu’une réponse HTTP ultérieure est refusée. Une erreur de certificat interdit l’échange concerné sans devenir un motif de blacklist globale.
- Les cooldowns sont stockés en dates UTC ; les deadlines actives sont monotones et non sérialisées. Prévoir une gestion prudente d’un recul de l’horloge système.
- Les résultats hérités restent lisibles, mais une ancienne indication `last_checked` ne sera pas convertie en succès inventé. Les preuves nouvelles proviennent uniquement d’observations attribuées correctement.
- Stocker des observations bornées ou des derniers résultats par contexte, pas un historique infini ni les contenus, cookies et secrets des requêtes.

Le mode strict exige les étapes pertinentes pour le protocole utilisé, pas toutes les phases de tous les protocoles. Par exemple, le TLS du proxy n’est pas exigible pour un proxy HTTP non chiffré. Une validation inconclusive exclut le candidat de cet usage strict, pas globalement de l’inventaire.

### 12.3. Corrélation : obstacle confirmé et approche retenue pour conception

**Constat :** `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/connectors/rquest.rs` ne remplace pas le transport Rquest en processus. Il lance un listener HTTP loopback et configure `rquest::Proxy`. Les `ConnectMetadata` restent côté serveur ; `EngineResponse` et `ArachneaResponse` n’ont pas de champ de sélection. Pour HTTPS, le listener relaie des octets TLS et ne voit pas le statut métier. Ajouter un champ aux métadonnées seules ne résoudrait donc pas la corrélation.

Le mécanisme actuel `mark_dynamic_proxy_*(&Destination)` retire ou consulte la dernière sélection associée à la destination. Il peut attribuer une réponse à une autre connexion, et le rejet marque aussi un membre du pool `Ko`. Ce statut doit être audité pour empêcher une propagation globale d’un refus propre à une origine.

**Approche recommandée : rendre la sélection explicite avant la tentative HTTP**, plutôt que tenter de reconstruire le proxy depuis la dernière connexion :

- `ProxySelectionToken` : identifiant opaque, clé d’endpoint, destination exacte, portée d’inventaire et génération de route.
- `ProxyAttemptContext` : jeton, échéance de recherche et état d’envoi ; partagé par les couches de cette tentative, non sérialisé dans une configuration YAML.
- Émettre une route épinglée à cet endpoint pour le client ou tunnel de la tentative. Toute nouvelle connexion de ce client reste sur cet endpoint ; la rotation appartient à l’orchestrateur.
- Inclure la génération de route/endpoint dans la clé du client réutilisable. Une réponse sur une ancienne connexion garde son ancien jeton même après une rotation concurrente.
- Les métadonnées enrichies de `ProxyStream` restent utiles pour les adaptateurs en processus et la route de médias, mais ne sont pas la seule source de corrélation de Rquest.
- Le jeton ne contient pas de credentials exposés et n’est pas transmis à l’origine dans un en-tête. Le routing loopback reste interne.

Signatures de frontière proposées :

```text
select_for_request(destination, allowed_countries, search) -> Result<SelectedProxy>
SelectedProxy = endpoint + route + selection_token
record_observation(selection_token, observation) -> Result<()>
record_response(selection_token, response_decision) -> Result<()>
```

Ces signatures remplacent l’identification par destination seule pour les parcours géoproxifiés ; les APIs historiques peuvent rester comme adaptateurs explicitement limités, sans être utilisées pour les retours concurrentiels nouveaux.

**Alternative non recommandée :** un registre de retours CONNECT avec identifiant et listener dédiés. Il ajoute une synchronisation aller-retour et reste délicat avec les pools, nouvelles connexions et redirects. Les headers de la réponse CONNECT ne constituent pas une API de métadonnées consommée par le code Rquest actuel.

**Redirections :** créer une sélection et classification contextualisées pour chaque origine ; une réussite finale ne valide pas automatiquement les précédentes. Une implémentation ne doit pas réutiliser le jeton de l’origine initiale pour classifier la suivante. Les requêtes de navigateur ont moins de contrôle : tant qu’une corrélation précise n’est pas fournie, ne pas leur attribuer de succès/exclusion de proxy par déduction et signaler leur limitation.

### 12.4. Contrat du handler et frontière de lecture

Types proposés dans `arachnea-proxy`, utilisant les types HTTP communs déjà présents :

```text
ProxyResponseDecision = Accepted | RejectedForDestination { cooldown, reason } | Inconclusive
ProxyResponseClassifier::body_requirement() -> ProxyBodyRequirement
ProxyResponseClassifier::classify(&ProxyResponseView) -> Result<ProxyResponseDecision>
```

- Trait `Send + Sync`, object-safe, synchrone et sans I/O ; une limite de temps ne protège pas d’un handler qui bloque. Son contrat interdit donc les opérations bloquantes et coûteuses non bornées.
- `ProxyBodyRequirement` exprime aucun corps ou un préfixe d’au plus 64 Kio. L’adaptateur fait la lecture asynchrone bornée ; le handler ne possède pas le stream.
- La vue fournit méthode, URL demandée/effective, origine, statut, en-têtes, jeton et extrait optionnel. L’extrait distingue fin de corps observée, limite atteinte et indisponibilité ; atteindre la limite ne prouve pas la complétude.
- Une erreur du handler donne un diagnostic et un résultat inconclusif. L’orchestrateur applique seul exclusion, préférence et éventuel retry.
- Une réponse HTTP reçue, même refusée, reste une preuve d’échange HTTP via le proxy ; cela ne crée pas une préférence acceptée.

**Obstacle confirmé :** `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/engine/rquest.rs` lit le statut et les en-têtes puis appelle `response.bytes().await` avant de retourner `EngineResponse`. Le contrat actuel `HttpEngine::send` retourne un corps complet. Il ne permet pas de garantir la classification précoce sur un téléchargement long.

**Adaptation recommandée :** ajouter au contexte d’exécution un observateur facultatif headers/préfixe que le moteur invoque avant matérialisation. Garder la réponse publique matérialisée et la méthode existante pour les usages ordinaires, avec un chemin d’exécution explicite pour les recherches proxifiées. Les mock-engines et moteurs alternatifs restent compatibles via un adaptateur ; un moteur sans observation précoce doit annoncer la capacité non supportée, et non prétendre respecter le budget en attendant son corps complet.

Sur la route médias, préserver la lecture streamée et restituer le préfixe avant la suite du corps. Après `Accepted`, clôturer le budget de recherche ; le timeout de téléchargement demeure une politique HTTP distincte. Après un rejet explicite, interrompre proprement la collecte et permettre la rotation sûre ; après `Inconclusive`, ne pas provoquer de rotation ni enregistrer de préférence artificielle.

### 12.5. Recherche et sécurité des retries

Contrats proposés :

```text
ProxySearchLimits { target_duration, extension, precheck_limit, validation_limit, origin_limit }
ProxySearchContext { id, started_at, target_deadline, hard_deadline, counters, attempted_endpoints }
ProxySearchContext::remaining() -> Duration
ProxySearchContext::try_start(work_kind, endpoint) -> Result<WorkPermit>
ProxyRetryPolicy::allows(method, replayability, delivery, failure) -> bool
```

- Utiliser `Arc` pour le contexte partagé et les primitives Tokio existantes pour compteurs, sémaphores et opérations mutualisées. Aucun `Instant` dans les types persistés.
- Une nouvelle tentative métier n’est autorisée qu’avant 120 secondes. La limite 135 secondes termine seulement celle déjà commencée. Aucun reset à la création d’un nouveau client ou après un refresh.
- La tentative métier reste séquentielle ; les tests techniques et chargements peuvent se chevaucher dans leurs limites respectives.
- Le transport actuel convertit les erreurs Rquest en `Proxy(String)` ou `Network(String)`. Cette information ne prouve pas l’absence d’envoi ; le comportement de secours est `PossiblySent`, donc pas de retry automatique d’une requête modificatrice.
- Les erreurs explicitement observées avant la connexion métier peuvent établir `NotSent`. Une erreur de collecte après headers ne doit pas être classée comme un rejet de proxy ni rejouer l’opération.
- La mutualisation conserve des abonnements par appel : le premier timeout ne détruit pas le travail encore demandé par un second. La fin du dernier abonnement annule le travail ; aucune tâche de découverte détachée.

Pour les appels existants au core qui ne demandent qu’un tunnel, conserver un budget d’établissement, sans inventer une observation HTTP inaccessible dans le tunnel. Les appels orchestrés HTTP portent, eux, le contexte jusqu’à la classification métier.

### 12.6. Persistance : garanties vérifiées et limites

**Modèle recommandé :** nouvelle entité `ProxyDestinationPreference` dans un namespace distinct, clé canonique schéma/hôte/port et tableau JSON de 3 associations. Réutiliser `ProxyKey`, sans faire du pays une composante de la clé de préférence. Dates de succès et exclusions restent persistées et revalidées avant emploi.

**Garanties du store :** `TypedEntityStore` fournit `get`, `put`, `put_all`, `delete`, `find`, mais pas de transaction multi-stores ni de compare-and-swap. `put_all` engage un batch du même type, pas une suppression d’inventaire et un historique dans plusieurs repositories.

- Mémoire : opérations protégées par le store ; pas de durabilité après redémarrage.
- SQLite : `put_all` utilise une transaction, `put` remplace une entité. Les mises à jour read-modify-write doivent quand même être sérialisées au repository par destination. Les garanties actuelles ne couvrent pas plusieurs processus qui mettent à jour le même historique : le périmètre reste un inventaire partagé dans un processus.
- Migration SQLite : les nouvelles colonnes sont ajoutées nullable, les clés primaires ne sont pas migrées automatiquement et un changement de type logique est rejeté. Ajouter les données de santé de façon compatible ; créer un store distinct plutôt que modifier la clé de l’inventaire.
- Fichier : le codec écrit un fichier temporaire, `sync_all`, puis rename. Cela évite un document partiellement écrit, mais ne garantit pas l’ordre des snapshots concurrents : `FileEntityStore` relâche le verrou avant `persist(snapshot)`.

**Conséquence pour le lot 2 :** une sérialisation par destination ne suffit pas pour un store fichier contenant plusieurs destinations. Proposer un repository de préférences avec une file/sérialisation de commit partagée par instance de store, en plus des mises à jour par destination. Chaque commit relit/construit l’état pertinent dans la séquence, et ses écritures sont attendues par l’appel. Il s’agit d’opérations à la demande, pas d’un worker autonome. Cette mesure n’offre pas de garantie multi-processus ni contre un autre propriétaire indépendant du même fichier.

Pour l’inventaire fichier déjà existant, la même limite doit être traitée au niveau de ses écritures si le parcours modifié les rend concurrentes. Si une sérialisation de repositories s’avère insuffisante, présenter séparément la correction générique minimale du store ; ne pas étendre silencieusement le chantier de `arachnea-core`.

La suppression d’un proxy puis le nettoyage des préférences ne sont pas atomiques entre stores. Préférer supprimer/mettre hors disponibilité l’endpoint, puis nettoyer les associations ; au chargement, vérifier la présence et l’éligibilité pour ignorer une référence orpheline. Une reprise à la demande termine le nettoyage. Les états d’échec conservés doivent être bornés et ne pas renouveler `first_discovered` à chaque fournisseur.

Chemin d’injection : `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-stream/src/stream_scraper.rs` → constructeurs de `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs` → `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/proxy_provider.rs` → inventaire. Préserver aussi les constructeurs mémoire et les copies de configuration de l’agrégateur.

### 12.7. Timeouts de bout en bout

| Couche inspectée | État actuel | Action prévue |
| --- | --- | --- |
| Listener CONNECT | 120 s par établissement et surveillance de déconnexion client | Réduire au restant du contexte quand disponible ; conserver le timeout autonome pour le tunnel seul |
| Scrapyfy | 25 s ordinaires ; 120 + 25 s dans le cas géoproxifié | Remplacer l’addition par la propagation du budget de recherche, téléchargement séparé |
| Config HTTP | 30 s par défaut | Ne pas changer tous les appels ; adapter explicitement le parcours de recherche |
| Moteur Rquest | Timeout de requête incluant collecte complète | Ajouter observation précoce et traitement des deadlines avant/après acceptation |
| API REST inspectée | Pas de deadline métier de 2 minutes identifiée ; temporisations de shutdown distinctes | Vérifier l’annulation de la requête lors de l’intégration ; ne pas confondre arrêt du serveur et timeout HTTP |
| Frontend public `rustify.ts`, admin `adminApi.ts` | Appels fetch inspectés sans timer d’abandon explicite | Pas de modification nécessaire identifiée pour ces appels ; la lecture média a ses propres contraintes |
| Docker Compose | Expose le backend directement ; pas de reverse proxy avec timeout déclaré trouvé | Documenter les reverse proxies ajoutés par l’exploitant |

Fichiers frontend inspectés : `/Users/jdecker/Downloads/arachnea/front/public-app/src/services/rustify.ts` et `/Users/jdecker/Downloads/arachnea/front/admin-app/src/services/adminApi.ts`. Le timeout des storyboards dans `/Users/jdecker/Downloads/arachnea/front/public-app/src/composables/video/useVideoJsMediaRenderer.ts` est spécifique à cet usage et n’est pas un budget API général.

Ces constats ne garantissent pas que le navigateur, un lecteur vidéo, un reverse proxy externe ou un hébergeur accepte 135 secondes. La configuration réelle de production reste inconnue. Ne pas ajouter de heartbeat ou d’API asynchrone sans besoin et décision distincts.

### 12.8. État initial des validations exécutées

Commandes lancées depuis `/Users/jdecker/Downloads/arachnea/server`, hors réseau avec `CARGO_NET_OFFLINE=true` sauf formatage :

| Commande | Résultat |
| --- | --- |
| `cargo test -p arachnea-proxy --lib --features persistence` | 83 tests réussis, 0 échec |
| `cargo test -p arachnea-http --lib --features arachnea-proxy` | 44 tests réussis, 0 échec |
| `cargo test -p arachnea-core --lib --features sqlite-persistence persistence` | 19 tests réussis, 0 échec ; 12 autres tests filtrés |
| `cargo check -p arachnea-scrapyfy --features arachnea-proxy,sqlite-persistence` | Échec préexistant E0432 : import de `http::proxy_service` alors que `controller-service` n’est pas activée |
| `cargo check -p arachnea-scrapyfy --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service` | Réussite, vérification du cas avec feature additionnelle |
| `cargo check -p arachnea-stream` | Réussite ; l’application active `controller-service` dans sa dépendance proxy |
| `cargo fmt --all -- --check` | Échec préexistant sur plusieurs fichiers, sans réécriture |

L’import fautif est dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/actions/resolve_url.rs`. La réussite avec feature additionnelle ne résout pas le défaut de compilation isolée ; conserver les deux commandes dans le suivi et traiter explicitement la dépendance ou déplacer le helper approprié lors du lot concerné.

Le formatage signale notamment `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/browser.rs`, `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/engine/mod.rs`, des modules admin/configuration de Scrapyfy et des fichiers de `arachnea-stream`, dont son main et le resolver RMC BFM Play. Ne pas lancer un reformatage global hors périmètre ; distinguer ces différences initiales des futures modifications.

Les logs locaux de cette session se trouvent dans `/tmp/arachnea-lot0-*.log`, avec codes de sortie associés `.exit` ; ces fichiers temporaires ne sont pas des artefacts versionnés ni une preuve pérenne. Aucun nouveau test ni fixture n’a été créé. Ces 146 tests valident l’état initial des suites choisies, pas les contrats proposés, la concurrence Rquest, les blacklists réelles ou les performances réseau. La suite Scrapyfy et les variantes navigateur n’ont pas été exécutées dans ce lot ; le contrôle compilé ne les remplace pas.

### 12.9. Passage au lot 1

Les décisions fonctionnelles demeurent applicables ; les adaptations sont techniques. Avant implémentation, présenter en priorité :

1. Les nouveaux types d’observation et leur migration optionnelle, sans changement de clé d’inventaire.
2. La sélection explicite et la route épinglée nécessaires à une corrélation fiable avec le loopback Rquest ; la structure proposée du jeton et les adaptations de réutilisation de clients.
3. Le point d’observation headers/préfixe avant matérialisation, sans imposer une nouvelle réponse streamée publique ni prétendre supporter les moteurs navigateur sans preuve.

Le défaut de features Scrapyfy et la sérialisation des snapshots fichier sont suivis séparément dans `/Users/jdecker/Downloads/arachnea/docs/TODO.md`. Le lot 0 n’a corrigé aucun de ces problèmes et n’a changé aucun contrat exécuté.

## 13. Implémentation du lot 1 — 14 octobre 2026

La conception technique a été présentée puis confirmée avant modification.

Livré dans le code :
- [x] Observations bornées à 64 contextes par endpoint, phases séparées et dernier succès conservé par contexte ; santé positive et découverte optionnelles persistées sans migration fictive de `last_checked`.
- [x] Identité de configuration de probe : changement d’URL/mode/timeouts invalide les anciens agrégats et tentatives distantes, sans effacer les observations contextualisées.
- [x] Réponse HTTP reçue distincte du TLS : les statuts 5xx ne rendent plus le TLS indisponible ; échec distant sans promotion KO ni éviction globale.
- [x] Sélection explicite et core épinglé, jeton opaque, cache Rquest borné par origine/endpoint/identité de route ; retours Scrapyfy et médias corrélés, y compris après redirection.
- [x] Observation des en-têtes Rquest avant matérialisation ; erreurs attribuées avec `NotSent`, `PossiblySent` ou `ResponseReceived`. Aucun handler de contenu ni budget global anticipé.
- [x] Moteurs navigateur sans corrélation : aucune mutation de préférence/exclusion par déduction. APIs historiques par destination conservées comme adaptateurs limités.

Limites de recette : aucun nouveau test créé ni benchmark réseau réalisé. Les tests existants ne démontrent pas tous les nouveaux invariants, notamment les réponses concurrentes sur tunnels réutilisés, les changements de configuration et les annulations de probes. La négociation SOCKS reste couplée au tunnel dans les helpers existants : ne pas inventer de preuve indépendante d’authentification si le tunnel échoue. L’interruption externe d’un probe complet reste à traiter avec l’annulation du lot 5.

Procédure manuelle reproductible à compléter avant recette finale : utiliser deux proxies locaux contrôlés, effectuer des appels concurrents sur une même origine, retarder la réponse du premier, rejeter le second et vérifier dans l’inventaire que seul son endpoint est exclu ; répéter après réutilisation de connexion et redirection vers une seconde origine. Faire répondre le serveur complémentaire 503 après TLS valide puis vérifier les observations TLS/HTTP et l’absence de KO global. Remplacer l’URL de probe et vérifier que la temporisation distante ancienne n’empêche pas la nouvelle validation. Aucun fournisseur public n’est nécessaire pour cette procédure.

À la clôture du lot 1, les règles complètes d’éviction et la persistance des préférences restaient au lot 2, maintenant intégré et suivi en section 14. Les handlers, budgets et circuits restent aux lots 3 à 6.

### 13.1. Validations exécutées

Commandes hors réseau depuis `/Users/jdecker/Downloads/arachnea/server` :
- `cargo check -p arachnea-proxy --no-default-features --features core` : réussite.
- `cargo check -p arachnea-proxy --features persistence,controller-service,rquest,arachnea-dns` : réussite, binaire inclus. Les frontières async de sélection et connexion utilisent des futures boxées pour borner la profondeur des types, sans nouvelle dépendance.
- `cargo check -p arachnea-http --features arachnea-proxy` et `cargo check -p arachnea-stream` : réussite.
- `cargo test -p arachnea-proxy --lib --features persistence,controller-service` : 89 réussis.
- `cargo test -p arachnea-http --lib --features arachnea-proxy` : 44 réussis.
- `cargo test -p arachnea-proxy --test post_action_integration --features controller-service` : 4 réussis.
- Suite Scrapyfy avec `arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service` : 76 réussis, 3 échecs liés à l’initialisation globale du répertoire applicatif, y compris en série. Le filtre `scraper_agregator::tests -- --test-threads=1` passe ses 15 tests isolément. Ce blocage de suite n’a pas été corrigé hors périmètre.
- Compilation Scrapyfy sans `arachnea-proxy/controller-service` : défaut E0432 déjà relevé au lot 0, inchangé.
- `cargo fmt --all -- --check` : différences préexistantes hors périmètre ; fichiers Rust modifiés formatés individuellement avec `skip_children=true`. `git diff --check` passe.

Aucun nouveau test automatique n’a été ajouté. Deux tests existants ont été adaptés : date de découverte attendue dans le roundtrip fournisseur et recherche des headers par nom plutôt que position instable. Les scénarios réseau/concurrence de la procédure ci-dessus restent non exécutés ; le lot 1 est intégré et compilable mais sa recette exhaustive n’est pas déclarée terminée.

## 14. Implémentation du lot 2 — 14 octobre 2026

Conception présentée puis confirmée avant implémentation. Le lot est intégré ;
les livrables de code sont cochés en section 11.6, distinctement de la recette.

### 14.1. Livrables

- [x] `ProxyDestinationPreference` et repository typé : destination canonique sans pays, trois endpoints distincts maximum, dates UTC à précision milliseconde, tri récent puis départage stable, pas d’expiration par âge.
- [x] Stores applicatifs SQLite `proxy-preferences` et `proxy-failures`, injection dans l’agrégateur et partage par snapshots/reconstruction ; constructeurs historiques mémoire conservés.
- [x] Restauration paresseuse avant découverte, résolution des endpoints par clé sans dépendre d’un ancien pays, filtrage par éligibilité actuelle et conservation des associations non autorisées pour un appel.
- [x] Verrous par destination et commits attendus, sérialisés entre repositories utilisant le même store `Arc` ; fusion durable empêchant un ancien succès d’écraser un succès récent. Pas de verrou global de données pendant l’I/O ni worker autonome.
- [x] Exclusions persistées conservées après succès technique, panne globale temporaire et actualisation ; échéance configurable sans raccourcissement implicite. Historique accepté conservé pendant l’exclusion.
- [x] Éviction seulement sur échec établi de connexion à l’endpoint après 24 heures depuis preuve positive, sinon découverte initiale. Aucun recours à `last_checked` comme preuve. Avant éviction, indisponibilité globale et nouvel essai uniquement à la demande après le cooldown (15 minutes par défaut).
- [x] Suppression de l’inventaire disponible et nettoyage mémoire/durable des préférences et affinités ; état d’éviction séparé borné à 1 024 endpoints, conservé 48 heures et nettoyé à la demande. Une redécouverte ne renouvelle ni preuve ni découverte, et ne contourne pas le cooldown.
- [x] Échec de préférence journalisé avec destination/endpoint, compteur `preference_persistence_failures()` exposé, réponse métier et mémoire préservées sans rejeu. Une écriture peut être retentée lors d’un succès ultérieur.
- [x] README proxy/Scrapyfy/application, spécifications proxies FR/EN, TODO et changelog actualisés ; aucun YAML ni framework ajouté.

### 14.2. Validations exécutées

Commandes hors réseau depuis `/Users/jdecker/Downloads/arachnea/server` :

| Validation | Résultat |
| --- | --- |
| `cargo check -p arachnea-proxy --no-default-features --features core` | Réussite |
| `cargo check -p arachnea-proxy --features persistence,controller-service,rquest,arachnea-dns` | Réussite |
| `cargo check -p arachnea-stream` | Réussite, intégration HTTP/Scrapyfy incluse |
| `cargo test -p arachnea-proxy --lib --features persistence,controller-service` | 89 réussis |
| `cargo test -p arachnea-proxy --test post_action_integration --features controller-service` | 4 réussis |
| `cargo test -p arachnea-http --lib --features arachnea-proxy` | 44 réussis |
| Suite Scrapyfy avec `arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service` | 76 réussis, 3 échecs préexistants d’initialisation globale du répertoire applicatif |
| Même suite, filtre `scraper_agregator::tests -- --test-threads=1` | 15 réussis |
| Compilation Scrapyfy sans `arachnea-proxy/controller-service` | E0432 préexistant dans `actions/resolve_url.rs`, inchangé |
| `cargo fmt --all -- --check` | Différences préexistantes hors périmètre ; fichiers modifiés formatés individuellement |
| `git diff --check` | Réussite |

Aucun nouveau test n’a été créé. Quatre tests existants ont été étendus :
préférence avec permutations des pays et filtre restreint ; restauration des
préférences avant découverte et maintien d’une exclusion après succès technique ;
roundtrip des préférences mémoire/fichier/SQLite et observation ancienne ne
remplaçant pas un succès récent ; éviction SQLite et redécouverte immédiate après
reconstruction de l’inventaire. Ces validations ne constituent pas un benchmark.

### 14.3. Limites et recette restant ouverte

- [x] Injecter un échec du store de préférences et vérifier statut HTTP conservé, compteur incrémenté, préférence mémoire conservée, une seule requête et log contextualisé capturé — sections 15 et 16.
- [x] Exécuter des réponses HTTP réellement concurrentes et des réponses retardées pendant une éviction et une reconstruction d’agrégateur avec repositories partagés — sections 15 et 16. Le test d’agrégateur exerce les constructeurs et le snapshot, pas une commande admin complète.
- [x] Vérifier le cooldown de 15 minutes et sa fin par date expirée injectée, l’éviction après 24 heures depuis preuve/découverte et la rétention de 48 heures avec nettoyage à la demande — section 15.
- [x] Vérifier un état SQLite équivalent à une interruption après écriture du tombstone, avant suppression de l’inventaire et nettoyage de l’historique ; réouverture et exclusion de l’endpoint vérifiées — section 15.
- [x] Simuler les quatre frontières de commit de l’éviction : avant tombstone, après tombstone, après suppression d’inventaire, après nettoyage des préférences ; vérifier réouverture et reprise idempotente — section 16.
- [ ] Tester les véritables coupures du processus/disque ; les simulations logiques ne démontrent pas la résistance à une panne matérielle et il n’existe pas de transaction inter-stores.

Les garanties de commit portent sur des wrappers du même objet store partagé,
pas sur des objets indépendants visant le même fichier ni plusieurs processus.
La précision durable est la milliseconde, avec départage stable des égalités.
Les records hérités sans découverte reçoivent leur date lors de leur import,
sans reconstituer une ancienneté ou une preuve positive inconnue. Les états
d’éviction sont volontairement bornés ; après leur rétention, un fournisseur peut
à nouveau proposer un endpoint comme candidat inconnu à valider. Une erreur de
stockage ne garantit pas la restauration après redémarrage et reste signalée.

Le choix dans l’historique accepté est indépendant de l’ordre des pays dès ce lot.
Le classement des autres candidats et le parcours fournisseur restent au lot 4.
La politique actuelle de classification HTTP est inchangée, jusqu’au lot 3 ;
le terme « accepté » désigne encore les observations produites par ces adaptateurs.

## 15. Tests ciblés supplémentaires des lots 1 et 2

Créés et exécutés à la demande de l’utilisateur, sans nouvelle dépendance ni framework. Seuls des tests et la documentation ont été ajoutés ; aucun comportement de production n’a été corrigé dans cette étape.

### 15.1. Scénarios automatisés ajoutés

Les fixtures réseau écoutent sur `127.0.0.1` avec ports attribués par le système. Les réponses simultanées sont synchronisées par une barrière et les scénarios HTTP sont limités à 5 secondes. Aucun fournisseur, DNS distant ou proxy public n’est nécessaire.

Dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/tests/proxy_lifecycle_tests.rs`, inclus dans le module de tests de l’inventaire :

- `concurrent_http_responses_keep_endpoint_tokens_and_pinned_reconnects` : deux endpoints, même origine, réponses 200/403 réellement concurrentes ; deux connexions successives sur chaque route, puis succès/rejet concurrents attribués aux bons endpoints.
- `preference_write_failure_preserves_http_success_and_does_not_replay` : store typé rejetant l’écriture ; réponse 200 conservée, un seul appel HTTP, compteur d’échec à 1 et préférence réutilisable en mémoire. Le log existe dans le parcours appelé mais son contenu n’est pas asserté.
- `endpoint_failure_retention_uses_positive_evidence_not_attempt_date` : preuve positive de 23 h conservée avec cooldown ; preuve de 25 h et découverte de 25 h sans succès entraînant l’éviction malgré une tentative récente.
- `cooldown_expiry_allows_endpoint_revalidation_and_preserves_discovery` : exclusion avant expiration, candidat de nouveau admissible après injection d’une échéance passée, connexion TCP locale réussie et preuve renouvelée sans changer la découverte initiale. Le test vérifie la valeur par défaut de 15 minutes, sans attente réelle et sans démontrer le futur ordonnanceur automatique.
- `probe_target_change_invalidates_aggregates_without_erasing_positive_facts` : changement de configuration/URL, invalidation des agrégats et tentatives distantes, preuve positive conservée.
- `unavailable_http_probe_target_keeps_endpoint_positive_evidence` : vrai échange HTTP 503 via l’endpoint local, sans KO global ni exclusion d’origine.
- `redirect_response_token_belongs_to_final_origin_only` : deux réponses HTTP locales, redirection inter-domaines, jeton et préférence associés seulement à l’origine finale, corps conservé.
- `late_concurrent_token_results_do_not_resurrect_evicted_endpoint` : callbacks concurrents de réponse et succès après éviction ; aucune résurrection ni création d’historique orphelin.

Dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_repository.rs` :

- `interrupted_eviction_hides_available_record_and_orphan_preference` : stores SQLite réouverts avec tombstone durable, endpoint encore présent et préférence orpheline ; endpoint masqué par le repository, inutilisable après restauration/redécouverte, suppression rejouable. Simulation logique d’état partiel, pas arrêt forcé du processus.
- `failure_retention_expires_after_48_hours_and_prunes_on_demand` : tombstone de 49 h ignoré, conservé physiquement jusqu’au nettoyage déclenché par une éviction suivante.
- `concurrent_file_history_commits_survive_reopen_across_destinations` : 20 écritures concurrentes via deux wrappers du même store fichier, 4 destinations ; réouverture, 3 endpoints par historique et succès les plus récents conservés.

### 15.2. Portée et limites

Les 11 tests complètent les tests existants ; ils ne ferment pas toutes les cases de recette. Restent notamment la réutilisation de socket du client Rquest, le TLS distant valide suivi d’un 503 HTTPS, les requêtes actives pendant remplacement d’agrégateur, les autres points de coupure inter-stores et les pannes réelles du processus/disque. Les sockets des fixtures concurrentes sont rouvertes : ce sont des reconnexions sur une route épinglée, pas un test de keep-alive HTTP.

Les âges sont simulés par des dates historiques et un cooldown expiré injecté dans l’état de test. Aucun champ ni horloge de production n’a été ajouté. Les statuts sont classifiés explicitement par le scénario, sans prétendre tester le handler du lot 3 qui reste à implémenter.

### 15.3. Validation

Depuis `/Users/jdecker/Downloads/arachnea/server`, avec `CARGO_NET_OFFLINE=true` :

- `cargo test -p arachnea-proxy --lib --features persistence,controller-service` : **100 tests réussis, aucun échec**, dont les 11 nouveaux.
- `cargo test -p arachnea-http --lib --features arachnea-proxy` : **44 tests réussis, aucun échec**.
- `cargo test -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service scraper_agregator::tests -- --test-threads=1` : **15 tests réussis, aucun échec**, 64 filtrés. Ce filtre ne remplace pas la suite complète dont les limites sont décrites en section 14.2.
- `cargo test -p arachnea-proxy --lib --no-default-features --features core proxy_inventory::tests` : **19 tests réussis, aucun échec**, 66 filtrés ; vérifie notamment les nouveaux tests ne nécessitant pas la persistance.
- `rustfmt --edition 2021 --check` sur le fichier de fixtures et le repository modifié, et `git diff --check` : **réussite**.

Les deux nouveaux fichiers/blocs de tests sont formatés avec Rustfmt ; les différences préexistantes de formatage global ne sont pas réécrites. Les premiers essais ont identifié une erreur de fixture (`ClientContext` utilise `insert`) et une attente incorrecte de sélection sans prober pour un candidat KO : les tests ont été ajustés aux interfaces et frontières effectivement testées, sans changer le code métier.

## 16. Complément de recette automatisée des lots 1 et 2

### 16.1. Tests réalisés

- **Rquest keep-alive et HTTPS 503** : `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-http/src/engine/rquest_proxy_tests.rs`, `keep_alive_tls_503_keeps_exact_selection_and_positive_phases`. Un proxy HTTP local reçoit CONNECT puis sert TLS ; le serveur lit deux requêtes sur le même stream accepté et renvoie 200 puis 503. La CA de test est explicitement approuvée par Rquest ; vérifications TLS et hostname restent actives. Une autre sélection rejetée entre les appels conserve son exclusion propre ; tunnel, TLS et réception HTTP restent des faits positifs pour l’endpoint réellement utilisé. Aucune préférence nouvelle n’est créée sur le seul 503.
- **Éviction pendant réponse réseau active** : `endpoint_eviction_during_delayed_http_response_does_not_resurrect_history`, dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/tests/proxy_lifecycle_tests.rs`. Le serveur signale la réception avant de suspendre sa réponse ; après injection d’une preuve ancienne et éviction, il rend 200. Le jeton tardif ne ressuscite pas l’endpoint ni son historique. L’âge est injecté pour construire l’entrelacement, pas obtenu en attendant 24 heures.
- **Reconstruction d’agrégateur pendant requête active** : `replacement_preserves_shared_proxy_history_during_active_response`, dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-scrapyfy/src/scrapyfy/scraper_agregator.rs`. Les constructeurs existants, `set_proxy_repositories` et `provider_snapshot` partagent les repositories ; l’ancien agrégateur est détruit après réception HTTP et avant réponse. L’appel conservé termine et sa préférence est restaurée par le nouvel inventaire sans fournisseur. Ce scénario contrôlé ne prétend pas exercer toute la commande administrative de reload.
- **Frontières de commit inter-stores** : `eviction_recovery_handles_remaining_inter_store_commit_boundaries`, dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/proxy_repository.rs`. Quatre états SQLite sont construits et réouverts : aucune écriture d’éviction, tombstone seul, inventaire supprimé, historique nettoyé. L’éviction/cleanup explicite peut être rejoué deux fois. Avant la première écriture durable, le redémarrage ne peut pas connaître la panne observée uniquement en mémoire : le test documente cette limite, il ne la masque pas.
- **Log de persistance** : le test existant d’échec d’écriture capture maintenant un subscriber scoped avec `WithSubscriber`. Il vérifie message, cause injectée et destination, sans modifier le subscriber global ni rejouer la requête.

Quatre nouveaux tests ont été ajoutés et un test précédent renforcé. `rustls` et `tokio-rustls`, déjà utilisés par le workspace, sont ajoutés uniquement aux dev-dependencies HTTP ; aucun framework nouveau. Les fixtures DER et leur clé serveur publique de test sont documentées dans `/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/tests/fixtures/README.md`. Leur validité est du 14 octobre 2026 au 11 octobre 2036 ; elles ne doivent jamais être utilisées en production. La clé privée de la CA n’est pas versionnée.

### 16.2. Validations finales exécutées

Depuis `/Users/jdecker/Downloads/arachnea/server` avec `CARGO_NET_OFFLINE=true` :

| Commande | Résultat |
| --- | --- |
| `cargo test -p arachnea-proxy --lib --features persistence,controller-service` | 102 réussis, 0 échec |
| `cargo test -p arachnea-http --lib --features arachnea-proxy` | 45 réussis, 0 échec |
| `cargo test -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service scraper_agregator::tests -- --test-threads=1` | 16 réussis, 0 échec ; 64 filtrés |
| `cargo check -p arachnea-stream` | Réussite |
| Rustfmt ciblé et `git diff --check` | Réussite |

Les premiers essais des nouvelles fixtures ont exposé des erreurs de noms d’API/valeurs serde et le besoin d’initialiser le répertoire applicatif de test ; ils ont été corrigés dans les tests, sans modification du comportement de production. Les suites finales ci-dessus ont toutes été relancées avec succès.

### 16.3. Limites encore ouvertes et clôture

Les tests désormais réalisés ferment les scénarios locaux de keep-alive métier, TLS métier + 503, callbacks pendant requêtes retardées, log et états partiels d’éviction. Les assertions de la section 15 restent valables ; ses limites décrivent l’état à cette étape, complété ici.

La recette exhaustive du lot 1 n’est pas cochée : `ProxyProbe` construit ses racines WebPKI en interne sans injection de CA. Tester son propre parcours HTTPS avec une CA locale demanderait un point d’injection de confiance, à concevoir et confirmer séparément, ou une cible externe réellement approuvée. Le présent test Rquest ne remplace pas cette validation et aucune désactivation de certificat n’a été introduite.

Le lot 2 dispose des scénarios locaux demandés, mais ne garantit pas une coupure matérielle, un kill de processus à tous les instants ni une transaction inter-stores. Ces niveaux supplémentaires restent explicitement hors couverture. Les trois échecs préexistants de la suite globale Scrapyfy et la configuration de feature isolée restent suivis ; le filtre agrégateur passant ne les corrige pas.

Aucun changement de comportement backend n’a été réalisé dans ce complément : uniquement tests, fixtures, dépendances de développement et suivi documentaire.

## 17. Lot 3 — handler commun intégré (15 octobre 2026)

### 17.1. Implémentation et contrats

La conception a été confirmée avant modification. Le module
`/Users/jdecker/Downloads/arachnea/server/crates/arachnea-proxy/src/core/http/response_classifier.rs`
définit une politique synchrone `Send + Sync`, une vue empruntée indépendante
d’arachnea-http, une décision acceptée/rejetée pour destination/inconclusive et
une inspection non destructive réutilisée par Rquest et le client médias.

- Par défaut seuls les 2xx sont acceptés. Les autres statuts, dont 5xx et 403
  non configuré, restent inconclusifs. Un rejet ne dégrade jamais globalement
  l’endpoint et un résultat inconclusif ne modifie ni historique ni exclusion.
- Les politiques connues sérialisables sont `default`, `http403`, `statuses`.
  Cette dernière valide les statuts 100..599, leur non-chevauchement et le
  cooldown optionnel positif/représentable. Une liste d’acceptation omise signifie
  2xx ; une liste vide n’accepte rien. Les objets traits ne sont pas sérialisés.
- Priorité : injection code, configuration connue explicite, adaptation de
  `proxy_rejection_statuses`, politique prudente. Une politique enfant remplace
  celle du parent ; `default` permet une réinitialisation explicite. Les clients
  Scrapyfy configurés/à cookies isolés préservent la politique injectée.
- Les options JSON d’URL médias acceptent la même configuration, validée en
  entrée et conservée lors de la réécriture des redirections. Les activations
  403 FranceTV/TF1 (ainsi que M6Play) existantes sont conservées sans modifier
  les services. Aucun shaping métier n’est déplacé dans leurs façades.
- La vue porte méthode réelle, URL initiale/effective, redirections disponibles,
  statut, en-têtes originaux, jeton exact et état du préfixe. Un rejet explicite
  d’une redirection est rendu à l’orchestrateur avant le saut suivant. Seul le
  jeton de la réponse finale rendue est validé ; les origines précédentes ne sont
  pas validées par inférence.
- Le handler de statuts ne lit aucun corps. Un handler code peut demander
  `Prefix(n)` ; l’adaptateur limite l’extrait à 64 Kio et l’attente à trois
  secondes (également au timeout restant pour Rquest). Les octets et erreurs
  consommés sont rejoués dans l’ordre. Les états complet, limité et indisponible
  sont distincts ; atteindre la limite ne prouve jamais EOF. Un extrait
  incomplet ne démontre pas l’absence d’un marqueur. Les erreurs retournées
  par le handler sont journalisées puis rendues inconclusives.
- La classification précède la matérialisation Rquest et les post-actions
  médias. Les orchestrateurs appliquent la décision déjà produite au jeton,
  sans refaire une classification après transformation. Un cooldown de rejet
  optionnel utilise le stockage existant et ne raccourcit pas une exclusion.
- La feature `stream` de la dépendance Rquest existante est activée, sans nouveau
  framework. Les moteurs navigateur/personnalisés sans frontière équivalente
  restent explicitement sans décision corrélée et signalent cette limitation
  lorsqu’une politique est configurée.

### 17.2. Validation et adaptations des tests existants

Aucun nouveau test ni infrastructure de test n’a été créé. Des scénarios
existants sont adaptés :

- le keep-alive TLS/Rquest 200 puis 503 vérifie les décisions prudentes et un
  préfixe de deux octets `do` tout en restituant le corps complet `down` ;
- le test d’options de redirection vérifie la conservation de `http403`, sa
  priorité sur une liste historique `[500]`, 200 accepté/403 rejeté/451 et 500
  inconclusifs, ainsi que plusieurs configurations invalides ;
- le test de redirection inter-domaines applique la décision au jeton final,
  conserve l’historique sur résultat inconclusif et vérifie un cooldown injecté ;
- le test de configuration Scrapyfy vérifie l’héritage et la surcharge explicite
  ainsi que la conservation du handler code dans les clients dérivés.

Commandes exécutées depuis `/Users/jdecker/Downloads/arachnea/server` avec
`CARGO_NET_OFFLINE=true` :

| Contrôle | Résultat |
|---|---|
| `cargo test -p arachnea-proxy --lib --features persistence,controller-service` | 102 tests réussis |
| `cargo test -p arachnea-proxy --test post_action_integration --features controller-service` | 4 tests réussis |
| `cargo test -p arachnea-http --lib --features arachnea-proxy` | 45 tests réussis |
| `cargo test -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service -- scrapyfy::http_client` | 2 tests réussis |
| `cargo check -p arachnea-http --no-default-features` | Réussite ; warning préexistant dans arachnea-core |
| `cargo check -p arachnea-scrapyfy --no-default-features` | Réussite ; warnings de code inutilisé selon features |
| `cargo check -p arachnea-stream` | Réussite |
| Rustfmt limité aux fichiers Rust concernés et `git diff --check` | Réussite |

La suite globale Scrapyfy exécutée avec les mêmes features obtient **76 succès
et 4 échecs** : résolution du répertoire applicatif avant configuration, puis
empoisonnement du verrou `Once`. C’est la même cause préexistante que les trois
échecs signalés précédemment ; cette exécution touche aussi le scénario de
remplacement d’agrégateur. Le filtre HTTP passant ne corrige pas ce défaut.
La compilation isolée avec proxy sans `controller-service` reste le problème
de feature préexistant suivi dans TODO. L’outil build de l’IDE JetBrains n’a pas
de projet Arachnea ouvert dans cet environnement VS Code ; les validations
effectives ci-dessus utilisent Cargo.

### 17.3. Limites de couverture et périmètre restant

L’implémentation ne signifie pas recette exhaustive : aucun scénario dédié
ne valide encore les 64 Kio exacts, un corps binaire fragmenté, les timeouts/
erreurs de lecture ou de handler, les flux HLS/MPD/chunked avec handler de
contenu, le masquage par post-action, ni tous les moteurs sans corrélation.
Leurs contrats sont implémentés et documentés, mais ces cas doivent faire l’objet
d’une autorisation explicite de tests supplémentaires avant de cocher §11.12.
Les tests de post-actions existants ne prouvent pas à eux seuls l’ordre de
classification sur un vrai flux réseau transformé.

L’inspection du contenu utilise un flux décodé du transfert HTTP. La tolérance
historique des réponses déjà déchunkées portant encore l’en-tête chunked est
préservée dans le parcours bufferisé à handler d’en-têtes ; elle n’est pas
disponible pour l’inspection précoce de contenu, qui reste stricte. Le corps
public arachnea-http reste matérialisé. Aucun timeout global recherche + essais,
budget commun de retries, nouvel ordre de pays ou parallélisme de sélection
n’est ajouté ; lots 4/5 et suivants inchangés. Un handler synchrone doit rester
non bloquant : le plafond de lecture ne peut pas interrompre un handler code
qui viole ce contrat.


## 18. Lot 4 — sélection dans un ensemble de pays, cache d’abord (15 octobre 2026)

### 18.1. Implémentation

La conception a été confirmée avant modification. Les pays sont normalisés,
filtrés en codes alpha-2, dédupliqués et triés pour leurs identités : paramètres,
marqueurs multi-pays (y compris leur lecture), URLs médias, configuration HTTP
Scrapyfy et requêtes fournisseur. Le tri ne donne aucune priorité géographique.
Les handlers statiques du même paramètre départagent leurs routes autorisées par
endpoint puis nom ; les pools statiques indépendants ne sont pas modifiés.

L’inventaire conserve le parcours rapide historique accepté, puis affinité.
Une préférence non autorisée reste conservée. Sans résultat sur ce parcours,
il importe les caches persistants des pays autorisés avant toute exploration
réseau des inconnus, puis classe tous les candidats ensemble, pour HTTP comme
pour HTTPS : capacités complètes fraîches, capacités complètes à revalider,
preuves pertinentes partielles, inconnus. La latence, les échecs puis l’endpoint
départagent chaque catégorie. Les candidats prêts passent avant toute fenêtre
de préchecks. Santé, authentification, pays, capacités, exclusions de destination
et certificat du proxy restent contrôlés ; un certificat explicitement invalide
est également refusé sur le parcours rapide des préférences et affinités.

Après épuisement du cache combiné, les chargements fournisseur restent séquentiels
et mutualisés par pays. Tous les pays autorisés sont chargés avant de classer
ensemble leurs résultats : une source lente peut encore retarder cette découverte
à froid. Les budgets communs, l’exploration bornée et le pipeline concurrent
restent au lot 5 ; aucun travail en arrière-plan n’est ajouté.

### 18.2. Validation et limites

Aucun nouveau test n’est créé. Les tests existants sont adaptés pour vérifier
les permutations/normalisations des pays, le départage statique par endpoint,
les marqueurs canoniques, les en-têtes d’URLs médias et la configuration Scrapyfy.
Le scénario multi-pays vérifie HTTP et HTTPS avec un prober configuré, un inconnu
moins coûteux annoncé et un candidat prêt d’un autre pays, chargé uniquement
depuis le repository lorsque la feature persistence est active. Aucun précheck
ni test complet n’est enregistré pour l’inconnu, aucun fournisseur n’est appelé.
Une seconde partie départage deux candidats prêts de pays différents par latence.
Les scénarios existants d’historique, d’exclusion et de persistance sont conservés.
Le scénario d’éligibilité vérifie aussi le rejet d’un certificat explicitement
invalide sur le parcours rapide, puis la réutilisation après restauration du statut.

Les premiers contrôles ont relevé une référence de log supprimée, puis un
classement trop proche des capacités partielles et complètes anciennes ; ils ont
été corrigés. Les fixtures réseau exigent une exécution hors sandbox pour ouvrir
leurs sockets locales. Les résultats finaux sont consignés ci-dessous.


Contrôles finaux, depuis `server/`, en mode Cargo offline :

| Commande | Résultat |
|---|---|
| `cargo test --offline -p arachnea-proxy --lib --features persistence,controller-service --quiet` | 102 réussis, 0 échec |
| `cargo test --offline -p arachnea-http --lib --features arachnea-proxy --quiet` | 45 réussis, 0 échec |
| `cargo test --offline -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service -- scrapyfy::http_client` | 2 réussis, 78 filtrés |
| `cargo test --offline -p arachnea-proxy --lib --no-default-features --features core multi_country_selection_checks_all_cached_countries_before_loading` | 1 réussi, 85 filtrés ; parcours mémoire sans persistance |
| `cargo test --offline --manifest-path server/Cargo.toml -p arachnea-proxy --lib --features persistence,controller-service validated_proxy_preference_never_bypasses_eligibility` (depuis la racine) | 1 réussi, 101 filtrés ; complément certificat après la suite globale |
| `cargo check --offline -p arachnea-scrapyfy --no-default-features` | Réussite ; warnings de code inutilisé dans core/Scrapyfy selon features |
| `cargo check --offline -p arachnea-stream` | Réussite |
| Rustfmt ciblé et `git diff --check` | Réussite |

Aucun benchmark réseau ni appel aux fournisseurs réels n’a été effectué.
La suite Scrapyfy globale n’a pas été relancée ; ses limites préexistantes sont
conservées en section 17. Les tests locaux ne constituent pas une mesure du délai
réel de découverte multi-pays. Les retests concurrents, budgets et retries restent
au lot 5. Aucun test ni infrastructure de test supplémentaire n’a été créé.


## 19. Lot 5 — budget commun, retries sûrs et pipeline progressif (15 octobre 2026)

### 19.1. Contexte et frontières

La conception a été confirmée avant modification. Le module `proxy_search.rs`
définit `ProxySearchLimits`, `ProxySearchContext` et `proxy_retry_allowed`, sans
dépendance à arachnea-http ou aux services. Un contexte possède un départ
monotone, une cible de 120 secondes et une limite absolue de 135 secondes.
Les scopes Tokio propagent le contexte sans le sérialiser. Le core épinglé le
conserve explicitement pour les tâches du connecteur loopback.

La sélection, les imports de cache, l’attente de verrous/créneaux et les fournisseurs
restent bornés à la cible. La marge de 15 secondes ne concerne que la tentative
métier déjà engagée ; aucun nouveau candidat ou retry n’est lancé pendant cette
marge. Les plafonds communs sont 64 réservations TCP, 32 réservations de validation
et 8 endpoints métier distincts. Une réservation peut être consommée pendant
l’attente d’un créneau puis annulée : ce choix conservateur garantit les plafonds
mais peut réduire le nombre de tests réseau effectivement exécutés sous forte
contention. Les lectures et candidats prêts ne consomment pas de réservation.
Une opération mutualisée utilise une exécution réseau et une réservation dans
chaque recherche participante ; aucune remise à zéro entre les couches.

Les tentatives d’un même endpoint/URL sont exclues localement. Les URL effectives
distinguent les redirections de la répétition d’une tentative ; les endpoints
restent comptés une seule fois dans le plafond commun. Les requêtes métier restent
séquentielles, avec leurs propres méthodes, corps, cookies et identités.
Le fallback SOCKS DNS ne réessaie pas le même endpoint dynamique dans ce parcours.
Les pools statiques indépendants conservent leurs politiques.

L’acceptation finale ferme la recherche dès les en-têtes ou l’extrait requis,
y compris pendant la marge. Un canal watch libère alors les attentes d’échéance
encore actives : un corps accepté peut dépasser 135 secondes. Rquest utilise
séparément le timeout HTTP ordinaire pour les en-têtes puis le transfert accepté
(25 secondes via Scrapyfy, 30 par défaut dans arachnea-http). Le client médias
limite ses en-têtes contextuels et corps bufferisés à 30 secondes ; les flux
streamés acceptés conservent leur politique de transfert. Les appels médias sans
contexte ne reçoivent pas ce nouveau timeout. Le tunnel seul conserve un budget
d’établissement de 120 secondes, sans inventer d’acceptation HTTP.

### 19.2. Pipeline et durée de vie

L’inventaire partage ses données, limites, verrous et métriques entre clones.
Les préchecks utilisent `buffer_unordered` et alimentent immédiatement les
validations, sans collecte de fenêtre complète. Limites par recherche/inventaire :
TCP 8/16, validations 2/4 et chargements pays 2/2. `probe_batch_size` est borné
à 1..=8 ; Scrapyfy n’installe plus sa valeur historique de 32.

Les préchecks sont mutualisés par endpoint ; les validations par endpoint,
phase HTTP/HTTPS et identité de configuration ; les fournisseurs par pays.
Les registres conservent seulement des `WeakShared`. Chaque participant pilote
la future partagée et borne sa propre attente. Le départ d’un participant ne
coupe pas ceux qui attendent encore. Le départ du dernier abandonne la future,
libère les créneaux et ne produit aucun échec ni résultat inconclusif artificiel.
L’abandon d’une opération technique est journalisé séparément d’un résultat.
Aucun worker de découverte ni tâche de réparation n’est détaché.

Les validations complètes conservent leur plafond de huit secondes. Seules les
tentatives terminées, y compris un timeout réel de ce plafond, enregistrent leur
temporisation. Une validation interrompue avant résultat n’écrit pas un échec ;
un redémarrage peut donc la retenter. Les preuves positives effectivement acquises
restent valides. Chaque appel réapplique pays, exclusions et sécurité à un résultat
partagé avant de le proposer.

Les loopbacks contextuels vivent avec l’appel, hors cache inter-appels. Leur
`JoinSet` annule les connexions enfants quand le listener est abandonné. Cela
évite de garder un ancien budget sur une reconnexion, mais entraîne actuellement
une reconnexion entre appels contextuels. Les préférences par destination et les
clients keep-alive ordinaires restent disponibles. Le remplacement par un
connecteur natif ou une corrélation permettant une meilleure réutilisation de
socket reste un chantier distinct ; il n’est pas dissimulé dans le lot 5.

### 19.3. Retry, configuration et intégration

La façade arachnea-http porte la boucle de retry Rquest ; les deux boucles
textuelle/binaire Scrapyfy et leur addition 120 + 25 secondes sont supprimées.
Le contexte Scrapyfy commence avant l’acquisition du client partagé. La route
médias utilise la même politique et le même contexte jusqu’à la classification.
Les refus sont enregistrés au jeton exact même si la requête n’est pas rejouable.
Un résultat inconclusif ou une erreur de collecte après en-têtes ne tourne pas.

GET/HEAD peuvent réessayer un refus configuré ou une erreur de transport pertinente.
Une opération modificatrice ne le peut qu’avant envoi démontré, ou avec garantie
explicite de rejeu et corps reproductible. Une erreur Rquest opaque reste
`PossiblySent`. Les erreurs médias distinguent les frontières pré-envoi,
écriture/lecture d’en-têtes incertaines et collecte de corps. `ProxyError` expose
le variant contextualisé `RequestAttempt` ; les erreurs sans preuve d’envoi
absent ne sont pas requalifiées à partir d’une chaîne de caractères.

L’option `http.proxy_retry_replayable: true` est facultative et héritée ; `false`
annule une autorisation parent. La configuration code arachnea-http propose le
builder équivalent. Les options JSON d’URL médias acceptent le même booléen,
strictement validé et conservé lors de la réécriture des redirections. Aucun
service n’est déclaré rejouable automatiquement. Les schémas YAML ne changent
pas d’autres champs ni les opérations métier des services.

Les moteurs navigateur/personnalisés sans corrélation précoce retournent une
erreur d’opération non supportée lorsqu’ils sont invoqués dans cette recherche,
y compris en fallback automatique. Les API explicites de page navigateur restent
un autre parcours ; elles ne sont pas présentées comme couvertes par ces budgets.
L’épuisement d’une recherche contextualisée ne déclenche pas un fallback direct.

Le contrôle combiné des features HTTP/proxy a exposé le test serveur SOCKS déjà
existant : son parseur séparait aussi les virgules internes à la liste JSON de
pays. La correction locale conserve les séparateurs historiques hors tableaux/
chaînes JSON, sans réorganiser le serveur ni changer le contrat du paramètre.

### 19.4. Validation et limites de recette

Aucun nouveau test ni infrastructure de test n’a été créé. Les scénarios existants
sont adaptés : expiration d’un premier appel pendant qu’un second continue le
même chargement fournisseur ; réponse TLS acceptée pendant une marge simulée en
millisecondes, corps terminé après sa limite absolue et seconde réponse 503 sur
le même stream ; règles de rejeu POST/GET ; héritage/surcharge du booléen et
conservation/validation dans les options de redirection. Les autres scénarios
locaux des lots précédents restent exécutés.

Les premiers essais ont corrigé une contrainte de durée de vie des futures
concurrentes et les correspondances exhaustives du nouveau variant d’erreur.
Les tests réseau doivent s’exécuter hors sandbox pour leurs sockets locales.
La combinaison de features élargit la suite proxy de 102 à 125 tests existants ;
le défaut SOCKS constaté dans cette combinaison a été corrigé, et non filtré.

Les suites ne constituent pas une recette exhaustive de charge : les plafonds
64/32/8 avec toutes les combinaisons de retries, la mesure des sémaphores sous
forte concurrence, toutes les annulations de validation/fournisseur et l’absence
de rejeu POST sur de vrais effets serveur restent à compléter sur autorisation
de tests supplémentaires. Aucun benchmark fournisseur réel, proxy externe ou
reverse proxy de production n’a été utilisé. La configuration d’exploitation
doit autoriser l’horizon de recherche de 135 secondes et le transfert séparé.
La suite globale Scrapyfy conserve les limites préexistantes de la section 17.
La publication progressive des sources, le chevauchement après 10 secondes/8
validations et les circuits de cible restent au lot 6.

Le filtre d’agrégateur a également révélé un dépassement de pile dû à la taille
des futures HTTP composées. Les frontières publiques/internes concernées sont
boxées, sans augmenter la pile du runtime ni modifier les scénarios. Le filtre
complet de 16 tests repasse avec la taille de pile habituelle.

Contrôles finaux, Cargo offline (commandes depuis la racine sauf indication) :

| Commande | Résultat |
|---|---|
| `cargo test --offline --manifest-path server/Cargo.toml -p arachnea-proxy -p arachnea-http --lib --features arachnea-proxy/persistence,arachnea-proxy/controller-service,arachnea-http/arachnea-proxy --quiet` | Proxy : 125 réussis ; HTTP : 45 réussis ; aucun échec |
| `cargo test --offline --manifest-path server/Cargo.toml -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service -- scrapyfy::http_client` | 2 réussis, 78 filtrés |
| `cargo test --offline -p arachnea-scrapyfy --lib --features arachnea-proxy,sqlite-persistence,arachnea-proxy/controller-service scraper_agregator::tests -- --test-threads=1` (depuis `server/`) | 16 réussis, 64 filtrés ; pile normale |
| `cargo test --offline --manifest-path server/Cargo.toml -p arachnea-proxy --lib --no-default-features --features core multi_country_selection_checks_all_cached_countries_before_loading` | 1 réussi, 85 filtrés |
| `cargo check --offline --manifest-path server/Cargo.toml -p arachnea-http -p arachnea-scrapyfy --no-default-features` | Réussite ; warnings préexistants de code inutilisé selon features |
| `cargo check --offline --manifest-path server/Cargo.toml -p arachnea-stream` | Réussite |
| Rustfmt ciblé et `git diff --check` | Réussite |

La suite combinée proxy/HTTP a passé avant le boxing final ; le filtre complet
d’agrégateur et les compilations ont ensuite repassé après ce changement de
représentation des futures. Les résultats ne valent pas validation exhaustive
des plafonds et entrelacements non couverts, listés ci-dessus.
