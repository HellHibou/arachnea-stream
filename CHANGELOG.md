# Changelog

All notable changes to the server workspace are recorded here.
## Unreleased

### <u>build-release</u>

#### Added

#### Changed

- Release tooling versions are now centralized in `build-release/build-config.json` (minimum rustc, cross image tag, in-image Tauri CLI) instead of constants scattered across `lib.mjs`/`docker.mjs`/`Dockerfile`; the Docker image build forwards the Tauri CLI version as a build arg.

#### Fixed

- Release tooling now checks the active rustc version before building: when it is older than the `minRustcVersion` floor in `build-config.json` (1.91.0, required by the locked `foyer@0.22.4+` dependency), it offers to run `rustup update` with confirmation instead of failing late inside `cargo tauri build`.
- Portable `.tar.gz` archives built on Windows are now usable: `createArchive` / `createTarGz` normalize the POSIX modes recorded in the archive (`normalizeTarGzModes`), because Windows has no permission bits and its `bsdtar` stores every entry as `0666`/`0777` — the amd64 archive produced on a Windows host therefore held a **non-executable** `arachnea`. That broke `arachnea-docker`'s image build (`test -x /tmp/app/arachnea` aborted the `fetcher` stage with `exit code: 1` and no diagnostic, since the check only runs under `set -e`) and would also have handed Linux users an archive that does not run after extraction. Windows' `bsdtar` (3.8.8) rejects `--mode`, so the modes are rewritten in the archive itself — a pure-Node helper reading the gzipped tar with `node:zlib`, setting files to `0644`, directories to `0755` and the entries listed by the caller to `0755` (macOS and GNU tar values), which makes the archive independent of the build host, and recomputing each header checksum. `release.mjs` passes the release executable for the portable archive and `Arachnéa.app/Contents/MacOS/arachnea` for the macOS `.app` archive; a missing expected entry fails the build instead of shipping a silently broken archive.

### <u>arachnea-docker</u>

#### Added

- Multi-arch (`linux/amd64` + `linux/arm64`) runtime image for the prebuilt portable Linux `arachnea` server: one Dockerfile where `TARGETARCH` selects the baked-in archive. The runtime stays minimal (Debian `trixie-slim` base, one `apt` layer, no build toolchain), ships Debian's `chromium` for the `chaser-cf` Cloudflare solver with one entrypoint-owned `Xvfb` on `:99` for its headed virtual display, runs as the non-root `arachnea` user (Chromium's user-namespace sandbox still needs the container's seccomp profile unconfined — see the `Fixed` entry), persists state in `/app/data`, and uses `tini` as PID 1.
- Build configuration as Compose variables: `ARACHNEA_VERSION` and `ARACHNEA_RELEASES_DIR` live in the committed `arachnea-docker/.env` (template `.env.example`) or the environment, with no default and a fail-fast error when missing. The version expands into the two `ARACHNEA_AMD64_ARCHIVE` / `ARACHNEA_ARM64_ARCHIVE` paths (declared in `docker-compose.yml`, relative to the `releases` named build context), is stamped as `ENV ARACHNEA_VERSION`, and becomes the image tag: `image: arachnea-stream:${ARACHNEA_VERSION}` plus a `latest` alias via `build.tags`, so `docker compose up` runs the release just built. The archives are read straight from the release tree instead of a `bin/` staging folder; Buildx fails the build on a wrong path (`failed to compute cache key`), and the extracted binary's ELF `e_machine` is checked against the target port (`3e00` amd64 / `b700` arm64) so a swapped archive aborts the build instead of failing later with `exec format error`.
- `arachnea-docker/docker-entrypoint.sh`: wires `CHROME_BIN`, `CHASER_VIRTUAL_DISPLAY=0` and `CHASER_EXTRA_ARGS` (always ensuring `--disable-dev-shm-usage`), starts/reuses the one Xvfb display, and pins `ARACHNEA_PORT`/`ARACHNEA_NETWORK`/`ARACHNEA_ROOT` (all from the committed `.env`, empty root drops the key) into `/app/data/config.json` (the only reliable channel for those settings: `arachnea` rejects `--server-port 8080` with `unknown argument: 8080`). The published `ports:` mapping follows `ARACHNEA_PORT` on both sides.
- Stamped `HEALTHCHECK` (HTTP probe on `127.0.0.1:$ARACHNEA_PORT` via `/bin/bash`, valid in every `ARACHNEA_NETWORK` mode), Compose service (`ports`, `shm_size`), `.dockerignore`, `.gitignore`, and `README.md` (release-tree layout, named context, versioned tags, run notes).
- `arachnea-docker/build-image.sh` and `arachnea-docker/build-image.cmd`: build wrappers around `docker compose build` that resolve the release version to package. `--version=<version>` (or `--version <version>`) wins; otherwise the highest `release-*` folder of the release tree is picked (`../releases`, or `ARACHNEA_RELEASES_DIR` from the environment or `.env`, same precedence as Compose), and the script stops with an explicit error when neither is available. The resolved version is exported as `ARACHNEA_VERSION` (the environment wins over `.env`, so the image tag, the two archive paths and the stamped `ENV ARACHNEA_VERSION` all follow it), the two Linux portable archives are checked before the Docker build starts (clear message instead of a Buildx `failed to compute cache key`), any other argument is forwarded to `docker compose build` (e.g. `--no-cache` to rebuild every layer instead of reusing the cache of a previous release, `--progress=plain` or `--push` for the output), and the script ends by printing the version-pinned `docker compose up -d` line that starts the image just built.

