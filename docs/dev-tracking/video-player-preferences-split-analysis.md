# Analyse : Éclatement des préférences du lecteur vidéo en clés individuelles

> Généré le 2026-07-30 — Analyse avant implémentation

---

## 1. État actuel

Toutes les préférences du lecteur vidéo sont stockées dans une **unique clé localStorage** :

```
arachnea.videoPlayer.preferences
```

Cette clé contient un objet JSON avec l'ensemble des champs :

```json
{
  "volume": 1,
  "muted": false,
  "qualityLabel": null,
  "audioTrack": null,
  "textTrack": { "id": null, "language": null, "label": null, "kind": null, "mode": "disabled" },
  "textTrackSettings": { "backgroundColor": "#000", ... }
}
```

### Mécanisme actuel (simplifié)

1. **Initialisation** (`front/src/services/storage.ts:239-243`) : lecture depuis `arachnea.videoPlayer.preferences`, désérialisation JSON, sanitization via `sanitizeVideoPlayerPreferences()`.
2. **Persistance** (`storage.ts:321-330`) : un `watch` avec `{ deep: true }` sur le ref `videoPlayerPreferences` écrit l'objet complet dans la même clé à chaque changement.
3. **API publique** (`storage.ts:454-464`) : `getVideoPlayerPreferences()` / `setVideoPlayerPreferences()` exposées par le store Pinia.
4. **Consommation** (`front/src/composables/video/useVideoPlayer.ts:35-66, 355-364, 905-911`) : le composable lit/écrit les préférences via le store.

### Fichiers impactés

| Fichier | Rôle |
|---|---|
| `front/src/services/storage.ts` | Interface `VideoPlayerPreferences`, ref, watcher, sanitization |
| `front/src/composables/video/useVideoPlayer.ts` | `createInitialVideoPlayerState`, `createVideoPlayerPreferences`, appels store |
| `front/src/types/media.ts` | Types `VideoJsTrackPreference`, `VideoJsTextTrackPreference`, `VideoJsTextTrackSettings` |

---

## 2. Objectif

Stocker **chaque champ** de `VideoPlayerPreferences` sous sa **propre clé localStorage** :

| Clé | Type | Valeur exemple |
|---|---|---|
| `arachnea.videoPlayer.preferences.volume` | `number` | `1` |
| `arachnea.videoPlayer.preferences.muted` | `boolean` | `false` |
| `arachnea.videoPlayer.preferences.qualityLabel` | `string \| null` | `null` |
| `arachnea.videoPlayer.preferences.audioTrack` | `VideoJsTrackPreference \| null` | `null` |
| `arachnea.videoPlayer.preferences.textTrack` | `VideoJsTextTrackPreference` | `{"id":null,"language":null,"label":null,"kind":null,"mode":"disabled"}` |
| `arachnea.videoPlayer.preferences.textTrackSettings` | `VideoJsTextTrackSettings \| null` | `null` |

---

## 3. Changements nécessaires

### 3.1. `storage.ts` — Constantes de clés

Remplacer `VIDEO_PLAYER_PREFERENCES_KEY` unique par des constantes individuelles :

```typescript
const VIDEO_PLAYER_PREFERENCES_KEY = 'videoPlayer.preferences'
// ↓
const VIDEO_PLAYER_VOLUME_KEY = 'videoPlayer.preferences.volume'
const VIDEO_PLAYER_MUTED_KEY = 'videoPlayer.preferences.muted'
const VIDEO_PLAYER_QUALITY_LABEL_KEY = 'videoPlayer.preferences.qualityLabel'
const VIDEO_PLAYER_AUDIO_TRACK_KEY = 'videoPlayer.preferences.audioTrack'
const VIDEO_PLAYER_TEXT_TRACK_KEY = 'videoPlayer.preferences.textTrack'
const VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY = 'videoPlayer.preferences.textTrackSettings'
```

### 3.2. `storage.ts` — Ref → refs individuels

Remplacer le ref unique `videoPlayerPreferences` par 6 refs individuels :

