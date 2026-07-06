# Chargement dynamique des proxies par pays - Analyse

## Resume

La gestion actuelle des proxies par pays est principalement statique : le code
applicatif construit un `ProxyNode` pour un pays donne, puis `arachnea-proxy`
ajoute ce noeud a la chaine de connexion quand une requete porte le parametre
`country`. Cette base fonctionne pour un proxy connu a l'avance, mais elle ne
permet pas encore de charger, tester, enrichir et persister une liste de proxies
recuperee dynamiquement.

La direction recommandee est de separer clairement trois responsabilites :

1. `arachnea-proxy` possede les modeles generiques, le trait de chargement, le
   test des proxies, l'inventaire runtime, la selection et la connexion.
2. `arachnea-scrapyfy` gere par defaut la recuperation de donnees proxy et le
   cablage du provider avec le proxy core, en s'appuyant sur ses mecanismes
   YAML/actions existants.
3. `arachnea-stream` peut encore fournir une configuration applicative ou un
   override explicite, mais ne doit plus etre responsable par defaut de la
   gestion des proxies.

Ce decoupage evite une dependance inverse de `arachnea-proxy` vers
`arachnea-scrapyfy`, tout en permettant a la partie proxy de demander des
donnees quand elle n'a plus de candidat fonctionnel pour un pays.

## Etat actuel observe

### Routage pays

`arachnea-proxy/src/core/parameters.rs` contient deja :

- `PROXY_HEADER_PARAMETER_COUNTRY` (`Arachnea-Proxy-Country`) ;
- `PROXY_PARAMETER_COUNTRY` (`country`) ;
- `CountryRoutingProxyHandler`, qui mappe une valeur pays vers un `ProxyNode` ;
- `ProxyParameterHandler`, trait synchrone qui retourne un proxy supplementaire
  a ajouter a la chaine courante.

Le comportement actuel est donc :

```text
requete avec country=FR
  -> CountryRoutingProxyHandler
  -> lookup dans une map FR -> ProxyNode
  -> append du ProxyNode a la chaine
```

Cette approche suppose que les proxies par pays sont connus au moment ou le
core est construit. Elle ne sait pas charger une liste au besoin, ni choisir
entre plusieurs proxies FR selon leur etat.

### Pools d'egress

`arachnea-proxy/src/core/policy/egress.rs` contient deja une notion de
`EgressPool` avec :

- un `name` ;
- un `country: Option<String>` ;
- une liste de `proxy_nodes` ;
- une strategie `FirstAvailable` ;
- un etat runtime par membre : `Untested`, `Ok`, `Ko`.

`arachnea-proxy/src/core/core.rs` sait remplacer un noeud
`TransportKind::ProxyPool` par un membre concret et tester les candidats. Ce
socle est proche du besoin, mais il reste limite :

- l'etat est uniquement en memoire ;
- il n'y a pas de latence mesuree ;
- il n'y a pas de resultat de test structure indiquant le support HTTPS ;
- le protocole doit deja etre connu via `ProxyNode.kind` ;
- les pools sont declares dans la configuration resolue, qui est actuellement
  consideree comme immutable apres construction.

### Modele de proxy concret

`ProxyNode::from_url(...)` exige un schema, par exemple `http://`,
`https://`, `socks5://` ou `socks4a://`. Sans schema, le code applique le
schema par defaut `http`. Cela suffit pour une URL complete, mais pas pour une
source de donnees qui fournit seulement une adresse IP ou un hostname, un port,
et laisse le protocole inconnu.

Les donnees dynamiques doivent accepter IPv4, IPv6 et noms d'hote. La
normalisation vers `ProxyNode` devra produire un endpoint `host:port` pour IPv4
ou hostname, et `[ipv6]:port` pour IPv6.

Le point important : `ProxyNode` represente un proxy utilisable par la couche
transport. Il est preferable qu'il reste concret. Le protocole optionnel doit
vivre dans un modele de donnee amont, avant validation/probe. Quand ce
protocole est fourni par la source, on le considere correct et on ne maintient
pas deux champs de protocole concurrents.

### Scrapyfy et proxy

`arachnea-scrapyfy/src/scrapyfy/http_client.rs` expose deja
`ScraperHttpConfig::proxy_country(...)`. Cette option normalise le pays et le
passe a `arachnea-http`, qui transmet ensuite le parametre `country` au proxy
configure.

Cela signifie que le chemin "un resolver demande une sortie pays" existe deja.
Ce qui manque est le chemin inverse : "le proxy n'a pas de proxy fonctionnel
pour ce pays, donc il demande a un provider de charger de nouveaux candidats".

### Composition applicative actuelle

`arachnea-stream/src/main.rs` construit actuellement un proxy FR en dur avec
`ProxyNode::from_url(...)`, puis cree un `ProxyConfig` contenant un
`country_routing` handler pour `FR`.

Cette configuration est le meilleur exemple de ce qu'il faut remplacer :
au lieu d'un unique proxy FR code dans l'application,
`arachnea-scrapyfy` devrait fournir et brancher par defaut le provider
dynamique. `arachnea-stream` peut rester capable de remplacer ou completer cette
configuration, mais ce ne doit plus etre son chemin nominal.

## Probleme a resoudre

Le besoin couvre plusieurs cas :

- modifier ou charger une liste de proxies sans modifier le code Rust ;
- charger dynamiquement des donnees proxy, par pays si necessaire ;
- garder la recuperation des donnees dans `arachnea-scrapyfy` ;
- garder la selection, le test et la connexion dans `arachnea-proxy` ;
- relier les deux par un trait ;
- tester un proxy : reponse, temps de reponse, support HTTPS ;
- ne conserver comme utilisables que les proxies fonctionnels ;
- etudier le protocole optionnel quand la source ne le fournit pas ;
- etudier la persistance ;
- determiner le pays quand il est inconnu ;
- charger des donnees quand aucun proxy fonctionnel n'est disponible pour un
  pays demande.

## Principe de separation recommande

### Fiche proxy canonique vs proxy utilisable

Une structure dediee reste utile, mais le nom `ProxyCandidate` est trop limite :
apres chargement, test et persistance, ce n'est plus seulement un candidat.
L'analyse retient donc une structure canonique de type `ProxyRecord` (nom final
a valider) pour representer une fiche proxy mutable et persistable.

Les structures existantes ne couvrent pas ce role sans duplication :

- `ProxyNode` est la forme transport concrete utilisee pour ouvrir une
  connexion ; elle suppose un protocole resolu et ne porte pas les metadonnees
  dynamiques comme latence, statut, pays declare ou dernier controle.
- `EgressPool` regroupe des `ProxyNode` declares ou resolus, mais ce n'est pas
  un magasin de donnees dynamique.
- `ProxyPoolMemberState` expose aujourd'hui surtout `Untested`, `Ok`, `Ko` et
  le membre selectionne ; il ne suffit pas pour stocker les donnees source,
  l'authentification requise, le support HTTPS, la latence ou la persistance.

L'objectif est donc de limiter les conversions : le provider `scrapyfy`, le
probe, l'inventaire runtime et la persistance manipulent `ProxyRecord`. La
conversion en `ProxyNode` se fait seulement au dernier moment, quand un proxy
est selectionne pour une connexion.

Exemple conceptuel cote `arachnea-proxy` :

```rust
pub struct ProxyRecord {
    pub protocol: Option<ProxyProtocol>,
    pub host: String,
    pub port: u16,
    pub country: Option<String>,
    pub supports_https: Option<bool>,
    pub status: ProxyRuntimeStatus,
    pub latency_ms: Option<u64>,
    pub failure_count: u32,
    pub authentication_required: Option<bool>,
    pub availability: Option<ProxyAvailabilityHint>,
    pub destination_failures: Vec<ProxyDestinationFailure>,
    pub source: Option<String>,
    pub last_checked: Option<SystemTime>,
    pub cooldown_until: Option<SystemTime>,
}

pub enum ProxyProtocol {
    Http,
    Https,
    Socks4,
    Socks4a,
    Socks5,
}

pub enum ProxyRuntimeStatus {
    Unknown,
    Ok,
    Ko,
    AuthenticationRequired,
}

pub enum ProxyAvailabilityHint {
    Unknown,
    Low,
    Medium,
    High,
}

pub struct ProxyDestinationFailure {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub reason: ProxyDestinationFailureReason,
    pub failure_count: u32,
    pub last_failed: SystemTime,
    pub cooldown_until: Option<SystemTime>,
}

pub enum ProxyDestinationFailureReason {
    ConnectRefused,
    TlsFailed,
    BlockedByOrigin,
    Timeout,
    Other,
}
```