#### Changed

#### Fixed

- `arachnea-docker` now keeps version resolution exclusively in `build-image.sh` / `build-image.cmd`: each wrapper selects the highest `release-*` directory when no version is passed, builds the versioned tag and its `latest` alias, while a bare `docker compose up -d` always starts the local `arachnea-stream:latest` image. The wrappers explicitly use the Buildx builder associated with the active Docker context and verify both tags in the local image store, preventing a globally selected `docker-container` builder from reporting success while retaining the image only in its private BuildKit cache. `pull_policy: never` prevents Compose from pulling or rebuilding a nonexistent `release-latest` archive pair.
- `docker-compose.yml` now sets `security_opt: [seccomp=unconfined]`. The image starts Chromium and Docker's *default* seccomp profile blocks the `clone`/`unshare` syscalls it needs to create its user-namespace sandbox, so the browser aborted with `No usable sandbox! ... you can try using --no-sandbox` and every Cloudflare fetch failed with `Failed to initialize browser: Browser process exited with status ExitStatus(unix_wait_status(256))` — reproduced on a Windows/WSL2 Docker Desktop host with the amd64 image, while the arm64 image on Apple Silicon was unaffected. Unconfining seccomp keeps Chromium's sandbox enabled (unlike the previously documented `CHASER_EXTRA_ARGS: "--no-sandbox"` fallback, which now also actually reaches the browser — see the `arachnea-http` entry) and needs no host-side configuration, which matters because WSL2's managed VM exposes neither `kernel.unprivileged_userns_clone` nor a settable AppArmor profile. `README.md` documents the required option for the Compose and plain-`docker run` paths, and the stale "no `--no-sandbox` needed, the sandbox works out of the box" claims in the `Dockerfile` and `docker-entrypoint.sh` comments were corrected.
- The `apt` layer ends with a file-presence check rather than executing Chromium under foreign-architecture emulation, where Rosetta on Apple Silicon lacks Chromium's required SSE3 CPU feature.
- `arachnea-docker/docker-entrypoint.sh` no longer reuses `DISPLAY=:99` on the lock file alone: it verifies that an Xvfb process is actually alive, purges a stale `/tmp/.X99-lock` (leftover from a dead Xvfb, since `/tmp` survives `docker restart`) with its socket before starting a fresh server, and warns when the fresh server fails to come up instead of handing headed Chromium a dead `DISPLAY`.
- An image built from a Windows working tree (`core.autocrlf=true`, no
  `.gitattributes`) never started: tini restart-looped with `exec
  /usr/local/bin/docker-entrypoint.sh failed: No such file or directory`
  because the copied entrypoint carried CRLF line endings (shebang
  `#!/bin/sh\r`). A root `.gitattributes` now pins `*.sh` and `Dockerfile`
  to LF on every checkout (`*.cmd`/`*.bat` stay CRLF), and the `Dockerfile`
  strips a trailing CR from the entrypoint's first line at `COPY` time so
  working trees checked out before that pinning still build a working image.

