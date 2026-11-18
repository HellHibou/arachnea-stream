import { acceptPlayerPreferences } from '@/services/profilePlayerPreferences'
import { nextTick } from 'vue'
import { setLanguage } from '@/i18n'
import { useStorage } from '@/services/storage'
import { applyTheme, findThemePreset } from '@/services/theme'
import { isCurrentProfile, type ProfileContext } from '@/services/profileContext'
import type { ProfileSnapshot } from '@/services/profileApi'

let application = 0

/** Copies JSON preference documents without retaining reactive or nested references. @param value JSON value to copy. */
export function copyPreferences<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

/** Applies a complete confirmed snapshot, ignoring partial operations and obsolete contexts.
 * @param snapshot Server snapshot after initialization or a completed save.
 * @param context Profile generation that requested the snapshot.
 */
export async function applyProfilePreferences(snapshot: ProfileSnapshot, context: ProfileContext): Promise<void> {
  if (!isCurrentProfile(context) || (snapshot.pending_operation && snapshot.pending_operation.state !== 'completed')) return
  const request = ++application
  const storage = useStorage()
  const parameters = storage.getParameters()
  if (parameters.language.value !== snapshot.common.values.language) await setLanguage(snapshot.common.values.language)
  await nextTick()
  if (!isCurrentProfile(context) || request !== application) return
  parameters.theme.value = snapshot.common.values.theme
  applyTheme(findThemePreset(snapshot.common.values.theme))
  const values = snapshot.streaming?.values
  if (!values) return
  for (const key of Object.keys(parameters) as Array<keyof typeof parameters>) {
    if (Object.prototype.hasOwnProperty.call(values, key) && JSON.stringify(parameters[key].value) !== JSON.stringify(values[key])) {
      parameters[key].value = copyPreferences(values[key]) as never
    }
  }
  storage.getHomePreferences().showSectionEditingButtons.value = values['arachnea.home.showSectionEditingButtons'] as boolean
  await nextTick()
  if (isCurrentProfile(context) && request === application && acceptPlayerPreferences(snapshot, context)) storage.applyVideoPlayerPreferences(values)
}
