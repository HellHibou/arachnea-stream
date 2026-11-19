import { mkdir, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const avatarDirectory = fileURLToPath(
  new URL('../../server/crates/arachnea-stream/resources/avatars/', import.meta.url),
);
const imageSize = 512;

async function main() {
  const { values } = parseArgs({
    options: {
      source: { type: 'string' },
      output: { type: 'string' },
      help: { type: 'boolean', short: 'h' },
    },
  });
  if (values.help) {
    console.log(`Usage: node tools/makepng/makepng.mjs [--source <directory>] [--output <directory>]

Export SVG avatars as opaque 512 x 512 PNGs using at most four source colors.
Defaults: the repository avatar sources/ directory and its parent directory.
Explicit paths are resolved relative to the current working directory.
Existing PNG files with matching names are overwritten.`);
    return;
  }

  const sourceDirectory = path.resolve(values.source ?? path.join(avatarDirectory, 'sources'));
  const outputDirectory = path.resolve(values.output ?? avatarDirectory);
  const files = (await readdir(sourceDirectory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && path.extname(entry.name) === '.svg')
    .map((entry) => entry.name)
    .sort();
  if (files.length === 0) {
    throw new Error(`No SVG sources found in ${sourceDirectory}`);
  }

  const { default: sharp } = await import('sharp');
  await mkdir(outputDirectory, { recursive: true });
  for (const filename of files) {
    const source = path.join(sourceDirectory, filename);
    try {
      const svg = await readFile(source, 'utf8');
      const colors = [...new Set(svg.match(/#[0-9a-fA-F]{6}/g) ?? [])].sort();
      if (colors.length === 0 || colors.length > 4) {
        throw new Error(`Avatar sources must use one to four hex colors; found ${colors.length}`);
      }
      const palette = colors.map((hex) => [
        Number.parseInt(hex.slice(1, 3), 16),
        Number.parseInt(hex.slice(3, 5), 16),
        Number.parseInt(hex.slice(5, 7), 16),
      ]);
      const pixels = await sharp(Buffer.from(svg))
        .resize(imageSize, imageSize, { fit: 'fill' })
        .toColourspace('srgb')
        .removeAlpha()
        .raw()
        .toBuffer();

      // Quantize edge pixels to the source palette, matching the original exporter.
      for (let offset = 0; offset < pixels.length; offset += 3) {
        let closest = palette[0];
        let bestDistance = Infinity;
        for (const color of palette) {
          const distance = color.reduce((sum, channel, index) => (
            sum + (pixels[offset + index] - channel) ** 2
          ), 0);
          if (distance < bestDistance) {
            bestDistance = distance;
            closest = color;
          }
        }
        pixels.set(closest, offset);
      }

      const output = path.join(outputDirectory, `${path.basename(filename, '.svg')}.png`);
      await sharp(pixels, { raw: { width: imageSize, height: imageSize, channels: 3 } })
        .png()
        .toFile(output);
      console.log(output);
    } catch (error) {
      throw new Error(`Cannot export ${source}: ${error.message}`, { cause: error });
    }
  }
}

main().catch((error) => {
  console.error(`makepng: ${error.message}`);
  process.exitCode = 1;
});