### <u>arachnea-proxy</u>

#### Added

#### Changed

- Dynamic proxy provider refreshes now deduplicate endpoints and compare them with the memory/persistent inventory before probing. Existing protocol, HTTPS capability, health, latency, failure and cooldown data are reused while `last_checked` remains within `probe_ttl`; only new, untested or expired endpoints incur capability probes.
- Dynamic proxy provider loading is now single-flight per country and governed by a shared inventory cooldown: concurrent selections wait for the same refresh, and independent clients cannot repeat a provider consultation for 120 seconds after an attempt, including failures and empty lists. Multi-country selection checks every requested country's memory and persistent caches before provider loading, then progresses through eligible country refreshes in caller order.
- Dynamic proxy cache lifetime now uses a fixed 24-hour per-record validation age instead of a provider-refresh TTL or hard persistence expiration. Accepted HTTP responses renew the selected proxy without clearing unrelated destination failures; stale proxies remain selectable and are removed only after an observed proxy failure or configured origin rejection.
- Accepted origin responses now make the corresponding dynamic proxy the preferred candidate for that exact country, scheme, host and port instead of globally preferring it for every destination in the country. Concurrent domains therefore retain independent preferred proxies, and preferred reuse still passes every normal eligibility check.

#### Fixed

- Dynamic proxy failures reported by an HTTP engine after route establishment are now correlated back to the selected endpoint, persisted as destination-scoped failures, and allowed to release the provider-refresh cooldown. The stale HTTP connection pool and affinity are cleared before one replacement attempt, and a failed replacement is recorded without sending a third request.
- Dynamic country proxy connection and tunnel failures now release the affected countries' provider-refresh cooldown after excluding the failed endpoint. If the remaining memory and persistent cache is exhausted, the same request can therefore refresh the provider and discover replacement endpoints instead of failing behind the cooldown created by its initial load; configured HTTP origin rejections continue to rotate without granting an early provider refresh.
- Configured origin rejections such as HTTP 403 now exclude the dynamic proxy from the exact scheme, host and port for 24 hours through the exported `PROXY_DESTINATION_FAILURE_COOLDOWN` default. The exclusion remains persisted in `proxy-inventory`, and rejecting a preferred proxy clears only that destination's runtime preference.
- Dynamic geo-proxy selection no longer reloads an earlier country before checking cached candidates from later requested countries. Origin-specific proxy blocks remain scoped to their destination and no longer contribute to the threshold that marks a proxy globally unavailable.
- Dynamic proxy provider consultation is no longer controlled by a per-HTTP-client load scope. `ProxyInventory` alone decides whether exhausted cached candidates justify a refresh, while Scrapyfy clients only retain destination-to-proxy correlation needed to report accepted or rejected origin responses.
- Dynamic country proxies that time out or fail during transport/TLS setup are now marked globally KO for their cooldown instead of only being excluded from the current destination. This prevents a known dead proxy from adding another full timeout to subsequent M6+ origin requests; HTTP origin rejections remain destination-scoped, and SOCKS5 local-DNS fallback remains reserved for explicit remote DNS failures.

### <u>arachnea-stream</u>

#### Added

