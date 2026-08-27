# Linux installers (.deb/.rpm/.AppImage) via Docker — analyse et logique de résolution

Objectif (issue initiale) : re-évaluer et implémenter la génération des paquets
`deb`/`rpm`/`AppImage` depuis le conteneur Docker dans `build-release/`,
**en conservant** l'artefact portable `.tar.gz` existant.

Statut : quasi terminé. `.deb` et `.rpm` produits avec succès ; l'`.AppImage`
échoue dans ce conteneur Docker Desktop (macOS, pas d'accès FUSE) — le code
rend cet échec non-bloquant (voir §6). Ce fichier trace l'étude de faisabilité,
les modifications, et la logique de résolution des problèmes rencontrés.

---

## 1. Faisabilité (recherche sur les sources tauri-bundler v2)

| Bundle | Implémentation | Outils externes requis |
|---|---|---|
| `.deb` | 100 % Rust (`debian.rs` : crates `tar`, `ar`, `flate2`) | aucun |
| `.rpm` | 100 % Rust via crate `rpm-rs` (`rpm.rs`) | aucun (signature optionnelle) |
| `.AppImage` | télécharge linuxdeploy/AppRun/plugin-gtk depuis GitHub (caché sous `~/.cache/tauri`), lancé avec `--appimage-extract-and-run` + `APPIMAGE_EXTRACT_AND_RUN=1` | **pas de FUSE** ; accès Internet au 1er bundling |

=> Tous les formats fonctionnent dans un conteneur non privilégié. Le CLI Tauri
(`cargo tauri build`) doit être présent dans l'image ; le frontend est déjà
monté (repos `/io`) et pré-construit par l'hôte, donc `beforeBuildCommand: null`.

Point bloquant identifié : upstream ne publie un binaire `cargo-tauri`
précompilé que pour `x86_64-unknown-linux-gnu` ; pas d'asset `aarch64-linux`
récent. Le port arm64 de l'image compile donc `tauri-cli` depuis crates.io.
Version épinglée : `tauri-cli-v2.11.4`.

## 2. Modifications apportées au code

Fichiers modifiés : `build-release/docker/Dockerfile`, `docker.mjs`,
`capabilities.mjs`, `release.mjs`, `install-tools.mjs` (commentaire),
`README.md`, `CHANGELOG.md`.

Résumé :
- Table `DOCKER_BUNDLED_TARGETS` + export `dockerBundlesFor()` (intersection
  bundles config x support image).
- `needsDockerBuild()` se déclenche aussi sur les bundles déclarés.
- `docker.mjs` : refactor `crossRunBase()`, export `crossBundleArgs()` (un seul
  `cargo tauri build --target <triple> --config
  '{"build":{"beforeBuildCommand":null},"bundle":{"targets":[...]}}'`),
  `describeCrossBundling()`, sonde `assertCrossImageFor()`.
- `release.mjs` : branche Docker choisit bundling Tauri vs build brut ; l'étape
  d'assemblage hôte copie/checksumme les installateurs conteneur comme les natifs.
- Image bumpée `arachnea-cross-builder:1.0.0 -> 1.1.0` (règle AGENTS.md).
- README + CHANGELOG mis à jour.

## 3. Problème 1 : échec `cargo install` arm64 (musl)

**Symptôme** : build image `linux/arm64` -> `linking with cc failed` :
`_DYNAMIC` indéfini, symboles `libm` manquants (`sinf`, `powf`, ...),
conflits glibc/musl.

**Cause** : la config cargo de l'image de base
(`joseluisq/rust-linux-darwin-builder`) force une cible **musl statique** pour
`cargo install` sur le port aarch64 ; `tauri-cli` compilé pour
`aarch64-unknown-linux-musl` mélangeait objets glibc système et runtime musl.

**Correctif** : `--target aarch64-unknown-linux-gnu` dans le Dockerfile.

**Validation** : rebuild arm64 OK ; `cargo tauri --version` -> `tauri-cli 2.11.4`.

## 4. Problème 2 : tag image mono-architecture (multi-arch local)

**Symptôme** : le tag `1.1.0` ne pointait que vers UNE variante à la fois ; le
probe `linux/amd64` échouait après un rebuild arm64 et inversement, avec l'erreur
trompeuse `pull access denied ... requires docker login`.

**Analyse** : Docker Desktop conserve une seule image par tag. L'ancienne
`1.0.0` était publiée comme "manifest list amd64 + arm64". Voies tentées :
1. `docker manifest create` local -> échec `denied/unauthorized` (exige un
   registre).
2. Builds séparés `1.1.0-amd64` / `1.1.0-arm64` -> chaque variante OK, mais non
   regroupables sous `1.1.0` sans registre local.

**Résolution retenue** : un **build unique multi-plateforme** :
```
docker build --platform linux/amd64,linux/arm64 \
  --tag arachnea-cross-builder:1.1.0 build-release/docker
```
=> manifest list local accessible via `docker run --platform`. Les deux
variantes renvoient `tauri-cli 2.11.4`.

**Garde ajoutée** : `assertCrossImageFor(platform)` sonde la variante requise
avant chaque build (échec actionnable = commande `docker build` à exécuter),
au lieu du message registre trompeur.

## 5. Problème 3 (résolu) : panique du bundler au rpm, après le deb

**Symptôme** (run réel `linux-x86_64`) :
```
Built application at: .../release/arachnea
  Patching .../release/arachnea with bundle type information: deb
Bundling Arachnéa_0.1.0_amd64.deb
  Patching .../release/arachnea with bundle type information: rpm
thread 'main' panicked at crates/tauri-bundler/src/bundle.rs:86:45:
Could not read binary file.: Os { code: 2, kind: NotFound, "No such file or directory" }
```

Le `.deb` est créé ; le panique survient au `patch_binary(std::fs::read)` du
type suivant (rpm) sur le binaire `release/arachnea`.

**Hypothèse** : dans `bundle_project` (tauri-bundler), chaque type de paquet est
préparé avec un binaire/copie dédié et `patch_binary` relit un chemin qui
n'existe pas pour le 2e type — possiblement lié à un nettoyage (`remove_dir`) du
répertoire du type précédent, ou à un chemin dérivé du `productName` /
`mainBinary` différent entre deb et rpm.

**Diagnostic (confirmé empiriquement)** : le bundler `patch_binary` relit le
binaire `release/arachnea` entre chaque type ; pour le 2e type (rpm) il obtient
`NotFound` sur le volume monté. Tests manuels : `bundle.targets=["deb"]` seul ->
OK (.deb produit) ; `bundle.targets=["rpm"]` seul -> OK (.rpm produit,
`Finished 1 bundle`). => **le bug est le multi-types dans UN seul processus**.

**Correctif** : un `cargo tauri build` par type. `crossBundleArgs()` accepte un
paramètre `bundles` ; `release.mjs` boucle sur `dockerBundlesFor()` et lance une
passe par type. Chaque passe réutilise le binaire compilé (cargo en cache) pour
le portable.

**Note performance** : sur cet Apple Silicon, le conteneur amd64 est émulé
(QEMU), donc chaque bundling est lent (~10 min pour le rpm) ; natif en prod.

## 6. Problème 4 : AppImage échoue dans Docker Desktop (FUSE)

**Validation** : avec le correctif par-type, la passe `deb` -> `Finished 1 bundle`
(OK), `rpm` -> `.deb` puis `.rpm` produits. La passe `appimage` télécharge
linuxdeploy + les plugins, génère l'AppDir, mais `linuxdeploy` plante sur
`subprocess failed (exit 2)`.

**Diagnostic** : `squashfs-tools`/`mksquashfs` ajouté (couche séparée, pour
garder le cache des couches tauri-cli) — l'AppImage est bien générée sur un hôte
Linux. Ici, sur Docker Desktop (macOS), l'exécution de linuxdeploy requiert
**FUSE** (`dlopen libfuse.so.2`) pour certaines sous-étapes (relançement des
plugins, `-plugin gtk` étape "Copying more libraries"), or Docker Desktop ne
fournit pas `/dev/fuse`. Test confirmé : `patchelf` écrit correctement sur le
volume monté (donc pas un souci de mount), mais linuxdeploy échoue à tourner
dans ce conteneur sans FUSE.

**Décision / correctif** : rendre la production **robuste par type**. Chaque
passe `cargo tauri build` est tentée indépendamment ; si un type échoue
(typiquement `.AppImage` sur un hôte sans FUSE), on avertit et on continue, et
la plateforme reste **succès** tant qu'au moins un bundle (ou le portable) est
produit. => sur ce Mac, `deb` + `rpm` + portable sont livrés, l'AppImage est
signalée en warning et produit sur un hôte Linux (hôte natif, GitHub Actions
ou conteneur avec `/dev/fuse`).

## 7. Commandes utiles (validation)

```bash
# reconstruire l'image multi-arch locale
docker build --platform linux/amd64,linux/arm64 \
  --tag arachnea-cross-builder:1.1.0 build-release/docker

# sonder une variante
docker run --rm --platform linux/amd64 arachnea-cross-builder:1.1.0 sh -c 'cargo tauri --version'

# lancer une plateforme
node build-release/release.mjs --no-install -p linux-x86_64
```

## 8. Reste à faire

- [x] Diagnostiquer le panique du bundler rpm (cause : multi-types en un processus).
- [x] Produire `deb` et `rpm` (une passe par type).
- [x] Tenter l'AppImage + comprendre l'échec FUSE sur Docker Desktop.
- [x] Implémenter la robustesse par type dans la boucle (échec AppImage non-bloquant).
- [x] Valider l'assemblage final `releases/release-0.1.0/linux/` (deb + rpm + portable + checksums).
- [ ] (optionnel) vérifier `linux-arm64` sous émulation QEMU.

