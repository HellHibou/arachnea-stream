/** Minimum accepted public password length in Unicode scalar values. */
export const PASSWORD_MIN_LENGTH = 4
/** Maximum accepted public password length in Unicode scalar values. */
export const PASSWORD_MAX_LENGTH = 128

/** Increasing password complexity levels shown by the strength indicator. */
export type PasswordStrengthLevel = 'veryWeak' | 'weak' | 'fair' | 'good' | 'strong'

/** Complexity result shared by the public and admin password forms. */
export interface PasswordStrength {
  /** Coarse level translated by the owning app. */
  level: PasswordStrengthLevel
  /** Progress value from 0 to 100 for a strength bar. */
  percent: number
}

/** Counts the distinct character classes used by a password. */
function characterClasses(value: string): number {
  let classes = 0
  if (/\p{Ll}/u.test(value)) classes += 1
  if (/\p{Lu}/u.test(value)) classes += 1
  if (/\p{N}/u.test(value)) classes += 1
  if (/[^\p{L}\p{N}\s]/u.test(value)) classes += 1
  return classes
}

/**
 * Scores an in-memory password on length and character variety.
 *
 * The score never blocks submission: the server only enforces the minimum
 * length, while this indicator helps the user pick a stronger secret.
 *
 * @param value - Password draft, never persisted by this helper.
 * @returns Coarse level and progress percentage for the strength bar.
 */
export function passwordStrength(value: string): PasswordStrength {
  const length = Array.from(value).length
  if (length === 0) return { level: 'veryWeak', percent: 0 }
  const classes = characterClasses(value)
  let score = length >= PASSWORD_MIN_LENGTH ? 1 : 0
  if (length >= 8) score += 1
  if (classes >= 2) score += 1
  if (length >= 8 && classes >= 3) score += 1
  if (length >= 12 && classes >= 3) score += 1
  const levels: PasswordStrengthLevel[] = ['veryWeak', 'weak', 'fair', 'good', 'strong']
  return { level: levels[score] ?? 'veryWeak', percent: score * 20 }
}
