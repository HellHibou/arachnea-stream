# Arachnea Frontend Agent Profile

## Project Context
- This directory contains the Vue.js frontend built with Vue 3, TypeScript, and Vite.
- UI code lives primarily under `src/components/*`, shared types under `src/types/*`, and local sample or fixture data under `src/data/*`.
- Prefer small, local component updates over broad rewrites.
- Preserve the established visual direction unless the request is explicitly about redesign.

## Component and Data Consistency
- Keep component props, emitted events, local data files, and TypeScript types in sync.
- If a component starts using new or renamed fields, update the relevant files under `src/types/*` and `src/data/*` in the same change.
- Prefer shared types over duplicating inline object shapes.
- Prefer extending existing UI patterns over adding one-off abstractions when that keeps the code simpler.

## Vue and Styling Rules
- Follow the existing Vue 3 Composition API style and keep logic close to the component that owns it.
- Avoid unnecessary watchers, global state, or abstraction layers for local UI behavior.
- Preserve accessibility basics when editing interactive UI, including keyboard behavior, focus handling, and meaningful button or image attributes.
- Keep templates readable and avoid moving simple presentation logic into indirection unless it clearly improves maintainability.
- Preserve the existing CSS naming and responsive behavior in modified components.
- Avoid editing generated files under `dist/` unless the user explicitly asks for built assets to be updated.

## Documentation Rules
- Add comments only when logic or styling constraints are not obvious from the code.
- Document frontend JavaScript and TypeScript functions with JSDoc-style comments that describe their purpose and list their parameters.
- When a function has a meaningful return value or side effect that is not obvious from its name, document it in the same JSDoc block.
- For Vue components, document the props they use and include their default values whenever defaults exist.
- Keep JSDoc concise and in English, and update it when signatures, defaults, or behavior change.
- In Vue templates, use standard HTML comments such as `<!-- ... -->` only when a structure, accessibility constraint, or rendering choice is not obvious.
- Template comments should explain intent or constraints, not restate visible markup.
- Place template comments immediately above the relevant block and keep them short.
- Keep template comments in English, consistent with the rest of the codebase.
- If a template would need many comments to stay understandable, prefer extracting a subcomponent or moving non-presentational logic into the script section.
- Keep any component-level documentation or inline comments accurate when behavior changes.

## Engineering Defaults
- Prefer data-driven UI changes over hardcoded special cases when both are similarly simple.
- Avoid unrelated visual refactors while working on a focused change.
