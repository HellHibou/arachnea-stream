/** CSS class suppressing initial playback overlays during an in-place variant switch. */
export const VARIANT_SWITCHING_CLASS = 'arachnea-variant-switching'

/** Offset in seconds applied to live playback timer. */
export const TIMER_OFFSET_LIVE = 10

/** CSS class name applied when remaining time display is enabled. */
export const REMAINING_TIME_CLASS = 'arachnea-show-remaining-time'

/** CSS class name applied to the player while subtitles or captions are showing. */
export const SUBTITLES_ACTIVE_CLASS = 'arachnea-subtitles-active'

/** Interval in seconds between playback position saves. */
export const PLAYBACK_SAVE_STEP_SECONDS = 5

/**
 * Playback rates exposed by the control bar speed menu.
 *
 * Video.js builds its menu items from the last entry to the first, so this
 * list is stored from fastest to slowest to display the slowest rate first.
 */
export const PLAYBACK_RATES = [3, 2, 1.75, 1.5, 1.25, 1, 0.75, 0.5, 0.25]

/** CSS class name applied to the player while a non-default playback rate is active. */
export const CUSTOM_PLAYBACK_RATE_CLASS = 'arachnea-custom-playback-rate'

/** CSS class name for the episode autoplay toggle control. */
export const EPISODE_AUTOPLAY_CONTROL_CLASS = 'vjs-episode-autoplay-toggle'

/** CSS class name for the active state of the episode autoplay toggle control. */
export const EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS = 'vjs-episode-autoplay-toggle--active'

/** CSS class name for the episode autoplay menu wrapper control. */
export const EPISODE_AUTOPLAY_MENU_CLASS = 'vjs-episode-autoplay-menu'

/** CSS class name for the previous video control. */
export const PREV_VIDEO_CONTROL_CLASS = 'vjs-prev-video-control'

/** CSS class name for the next video control. */
export const NEXT_VIDEO_CONTROL_CLASS = 'vjs-next-video-control'