`host` doit accepter une IPv4, une IPv6 ou un nom d'hote. Le couple
`host`/`port` est ensuite normalise en endpoint compatible `ProxyNode`, avec
crochets pour IPv6.

`protocol` est le champ unique de protocole. Si la source le fournit, il est
considere correct. Si elle ne le fournit pas, le proxy peut tenter de le
determiner pendant le probe et renseigner ce meme champ avant de convertir le
candidat en `ProxyNode`.

`supports_https` est une donnee optionnelle que la source peut fournir au proxy.
Quand elle vaut `Some(false)`, le candidat ne doit pas etre choisi pour une
destination HTTPS. Quand elle vaut `Some(true)`, il peut etre accepte comme
compatible HTTPS selon la politique de confiance ou etre reverifie si le probe
strict est active. Quand elle est absente, le probe peut la mesurer.

`last_checked` est l'unique date portee par `ProxyRecord`. Elle represente le
dernier probe effectue, qu'il soit positif ou negatif. Les dates de chargement
de source ne doivent pas etre dupliquees dans chaque record ; elles relevent de
l'inventaire, du registre de sources ou des metadonnees du fichier persiste.

`authentication_required` sert a exclure les proxies qui demandent une
authentification que l'inventaire dynamique ne peut pas fournir. Ces proxies
peuvent rester stockes pour diagnostic, mais ils ne doivent pas etre
selectionnes dans un pool public.

`availability` est uniquement un indice annonce par la source ou derive de sa
forme de sortie, par exemple `Low`, `Medium`, `High` ou `Unknown`. Il ne doit
pas remplacer `status`, qui reste le resultat runtime du probe. En selection,
cet indice peut departager deux candidats deja fonctionnels, mais il ne doit
jamais rendre admissible un proxy KO, non teste en mode strict ou demandant une
authentification.

`destination_failures` stocke des echecs specifiques a une destination reelle,
par exemple un proxy qui fonctionne globalement mais qui est bloque ou inutilisable
pour `https://www.tf1.fr:443`. Cette donnee ne doit pas contenir l'URL complete :
on persiste seulement `scheme`, `host` et `port`, afin d'eviter de stocker des
chemins, query strings, tokens ou identifiants applicatifs. Ces echecs servent a
eviter de reessayer immediatement le meme proxy pour la meme origine, sans
condamner le proxy pour toutes les autres destinations.

La liste doit rester courte. La limite recommandee est `10` echecs destination
actifs, deduplices par origine. Quand un proxy atteint ce seuil, il doit etre
considere comme probablement defaillant globalement : l'inventaire le passe en
`status = Ko`, incremente son `failure_count`, applique le cooldown global, puis
vide `destination_failures` avant persistance pour eviter de conserver une liste
inutile en memoire ou sur disque.

`ProxyRecord` represente la donnee source, runtime et persistable.
`ProxyNode` represente ce que le core sait utiliser pour ouvrir une connexion.
La transformation `ProxyRecord -> ProxyNode` ne devrait arriver qu'apres
normalisation, resolution d'un `protocol` concret et validation des criteres de
selection.

### Trait de chargement

Le trait doit etre defini dans `arachnea-proxy`, car c'est le proxy qui exprime
son besoin. `arachnea-scrapyfy` pourra ensuite implementer ce trait pour un type
local.

Exemple conceptuel :

```rust
#[async_trait]
pub trait ProxyDataProvider: Send + Sync {
    async fn load_proxies(
        &self,
        request: ProxyLoadRequest,
    ) -> Result<Vec<ProxyRecord>>;
}

pub struct ProxyLoadRequest {
    pub country: Option<String>,
}
```

Ce trait ne doit pas exposer `scrapyfy` dans sa signature. Il parle uniquement
de donnees proxy generiques. Les notions de limite, de besoin HTTPS, de TTL ou
de rafraichissement force ne doivent pas etre dans `ProxyLoadRequest` : elles
relevent de l'inventaire proxy, de la selection ou de la configuration runtime.
La decision de recharger une source ou d'utiliser le cache est mieux placee dans
`ProxyInventory`, qui connait les dates, cooldowns et echecs recents.

### Implementation scrapyfy

Dans `arachnea-scrapyfy`, le service de recuperation doit utiliser le moteur
existant et devenir le chemin par defaut de gestion des proxies :

- une ou plusieurs definitions YAML pour les sources de proxies ;
- une normalisation des champs vers le format de `ProxyRecord` :
  `protocol`, `host` ou `ip`, `port`, `country`, `supports_https`,
  `availability`, `source` ;
- une implementation `ProxyDataProvider` activee derriere la feature
  `arachnea-proxy`.

Il faut eviter de coder des particularites de sites de listes proxy en Rust. La
mise en forme source-specifique doit rester dans les YAML/actions, comme pour
les autres scrapers.

### Composition par defaut dans arachnea-scrapyfy

`arachnea-scrapyfy` doit prendre en charge le cablage par defaut :

```text
arachnea-scrapyfy
  -> charge les sources YAML proxy
  -> construit ScrapyfyProxyDataProvider
  -> expose un proxy-aware HttpClient / SharedProxyConfigHandle configure
     avec le provider dynamique
  -> fournit le core ou la configuration proxy prete a l'application
```

`arachnea-stream` reste libre d'injecter une configuration applicative ou un
core deja construit, mais cela devient un override. Le chemin nominal doit etre
que `arachnea-scrapyfy` sache initialiser la gestion proxy sans que
`arachnea-stream` ait a declarer les proxies par pays dans son `main`.

Ce choix garde les crates dans le bon sens :

```text
arachnea-proxy   <- trait et logique proxy
arachnea-scrapyfy -> implementation du trait et gestion proxy par defaut
arachnea-stream   -> configuration applicative optionnelle
```

## Integration dans le coeur proxy

La decision retenue est l'option B : inventaire dynamique au niveau des pools.
L'option C reste a envisager en phase 2 comme complement, pour precharger ou
rafraichir les donnees en arriere-plan sans remplacer le chargement a la
demande.

### Option B retenue - Inventaire dynamique au niveau des pools

Le handler pays retourne une intention de routage, par exemple un pool logique
`country:FR`. La resolution des pools regarde ensuite l'inventaire runtime :

```text
country=FR
  -> handler pays ajoute un marqueur de pool "country:FR"
  -> resolution de pool
  -> cherche des membres FR fonctionnels dans l'inventaire
  -> si absent ou epuise, charge via ProxyDataProvider
  -> teste ou met a jour les ProxyRecord
  -> selectionne le meilleur ProxyNode utilisable
```

Avantages :

- reutilise le concept existant de `EgressPool` ;
- garde le test et la selection la ou ils existent deja ;
- permet de charger quand le pool est vide ou epuise ;
- evite de rendre `ProxyParameterHandler` async ;
- isole la logique dynamique dans un `ProxyInventory` / `DynamicProxyPool`.

Inconvenients :

- demande d'ajouter une source de pools runtime en plus des pools statiques de
  `ProxyConfig` ;
- il faut traiter proprement les verrous par pays pour eviter que plusieurs
  requetes declenchent le meme chargement en parallele.

C'est le chemin a mettre en oeuvre en premiere phase.

### Coexistence avec les pools statiques

Les pools declares dans `ProxyConfig` doivent conserver leur comportement actuel.
Le dynamique ne doit donc ni fusionner ni remplacer implicitement un pool statique
qui porte le meme pays.

Regle recommandee pour la v1 :

- les pools statiques restent des routes explicites de configuration ;
- les pools dynamiques vivent dans un espace logique separe, par exemple
  `dynamic-country:FR`, meme s'ils representent le meme pays qu'un pool statique ;
- si le handler statique actuel mappe `FR` vers un `ProxyNode` ou un `ProxyPool`,
  cette route garde sa semantique actuelle ;
- si le handler dynamique est utilise pour `FR`, il consulte l'inventaire
  dynamique et ne consomme pas automatiquement les membres statiques ;
