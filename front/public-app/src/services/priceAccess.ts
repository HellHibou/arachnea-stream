/** Translation callback used to format an access-price label. */
type Translate = (key: string, params?: Record<string, string>) => string

/** Canonical amount and ISO 4217 currency code extracted from a price value. */
interface MonetaryPrice {
  /** Numeric amount written with a decimal point. */
  amount: number
  /** ISO 4217 currency code. */
  currency: string
}

/** Canonical monetary price pattern emitted by scraper YAML files. */
const MONETARY_PRICE_PATTERN = /^(\d+(?:\.\d+)?)\s+([A-Z]{3})$/

/**
 * Validates and normalizes one optional raw access-price value.
 *
 * @param value Raw value returned by a scraper result.
 * @returns Canonical `premium`, `free`, or `<amount> <ISO>` value; otherwise `null`.
 */
export function normalizePriceAccess(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }

  const normalized = value.trim()
  const semanticValue = normalized.toLocaleLowerCase()
  if (semanticValue === 'free' || semanticValue === 'premium') {
    return semanticValue
  }

  const monetaryPrice = parseMonetaryPrice(normalized)
  return monetaryPrice ? `${monetaryPrice.amount} ${monetaryPrice.currency}` : null
}

/**
 * Formats a normalized access-price value for the active interface language.
 *
 * Free and absent values intentionally remain silent because they do not need
 * an access warning in the catalog.
 *
 * @param price Normalized access-price value.
 * @param locale Active application locale.
 * @param translate Translation callback.
 * @returns Display label, or `null` when no label should be shown.
 */
export function formatPriceAccess(
  price: string | null,
  locale: string,
  translate: Translate,
): string | null {
  if (!price || price === 'free') {
    return null
  }

  if (price === 'premium') {
    return translate('media.subscriptionRequired')
  }

  const monetaryPrice = parseMonetaryPrice(price)
  if (!monetaryPrice) {
    return null
  }

  const formattedAmount = new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: monetaryPrice.currency,
  }).format(monetaryPrice.amount)

  return translate('media.purchaseRental', { price: formattedAmount })
}

/**
 * Parses the canonical monetary access-price representation.
 *
 * @param value Candidate price value.
 * @returns Parsed amount and currency, or `null` when the value is invalid.
 */
function parseMonetaryPrice(value: string): MonetaryPrice | null {
  const match = MONETARY_PRICE_PATTERN.exec(value)
  if (!match) {
    return null
  }

  const rawAmount = match[1]
  const currency = match[2]
  if (!rawAmount || !currency) {
    return null
  }

  const amount = Number(rawAmount)
  return Number.isFinite(amount) && amount >= 0 ? { amount, currency } : null
}