- Player geo-proxy routing now accepts ordered `proxy_countries` lists end to end, while preserving the legacy single-country request and YAML fields as fallbacks. Specialized M6 Play, TF1+, France TV and TV5MONDE+ resolvers carry the selected list through negotiation, proxied media URLs and deferred DRM requests.
- FranceTV resolved streams now expose `previously`, `coming_next`, `intro`, and `outro` chapters from K7 playback markers. Marker durations define chapter ends, bounded by the full video duration, while closing credits fall back to that duration when FranceTV only provides their start time. When `coming_next` and `outro` share a start time, the outro now starts at the end of `coming_next` and is discarded if no valid interval remains.

- `resolve_stream` responses can now expose an optional `subtitles` list (`{ lang?, label?, link }`) on `ResolvedPlayerStream`. Generic YAML-to-Rust conversion preserves resolver order, trims optional fields, discards tracks without a browser-consumable HTTP(S) or application-proxy `link`, and deduplicates exact `(lang, label, link)` entries. The field is omitted from JSON when empty, so existing resolvers keep their current payloads.
- Vidzy (`vidzy.cc` and `vidzy.live`) now extracts subtitle entries from `player.loadTracks`, retains only `kind: 'subtitles'` WebVTT tracks, and proxies each track with its required `Referer`, `Origin`, and `User-Agent` headers.
- Anime-Sama now exposes its daily release panels as selectable `subsections` under one daily-releases section, while Coflix adds its day, week, and month popular-content rails from the asynchronous `/ajax/movie/top` endpoints. Coflix preserves the source title, entry URL, and poster image without treating view counters as ratings or guessing a media type from its `/film/` URLs.
- The `arachnea-stream-hoster` group gains a StreamHG resolver (`streamhg.yaml`) covering the StreamHG-branded JW Player embeds served by rotating mirror domains such as `audinifer.com`, `vibuxer.com`, and `hanerix.com`, plus the `hgcloud.to` JavaScript router. Direct player recognition is content-based (the brand declared to the packed player setup), so no player mirror domain is required by the resolver; stable `hgcloud.to/e/<id>` entry URLs are mapped to a real StreamHG player mirror because the generic resolver deliberately does not execute their obfuscated redirect script. `resolve_stream` fetches the effective player page, unpacks the Dean Edwards Packer block, reads the page `links` object in the site's own priority (`hls4`, then `hls3`, then `hls2`) as ordered `stream_url` candidates whose order no longer depends on the key order the hoster emits, and returns the HLS manifest proxied with the effective embed-page `Referer`, the page title, the `#vplayer` poster, and the `/dl?op=get_slides` sprite VTT.
- New Boukè (`bouke-be.yaml`) legal source for the Belgian local channel bouke.media: the home page exposes its banners, navigation categories and editorial rails (each rail regrouped from its section-header paragraph plus its slide cards by `group_items_by_header`), `/info`, `/sport`, `/culture` and `/emissions` are browsable categories paginated through the zero-based Drupal pager, entries resolve their Freecaster player to an HLS manifest (replay and `/direct` live alike) and expose programme seasons through `get_season`, and the single live channel is listed by `list_lives`. Registered in `services.json` as disabled by default.

#### Changed

- TV5MONDE+ playback now derives its ordered proxy-country list from the entitlement `/play` response's comma-separated `materialProfile`, normalizing and deduplicating alpha-2 codes before applying them to the manifest, storyboard, and deferred Widevine license request. The player-supplied list remains the fallback when the profile is absent or unusable.
- M6 Play replay players now derive ordered geo-proxy countries from `/clips/0/areas/*/zone_id`: area `11` emits `AD, FR, GP, GF, MQ, YT, MC, NC, PF, RE, BL, MF, PM, TF, WF`, while area `34`, missing areas, and unknown values emit no geo-proxy constraint.
- The `arachnea-stream-hoster` Vidzy resolver is unified for `vidzy.cc` and `vidzy.live` (including optional `www.`). Its HLS decoder derives the XOR key from the request hostname, and all stream and subtitle proxy requests use the resolved request origin. The decoder returns its string through `document.write`, which is the supported `exec_js` string channel.
- The public Vue player propagates resolved subtitles through primary and fallback media sources, adds them as Video.js remote subtitle tracks, restores saved text-track preferences, and explicitly removes renderer-owned tracks on source changes and disposal.