- si une configuration active deux handlers capables de repondre au meme
  parametre pays, elle doit etre consideree ambigue sauf si une politique
  explicite est fournie.

Les politiques explicites possibles sont :

```text
static_only         -> comportement actuel, aucun chargement dynamique
dynamic_only        -> inventaire dynamique seul
static_then_dynamic -> essayer le pool statique, puis charger dynamiquement si
                       le pool statique est vide ou inutilisable
dynamic_then_static -> essayer le dynamique, puis le statique comme secours
```

Le defaut recommande pour le cablage fourni par `arachnea-scrapyfy` est
`dynamic_only`. Le defaut recommande pour une configuration applicative existante
reste `static_only`, afin de preserver le comportement actuel. Les modes mixtes
doivent etre opt-in, car ils changent le contrat de routage et rendent le debug
plus difficile si la bascule n'est pas visible dans la configuration.

### Option C phase 2 - Rafraichissement de fond

Un service charge et teste les proxies periodiquement, puis le proxy ne fait que
consommer l'inventaire local.

Avantages :

- peu intrusif dans le chemin de connexion ;
- latence de requete plus stable.

Inconvenients :

- ne repond pas completement au cas "charger quand aucun proxy fonctionnel n'est
  disponible" ;
- peut charger trop de donnees inutiles ;
- necessite une orchestration de taches de fond.

Cette option doit etre consideree comme un complement de phase 2 : elle peut
precharger des pays frequents, retester les proxies OK avant expiration, ou
nettoyer les KO en cooldown. Elle ne suffit pas seule, car le systeme doit
encore pouvoir charger a la demande quand un pays est demande et que le pool
local est vide ou inutilisable.

## Test d'un proxy

La verification ne doit pas produire une structure doublon de type
`ProxyProbeReport`. Elle doit prendre un `ProxyRecord`, tester ce qui manque ou
ce qui a expire, puis retourner ou persister le meme `ProxyRecord` mis a jour.
Un probe n'est donc pas seulement une lecture : il enrichit la fiche proxy pour
eviter de repeter les memes detections lors des connexions suivantes.

### Ce qui existe deja

Le test des membres de `EgressPool` verifie deja une compatibilite minimale :

- pour un proxy HTTP/HTTPS en mode HTTP forward, il teste la connexion au proxy ;
- pour les autres cas, il ouvre une chaine vers la destination cible ;
- les echecs marquent le membre `Ko`.

### Ce qu'il faut ajouter

Il faut une API de probe explicite, reutilisable par l'inventaire et par une
future commande d'administration :

1. mesurer le temps de connexion TCP au proxy ;
2. verifier le handshake du protocole proxy ;
3. verifier la capacite a joindre une destination HTTP simple si besoin ;
4. verifier la capacite HTTPS en ouvrant un tunnel vers une cible HTTPS de test
   et en faisant au minimum un handshake TLS ou une requete `HEAD`/`GET` courte ;
5. detecter si le proxy exige une authentification ;
6. mettre a jour le `ProxyRecord` avec les donnees mesurees ;
7. convertir uniquement les records valides en `ProxyNode` selectionnable.

Si `protocol` est deja present sur le candidat, le probe l'utilise tel quel.
Le test confirme alors la disponibilite du proxy pour ce protocole, mais ne
cherche pas a le remplacer par un autre protocole. Si `protocol` est absent, le
probe peut essayer plusieurs protocoles et renseigner le champ unique
`protocol` avec le premier protocole valide selon la strategie configuree.

Si `supports_https` est deja present, il sert de hint initial pour la selection.
La politique peut soit le croire, soit le reverifier quand le candidat est
ancien ou quand la destination impose une verification stricte.

Apres chaque probe, l'inventaire doit mettre a jour la fiche proxy concernee :

- `protocol` si le protocole etait absent et a ete resolu ;
- `supports_https` si le probe l'a mesure ou revalide ;
- le statut de disponibilite ;
- les latences mesurees ;
- `authentication_required` et `status = AuthenticationRequired` si le proxy
  refuse les connexions sans authentification ;
- `last_checked`, meme quand le resultat est un echec utile a la selection.

La detection d'authentification doit exclure le proxy de la selection dynamique :

- HTTP/HTTPS proxy : reponse `407 Proxy Authentication Required` ou challenge
  proxy equivalent ;
- SOCKS5 : absence de methode no-auth acceptee ou demande explicite
  username/password ;
- SOCKS4/SOCKS4a : rejet lie au champ utilisateur ou a une politique
  d'authentification upstream.

Ces proxies peuvent rester dans le store avec leur statut pour eviter de les
retenter immediatement. La v1 ne prend pas en charge l'authentification des
proxies dynamiques : tout proxy qui demande des credentials est ignore par la
selection.

Le support HTTPS doit etre interprete selon le type de proxy :

- HTTP proxy : support HTTPS = `CONNECT host:443` fonctionne ;
- HTTPS proxy : TLS vers le proxy puis `CONNECT host:443` fonctionne ;
- SOCKS4/SOCKS4a/SOCKS5 : tunnel TCP vers `host:443` fonctionne, puis TLS vers
  la cible fonctionne ;
- HTTP forward-only : fonctionnel pour HTTP, mais pas acceptable pour une
  requete HTTPS.

La cible de test doit etre configurable, par exemple :

```text
https_probe_url = "https://example.com/"
http_probe_url = "http://example.com/"
timeout_ms = 5000
```

Un probe HTTPS complet a besoin d'une cible distante. Ouvrir seulement une
connexion TCP vers le proxy prouve que le proxy est joignable, mais ne prouve
pas que `CONNECT` est accepte ni que le tunnel HTTPS fonctionne. On peut envoyer
une requete `CONNECT host:443` pour verifier la syntaxe, mais la reponse utile
depend encore d'une destination reelle : politique du proxy, DNS, port autorise,
authentification et joignabilite de la cible.

Le contrat recommande est donc :

- `http_probe_url` et `https_probe_url` sont configurables ;
- les exemples et presets de developpement peuvent utiliser `http://example.com/`
  et `https://example.com/` pour demarrer vite ;
- le core ne doit pas coder une URL externe unique en dur ;
- en mode strict ou production, l'URL HTTPS doit etre fournie explicitement si
  l'on veut certifier `supports_https = true` par probe ;
- si `https_probe_url` est absente, le probe peut encore marquer un proxy comme
  utilisable pour HTTP, mais il doit laisser `supports_https` a `None` ou
  conserver uniquement une valeur fiable fournie par la source ;
- une destination HTTPS ne doit pas selectionner un record dont
  `supports_https` est absent en mode strict ;
- l'endpoint de probe doit etre stable, petit, sans authentification, sans
  cookie requis, compatible `HEAD` ou `GET` court, et avec peu ou pas de
  redirections.

Pour les builds de production, la recommandation est d'utiliser un endpoint
controle par l'application ou l'operateur plutot qu'un domaine public implicite.
Les valeurs `example.com` restent acceptables comme confort local, mais elles
introduisent une dependance externe qui doit etre visible dans la configuration.

### Probe hors contexte et validation par destination

Il faut distinguer deux usages :

```text
refresh ou import de liste -> probe explicite configure
connexion runtime          -> validation contre la destination demandee
```

Quand une commande ou un executable recupere une liste de proxies hors contexte
applicatif, elle ne dispose pas d'une destination utilisateur. Elle doit donc
recevoir explicitement les URLs de probe HTTP/HTTPS a appeler. En mode strict,
ces URLs sont obligatoires pour certifier les capacites mesurees pendant le
refresh.

Quand le proxy traite une vraie requete, par exemple une connexion avec
`country=FR` vers `https://www.tf1.fr/...`, il est pertinent d'utiliser cette
destination comme validation specifique. Cela permet d'exclure temporairement un
proxy qui fonctionne sur une cible generique mais qui est bloque, filtre ou
blackliste par l'origine reelle.

Cette validation runtime ne doit pas remplacer le probe generique :

- le probe generique etablit que le proxy est vivant, parle le protocole attendu
  et sait faire HTTP/HTTPS ;
- la validation destination etablit que ce proxy est utilisable pour cette
  origine precise, dans le contexte de la requete courante.

Un echec destination doit etre classe prudemment :

- proxy injoignable, handshake proxy impossible ou authentification requise :
  echec global, le `ProxyRecord` peut devenir `Ko` ou
  `AuthenticationRequired` ;
