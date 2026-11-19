# makepng

Cross-platform replacement for the former macOS Swift avatar exporter. Requires
Node.js 20.9 or newer and npm. SVG rendering uses `sharp`, which provides native
packages for common Windows, macOS and Linux architectures; npm must be allowed
to install optional dependencies.

From the repository root:

```sh
npm ci --prefix tools/makepng
node tools/makepng/makepng.mjs
```

The default paths are resolved from the script location, so the tool also works
from another working directory. It reads SVG files directly inside
`server/crates/arachnea-stream/resources/avatars/sources/` and overwrites matching
PNGs in the parent `avatars/` directory. It does not change the SVGs or catalogue.

Each PNG is 512 × 512 pixels and opaque. Source colors must use six-digit hex
notation (`#RRGGBB`), with one to four colors per SVG. After rendering, each pixel
is mapped to the nearest source color by squared RGB distance, keeping edge
antialiasing from introducing extra colors. Different SVG renderers can produce
slightly different edges compared with the former AppKit exporter.

To export elsewhere or use another source directory:

```sh
node tools/makepng/makepng.mjs --source path/to/svgs --output path/to/pngs
node tools/makepng/makepng.mjs --help
```

Explicit paths are relative to the current working directory; omitted paths keep
their repository defaults. Only lowercase `.svg` files are processed, in filename
order, without recursion. The output directory is created if needed. An empty
source directory or an invalid source produces an error and a nonzero exit code;
files exported before an error remain in place.