#### Fixed

- ARTE live stream resolution now preserves the canonical `{proxy_countries}` HTTP template during configuration loading, then expands its runtime JSON list when `resolve_stream` executes instead of silently dropping the unresolved placeholder and using a direct connection. Its player descriptor uses the live config's `DE_FR` geoblocking rights as the ordered `[FR, DE]` proxy list, allowing a German proxy when no usable French proxy is available; the public frontend also accepts Scrapyfy's nested scalar-node representation for this static country list.
- TF1+ now derives proxy countries from `media.geoList` in `mediainfocombo`, retries a geo-blocked negotiation through those territories, and preserves the discovered list for the manifest, storyboard, and deferred Widevine license request. Resolver-level proxy retry loops and direct fallbacks have been removed; internal requests and the manifest/storyboard `/api/proxy` URLs delegate HTTP 403 rotation to the proxy layer.

- FranceTV playback now preserves every supplied non-empty proxy-country list and falls back to `FR` only when the player supplies no country, ensuring that HTTP 403 responses remain associated with a dynamic geo-proxy. It keeps one opaque proxy affinity across K7, manifest signing, DRM authorization, proxied manifest/subresource loading, and deferred Widevine licensing. Both internal HTTP calls and returned `/api/proxy` URLs classify HTTP 403 as a destination-specific rejection, clear the rejected affinity binding, rotate to another eligible proxy, and retry once; rewritten HLS key URLs preserve the inherited proxy options.
- Dynamic proxy selection now delegates exhausted-cache refresh decisions to the shared inventory, so independently created M6+ clients reuse the same per-country refresh cooldown instead of downloading and probing the same list again.
- A configured origin rejection (such as M6+ HTTP 403) now rotates away from the rejected proxy without granting the HTTP client a new provider refresh. The inventory may refresh only when its shared per-country cooldown allows it.
- M6+ now treats HTTP 403 responses during its authenticated playback sequence as rejection of the current geo-proxy, records a destination-scoped cooldown, and retries the request once with another cached proxy candidate.
- Boukè now declares `get_players`. Season episode cards carry no embedded player, so the frontend publishes their media link as the `players` collection and lazily loads it through the `get_players` command; without that query the backend returned an empty player list and "Regarder" silently started nothing. The Freecaster player block is factored into the shared `entry_players_freecaster` anchor reused by `get_entry` and `get_players`, both resolving `div.freecaster-player[data-video-id]` to the HLS rendition exposed at `/video/src`.
- Anime-Sama catalog cards now extract genres only once from their genre tags and exclude the decorative ellipsis tag, preventing duplicate or empty genre labels in search and home rails.
- Coflix search now parses the HTML returned by `/filter?keyword=…` instead of treating it as the retired JSON payload, restoring result titles, links, poster extraction, VF/VOSTFR language metadata, and next-page navigation.
- DoodStream-compatible embeds now return the generated CDN media URL instead of proxying their intermediate `/pass_md5/...` request. The resolver sends its JavaScript result through `document.write`, which is the `exec_js` string-return channel, extracts it before proxy wrapping, derives the media `Referer` from the resolved embed origin rather than hardcoding `playmogo.com`, and no longer exposes an unsupported storyboard.

- `arachnea-stream` no longer compiles or initializes the Ghostwire smart Cloudflare solver. Automatic Cloudflare handling now goes from `rquest` directly to the Chromium-backed `chaser-cf` solver, preventing the native `SIGSEGV` that restarted the Docker container during Papadustream searches on Linux arm64.

### <u>arachnea-scrapyfy</u>

#### Added

- Scraper HTTP configuration now supports `proxy_countries`, an ordered normalized country list that is forwarded to proxy routing as a JSON array and takes precedence over legacy `proxy_country`.
- Scraper HTTP configuration now supports `proxy_rejection_statuses`, allowing selected origin statuses to rotate a dynamic proxy and retry the request once.
- HTTP scraper queries now support `empty_on_statuses`, allowing source configurations to map selected response statuses to an empty typed result instead of parsing the error body.
- Proxy sources can expose a `protocols` array; the proxy provider selects one supported protocol deterministically while preserving a valid singular `protocol`. The disabled-by-default ProxyCompass source uses this support, maps ISO country codes to API country names, and requests at most 1,000 proxy candidates.