- `CONNECT target:443` refuse, handshake TLS vers la cible impossible, timeout
  vers cette origine ou reponse applicative indiquant un blocage probable :
  echec pour cette destination, avec cooldown par `scheme/host/port` ;
- statut HTTP applicatif comme `403`, `451` ou page anti-bot : echec destination
  seulement si la politique du service ou du resolver le classe explicitement
  comme blocage proxy.

La validation ne doit pas forcement faire une requete de probe supplementaire
avant la vraie requete. Pour HTTPS, le minimum utile est `CONNECT target:443`
puis handshake TLS. Pour HTTP ou pour verifier une reponse applicative, la vraie
requete peut servir de validation. Si elle echoue pour une raison classifiee
comme proxy/destination, l'inventaire marque le couple proxy+origine en cooldown
et peut reessayer avec un autre proxy.

Les reessais doivent rester bornes. Les methodes idempotentes (`GET`, `HEAD`)
peuvent etre reessayees a travers un autre proxy selon la politique de routage.
Les methodes non idempotentes (`POST`, `PUT`, etc.) ne doivent pas etre rejouees
automatiquement sans opt-in explicite de l'appelant.

## Protocole optionnel

Rendre le protocole optionnel est possible, mais uniquement dans les donnees
brutes. Le protocole ne devrait pas rester optionnel au moment d'ouvrir une
connexion. Le modele doit garder uniquement `protocol`.

### Strategie de resolution

Quand la source fournit `protocol`, cette valeur est consideree correcte et
utilisee pour construire le test et le futur `ProxyNode`.

Quand une source fournit seulement `host:port`, le probe peut essayer plusieurs
protocoles et remplir `protocol` avec le protocole retenu :

1. utiliser un indice de port si disponible ;
2. essayer HTTP proxy clair ;
3. essayer SOCKS5 ;
4. essayer SOCKS4a/SOCKS4 si pertinent ;
5. essayer HTTPS proxy si le port ou la source le suggere.

Les ports peuvent aider, mais ne doivent pas etre consideres comme fiables :

| Port courant | Hypothese utile | Fiabilite |
|---|---|---|
| 80, 8080, 3128 | HTTP proxy | moyenne |
| 1080, 1081 | SOCKS | moyenne |
| 443 | HTTPS proxy ou HTTP proxy atypique | faible |

### Risques

- certains proxies HTTP acceptent les requetes HTTP mais refusent `CONNECT` ;
- certains serveurs ferment silencieusement les handshakes inconnus ;
- les tests multiples peuvent etre lents ;
- un proxy malveillant peut repondre de maniere trompeuse ;
- "HTTPS proxy" peut signifier deux choses differentes : proxy HTTP transporte
  dans TLS, ou proxy HTTP capable de tunneler HTTPS via `CONNECT`.

Conclusion : le protocole peut etre optionnel dans `ProxyRecord`, mais un
candidat selectionnable doit avoir un `protocol` concret. Si la source fournit
le champ, on le conserve. Si elle ne le fournit pas, le probe peut le renseigner
dans ce meme champ. `ProxyNode` reste concret dans tous les cas.

## Persistance

La persistance est geree dans `arachnea-proxy`, car elle fait partie de
l'inventaire et de la selection proxy. `arachnea-scrapyfy` fournit les donnees,
mais ne doit pas posseder le stockage runtime des proxies.

Le format persiste doit etre aligne sur `ProxyRecord`, sans schema JSON separe.
Il faut utiliser `serde` pour serialiser/deserialiser directement la structure
canonique, afin d'eviter les conversions inutiles.

La lecture/ecriture doit etre isolee derriere un trait dans un fichier dedie,
par exemple `arachnea-proxy/src/core/proxy_store.rs`. Le store fichier doit etre
genericise autour de `serde` : le JSON est le format par defaut, mais le meme
store doit pouvoir recevoir un codec YAML ou autre plus tard.

Exemple conceptuel :

```rust
#[async_trait]
pub trait ProxyStore: Send + Sync {
    async fn load_proxies(&self) -> Result<Vec<ProxyRecord>>;
    async fn save_proxies(&self, proxies: &[ProxyRecord]) -> Result<()>;
}

pub trait ProxySerdeCodec: Send + Sync {
    fn format_name(&self) -> &'static str;
    fn serialize_proxies(&self, proxies: &[ProxyRecord]) -> Result<Vec<u8>>;
    fn deserialize_proxies(&self, bytes: &[u8]) -> Result<Vec<ProxyRecord>>;
}

pub struct ProxySerdeStore {
    path: PathBuf,
    codec: Arc<dyn ProxySerdeCodec>,
}
```

Le constructeur de `ProxySerdeStore` doit exiger explicitement un codec. Une
fonction de confort peut fournir `JsonProxyCodec` par defaut, mais le store ne
doit pas etre lie structurellement au JSON.

### Format recommande au depart

Le workspace n'a pas aujourd'hui de dependance base de donnees embarquee pour ce
besoin. Une persistance fichier JSON est donc le meilleur premier palier :

```text
data/proxies.json
```

avec ecriture atomique :

```text
proxies.json.tmp -> fsync si necessaire -> rename proxies.json
```

Schema conceptuel : le fichier est la serialisation generique d'un
`Vec<ProxyRecord>` via `serde`.

```json
[
  {
    "protocol": "http",
    "host": "1.2.3.4",
    "port": 8080,
    "country": "FR",
    "supports_https": true,
    "status": "ok",
    "latency_ms": 320,
    "failure_count": 0,
    "authentication_required": false,
    "availability": "unknown",
    "destination_failures": [],
    "source": "my-proxy-source",
    "last_checked": "2026-07-02T10:05:00Z",
    "cooldown_until": null
  }
]
```

Si un versionnement devient necessaire, il pourra etre ajoute plus tard via une
structure de snapshot explicite, toujours serialisee par `serde`, sans convertir
les records vers un autre format manuel.

### Donnees a persister via ProxyRecord

- endpoint normalise : host, port, avec support IPv4, IPv6 et hostname ;
- protocole unique `protocol` ;
- pays declare optionnel ;
- disponibilite annoncee par la source ;
- statut probe : unknown, ok, ko, authentication-required ;
- latence ;
- support HTTPS optionnel ou mesure ;
- authentification requise, quand detectee ;
- date unique `last_checked` ;
- compteur d'echecs ;
- `cooldown_until` pour eviter de retester en boucle un proxy mort ;
- echecs par destination, limites a `scheme`, `host`, `port`, raison, compteur
  et cooldown ;
- source d'origine pour faciliter le debug.

### TTL et revalidation

Il faut separer :

- cadence de rechargement des sources, suivie au niveau de l'inventaire ou du
  registre de sources, pas dans chaque `ProxyRecord` ;
- TTL de probe base sur `last_checked` : quand retester un proxy ;
- cooldown d'echec : quand un proxy KO peut etre retente.
- cooldown par destination : quand un proxy globalement OK peut etre retente
  pour une origine precise apres un blocage ou timeout specifique.
- seuil des echecs destination : quand trop d'origines distinctes echouent pour
  le meme proxy, le proxy devient KO globalement et la liste est videe.

Exemple de politique :

```text
source_ttl = 30 min
ok_probe_ttl = 10 min
ko_cooldown = 15 min
destination_failure_cooldown = 5 min
max_failure_count_before_drop = 3
max_destination_failures_before_ko = 10
```

### SQLite plus tard

SQLite deviendra interessant si :

- les listes deviennent volumineuses ;
- plusieurs taches ecrivent souvent ;
- on veut des requetes plus fines par pays/protocole/latence ;
- on veut conserver un historique de mesures.

Ce n'est pas necessaire pour la premiere version.

## Pays inconnu

Pour les pays inconnus, le mecanisme retenu doit etre similaire au chargement
des proxies :

1. `arachnea-scrapyfy` recupere des donnees IP -> pays dans un format
   normalise.
2. `arachnea-proxy` definit les structures, les traits et la lecture du store.
3. `arachnea-scrapyfy` expose la fonction principale de rafraichissement, car
   elle possede l'implementation concrete du provider scrapyfy.
4. Si l'adresse IP n'est pas dans la liste, ou si la liste est vide, cette
   fonction declenche la recuperation via un trait provider.
