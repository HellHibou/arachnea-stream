import { bannerReferenceKey, MAX_CATEGORY_ANCESTORS } from '@/services/customHomeReferences'
import type {
  CategoryReference,
  CustomHomeBlock,
  CustomHomeDefinition,
  SectionReference,
} from '@/types/customHome'

/** @param value Untrusted value. @param path Field path used in errors. @returns Plain object. */
function record(value: unknown, path: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${path} must be an object`)
  }
  const prototype = Object.getPrototypeOf(value)
  if (prototype !== Object.prototype && prototype !== null) {
    throw new Error(`${path} must be a plain object`)
  }
  return value as Record<string, unknown>
}

/** @param value Untrusted value. @param path Field path. @returns Non-blank string. */
function text(value: unknown, path: string): string {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${path} must be non-blank`)
  return value
}

/** @param value Untrusted value. @param path Field path. @returns Exact identifier. */
function identifier(value: unknown, path: string): string {
  const result = text(value, path)
  if (result !== result.trim()) throw new Error(`${path} must not have surrounding whitespace`)
  return result
}

/** @param value Untrusted value. @param path Field path. @returns Optional display title. */
function nullableText(value: unknown, path: string): string | null {
  return value === null ? null : text(value, path)
}

/** @param value Untrusted value. @param path Field path. @returns Non-negative safe integer. */
function integer(value: unknown, path: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${path} must be a non-negative safe integer`)
  }
  return value
}

/** @param value Untrusted value. @param path Field path. @returns Boolean without coercion. */
function boolean(value: unknown, path: string): boolean {
  if (typeof value !== 'boolean') throw new Error(`${path} must be a boolean`)
  return value
}

/** @param value Untrusted value. @param path Field path. @returns Array without coercion. */
function array(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`${path} must be an array`)
  return value
}

/**
 * Copies only declared fields, rejecting accidental persistence of content or secrets.
 * @param value Object to inspect.
 * @param keys Allowed versioned fields.
 * @param path Field path.
 */
function fields(value: Record<string, unknown>, keys: string[], path: string): void {
  for (const key of Object.keys(value)) {
    if (!keys.includes(key)) throw new Error(`${path}.${key} is not a supported field`)
  }
}

/**
 * Validates a source-owned category reference before category resolution.
 * @param value Untrusted reference.
 * @param path Field path.
 * @returns Detached category reference without credentials.
 */
function category(value: unknown, path: string): CategoryReference {
  const input = record(value, path)
  fields(input, ['sourceId', 'categoryKey', 'descriptor', 'labelSnapshot', 'ancestorKeys', 'titleOverride'], path)
  const sourceId = identifier(input.sourceId, `${path}.sourceId`)
  const rawDescriptor = record(input.descriptor, `${path}.descriptor`)
  const descriptor: Record<string, string> = {}
  for (const [key, value] of Object.entries(rawDescriptor)) {
    if (['__proto__', 'constructor', 'prototype'].includes(key) ||
      /authorization|cookie|password|secret|token|api[_-]?key/i.test(key)) {
      throw new Error(`${path}.descriptor contains a forbidden credential field`)
    }
    if (typeof value !== 'string') throw new Error(`${path}.descriptor.${key} must be a string`)
    if (/(?:[?&](?:access_token|token|api_key|password|secret)=)|(?:https?:\/\/[^/]*@)/i.test(value)) {
      throw new Error(`${path}.descriptor.${key} contains credentials`)
    }
    descriptor[key] = value
  }
  if (descriptor.name !== sourceId || (descriptor.source && descriptor.source !== sourceId)) {
    throw new Error(`${path}.descriptor must belong to ${sourceId}`)
  }
  const ancestorKeys = input.ancestorKeys === undefined ? [] : array(input.ancestorKeys, `${path}.ancestorKeys`)
    .map((key, index) => identifier(key, `${path}.ancestorKeys[${index}]`))
  if (ancestorKeys.length > MAX_CATEGORY_ANCESTORS) throw new Error(`${path}.ancestorKeys is too long`)
  return {
    sourceId,
    ...(ancestorKeys.length ? { ancestorKeys } : {}),
    categoryKey: identifier(input.categoryKey, `${path}.categoryKey`),
    descriptor,
    labelSnapshot: text(input.labelSnapshot, `${path}.labelSnapshot`),
    titleOverride: input.titleOverride === undefined ? null : nullableText(input.titleOverride, `${path}.titleOverride`),
  }
}

/** @param value Untrusted reference. @param path Field path. @returns Detached section reference. */
function section(value: unknown, path: string): SectionReference {
  const input = record(value, path)
  fields(input, ['sourceId', 'sectionKey', 'subsectionKey', 'labelSnapshot', 'category'], path)
  const sourceId = identifier(input.sourceId, `${path}.sourceId`)
  const origin = input.category === undefined ? undefined : category(input.category, `${path}.category`)
  if (origin && origin.sourceId !== sourceId) throw new Error(`${path}.category must belong to ${sourceId}`)
  return {
    sourceId,
    ...(origin ? { category: origin } : {}),
    sectionKey: identifier(input.sectionKey, `${path}.sectionKey`),
    ...(input.subsectionKey === undefined ? {} : {
      subsectionKey: identifier(input.subsectionKey, `${path}.subsectionKey`),
    }),
    labelSnapshot: nullableText(input.labelSnapshot, `${path}.labelSnapshot`),
  }
}

/**
 * Validates version 1 without migrating, resetting, or executing untrusted stored data.
 * @param value Definition read from storage or supplied by the future editor.
 * @returns Detached definition containing only supported declarative fields.
 * @throws Error for unknown schema versions, malformed data, or duplicate block ids.
 */
export function validateCustomHomeDefinition(value: unknown): CustomHomeDefinition {
  const input = record(value, 'home')
  if (input.schemaVersion !== 1) {
    throw new Error(`Unsupported custom home schema version: ${String(input.schemaVersion)}`)
  }
  fields(input, [
    'schemaVersion', 'id', 'name', 'navigationOrder', 'bannerSources', 'categoryShortcuts',
    'blocks', 'sectionMergeMode', 'showBookmarks', 'createdAt', 'updatedAt',
  ], 'home')
  const blockIds = new Set<string>()
  const blocks = array(input.blocks, 'home.blocks').map((value, index): CustomHomeBlock => {
    const path = `home.blocks[${index}]`
    const block = record(value, path)
    fields(block, ['id', 'kind', 'reference', 'titleOverride', 'displayOptions'], path)
    const display = block.displayOptions === undefined ? undefined : record(block.displayOptions, `${path}.displayOptions`)
    if (display) {
      fields(display, ['collectionMode', 'thumbnailOrientation', 'thumbnailImageFit'], `${path}.displayOptions`)
      for (const [key, values] of Object.entries({ collectionMode: ['grid', 'single-row', 'list'],
        thumbnailOrientation: ['portrait', 'landscape'], thumbnailImageFit: ['contain', 'cover'] })) {
        if (display[key] !== undefined && !values.includes(display[key] as string)) throw new Error(`${path}.displayOptions.${key} is invalid`)
      }
    }
    const options = display ? { displayOptions: { ...display } as import('@/types/customHome').CustomHomeDisplayOptions } : {}
    const id = identifier(block.id, `${path}.id`)
    if (blockIds.has(id)) throw new Error(`${path}.id is duplicated`)
    blockIds.add(id)
    const titleOverride = nullableText(block.titleOverride, `${path}.titleOverride`)
    if (block.kind === 'home-section') {
      return { id, kind: block.kind, reference: section(block.reference, `${path}.reference`), titleOverride, ...options }
    }
    if (block.kind === 'category-content') {
      return { id, kind: block.kind, reference: category(block.reference, `${path}.reference`), titleOverride, ...options }
    }
    throw new Error(`${path}.kind is unsupported`)
  })
  const bannerSources = array(input.bannerSources, 'home.bannerSources')
    .map((value, index) => {
      const path = `home.bannerSources[${index}]`
      return typeof value === 'string' ? identifier(value, path) : category(value, path)
    })
  if (new Set(bannerSources.map(bannerReferenceKey)).size !== bannerSources.length) {
    throw new Error('home.bannerSources must not contain duplicates')
  }
  if (input.sectionMergeMode !== 'none' && input.sectionMergeMode !== 'title') {
    throw new Error('home.sectionMergeMode must be none or title')
  }
  const createdAt = integer(input.createdAt, 'home.createdAt')
  const updatedAt = integer(input.updatedAt, 'home.updatedAt')
  if (updatedAt < createdAt) throw new Error('home.updatedAt must not precede createdAt')
  return {
    schemaVersion: 1,
    id: identifier(input.id, 'home.id'),
    name: text(input.name, 'home.name').trim(),
    navigationOrder: integer(input.navigationOrder, 'home.navigationOrder'),
    bannerSources,
    categoryShortcuts: array(input.categoryShortcuts, 'home.categoryShortcuts')
      .map((value, index) => category(value, `home.categoryShortcuts[${index}]`)),
    blocks,
    sectionMergeMode: input.sectionMergeMode,
    showBookmarks: boolean(input.showBookmarks, 'home.showBookmarks'),
    createdAt,
    updatedAt,
  }
}
