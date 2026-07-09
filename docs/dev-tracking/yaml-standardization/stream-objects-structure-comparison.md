# Comparaison des structures JSON par objet et par service

Légende : **`X`** = la propriété est produite par ce service (dans une ou plusieurs queries).

---

## `categories (object[])`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| description (string) | | | | | | **X** | | | |
| image (string) | | | | | **X** | | **X** | **X** | |
| key (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| label (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| link (string) | **X** | **X** | **X** | | | | | **X** | |
| request (object) > channel (string) | | | | | | | | | **X** |
| request (object) > channel_label (string) | | | | | | | | | **X** |
| request (object) > name (string) | | | | | **X** | | | | **X** |
| request (object) > page_size (number) | | | | | | | | | **X** |
| request (object) > query_url (string) | | | | | **X** | **X** | **X** | | **X** |
| request (object) > source (string) | | | | | **X** | | | | **X** |

---

## `sections (object[])`

Inclut les rails de contenu de `load_home`, `get_category` et les résultats de `get_section`.

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| banners (object[]) > image (string) | | | | | | | | **X** | |
| banners (object[]) > key (string) | | | | | | | | **X** | |
| banners (object[]) > link (string) | | | | | | | | **X** | |
| banners (object[]) > title (string) | | | | | | | | **X** | |
| current_page (number) | **X** | | **X** | **X** | | **X** | **X** | | |
| entries (object[]) > casting (string[]) | | | **X** | | | | | | |
| entries (object[]) > channel (string) | | | | | | | **X** | | |
| entries (object[]) > count-season (number) | | | | | **X** | | | | |
| entries (object[]) > description (string) | **X** | | **X** | | **X** | **X** | **X** | | |
| entries (object[]) > director (string[]) | | | **X** | | | | | | |
| entries (object[]) > duration (string) | | | | | | **X** | **X** | | |
| entries (object[]) > episode (object) > label (string) | **X** | | | | | | | | |
| entries (object[]) > expire (string) | | | | | | | **X** | | |
| entries (object[]) > img/landscape (object) > link (string) | **X** | | **X** | | **X** | **X** | | | |
| entries (object[]) > img/logo (object) > link (string) | | | | | **X** | | | | |
| entries (object[]) > img/poster (object) > link (string) | | | **X** | | **X** | **X** | **X** | | **X** |
| entries (object[]) > img/portrait (object) > link (string) | | | | | | | | | **X** |
| entries (object[]) > lang (string[]) | **X** | | | | | | | | |
| entries (object[]) > link (string) | **X** | | **X** | | **X** | **X** | **X** | | **X** |
| entries (object[]) > media-type (string/string[]) | **X** | | | | **X** | **X** | **X** | | **X** |
| entries (object[]) > rating (number) | | | **X** | | | | | | |
| entries (object[]) > release-date (string) | **X** | | | | | **X** | **X** | | |
| entries (object[]) > source (string) | **X** | | **X** | | **X** | **X** | **X** | | **X** |
| entries (object[]) > theme (string[]) | **X** | | | | **X** | | **X** | | **X** |
| entries (object[]) > title (string) | **X** | | **X** | | **X** | **X** | **X** | | **X** |
| entries (object[]) > title/alt (string) | **X** | | | | | | **X** | | |
| entries (object[]) > web-link (string) | **X** | | **X** | | **X** | **X** | **X** | | **X** |
| entries (object[]) > year (number) | | | **X** | | | **X** | | | |
| have_more (boolean) | | | **X** | | **X** | **X** | **X** | | |
| id (string) | | **X** | | **X** | | | | **X** | |
| infer_have_more_from_full_page (boolean) | | | | | | **X** | | | |
| items (object[]) > img/poster (object) > link (string) | | **X** | | **X** | | | | | |
| items (object[]) > lang (string[]) | | | | **X** | | | | | |
| items (object[]) > link (string) | | **X** | | **X** | | | | | |
| items (object[]) > media-type (string/string[]) | | **X** | | **X** | | | | | |
| items (object[]) > source (string) | | **X** | | **X** | | | | | |
| items (object[]) > title (string) | | **X** | | **X** | | | | | |
| items (object[]) > title/alt (string) | | | | **X** | | | | | |
| items (object[]) > web-link (string) | | **X** | | **X** | | | | | |
| key (string) | | | | | | | | **X** | |
| label (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| link (string) | **X** | | **X** | | **X** | **X** | **X** | | |
| page_size (number) | | | **X** | **X** | | **X** | | | |
| source (string) | **X** | | **X** | | | **X** | **X** | | |
| total_pages (number) | **X** | | | **X** | | | | | |

---

## `players (object[])`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| embed-link (string) | **X** | | **X** | | | **X** | | | **X** |
| name (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| resolver (object) > kind (string) | | | | | **X** | **X** | **X** | **X** | **X** |
| resolver (object) > stream (object) > kind (string) | | | | | **X** | **X** | **X** | **X** | **X** |
| resolver (object) > target_id (string) | | | | | **X** | **X** | **X** | **X** | **X** |
| storyboard (object) > columns (number) | | | | | | **X** | | | |
| storyboard (object) > height (number) | | | | | | **X** | | | |
| storyboard (object) > interval (number) | | | | | | **X** | | | |
| storyboard (object) > link (string) | | | | | | **X** | | | |
| storyboard (object) > width (number) | | | | | | **X** | | | |

---

## `seasons (object[])`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| episodes (object[]) > description (string) | | | | | **X** | | **X** | | |
| episodes (object[]) > direct-link (string) | | **X** | | | | | | | |
| episodes (object[]) > duration (string) | | **X** | | | **X** | | **X** | | |
| episodes (object[]) > embed-link (string) | | | | **X** | | | | | |
| episodes (object[]) > episode-number (string/number) | | **X** | | **X** | | | | | |
| episodes (object[]) > expire (string) | | | | | | | **X** | | |
| episodes (object[]) > img/poster (object) > link (string) | | | | | **X** | | **X** | | |
| episodes (object[]) > img/preview (object) > link (string) | | **X** | | | **X** | | **X** | | |
| episodes (object[]) > link (string) | | **X** | | | **X** | | | | |
| episodes (object[]) > name (string) | | | | | | | | | |
| episodes (object[]) > players (object[]) > name (string) | | **X** | | **X** | **X** | | | | |
| episodes (object[]) > players (object[]) > resolver (object) > kind (string) | | | | | **X** | | **X** | | |
| episodes (object[]) > players (object[]) > resolver (object) > stream (object) > kind (string) | | | | | **X** | | **X** | | |
| episodes (object[]) > players (object[]) > resolver (object) > target_id (string) | | | | | **X** | | **X** | | |
| episodes (object[]) > release-date (string) | | **X** | | | | | **X** | | |
| episodes (object[]) > season-name (string) | | | | | **X** | | | | |
| episodes (object[]) > title (string) | | **X** | | **X** | **X** | | **X** | | |
| label (string) | **X** | | **X** | | | **X** | | **X** | |
| link (string) | **X** | | | | | **X** | **X** | **X** | |

---

## `episodes (object[])`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| description (string) | | | | | **X** | **X** | **X** | **X** |
| duration (string) | | **X** | | | **X** | **X** | **X** | **X** |
| embed-link (string) | **X** | | **X** | | | | | |
| episode-number (string) | **X** | **X** | **X** | **X** | | | **X** | **X** |
| expire (string) | | | | | | | **X** | |
| id (string) | | | **X** | | | | | |
| img/poster (object) > link (string) | | | | | **X** | | **X** | |
| img/preview (object) > link (string) | | **X** | **X** | | **X** | **X** | **X** | **X** |
| lang (string) | **X** | | | | | | | |
| link (string) | | **X** | | | **X** | **X** | **X** | **X** |
| name (string) | **X** | | | | | | | |
| players (object[]) > embed-link (string) | **X** | | **X** | | | | | |
| players (object[]) > name (string) | **X** | **X** | | **X** | **X** | **X** | | **X** |
| players (object[]) > resolver (object) > kind (string) | | | | | **X** | **X** | **X** | **X** |
| players (object[]) > resolver (object) > stream (object) > kind (string) | | | | | **X** | **X** | **X** | **X** |
| players (object[]) > resolver (object) > target_id (string) | | | | | **X** | **X** | **X** | **X** |
| players (object[]) > storyboard (object) > columns (number) | | | | | | **X** | | |
| players (object[]) > storyboard (object) > height (number) | | | | | | **X** | | |
| players (object[]) > storyboard (object) > interval (number) | | | | | | **X** | | |
| players (object[]) > storyboard (object) > link (string) | | | | | | **X** | | |
| players (object[]) > storyboard (object) > width (number) | | | | | | **X** | | |
| release-date (string) | | **X** | **X** | | | **X** | **X** | **X** |
| season-name (string) | | | | | **X** | **X** | | |
| title (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |

---

*Document généré le 09/07/2026 — Comparatif des structures JSON par objet, tous services confondus.*