5. Les donnees recuperees sont stockees via un trait de persistance.
6. Le meme trait sert ensuite a relire les donnees.

Si une donnee pays est disponible, elle est consideree correcte. La suppression
manuelle du fichier JSON par l'utilisateur doit permettre de forcer une mise a
jour complete des donnees au prochain besoin.

### Format IP -> pays

Exemple conceptuel :

```rust
pub struct IpCountryRecord {
    pub ip: IpAddr,
    pub country: String,
    pub source: Option<String>,
}
```

Pour des ranges CIDR, on pourra ajouter plus tard une structure dediee au lieu
de surcharger la forme IP simple.

### Provider et store

Le provider est defini cote `arachnea-proxy`, implemente cote
`arachnea-scrapyfy` :

```rust
#[async_trait]
pub trait IpCountryDataProvider: Send + Sync {
    async fn load_ip_countries(&self) -> Result<Vec<IpCountryRecord>>;
}
```

La persistance est geree cote `arachnea-proxy`, comme pour les proxies :

```rust
#[async_trait]
pub trait IpCountryStore: Send + Sync {
    async fn load_ip_countries(&self) -> Result<Vec<IpCountryRecord>>;
    async fn save_ip_countries(&self, entries: &[IpCountryRecord]) -> Result<()>;
}
```

Un premier backend fichier suffit, avec JSON comme codec par defaut :

```text
data/ip-countries.json
```

Comme pour les proxies, ce fichier doit pouvoir etre une serialisation directe
de `Vec<IpCountryRecord>` via le store `ProxySerdeStore` ou son equivalent
generique pour donnees IP -> pays. La suppression du fichier par l'utilisateur
force ainsi naturellement un rechargement au prochain pays inconnu.

La fonction principale cote `arachnea-scrapyfy`, par exemple
`refresh_ip_country_store`, doit exister sous deux formes :

- une fonction Rust appelable par la composition runtime ;
- une commande utilisateur permettant de forcer le rafraichissement.

La commande utilisateur recommandee n'est pas un binaire separe dans
`arachnea-scrapyfy` en v1. `arachnea-scrapyfy` doit posseder l'implementation et
l'API Rust, mais l'exposition CLI doit plutot etre une sous-commande du binaire
applicatif ou d'un futur binaire d'administration, par exemple :

```text
arachnea proxy refresh-ip-countries
```

Ce choix evite de transformer la crate de scraping en executable public, tout
en gardant la logique concrete du provider cote `arachnea-scrapyfy`. Un petit
binaire crate-local peut rester possible plus tard pour le debug, mais il ne
doit pas etre le contrat utilisateur principal.

Dans les deux cas, elle reconstruit entierement le fichier :

```text
load_ip_countries() via IpCountryDataProvider
  -> remplacer l'ensemble des entrees en memoire
  -> ecrire data/ip-countries.json.tmp via Serde
  -> fsync si necessaire
  -> rename atomique vers data/ip-countries.json
```

Le contenu precedent est donc logiquement supprime et recree a chaque execution,
mais l'original reste intact si le processus plante avant le `rename`.

### Flux de resolution

```text
resolve_country(ip)
  -> lire IpCountryStore
  -> si ip trouvee : retourner country
  -> si liste vide ou ip absente :
       prendre un verrou de refresh IP -> pays
       appeler le refresh principal fourni par arachnea-scrapyfy
       relire/rechercher ip
  -> si toujours absent : country inconnu
```

Pour une selection de proxy qui exige un pays strict, le premier miss IP -> pays
doit etre synchrone et borne : la resolution attend le refresh une seule fois,
avec timeout et verrou global ou par source, car elle ne peut pas decider si un
record sans pays est admissible tant que cette information manque. Si le refresh
echoue, depasse le timeout ou ne contient toujours pas l'IP, le pays reste
inconnu et le record n'est pas admissible pour une demande `country=FR`.

Le rafraichissement de fond reste utile en phase 2, mais il ne remplace pas ce
chemin synchrone borne. Il peut precharger les donnees ou les renouveler avant
expiration ; il ne doit pas laisser une requete stricte utiliser provisoirement
un proxy au pays inconnu.

## Chargement si aucun proxy fonctionnel n'est disponible

Le comportement cible doit etre deterministe et borne.

Flux recommande :

```text
1. Requete avec country=FR.
2. Le routage demande une sortie "country:FR".
3. L'inventaire cherche un proxy FR avec :
   - status ok ;
   - support HTTPS si la destination est HTTPS ;
   - pas d'authentification requise ;
   - probe pas expire ;
   - pas en cooldown global ;
   - pas en cooldown pour la destination demandee.
4. Si trouve : selection du meilleur candidat.
5. Si absent :
   - prendre un verrou de chargement par pays ;
   - memoriser les endpoints deja juges inutilisables pour cette resolution ;
   - recharger depuis ProxyDataProvider(country=FR) ;
   - normaliser/dedupliquer sans effacer les etats KO/auth/cooldown existants ;
   - tester un lot borne de candidats nouveaux ou reeligibles ;
   - persister les resultats ;
   - selectionner le meilleur candidat OK.
6. Pendant la connexion reelle :
   - valider le proxy contre la destination demandee ;
   - si l'echec est global, marquer le proxy KO/auth selon le cas ;
   - si l'echec est specifique a la destination, ajouter un cooldown
     `scheme/host/port` ;
   - essayer un autre candidat seulement si la methode et la politique de
     requete autorisent le retry.
7. Si aucun candidat OK :
   - retourner RouteUnavailable ;
   - ne pas basculer en direct sauf configuration explicite.
```

Le verrou par pays evite que dix requetes simultanees FR declenchent dix
chargements de liste. Un cache negatif court est aussi utile :

```text
country FR charge a 10:00, aucun proxy OK
-> ne pas recharger avant 10:02 sauf refresh force
```

Il faut aussi eviter une boucle infinie sur les memes serveurs. Un rechargement
ne doit pas reinitialiser les records connus comme inutilisables. La fusion doit
preserver `status = Ko`, `status = AuthenticationRequired`, `failure_count`,
`cooldown_until` et `last_checked` pour un meme endpoint normalise. Si la source
renvoie exactement les memes proxies et qu'aucun n'est devenu reeligible, la
resolution doit retourner `RouteUnavailable` au lieu de relancer un nouveau
cycle de chargement/probe.

Les echecs par destination doivent etre deduplices par endpoint proxy normalise
et par origine cible. Ils ne doivent pas effacer l'etat global du proxy : un
proxy peut rester `status = Ok` tout en etant temporairement exclu pour
`https://www.tf1.fr:443`.

En revanche, si la liste d'echecs destination actifs atteint le seuil configure,
par defaut `10`, l'echec n'est plus traite comme anecdotique par origine. Le
proxy doit etre marque `Ko` globalement, son `failure_count` doit etre incremente,
son `cooldown_until` global doit etre renseigne, puis `destination_failures` doit
etre vide pour limiter la consommation memoire/disque.

### Selection du meilleur candidat

Dans un pool, la selection doit d'abord filtrer les records non admissibles,
notamment `authentication_required == Some(true)`, puis prendre le proxy
admissible avec la latence la plus faible. Le tri recommande est donc :

1. statut OK ;
2. support HTTPS si requis ;
3. pays declare correspondant ;
4. aucune exclusion active pour l'origine demandee ;
5. latence la plus faible ;
6. moins d'echecs recents ;
7. ordre stable pour garder un comportement reproductible.

La latence est le critere principal entre proxies equivalemment admissibles dans
un meme pool. Un record sans latence recente peut etre teste avant selection ou
classe apres les records OK avec latence connue, selon la politique du pool.

Le pays est strict : pour une demande `country=FR`, le record doit correspondre
a `FR`. Si `country` est present, il est considere correct et n'est pas
reverifie. Si le pays est absent, la resolution IP -> pays peut etre tentee ;
si elle ne donne pas `FR`, le record n'est pas admissible.

## Configuration possible

Une configuration future pourrait ressembler a ceci :

