# front

This template should help get you started developing with Vue 3 in Vite.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Vue (Official)](https://marketplace.visualstudio.com/items?itemName=Vue.volar) (and disable Vetur).

## Recommended Browser Setup

- Chromium-based browsers (Chrome, Edge, Brave, etc.):
  - [Vue.js devtools](https://chromewebstore.google.com/detail/vuejs-devtools/nhdogjmejiglipccpnnnanhbledajbpd)
  - [Turn on Custom Object Formatter in Chrome DevTools](http://bit.ly/object-formatters)
- Firefox:
  - [Vue.js devtools](https://addons.mozilla.org/en-US/firefox/addon/vue-js-devtools/)
  - [Turn on Custom Object Formatter in Firefox DevTools](https://fxdx.dev/firefox-devtools-custom-object-formatters/)

## Type Support for `.vue` Imports in TS

TypeScript cannot handle type information for `.vue` imports by default, so we replace the `tsc` CLI with `vue-tsc` for type checking. In editors, we need [Volar](https://marketplace.visualstudio.com/items?itemName=Vue.volar) to make the TypeScript language service aware of `.vue` types.

## Customize configuration

See [Vite Configuration Reference](https://vite.dev/config/).

## Project Setup

```sh
npm install
```

### Compile and Hot-Reload for Development

```sh
npm run dev
```

### Type-Check, Compile and Minify for Production

```sh
npm run build
```

### Lint CSS

```sh
npm run lint:css
```

### Lint CSS and auto-fix

```sh
npm run lint:css:fix
```


## Desktop tabs

In Tauri desktop mode, the native host creates a dedicated webview for `DesktopTabShell.vue` and an independent content webview for each tab. The injected `window.__DESKTOP_TAB_SHELL__` flag selects the shell at bootstrap; normal browser and content webviews continue to mount `App.vue` with Vue Router. The shell uses `useDesktopTabs` and the typed `desktopTabs` bridge to receive native state and request tab actions.

The tab strip supports creation, activation, closing and arrow/Home/End keyboard navigation. Closing the final tab opens a fresh home page. Hidden tabs keep their page state and can continue media playback; closing a tab destroys that page. Administration remains in its dedicated window. The strip is hidden with only one tab, and while a managed page owns browser fullscreen, giving the page the full content height. From two tabs onward outside fullscreen, native host bounds reserve 44 logical pixels for the strip, matching its CSS height. The native controller can alternatively select independent window mode through `TauriControlerConfiguration::browsing_mode`; that mode mounts the regular application root.
