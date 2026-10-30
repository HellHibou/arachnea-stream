# Spécification du format YAML arachnea-ip-countries

## 1. Aperçu

Une collection arachnea-ip-countries déclare une ou plusieurs sources de
géo-localisation d'adresses IP. Chaque requête produit des enregistrements de
type `application/arachnea-ip-country` que le module
`ScrapyfyIpCountryDataProvider` de `arachnea-scrapyfy` consomme pour résoudre
le code pays d'une adresse IP.

Ce document ne décrit que les champs propres au format IP-country. La structure
générale d'un fichier YAML ( `id`, `parameters`, `http`, `queries`,
`entries`, `actions`, etc. ) est documentée dans
[la spécification YAML commune](arachnea-scrapyfy-fr.md).

---

## 2. Type média

```yaml
media_types:
  - application/arachnea-ip-country
```

Chaque requête de la collection doit déclarer ce type média. Il permet au
moteur de router les résultats vers `ScrapyfyIpCountryDataProvider`.

---

## 3. Structure des données de sortie

La requête `resolve_ip_country` reçoit un paramètre `ip` (l'adresse à
résoudre) et retourne un objet JSON unique contenant au moins le code pays.
Les champs utilisables sont les suivants :

| Champ YAML | Type YAML | Obligatoire | Description |
|---|---|---|---|
| `country_code` | `string` | oui | Code pays ISO 3166-1 alpha-2 (ex: `FR`, `US`). Le provider normalise automatiquement en majuscules. |
| `query_ip` | `string` | non | Adresse IP qui a été interrogée (utile pour débogage ou vérification). |

Le champ `country_code` est obligatoire. Sans lui, l'enregistrement est
ignoré par le provider.

La structure `row_pointer: "/"` indique que la réponse JSON est un objet
unique (pas un tableau). Avec ce réglage, le scraper traite l'objet racine
comme l'unique ligne de résultat.

---

## 4. Paramètre de requête

La requête `resolve_ip_country` attend un paramètre `ip` passé via les
paramètres d'exécution. Ce paramètre est injecté dans le template
`query_url` :

```yaml
query_url: "{base_url}/{ip}"
```

Par exemple, avec `ip=8.8.8.8` et `base_url=http://ip-api.com/json`,
la requête demande `http://ip-api.com/json/8.8.8.8`.

---

## 5. Exemple complet

```yaml
id: ip-api
title: "ip-api.com IP geolocation"
description:
  en: "Free IP geolocation API that returns country code for any public IP address."
http:
  mode: direct

parameters:
  - name: base_url
    value: http://ip-api.com/json
    description: Base URL for the ip-api.com JSON endpoint.
  - name: fields
    value: query,countryCode
    description: Response fields to request from the API.

queries:
  - name: resolve_ip_country
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}/{ip}"
    media_types:
      - application/arachnea-ip-country
    row_pointer: "/"
    entries:
      - name: country_code
        type: string
        pointer: /countryCode
      - name: query_ip
        type: string
        pointer: /query
```

---

## 6. Correspondance avec `IpCountryRecord`

Les champs extraits par le scraper sont mappés dans le code Rust comme suit :

- `country_code` → `IpCountryRecord.country` (`String`, normalisé en majuscules)
- L'adresse IP provient du paramètre d'appel, pas du résultat YAML
- `source` est défini statiquement dans le code (ex: `"ip-api.com"`)



---

## 7. Convention de nommage

### 7.1 Groupe de collection

Le groupe de collection utilisé par `ScrapyfyIpCountryDataProvider` est
`arachnea-ip-countries` (constante `IP_COUNTRY_GROUP_NAME` dans le code).

### 7.2 Requête

La requête doit s'appeler `resolve_ip_country`. C'est le nom attendu par
`ScrapyfyIpCountryDataProvider` et `ScrapyfyProxyDataProvider` lorsqu'ils
appellent le moteur de scraping.

### 7.3 Mode HTTP

