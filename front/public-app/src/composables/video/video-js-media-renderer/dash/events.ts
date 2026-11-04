/**
 * dash.js events consumed by the DASH modules.
 *
 * The dash.js build bundled inside the source handler is not importable, so the
 * event names are declared as literals instead of using the events enum.
 */
export const DASH_EVENTS = {
  manifestLoaded: 'manifestLoaded',
  periodSwitchCompleted: 'periodSwitchCompleted',
  playbackMetadataLoaded: 'playbackMetaDataLoaded',
  qualityChangeRendered: 'qualityChangeRendered',
  streamInitialized: 'streamInitialized',
} as const