```typescript
const videoPlayerVolume = ref<number>(...)
const videoPlayerMuted = ref<boolean>(...)
const videoPlayerQualityLabel = ref<string | null>(...)
const videoPlayerAudioTrack = ref<VideoJsTrackPreference | null>(...)
const videoPlayerTextTrack = ref<VideoJsTextTrackPreference>(...)
const videoPlayerTextTrackSettings = ref<VideoJsTextTrackSettings | null>(...)
```

Chaque ref est initialisé par `getStoreItem` + son sanitizer spécifique. Si la clé est absente, le sanitizer retourne sa valeur par défaut.

### 3.3. `storage.ts` — Watchers individuels

Remplacer le `watch` unique avec `{ deep: true }` par 6 watchers individuels, chacun persistant sa propre clé avec son propre sanitizer :

```typescript
watch(videoPlayerVolume, (next) => {
  setStoreItem(VIDEO_PLAYER_VOLUME_KEY, sanitizeVideoVolume(next))
})
watch(videoPlayerMuted, (next) => {
  setStoreItem(VIDEO_PLAYER_MUTED_KEY, sanitizeBoolean(next, false))
})
// ... etc
```

Avantage : plus de `deep: true` coûteux, mise à jour atomique par champ.

### 3.4. `storage.ts` — API publique

Adapter `getVideoPlayerPreferences()` et `setVideoPlayerPreferences()` pour assembler/désassembler l'objet `VideoPlayerPreferences` à partir des 6 refs :

```typescript
function getVideoPlayerPreferences(): VideoPlayerPreferences {
  return {
    volume: videoPlayerVolume.value,
    muted: videoPlayerMuted.value,
    qualityLabel: videoPlayerQualityLabel.value,
    audioTrack: videoPlayerAudioTrack.value,
    textTrack: videoPlayerTextTrack.value,
    textTrackSettings: videoPlayerTextTrackSettings.value,
  }
}

function setVideoPlayerPreferences(prefs: VideoPlayerPreferences): void {
  videoPlayerVolume.value = sanitizeVideoVolume(prefs.volume)
  videoPlayerMuted.value = sanitizeBoolean(prefs.muted, false)
  videoPlayerQualityLabel.value = sanitizeQualityPreference(prefs.qualityLabel)
  videoPlayerAudioTrack.value = sanitizeTrackPreference(prefs.audioTrack)
  videoPlayerTextTrack.value = sanitizeTextTrackPreference(prefs.textTrack)
  videoPlayerTextTrackSettings.value = sanitizeTextTrackSettings(prefs.textTrackSettings)
}
```

### 3.5. `storage.ts` — Suppression de `sanitizeVideoPlayerPreferences`

Cette fonction aggregate n'est plus nécessaire ; chaque sanitizer est appelé individuellement au moment de la lecture/écriture de son champ.

### 3.6. `useVideoPlayer.ts` — Aucun changement

Le composable utilise déjà `getVideoPlayerPreferences()` et `setVideoPlayerPreferences()` via le store. L'interface `VideoPlayerPreferences` ne change pas. **Aucune modification nécessaire** dans ce fichier.

### 3.7. `types/media.ts` — Aucun changement

Les types ne sont pas impactés.

---

## 4. Plan d'implémentation

1. **`storage.ts`** :
   - Ajouter les 6 constantes de clés individuelles.
   - Remplacer le ref unique par 6 refs individuels (initialisés via `getStoreItem` + sanitizer).
   - Remplacer le watcher deep par 6 watchers individuels.
   - Mettre à jour `getVideoPlayerPreferences()` et `setVideoPlayerPreferences()`.
   - Supprimer `sanitizeVideoPlayerPreferences()`.
   - Mettre à jour les exports du store.

2. **Tests** : vérifier que les tests existants (s'il y en a) passent.

---

## 5. Risques et considérations

- **Performance** : 6 appels `localStorage.getItem` au lieu d'un seul à l'initialisation. Négligeable.
- **Performance** : 6 appels `localStorage.setItem` au lieu d'un seul à chaque mise à jour. Théoriquement plus d'écritures, mais chaque mise à jour ne change typiquement qu'un champ à la fois → moins de données sérialisées.
- **Atomicité** : Chaque champ est persisté indépendamment → pas de perte totale si une écriture échoue.