Les API de géo-localisation sont généralement des services simples sans
protection Cloudflare. Le mode `direct` est recommandé :

```yaml
http:
  mode: direct
```

---

## 8. Persistance

Les résolutions IP-pays sont persistées dans un fichier JSON (par défaut
`data/ip-countries.json`). Le fichier est atomiquement réécrit via un
temporaire (`data/ip-countries.json.tmp`) pour éviter la corruption en cas
de crash. Le codec utilisé est le JSON pretty-print (`JsonIpCountryCodec`).

Format du fichier persistant :

```json
[
  {
    "ip": "8.8.8.8",
    "country": "US",
    "source": "resolver"
  }
]
```

Le champ `source` vaut `"resolver"` pour les entrées ajoutées lors d'une
résolution à la volée, et peut valoir `"ip-api.com"` ou autre pour les
rafraîchissements en masse.

## 9. Requête de pays de la connexion sortante

`resolve_current_country` détermine le pays de la connexion publique sortante de
l'application. Elle ne reçoit **aucun paramètre `ip`**. Appeler le point d'entrée
du fournisseur pour l'IP appelante au lieu d'interpoler un `{ip}` absent.
Retourner les mêmes champs `country_code` et `query_ip` facultatif que
`resolve_ip_country`.

Ajouter cette seconde requête à la liste `queries` de l'exemple complet :

```yaml
  - name: resolve_current_country
    scraper_type: json
    base_url: "{base_url}"
    query_url: "{base_url}"
    media_types: [application/arachnea-ip-country]
    row_pointer: "/"
    entries:
      - name: country_code
        type: string
        pointer: /countryCode
      - name: query_ip
        type: string
        pointer: /query
```

Le moteur nettoie les espaces et met le pays extrait en majuscules ; il ne valide
pas ici son appartenance à la liste ISO. Émettre seulement un code alpha-2 non
vide. Les messages d'erreur du fournisseur, chaînes arbitraires et réponses ne
contenant que des espaces ne sont pas des pays valides. `query_ip` sert au
diagnostic : les enregistrements de lot utilisent l'IP demandée sans la comparer
à `query_ip`. Ne pas considérer ce champ comme un contrôle de confiance.

## 10. Liste de contrôle de rédaction et de validation

1. Placer le YAML dans `server/services/arachnea-ip-countries/` et l'inscrire dans
   `services.json`, selon le format de manifeste de la spécification commune.
2. Implémenter `resolve_ip_country` pour les adresses explicites et
   `resolve_current_country` si la détection du pays sortant est nécessaire.
3. Conserver `http.mode: direct` pour le fournisseur de référence ; passer la
   détection par un proxy ferait observer le pays sortant de ce proxy.
4. Déclarer le type de média sur les deux requêtes. Utiliser `row_pointer: "/"`
   pour un objet racine et un pointeur vers des objets répétés pour un tableau.
5. Vérifier que les IP non prises en charge et les erreurs fournisseur ne
   deviennent pas des pays. Le consommateur prend la première ligne contenant
   une valeur de pays non vide.
6. Déclarer seulement les paramètres réellement interpolés. Dans le service de
   référence, `fields` est déclaré mais n'est ajouté à aucune URL ; sa seule
   présence ne modifie pas la réponse HTTP du fournisseur.
7. Vérifier séparément la syntaxe YAML, le JSON final, l'activation du manifeste
   et la réponse réelle de l'API. La persistance est gérée par l'application ;
   ce n'est pas un champ YAML de service.

### Exemples de services existants

Ces exemples facultatifs illustrent des modèles propres aux fournisseurs. Le
contrat complet reste dans ce guide et la spécification commune ; ne pas copier
une URL, un identifiant ou un sélecteur sans vérifier la réponse de la cible.

- [ip-api.yaml](../../server/services/arachnea-ip-countries/ip-api.yaml) — Requêtes pour une IP explicite et pour le pays sortant.