```toml
[core.dynamic_proxies]
enabled = true
persist_path = "data/proxies.json"
persist_format = "json"
sources_registry = "server/services/arachnea-proxies/services.json"
strict_country = true
source_ttl_seconds = 1800
probe_ttl_seconds = 600
failure_cooldown_seconds = 900
destination_failure_cooldown_seconds = 300
load_batch_size = 50
probe_batch_size = 8
probe_mode = "strict"
require_explicit_probe_urls = true
runtime_destination_validation = true
max_runtime_proxy_retries = 2
max_destination_failures_before_ko = 10
https_probe_url = "https://example.com/"
http_probe_url = "http://example.com/"
coexistence_policy = "dynamic_only"

[[core.parameter_handlers]]
kind = "country_routing"
parameter_name = "country"
http_header = "Arachnea-Proxy-Country"
forward_header = false
stop_on_match = true
dynamic_pool = true
```

Ou, si l'on veut eviter de modifier le handler existant :

```toml
[[core.parameter_handlers]]
kind = "dynamic_country_routing"
parameter_name = "country"
http_header = "Arachnea-Proxy-Country"
```

La deuxieme forme evite de changer la semantique du handler statique actuel.
Les URLs `example.com` ci-dessus sont des placeholders acceptables pour un
preset de developpement ; en production, `require_explicit_probe_urls = true`
devrait forcer l'operateur a declarer ses propres endpoints de probe ou a
assumer explicitement une dependance publique.

## Service scrapyfy de recuperation de proxies

Le service cote `arachnea-scrapyfy` doit produire une liste normalisee alignee
sur `ProxyRecord`. Les YAML de sources proxy doivent donc viser directement
ces champs, sans format intermediaire propre a `arachnea-stream`.

Exemple de sortie attendue apres execution du YAML :

```yaml
- protocol: http
  host: 1.2.3.4
  port: 8080
  country: FR
  supports_https: true
  availability: unknown
  source: my-proxy-source
```

Champs minimum :

- `protocol`: optionnel ;
- `ip` ou `host`: obligatoire, compatible IPv4, IPv6 ou nom d'hote ;
- `port`: obligatoire ;
- `country`: optionnel ;
- `supports_https`: optionnel ;
- `availability`: optionnel.

Une IPv6 doit pouvoir etre fournie comme adresse brute, par exemple
`"2001:db8::10"`. La transformation vers `ProxyNode` se charge ensuite de
former l'authority `[2001:db8::10]:8080`.

Le service peut etre une abstraction Rust generique au-dessus des query
collections existantes :

```text
ScrapyfyProxyDataProvider
  -> execute une query de service proxy
  -> lit les items resultants
  -> convertit vers ProxyRecord
  -> fournit ces candidats au ProxyInventory par defaut de scrapyfy
```

Les sources de proxies doivent vivre dans un espace dedie :

```text
server/services/arachnea-proxies/services.json
server/services/arachnea-proxies/*.yaml
```

Le fichier `services.json` sert de registre et reference les YAML de sources
proxy. Ces YAML ne sont pas des sources media : ils produisent des `ProxyRecord`
normalises.

### Source implementee pour la phase 5

La phase 5 utilise maintenant une source publique declaree en YAML, traitee par
un provider Rust cote `arachnea-scrapyfy` derriere la feature
`arachnea-proxy`.

#### Source JSON `proxifly/free-proxy-list` ✅

Source :

```text
https://github.com/proxifly/free-proxy-list
```

URL brute par pays :

```text
https://raw.githubusercontent.com/proxifly/free-proxy-list/refs/heads/main/proxies/countries/{country}/data.json
```

Registre et YAML :

```text
server/services/arachnea-proxies/services.json
server/services/arachnea-proxies/proxifly.yaml
```

Le YAML declare la query `list_proxies_for_country`, utilise
`row_pointer: "/*"` et `result_item_field: proxies` pour convertir chaque
element racine du tableau JSON en ligne de resultat.

Mapping vers `ProxyRecord` :

- `protocol` vient de `/protocol` ;
- `host` vient de `/ip` ;
- `port` vient de `/port` ;
- `country` vient de `/geolocation/country`, puis est normalise en majuscules
  cote provider ;
- `supports_https` vient de `/https` ;
- `latency_ms = None` avant probe ;
- `availability = Unknown` ;
- les champs runtime restent initialises par defaut avant probe.

Le provider filtre ensuite les resultats sur le pays demande et deduplique les
records charges par authority normalisee.

### Sources candidates futures

#### Source texte `iplocate/free-proxy-list`

Source utilisateur :

```text
https://github.com/iplocate/free-proxy-list/blob/main/all-proxies.txt
```

L'URL GitHub `blob` pointe vers une page HTML. Pour le chargement automatique,
le provider doit utiliser l'URL brute equivalente :

```text
https://raw.githubusercontent.com/iplocate/free-proxy-list/main/all-proxies.txt
```

Le format est une liste texte, un proxy par ligne :

```text
socks5://57.129.123.224:30774
socks5://212.77.75.25:1088
socks4://184.181.217.194:4145
http://163.172.53.142:80
```

Mapping vers `ProxyRecord` :

- `protocol` vient du schema (`http`, `https`, `socks4`, `socks5`) ;
- `host` et `port` viennent de l'URL ;
- `country = None`, car cette source ne fournit pas de pays fiable ;
- `supports_https = None`, car la source ne certifie pas le tunnel HTTPS ;
- `latency_ms = None` avant probe ;
- `availability = Unknown` ;
- `source = "iplocate/free-proxy-list"`.

Important : le provider ne doit pas tagger artificiellement les records avec le
pays demande dans `ProxyLoadRequest.country`. Pour cette source, le pays reste
inconnu jusqu'a une future resolution IP -> pays. En selection stricte par pays,
ces records ne deviennent admissibles qu'apres l'etape 6 ou une autre source de
pays fiable.

#### Source JSON `vakhov/fresh-proxy-list`

Source :

```text
https://raw.githubusercontent.com/vakhov/fresh-proxy-list/refs/heads/master/proxylist.json
```

Le format est un tableau JSON d'objets :

```json
[
  {
    "host": "ip72-195-101-99.oc.oc.cox.net",
    "ip": "72.195.101.99",
    "port": "4145",
    "country_code": "US",
    "country_name": "United States",
    "delay": 1080,
    "checks_up": "3512",
    "checks_down": "498",
    "http": "0",
    "ssl": "0",
    "socks4": "1",
    "socks5": "0"
  }
]
```

Mapping vers `ProxyRecord` :

- `host` utilise prioritairement `host` quand il est present et non vide, sinon
  `ip` ;
- `port` est parse depuis la chaine `port` ;
- `country` utilise `country_code` quand il est present et non vide, normalise
  en majuscules ;
- `protocol` est derive des flags : `socks5 == "1"` -> `Socks5`, sinon
  `socks4 == "1"` -> `Socks4`, sinon `ssl == "1"` -> `Https`, sinon
  `http == "1"` -> `Http` ;
- si plusieurs flags protocole sont actifs, l'ordre recommande est
  `socks5`, `socks4`, `https`, `http` ;
- `supports_https = Some(true)` seulement si `ssl == "1"` ou si le protocole
  retenu est un protocole de tunnel qui sera confirme par probe ; sinon
  `None` de preference a `Some(false)` tant que le probe n'a pas mesure ;
- `latency_ms` peut etre initialisee depuis `delay` si la valeur est numerique ;
- `availability` peut rester `Unknown` en v1 ;
- `source = "vakhov/fresh-proxy-list"`.

Contrairement a la source texte, cette source fournit un `country_code`. Cette
donnee peut etre consideree comme le pays declare par la source et donc stockee
dans `ProxyRecord.country`. Elle n'est pas reverifiee en v1, conformement au
contrat general : un pays present est considere correct.

### Mode de pays pour la phase 5

Le cablage initial doit fonctionner avec `strict_country = false` pour permettre
l'utilisation de sources globales ou partiellement renseignees. Cela signifie :

- une demande sans pays peut utiliser les records globaux admissibles ;
- une demande avec pays peut utiliser les records dont `country` correspond ;
- les records `country = None` ne doivent pas etre faussement etiquetes avec le
  pays demande ;
- si une politique exige un pays strict, les records sans pays restent exclus
  tant que l'etape 6 IP -> pays n'est pas disponible.

## Securite et limites

Les proxies publics sont non fiables par definition. La documentation et la
configuration doivent rester claires :

