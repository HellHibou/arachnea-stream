# Profile avatars

The catalogue contains 28 flat portraits: `man`, `woman`, `boy` and `girl`, each
in the seven themes from `front/public-app/public/themes.json`. Each portrait uses
exactly four colors: the theme primary color, dark navy (`#24283b`), skin
(`#f2c6a5`) and cream (`#fff4df`). Saturation is clamped to 100%, matching CSS HSL
for the Orange Crunchy theme. There are no gradients or shadows.

Ten additional icons have their own fixed palettes, with at most four colors
each and no theme variants:

- Streaming: `stream-clapper`, `stream-popcorn`, `stream-tv`, `stream-headphones`.
- Animals: `animal-cat`, `animal-dog`, `animal-fox`, `animal-panda`, `animal-owl`,
  `animal-rabbit`.

Editable vector originals are in `sources/`. Their matching 512 × 512 PNG exports
are embedded through `avatars.json`; SVG files are not served by the API.
On Windows, macOS or Linux, regenerate the PNGs from the repository root with
Node.js 20.9 or newer:

```sh
npm ci --prefix tools/makepng
node tools/makepng/makepng.mjs
```

The exporter limits the output to the source palette, including edge pixels.
See [makepng](../../../../../tools/makepng/README.md) for options and requirements.
When changing a theme, update the corresponding SVG background before exporting.
When adding a theme, create one SVG/PNG pair for each character and register the
four PNGs in `avatars.json`. Recompile the server to publish catalogue changes.

The original neutral silhouette is also available in all seven theme colors:
`blue`, `purple`, `green`, `yellow`, `orange`, `pink` and `red`. Each uses exactly
two colors: the theme background and an off-white silhouette (`#f4f4f5`). The
original `blue`, `green` and `purple` IDs and the `blue` fallback ID are preserved
for existing profiles; their PNGs are now regenerated from SVG sources at the
same 512 × 512 resolution as the other avatars. New themes need a neutral SVG/PNG
pair as well as the four portrait pairs.

Portraits appear first in the selection list, grouped
by character, in frontend theme order, followed by streaming icons, animals and
the seven neutral silhouettes in frontend theme order. Choosing a theme does not change the
selected avatar: its color is part of its stored ID.