#### Changed

- Proxy protocol selection and runtime detection now share the priority `SOCKS5`, `SOCKS4`, `HTTPS`, `SOCKS4A`, then `HTTP`, making HTTP the lowest-priority option.
- Ghostwire is no longer enabled by default. Consumers that still need the smart Cloudflare solver must explicitly enable the `ghostwire` feature.

#### Fixed

- Text and binary scraper requests now rotate a dynamically selected proxy once when `arachnea-http` reports a proxy transport failure before an HTTP response, including failures occurring after an HTTP CONNECT tunnel was accepted. If the replacement also fails, both error contexts are retained and no third request is sent.
- Proxifly country lookups now treat a missing country file (`404`) as an empty proxy list, avoiding the misleading `Invalid JSON payload` error for unsupported countries such as Andorra (`AD`).

### <u>arachnea-http</u>

#### Added

#### Changed

#### Fixed

- The `chaser-cf` engine builds its default Chromium flags with `ChaserConfig::add_extra_arg` instead of `with_extra_args`. The latter *replaces* the whole flag set, so the defaults appended after `ChaserConfig::from_env()` silently discarded the flags that function had just parsed from `CHASER_EXTRA_ARGS`, making the documented override a no-op — a container passing `--no-sandbox` still started Chromium without it and hit `No usable sandbox!`.

### <u>front-public</u>

#### Added

- Player resolver descriptors now normalize ordered `resolver.proxy.countries` values and send `proxy_countries` to stream resolution, while also forwarding the first country through the legacy `proxy_country` field during the transition.

- Home and category rails can now render optional catalog subsections. The visible rail title combines parent and subsection labels, and accessible previous/next controls cycle through subsection rails with wraparound navigation.
- The player previous/next episode controls (`vjs-prev-video-control` / `vjs-next-video-control`) now show a rich preview on hover or keyboard focus instead of the former plain native tooltip, which is suppressed as soon as the neighboring episode is available. The preview is anchored bottom-left above the control bar and renders the navigation direction label (`entry.previousVideo` / `entry.nextVideo`), the adjacent episode title, its landscape poster on the left when the backend exposes one, and the shared media-card details block without its source fact (new `hideSource` prop on `MediaCardDetailsContent`). It stays display-only (`pointer-events: none`) so it never steals player interactions, its width is capped to the player width, and it hides itself when the neighboring item is unknown. The neighboring navigable episode is mapped to a `MediaItem` in `ProgramEntryDetails` (`toEntryEpisodeMediaItem`), the source display title is resolved in `EntryDetails` through `useServiceMetadata`, and both are threaded down through `EntryDetailsHeroContent`, `VideoPlayer` and `VideoJsMediaRenderer`.
- The player chapter handling now supports the `previously` chapter type. A *previously on* recap chapter shows the `Récapitulatif` / `Recap` label in the progress-bar overlay, and a dedicated skip button labelled *Passer le récapitulatif* / *Skip Recap* seeks to the end of the recap using the same mechanism as the intro button (`vjs-skip-previously-button`, wired in `installSkipChapterButton` through `SkipChapterType`).
- The player chapter handling now supports `coming_next` as a skippable chapter type, with localized progress-bar and skip-button labels plus a dedicated `videoPlayer.autoskip.coming_next` preference in the autoplay menu. Like intro autoskip, it is disabled by default and only applies while episode autoplay is enabled.
- The player settings add one `videoPlayer.autoskip.*` parameter per skippable chapter type (`previously`, `intro`, `coming_next`, `outro`, `ads`; all `false` by default). Advertising remains configured in the Lecteur / Player section, while the other types are configured from the autoplay menu. When a type's parameter is enabled, chapters of that type are skipped automatically as soon as playback enters them: `ads` unconditionally, other types only while episode autoplay is enabled. Contiguous same-type chapters are merged (`mergeContiguousChapters`) so a whole ad pod is jumped over with a single seek (`installAutoSkipChapters`, wired for both static and DASH period chapters).