- ne pas presenter ce systeme comme de l'anonymat ;
- ne pas router automatiquement du trafic sensible via une liste publique ;
- preferer HTTPS et verifier `supports_https` pour les medias et APIs ;
- refuser les destinations privees/locales via les politiques existantes ;
- ne pas logguer d'identifiants ou d'URLs sensibles associes aux proxies ;
- ne pas persister les URLs completes dans les echecs destination ; conserver
  seulement `scheme`, `host` et `port` ;
- garder le comportement de fallback direct explicite, pas implicite ;
- limiter la concurrence de probe pour eviter des scans agressifs ;
- respecter des timeouts courts et des cooldowns.

### Preference de protocole et profils

Il est pertinent de privilegier SOCKS5 lorsque plusieurs proxies sont
equivalents, surtout pour les profils qui veulent preserver les hostnames et
eviter le HTTP forward clair. Cette preference ne remplace pas les criteres
fonctionnels : un SOCKS5 lent ou KO ne doit pas battre un proxy HTTP fiable si
le profil courant privilegie la resilience ou la latence.

Les profils doivent influencer le scoring sans changer le format des records :

- `standard` ou usage media : filtrer `status = Ok`, `supports_https` si requis,
  pays strictement compatible, puis choisir la latence la plus faible.
- `resilience` : garder plusieurs fournisseurs/protocoles utilisables, eviter
  de s'enfermer dans un seul proxy, accepter un score un peu moins bon pour
  diversifier les sorties.
- `privacy` ou `anonymous` : preferer SOCKS5 ou Tor SOCKS, conserver les
  hostnames quand le protocole le permet, refuser le fallback direct implicite,
  eviter les proxies HTTP forward-only.
- `censorship_resistance` futur : combiner preference SOCKS5/Tor/tunnels,
  rafraichissement de fond et fallback controle, en documentant les limites du
  modele de menace.

Dans tous les profils, un proxy detecte comme demandant une authentification
non fournie reste exclu de la selection dynamique.

Un proxy HTTP simple peut lire et modifier le trafic HTTP clair. Pour HTTPS, le
proxy voit encore au minimum la destination du tunnel et la metadonnee de
connexion. Les cookies, comptes utilisateurs, empreintes TLS/applicatives et
timings restent des sources de fuite possibles.

## Plan de mise en oeuvre propose

### Etape 1 - Contrats et modele de donnees ✅

- Ajouter dans `arachnea-proxy` :
  - `ProxyRecord` → `proxy_record.rs` ;
  - `ProxyProtocol` → `proxy_record.rs` ;
  - `ProxyRuntimeStatus` → `proxy_record.rs` ;
  - `ProxyAvailabilityHint` → `proxy_record.rs` ;
  - `ProxyDestinationFailure` et `ProxyDestinationFailureReason` → `proxy_record.rs` ;
  - `ProxyLoadRequest` → `proxy_record.rs` ;
  - `ProxyDataProvider` (trait async) → `proxy_record.rs`.
- Conversion `ProxyRecord + protocol -> ProxyNode` avec normalisation IPv4, IPv6 et hostname.
- Sérialisation Serde + `SystemTime` encodé en millisecondes epoch.
- Module et ré-exports publics dans `core/mod.rs`.

Impact : API publique nouvelle, mais peu de changement comportemental.

### Etape 2 - Probe explicite ✅

- Nouvelle API `ProxyProbe` + `ProbeConfig` + `ProbeMode` dans `proxy_probe.rs`.
- Mesure latence TCP, test HTTP forward, HTTP CONNECT pour HTTPS,
  SOCKS5/SOCKS4 et tunnel HTTPS.
- Détection d'authentification (HTTP 407, SOCKS5 0xFF).
- Détection de protocole : itère `protocol_detection_order` quand `protocol` est absent.
- `ProbeConfig.http_probe_url` / `.https_probe_url` configurables.
- `validate_destination()` pour validation runtime sur destination réelle.
- Met à jour `ProxyRecord` in-place (status, protocol, latency_ms, supports_https, etc.).
- Module et ré-exports publics dans `core/mod.rs`.

Impact : base indispensable avant de charger des listes externes.

### Etape 3 - Inventaire runtime ✅

- Nouveau `ProxyInventory` + `InventoryConfig` + `CoexistencePolicy` dans `proxy_inventory.rs`.
- Stocke les records dans `HashMap<String, ProxyRecord>` indexée par authority, avec index pays.
- `select(country, require_https)` : filtre par status OK, pas en cooldown,
  pas d'auth requise, compatibilite HTTPS si demandee, puis tri par latence
  puis failure_count. Les proxies SOCKS restent admissibles pour HTTPS car ils
  tunnelisent TCP, meme si une source publique indique `supports_https = false`.
- `add_or_update()` : merge les records entrants sans écraser les champs runtime (status, latence, cooldown, destination_failures).
- Chargement lazy via `ProxyDataProvider` avec verrou par pays et cache négatif.
- `record_destination_failure()` : ajoute/met à jour un échec `scheme/host/port` ; au seuil de 10, marque KO global et vide la liste.
- `record_global_failure()`, `record_auth_required()`, `record_ok()` pour la mise à jour d'état.
- Module et ré-exports publics dans `core/mod.rs`.

Impact : coeur du comportement dynamique.

### Etape 4 - Persistance fichier ✅

- Nouveau `ProxyStore`, `ProxySerdeCodec`, `ProxySerdeStore` et
  `JsonProxyCodec` dans `proxy_store.rs`.
- Serialisation/deserialisation directe de `Vec<ProxyRecord>` via `serde`, avec
  JSON comme codec par defaut.
- `ProxySerdeStore` exige explicitement un codec et expose `path()` / `codec()`.
- Ecriture atomique : `path.tmp`, `sync_all`, puis `rename` vers le fichier
  final.
- Lecture absente = liste vide, ce qui permet un premier demarrage sans fichier
  persiste.
- `ProxyInventory::with_store(...)`, `load_from_store(...)` et
  `save_to_store(...)` permettent de charger au demarrage et de sauvegarder
  l'inventaire courant.
- L'inventaire sauvegarde automatiquement le store configure apres chargement
  lazy/probe et apres changements d'etat runtime (KO global, auth requise,
  retour OK, echecs destination/cooldowns).
- TTL, cooldown global et cooldown par destination restent portes par
  `InventoryConfig` et `ProxyRecord`, puis sont persistables via le store.

Impact : permet de ne pas retester/recharger a chaque lancement.

### Etape 5 - Provider scrapyfy et cablage dynamique

#### Etape 5.a - Provider scrapyfy ✅

- [x] Ajout de `ScrapyfyProxyDataProvider` dans `arachnea-scrapyfy`, compile
  derriere la feature `arachnea-proxy`.
- [x] Chargement de la collection de sources `arachnea-proxies` depuis
  `server/services/arachnea-proxies/services.json`.
- [x] `load_proxies(...)` appelle `execute_query_async(...)` sur
  `list_proxies_for_country`, convertit les resultats vers `ProxyRecord`, filtre
  par pays et deduplique les authorities.
- [x] Exposition de `default_scrapyfy_proxy_inventory()` pour construire un
  `ProxyInventory` dynamique par defaut branche sur le provider scrapyfy et le
  `ProxyProbe` par defaut. La persistance reste fournie par l'application via le
  store choisi.
- [x] Source initiale `proxifly/free-proxy-list` documentee et configuree via
  YAML.
- [x] Les observations sur les sources `iplocate/free-proxy-list` et
  `vakhov/fresh-proxy-list` restent documentees plus haut comme candidates
  futures.

Impact : le point d'extension cote `arachnea-scrapyfy` existe et charge une
source de donnees reelle ; la qualite finale de selection depend ensuite du
probe et de l'inventaire runtime.

#### Etape 5.b - Cablage runtime du routage pays dynamique ✅

- [x] Brancher un `ProxyInventory` dans `ArachneaProxyCore` ou dans la
  composition proxy utilisee par `arachnea-stream`.
- [x] Ajouter un routage pays dynamique, par exemple via un handler
  `dynamic_country_routing` ou via un marqueur de pool logique
  `dynamic-country:FR`.
- [x] Faire en sorte qu'une requete avec `country=FR` appelle
  `ProxyInventory::select("FR", require_https)` au lieu de consommer uniquement
  les routes statiques `CountryRoutingProxyHandler`.
- [x] En cas d'absence de candidat OK, laisser `ProxyInventory` declencher le
  chargement lazy via `ProxyDataProvider`, puis probe et selection.
