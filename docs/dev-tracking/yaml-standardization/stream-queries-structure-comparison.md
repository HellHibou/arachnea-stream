# Comparaison des structures JSON par service et par query

Légende : **`X`** = la propriété est produite par ce service.

---

## `service_stream_metadata`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| description (object) > en (string) | | | | | | | | | |
| description (object) > fr (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| id (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| logo (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| search_themes (string[]) | **X** | | | | | | | | |
| title (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |

---

## `load_home`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| banners (object[]) > description (string) | **X** | | **X** | | | **X** | **X** | **X** | **X** |
| banners (object[]) > entryUrl (string) | | | | | | **X** | | | |
| banners (object[]) > expire (string) | | | | | | **X** | | | |
| banners (object[]) > image (string) | **X** | | **X** | | | **X** | **X** | **X** | **X** |
| banners (object[]) > key (string) | **X** | | **X** | | | **X** | **X** | **X** | **X** |
| banners (object[]) > link (string) | **X** | | **X** | | | **X** | **X** | **X** | **X** |
| banners (object[]) > logo (string) | | | | | | **X** | | | **X** |
| banners (object[]) > release-date (string) | | | | | | **X** | | | |
| banners (object[]) > subtitle (string) | | | | | | | **X** | **X** | |
| banners (object[]) > theme (string[]) | | | **X** | | | | | | |
| banners (object[]) > title (string) | **X** | | **X** | | | **X** | **X** | **X** | **X** |
| banners (object[]) > video (string) | | | | | | | **X** | | **X** |
| banners (object[]) > web-link (string) | **X** | | **X** | | | **X** | **X** | | **X** |
| banners (object[]) > year (number) | | | | | | **X** | | | |
| categories (object[]) > description (string) | | | | | | **X** | | | |
| categories (object[]) > image (string) | | | | | **X** | | **X** | **X** | |
| categories (object[]) > key (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| categories (object[]) > label (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| categories (object[]) > link (string) | **X** | **X** | **X** | | | | | **X** | |
| categories (object[]) > request (object) > channel (string) | | | | | | | | | **X** |
| categories (object[]) > request (object) > channel_label (string) | | | | | | | | | **X** |
| categories (object[]) > request (object) > name (string) | | | | | **X** | | | | **X** |
| categories (object[]) > request (object) > page_size (number) | | | | | | | | | **X** |
| categories (object[]) > request (object) > query_url (string) | | | | | **X** | **X** | **X** | | **X** |
| categories (object[]) > request (object) > source (string) | | | | | **X** | | | | **X** |
| sections (object[]) > current_page (number) | | | | | | | | | |
| sections (object[]) > entries (object[]) > count-season (number) | | | | | **X** | | | | |
| sections (object[]) > entries (object[]) > description (string) | **X** | | | | **X** | | | | |
| sections (object[]) > entries (object[]) > episode (object) > label (string) | **X** | | | | | | | | |
| sections (object[]) > entries (object[]) > img/landscape (object) > link (string) | **X** | | **X** | | **X** | | | | |
| sections (object[]) > entries (object[]) > img/logo (object) > link (string) | | | | | **X** | | | | |
| sections (object[]) > entries (object[]) > img/poster (object) > link (string) | | | **X** | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > img/portrait (object) > link (string) | | | | | | | | | **X** |
| sections (object[]) > entries (object[]) > lang (string[]) | **X** | | | | | | | | |
| sections (object[]) > entries (object[]) > link (string) | **X** | | **X** | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > media-type (string/string[]) | **X** | | | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > rating (number) | | | **X** | | | | | | |
| sections (object[]) > entries (object[]) > release-date (string) | **X** | | | | | | | | |
| sections (object[]) > entries (object[]) > source (string) | | | | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > theme (string[]) | **X** | | | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > title (string) | **X** | | **X** | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > title/alt (string) | **X** | | | | | | | | |
| sections (object[]) > entries (object[]) > web-link (string) | **X** | | **X** | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > year (number) | | | **X** | | | | | | |
| sections (object[]) > have_more (boolean) | | | | | **X** | | | | |
| sections (object[]) > id (string) | | **X** | | **X** | | | | **X** | |
| sections (object[]) > items (object[]) > img/poster (object) > link (string) | | **X** | | **X** | | | | | |
| sections (object[]) > items (object[]) > lang (string[]) | | | | **X** | | | | | |
| sections (object[]) > items (object[]) > link (string) | | **X** | | **X** | | | | | |
| sections (object[]) > items (object[]) > media-type (string/string[]) | | **X** | | **X** | | | | | |
| sections (object[]) > items (object[]) > source (string) | | **X** | | **X** | | | | | |
| sections (object[]) > items (object[]) > title (string) | | **X** | | **X** | | | | | |
| sections (object[]) > items (object[]) > title/alt (string) | | | | **X** | | | | | |
| sections (object[]) > items (object[]) > web-link (string) | | **X** | | **X** | | | | | |
| sections (object[]) > key (string) | | | | | | | | **X** | |
| sections (object[]) > label (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| sections (object[]) > link (string) | **X** | | **X** | | **X** | **X** | **X** | | |
| sections (object[]) > page_size (number) | | | | | | | | | |
| sections (object[]) > total_pages (number) | | | | | | | | | |
| source (string) | | **X** | **X** | **X** | **X** | | | | **X** |

---

## `get_category`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| categories (object[]) > image (string) | | | | | | | **X** | **X** | |
| categories (object[]) > key (string) | **X** | | | | | | **X** | **X** | |
| categories (object[]) > label (string) | **X** | | | | | | **X** | **X** | |
| categories (object[]) > link (string) | **X** | | | | | | | **X** | |
| categories (object[]) > request (object) > query_url (string) | | | | | | | **X** | | |
| sections (object[]) > banners (object[]) > image (string) | | | | | | | | **X** | |
| sections (object[]) > banners (object[]) > key (string) | | | | | | | | **X** | |
| sections (object[]) > banners (object[]) > link (string) | | | | | | | | **X** | |
| sections (object[]) > banners (object[]) > title (string) | | | | | | | | **X** | |
| sections (object[]) > current_page (number) | **X** | | **X** | **X** | | | | | |
| sections (object[]) > entries (object[]) > casting (string[]) | | | **X** | | | | | | |
| sections (object[]) > entries (object[]) > count-season (number) | | | | | **X** | | | | |
| sections (object[]) > entries (object[]) > description (string) | **X** | **X** | **X** | | **X** | | | | |
| sections (object[]) > entries (object[]) > director (string[]) | | | **X** | | | | | | |
| sections (object[]) > entries (object[]) > img/landscape (object) > link (string) | **X** | | **X** | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > img/logo (object) > link (string) | | | | | **X** | | | | |
| sections (object[]) > entries (object[]) > img/poster (object) > link (string) | | **X** | **X** | **X** | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > img/portrait (object) > link (string) | | | | | | | | | **X** |
| sections (object[]) > entries (object[]) > lang (string[]) | **X** | | | **X** | | | | | |
| sections (object[]) > entries (object[]) > link (string) | **X** | **X** | **X** | **X** | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > media-type (string/string[]) | **X** | **X** | **X** | **X** | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > rating (number) | | **X** | **X** | | | | | | |
| sections (object[]) > entries (object[]) > source (string) | **X** | | **X** | **X** | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > theme (string[]) | **X** | | | | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > title (string) | **X** | **X** | **X** | **X** | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > title/alt (string) | **X** | | | | | | | | |
| sections (object[]) > entries (object[]) > web-link (string) | **X** | **X** | **X** | **X** | **X** | | | | **X** |
| sections (object[]) > entries (object[]) > year (number) | | | **X** | | | | | | |
| sections (object[]) > have_more (boolean) | | | | | **X** | | | | |
| sections (object[]) > key (string) | | | | | | | | **X** | |
| sections (object[]) > label (string) | **X** | | | **X** | **X** | **X** | **X** | **X** | **X** |
| sections (object[]) > link (string) | **X** | | | **X** | | **X** | **X** | | |
| sections (object[]) > page_size (number) | | | **X** | **X** | | | | | |
| sections (object[]) > total_pages (number) | **X** | | | **X** | | | | | |
| source (string) | | **X** | **X** | **X** | **X** | | | | |

---

## `get_section`

| Propriété | anime-sama | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|
| current_page (number) | **X** | **X** | **X** | | **X** | **X** |
| entries (object[]) > casting (string[]) | | **X** | | | | |
| entries (object[]) > channel (string) | | | | | | **X** |
| entries (object[]) > description (string) | **X** | **X** | | | **X** | **X** |
| entries (object[]) > director (string[]) | | **X** | | | | |
| entries (object[]) > duration (string) | | | | | **X** | **X** |
| entries (object[]) > expire (string) | | | | | | **X** |
| entries (object[]) > img/landscape (object) > link (string) | **X** | **X** | | | **X** | |
| entries (object[]) > img/poster (object) > link (string) | | **X** | **X** | | **X** | **X** |
| entries (object[]) > lang (string[]) | **X** | | **X** | | | |
| entries (object[]) > link (string) | **X** | **X** | **X** | | **X** | **X** |
| entries (object[]) > media-type (string/string[]) | **X** | **X** | **X** | | **X** | **X** |
| entries (object[]) > rating (number) | | **X** | | | | |
| entries (object[]) > release-date (string) | | | | | **X** | **X** |
| entries (object[]) > source (string) | **X** | **X** | **X** | | **X** | **X** |
| entries (object[]) > theme (string[]) | **X** | | | | | **X** |
| entries (object[]) > title (string) | **X** | **X** | **X** | | **X** | **X** |
| entries (object[]) > title/alt (string) | **X** | | | | | **X** |
| entries (object[]) > web-link (string) | **X** | **X** | **X** | | **X** | **X** |
| entries (object[]) > year (number) | | **X** | | | **X** | |
| have_more (boolean) | | **X** | | **X** | **X** | **X** |
| infer_have_more_from_full_page (boolean) | | | | | **X** | |
| link (string) | | **X** | **X** | | | |
| page_size (number) | | **X** | **X** | | **X** | |
| source (string) | **X** | **X** | **X** | | **X** | **X** |
| total_pages (number) | **X** | | **X** | | | |

---

## `search`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| current_page (number) | **X** | | **X** | **X** | | **X** | **X** | **X** | |
| entries (object[]) > casting (string[]) | | | **X** | | | | | | |
| entries (object[]) > channel (string) | | | | | | | **X** | | |
| entries (object[]) > count-season (number) | | | | | | | | **X** | |
| entries (object[]) > description (string) | **X** | | **X** | | **X** | **X** | **X** | **X** | **X** |
| entries (object[]) > director (string[]) | | | **X** | | | | | | |
| entries (object[]) > duration (string) | | | | | | | **X** | **X** | |
| entries (object[]) > embed-link (string) | | | | | | | **X** | | |
| entries (object[]) > expire (string) | | | | | | | **X** | **X** | |
| entries (object[]) > id (string) | | | | | | | **X** | | |
| entries (object[]) > img/landscape (object) > link (string) | | | | | | | | **X** | **X** |
| entries (object[]) > img/logo (object) > link (string) | | | | | | | | **X** | |
| entries (object[]) > img/poster (object) > link (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| entries (object[]) > img/portrait (object) > link (string) | | | | | | | | | **X** |
| entries (object[]) > key (string) | | | | | | | | **X** | |
| entries (object[]) > lang (string[]) | **X** | | | **X** | | | | | |
| entries (object[]) > link (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| entries (object[]) > media-type (string/string[]) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| entries (object[]) > rating (number) | | | **X** | | | | | | |
| entries (object[]) > release-date (string) | | | | | | **X** | **X** | **X** | |
| entries (object[]) > source (string) | | **X** | | **X** | **X** | | **X** | | **X** |
| entries (object[]) > subtitle (string) | | | | | | | **X** | **X** | |
| entries (object[]) > theme (string[]) | **X** | | | | **X** | **X** | **X** | **X** | **X** |
| entries (object[]) > title (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| entries (object[]) > title/alt (string) | **X** | | | | | | **X** | **X** | |
| entries (object[]) > web-link (string) | **X** | **X** | **X** | **X** | **X** | | **X** | **X** | **X** |
| entries (object[]) > year (number) | | | **X** | | | | | **X** | |
| have_more (boolean) | | | **X** | | **X** | **X** | | **X** | **X** |
| page_size (number) | | | | **X** | | | | | |
| source (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | |
| total_pages (number) | **X** | | | | | **X** | | | |

---

## `get_entry`

| Propriété | anime-sama | animeultime | coflix | frenchanimes | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| casting (string[]) | | | **X** | **X** | **X** | | **X** | **X** | |
| content-advisor (string) | | | | | **X** | **X** | | **X** | |
| count-season (number) | | | | | **X** | | **X** | **X** | |
| description (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | |
| director (string[]) | | **X** | **X** | **X** | **X** | | | **X** | |
| duration (string) | | | | **X** | **X** | **X** | **X** | **X** | |
| expire (string) | | | | | | **X** | **X** | **X** | |
| genre (string[]) | | | | | | | | **X** | |
| img/landscape (object) > link (string) | | | | | **X** | **X** | | **X** | |
| img/logo (object) > link (string) | | | | | **X** | **X** | **X** | **X** | |
| img/poster (object) > link (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| img/portrait (object) > link (string) | | | | | | | | | **X** |
| img/preview (object) > link (string) | | | | | | | | | |
| lang/audio (string[]) | | | | **X** | | | **X** | | |
| lang/subtitles (string[]) | | | | | | | **X** | | |
| language (string[]) | | | | | | | | **X** | |
| link (string) | | | | | **X** | **X** | | **X** | **X** |
| media-type (string[]) | | | | | **X** | **X** | | **X** | **X** |
| players (object[]) > embed-link (string) | | | **X** | | | | | | **X** |
| players (object[]) > name (string) | | | **X** | | **X** | | | **X** | **X** |
| players (object[]) > resolver (object) > kind (string) | | | | | **X** | | **X** | **X** | **X** |
| players (object[]) > resolver (object) > stream (object) > kind (string) | | | | | **X** | | **X** | **X** | **X** |
| players (object[]) > resolver (object) > target_id (string) | | | | | **X** | | **X** | **X** | |
| release-date (string) | | | | | **X** | **X** | **X** | | |
| seasons (object[]) > episodes (object[]) > description (string) | | | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > direct-link (string) | | **X** | | | | | | | |
| seasons (object[]) > episodes (object[]) > duration (string) | | **X** | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > embed-link (string) | | | | **X** | | | | | |
| seasons (object[]) > episodes (object[]) > episode-number (string/number) | | **X** | | **X** | | | | | |
| seasons (object[]) > episodes (object[]) > expire (string) | | | | | | | **X** | | |
| seasons (object[]) > episodes (object[]) > img/poster (object) > link (string) | | | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > img/preview (object) > link (string) | | **X** | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > link (string) | | **X** | | | **X** | | | | |
| seasons (object[]) > episodes (object[]) > name (string) | | | | | | | | | |
| seasons (object[]) > episodes (object[]) > players (object[]) > name (string) | | **X** | | **X** | **X** | | | | |
| seasons (object[]) > episodes (object[]) > players (object[]) > resolver (object) > kind (string) | | | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > players (object[]) > resolver (object) > stream (object) > kind (string) | | | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > players (object[]) > resolver (object) > target_id (string) | | | | | **X** | | **X** | | |
| seasons (object[]) > episodes (object[]) > release-date (string) | | **X** | | | | | **X** | | |
| seasons (object[]) > episodes (object[]) > season-name (string) | | | | | **X** | | | | |
| seasons (object[]) > episodes (object[]) > title (string) | | **X** | | **X** | **X** | | **X** | | |
| seasons (object[]) > label (string) | **X** | | **X** | | | **X** | | **X** | |
| seasons (object[]) > link (string) | **X** | | | | | **X** | **X** | **X** | |
| source (string) | | | **X** | | **X** | | | | |
| theme (string[]) | **X** | **X** | **X** | **X** | **X** | | **X** | **X** | **X** |
| title (string) | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** | **X** |
| title/alt (string) | **X** | **X** | | **X** | | | **X** | **X** | |
| video/trailer (string) | **X** | | | | | | | | |
| web-link (string) | | | | | **X** | **X** | **X** | **X** | **X** |
| year (number/string) | **X** | **X** | **X** | **X** | | **X** | | **X** | |

---

## `get_season`

| Propriété | anime-sama | coflix | m6play-fr | rtbf-auvio-be | rtlplay-be |
|:---|:---:|:---:|:---:|:---:|:---:|
| current_page (number) | **X** | **X** | **X** | **X** | **X** |
| episodes (object[]) > description (string) | | | **X** | **X** | **X** |
| episodes (object[]) > duration (string) | | | **X** | **X** | **X** |
| episodes (object[]) > embed-link (string) | **X** | **X** | | | |
| episodes (object[]) > episode-number (string) | **X** | **X** | | **X** | **X** |
| episodes (object[]) > expire (string) | | | | **X** | |
| episodes (object[]) > id (string) | | **X** | | | |
| episodes (object[]) > img/poster (object) > link (string) | | | | **X** | |
| episodes (object[]) > img/preview (object) > link (string) | | **X** | **X** | **X** | **X** |
| episodes (object[]) > lang (string) | **X** | | | | |
| episodes (object[]) > link (string) | | | **X** | **X** | **X** |
| episodes (object[]) > name (string) | **X** | | | | |
| episodes (object[]) > players (object[]) > embed-link (string) | **X** | **X** | | | |
| episodes (object[]) > players (object[]) > name (string) | **X** | | **X** | | **X** |
| episodes (object[]) > players (object[]) > resolver (object) > kind (string) | | | **X** | **X** | **X** |
| episodes (object[]) > players (object[]) > resolver (object) > stream (object) > kind (string) | | | **X** | **X** | **X** |
| episodes (object[]) > players (object[]) > resolver (object) > target_id (string) | | | **X** | **X** | **X** |
| episodes (object[]) > players (object[]) > storyboard (object) > columns (number) | | | **X** | | |
| episodes (object[]) > players (object[]) > storyboard (object) > height (number) | | | **X** | | |
| episodes (object[]) > players (object[]) > storyboard (object) > interval (number) | | | **X** | | |
| episodes (object[]) > players (object[]) > storyboard (object) > link (string) | | | **X** | | |
| episodes (object[]) > players (object[]) > storyboard (object) > width (number) | | | **X** | | |
| episodes (object[]) > release-date (string) | | **X** | **X** | **X** | **X** |
| episodes (object[]) > season-name (string) | | | **X** | | |
| episodes (object[]) > title (string) | **X** | **X** | **X** | **X** | **X** |
| have_more (boolean) | **X** | **X** | **X** | **X** | **X** |
| languages (object[]) > class (string) | **X** | | | | |
| languages (object[]) > lang (string) | **X** | | | | |
| languages (object[]) > link (string) | **X** | | | | |
| link (string) | **X** | | | | |

---

## `list_lives`

| Propriété | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|
| channel (string) | **X** | | **X** | | **X** |
| channel-id (string) | | | | **X** | |
| count-season (number) | | | | **X** | |
| description (string) | **X** | **X** | **X** | **X** | |
| duration (string) | | | | **X** | |
| expire (string) | | **X** | **X** | | **X** |
| genre (string[]) | | | | **X** | |
| img/landscape (object) > link (string) | **X** | **X** | **X** | **X** | **X** |
| img/poster (object) > link (string) | **X** | **X** | **X** | **X** | **X** |
| key (string) | **X** | **X** | **X** | **X** | **X** |
| link (string) | **X** | **X** | **X** | **X** | **X** |
| media-type (string[]) | **X** | | **X** | **X** | **X** |
| release-date (string) | | **X** | **X** | | **X** |
| subtitle (string) | | | | **X** | |
| theme (string[]) | | | | **X** | |
| title (string) | **X** | **X** | **X** | **X** | **X** |
| title/alt (string) | **X** | | **X** | | **X** |
| web-link (string) | **X** | **X** | **X** | **X** | **X** |
| year (number) | | | | **X** | |

---

## `get_live`

| Propriété | francetv | m6play-fr | rtbf-auvio-be | rtlplay-be | tf1-fr |
|:---|:---:|:---:|:---:|:---:|:---:|
| players (object[]) > embed-link (string) | | **X** | | | |
| players (object[]) > name (string) | **X** | | **X** | **X** | **X** |
| players (object[]) > resolver (object) > kind (string) | **X** | **X** | **X** | **X** | **X** |
| players (object[]) > resolver (object) > stream (object) > kind (string) | **X** | **X** | **X** | **X** | **X** |
| players (object[]) > resolver (object) > target_id (string) | **X** | **X** | **X** | **X** | **X** |

---

*Document généré le 09/07/2026 — Comparatif des structures JSON produites par les 9 sources de `server/services/arachnea-stream/`.*