#### Changed

- Home catalog normalization and multi-source merging preserve subsection boundaries and merge equivalent subsections only by their stable key under the same parent section.
- The player autoplay toggle button became an autoplay options menu (`vjs-episode-autoplay-menu`): clicking it now opens a Video.js popup menu (toggle button plus `.vjs-menu` sibling, positioned by the adaptive control-bar menu positioning) instead of toggling autoplay directly. The menu holds a checkbox entry for episode autoplay plus one entry per non-advertising skippable chapter type (labels reuse `settings.autoskip.previously` / `intro` / `coming_next` / `outro`, values write `videoPlayer.autoskip.*`), so recap/intro/next-preview/outro autoskip is configured exclusively from the player and those switches are absent from the parameters panel, which keeps only the advertising autoskip switch; the skip rule is unchanged (ads unconditionally, other types only while episode autoplay is enabled). The menu stays open when an entry is activated, closes on outside click, Escape, or when the controls go inactive, and refreshes its entries on open so parameters changed while the player was mounted never show stale states. Every menu entry renders a visible checkbox reflecting its checked state, selected entries keep the theme primary color without the bold weight, and the autoplay entry uses the shortened `player.autoplayMenu.autoplay` label ("Lecture automatique" / "Autoplay", replacing "Lecture automatique de l'épisode suivant" / "Autoplay the next episode"). The button now exposes `aria-haspopup`/`aria-expanded` plus a static `player.autoplayMenu.label` tooltip and keeps the active visual state for autoplay, replacing the action-dependent `player.enableAutoplay`/`player.disableAutoplay` titles (removed, superseded by the new `player.autoplayMenu.*` keys).
- Player skip chapter buttons (`vjs-skip-intro-button`, `vjs-skip-previously-button`, `vjs-skip-coming_next-button`, `vjs-skip-outro-button`, `vjs-skip-ads-button`) now follow the control bar state. While the control bar is visible they stay displayed for the whole chapter; while it is hidden (playing with an inactive user, the state that fades the control bar out) they are only revealed during the first `SKIP_CHAPTER_BUTTON_HIDDEN_CONTROLS_REVEAL_SECONDS` (5) seconds of the chapter and drop to 35px from the player bottom edge through the new `--controls-hidden` modifier class. Each button now renders its label followed by the Material Design `skip-forward` icon (`mdi-skip-forward`, `aria-hidden`, 27px) laid out with `inline-flex`. The `ads` skip button is exempt from the reveal window and stays displayed for the whole ad chapter even while the control bar is hidden. They additionally stay hidden until playback has started for the current source, reusing the `vjs-has-started` class Video.js sets on the first `play` — the same signal that hides the poster/title image. `installSkipChapterButton` therefore re-evaluates on `useractive`, `userinactive`, `play`, `playing`, `pause` and `loadstart` in addition to `timeupdate`, `loadedmetadata` and `seeked`. The button types are listed in `SKIP_CHAPTER_BUTTON_TYPES` (`intro`, `previously`, `coming_next`, `outro`, `ads`), with the always-visible exemption listed in `SKIP_CHAPTER_BUTTON_ALWAYS_VISIBLE_TYPES`; both call sites (`useVideoJsMediaRenderer`, DASH period chapters) install skip buttons by iterating that constant.

#### Fixed

- Media-card details now render the language fact with its own label and value, instead of incorrectly rendering the genre a second time or an empty genre value when only a language is available.
- Pinned home rails now render the active subsection entries and retain the cyclic subsection controls, rather than rendering the empty parent item list.
- The previous subsection control now appears before the rail title, while the next control remains in the header actions.

### <u>Other</u>

#### Added

#### Changed