- [x] Convertir le `ProxyRecord` selectionne en `ProxyNode` seulement apres
  resolution d'un protocole concret et validation des criteres de selection.
- [x] Respecter la politique de coexistence `static_only`, `dynamic_only`,
  `static_then_dynamic` ou `dynamic_then_static` pour ne pas fusionner
  implicitement pools statiques et dynamiques.
- [x] Ne pas faire de fallback direct implicite si aucun proxy dynamique n'est
  disponible pour le pays demande.

Impact : ce cablage est le morceau necessaire pour que le provider scrapyfy soit
effectivement utilise par le chemin applicatif `proxy_country("FR")` et par les
URLs proxy portant `Arachnea-Proxy-Country`.

#### Etape 5.c - Chargement/parsing des sources proxy publiques

- [x] Remplacer le `TODO: A implementer` de `ScrapyfyProxyDataProvider` par une
  vraie logique de chargement de donnees.
- [x] Ajouter le registre
  `server/services/arachnea-proxies/services.json`.
- [x] Ajouter la source YAML
  `server/services/arachnea-proxies/proxifly.yaml`.
- [x] Implementer le chargement de la source JSON
  `proxifly/free-proxy-list` par endpoint pays.
- [x] Normaliser la source Proxifly vers `ProxyRecord` sans format
  intermediaire propre a `arachnea-stream`.
- [x] Utiliser `row_pointer: "/*"` et `result_item_field: proxies` pour
  transformer le tableau JSON racine en un record par proxy.
- [x] Normaliser le pays demande et filtrer les resultats sur ce pays.
- [x] Deduplicer les records charges par authority normalisee.
- [ ] Implementer le chargement de la source texte `iplocate/free-proxy-list`,
  un proxy URL par ligne.
- [ ] Implementer le chargement de la source JSON `vakhov/fresh-proxy-list`,
  objets avec host/ip/port/protocoles et `country_code`.
- [ ] Normaliser ces deux formats vers `ProxyRecord` sans format intermediaire
  propre a `arachnea-stream`.
- [ ] Pour la source texte sans pays, conserver `country = None` et ne jamais
  copier artificiellement `ProxyLoadRequest.country` dans les records.
- [ ] Pour la source JSON, renseigner `country` depuis `country_code` quand il
  est present.
- [ ] Ajouter un champ source dans les records charges si l'on veut distinguer
  plusieurs fournisseurs dans les diagnostics et la persistance.

Impact : remplace le proxy FR code en dur par une source de donnees modifiable,
sans imposer a `arachnea-stream` de gerer les proxies par pays. Les sources
`iplocate` et `vakhov` restent des extensions futures, pas des pre-requis pour
le chemin Proxifly actuel.

### Etape 6 - Resolution pays optionnelle

- Ajouter `IpCountryRecord`, `IpCountryDataProvider` et `IpCountryStore`.
- Implementer le provider cote `arachnea-scrapyfy`.
- Implementer un store fichier Serde cote `arachnea-proxy`, JSON par defaut.
- Implementer la fonction principale de refresh cote `arachnea-scrapyfy` comme
  fonction Rust, puis l'exposer via une sous-commande du binaire applicatif ou
  d'administration.
- Charger les donnees de facon synchrone et bornee si la liste est vide ou si
  l'IP demandee est absente pendant une selection de pays stricte.
- Considerer correcte une donnee pays disponible.

Impact : ameliore les listes incompletes sans bloquer la v1.

### Etape 7 - Commandes et observabilite

- [ ] Exposer une commande ou API d'inspection avec liste des proxies connus,
  statut par pays, dernier test, latence, support HTTPS, source et pays declare.
- [x] Ajouter des logs structures pour le chargement, le probe et la selection
  dynamique.
- [ ] Completer les diagnostics d'echec destination/proxy si besoin apres retour
  d'usage.

Impact : indispensable pour diagnostiquer les erreurs de routage pays.

## Decisions clarifiees

- Les sources proxy sont decrites par des fichiers YAML dans
  `server/services/arachnea-proxies`, eux-memes references par
  `server/services/arachnea-proxies/services.json`.
- Le pays demande doit etre strictement respecte : pas de fallback vers un autre
  pays, et pas de fallback direct implicite. Si `country` est present, il est
  considere correct et n'est pas reverifie. Si le pays est absent, la resolution
  IP -> pays doit etre tentee avant d'exclure le record.
- `ProxyAvailabilityHint` est un indice optionnel venu de la source, avec des
  valeurs comme `Unknown`, `Low`, `Medium` et `High`. Il ne remplace pas le
  statut runtime issu du probe.
- Les pools statiques et dynamiques ne sont pas fusionnes implicitement. La
  coexistence passe par une politique explicite ; les defaults recommandes sont
  `static_only` pour les configurations existantes et `dynamic_only` pour le
  cablage par defaut fourni par `arachnea-scrapyfy`.
- L'authentification des proxies dynamiques n'est pas prise en charge en v1. Les
  proxies qui exigent une authentification sont detectes, stockes comme tels et
  ignores par la selection.
- Les probes HTTP/HTTPS ont besoin de cibles configurees. Les presets de
  developpement peuvent utiliser `example.com`, mais le mode strict ou
  production doit exiger une URL HTTPS explicite pour certifier
  `supports_https = true`.
- Les commandes de refresh/import de listes proxy doivent recevoir explicitement
  leurs URLs de probe, car elles n'ont pas de destination utilisateur reelle.
- Pendant une connexion runtime, la destination demandee peut servir de
  validation specifique. Un echec contre cette destination cree un cooldown par
  `scheme/host/port` et ne rend pas le proxy globalement KO sauf si l'erreur
  prouve un probleme global du proxy.
- Si un proxy atteint `max_destination_failures_before_ko`, recommande a `10`
  origines actives, il est marque KO globalement et sa liste
  `destination_failures` est videe avant persistance.
- Les retries runtime doivent etre bornes. Les requetes non idempotentes ne
  doivent pas etre rejouees automatiquement sans opt-in explicite.
- Le store generique s'appelle `ProxySerdeStore`.
- Le refresh IP -> pays cote `arachnea-scrapyfy` doit etre expose comme fonction
  Rust. La commande utilisateur recommandee est une sous-commande du binaire
  applicatif ou d'administration, pas un binaire public separe dans
  `arachnea-scrapyfy`.
- Quand une selection stricte depend d'une IP sans pays connu, le refresh
  IP -> pays est synchrone, borne par timeout et protege par verrou. Si le pays
  reste inconnu, le record n'est pas admissible pour ce pays.

## Points encore a eclaircir

- Les noms exacts des types et options de configuration restent a valider
  pendant l'implementation. Le comportement cible est clarifie : pas de fusion
  statique/dynamique implicite, probe HTTPS strict avec URL explicite hors
  contexte, validation runtime par destination reelle, et refresh IP -> pays
  synchrone borne quand il bloque une selection stricte.

## Recommandation finale

La meilleure trajectoire est d'ajouter un inventaire dynamique dans
`arachnea-proxy`, alimente par un trait `ProxyDataProvider` dont
`arachnea-scrapyfy` fournit l'implementation et le cablage par defaut. Le
protocole peut etre optionnel dans les donnees chargees ; s'il est fourni, il
est considere correct, et s'il est absent le proxy peut le renseigner dans le
champ unique `protocol` avant conversion en `ProxyNode`. Les tests doivent
mettre a jour directement `ProxyRecord` avec latence, support HTTPS, statut,
pays declare si disponible et `last_checked`.

Le chargement a la demande doit se faire au moment ou un pays est demande et ou
aucun proxy fonctionnel n'est disponible, avec un verrou par pays, des probes
bornes, une persistance Serde fichier en JSON par defaut, une protection contre
la reutilisation immediate des memes proxies inutilisables et aucun fallback
direct implicite.

Les contrats sensibles doivent rester explicites : les pools statiques ne sont
pas fusionnes avec les pools dynamiques sans politique declaree, un probe HTTPS
strict hors contexte exige une URL de test HTTPS configuree, une connexion
runtime peut exclure temporairement un proxy pour l'origine demandee sans le
marquer globalement KO, et une resolution IP -> pays qui conditionne un routage
strict attend un refresh borne avant d'exclure le record si le pays reste
inconnu.
