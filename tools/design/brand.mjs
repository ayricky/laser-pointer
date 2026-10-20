// Turns the chosen candidates into the app's brand assets:  node tools/design/brand.mjs
//   assets/brand/logo.svg, app.ico  — the app icon (exe, shortcuts, Store) from the chosen logo
//   assets/brand/tray.svg, tray-on.svg — the tray glyph the app embeds (INK and DOT filled at runtime)
//   docs/img/logo.svg, favicon.png, apple-touch-icon.png — for the website
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { logos } from './logos.mjs';
import { tray } from './tray.mjs';

/** The owner's picks from the design picker (docs/picker). */
const CHOSEN = { logo: 'comet-trail', tray: 'lasso' };

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = (path, data) => {
  const full = join(root, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, data);
};
const png = (svg, width) => new Resvg(svg, { fitTo: { mode: 'width', value: width } }).render().asPng();

/** An .ico holding PNG images, which Windows reads at every size since Vista. */
function ico(images) {
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ size, data }, i) => {
    const entry = 6 + 16 * i;
    header.writeUInt8(size >= 256 ? 0 : size, entry);
    header.writeUInt8(size >= 256 ? 0 : size, entry + 1);
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(data.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...images.map(image => image.data)]);
}

const logo = logos.find(l => l.slug === CHOSEN.logo);
const glyph = tray.find(t => t.slug === CHOSEN.tray);
if (!logo || !glyph) throw new Error('a chosen candidate is missing');

out('assets/brand/logo.svg', logo.svg + '\n');
out('assets/brand/app.ico', ico([16, 20, 24, 32, 40, 48, 64, 256].map(size => ({ size, data: png(logo.svg, size) }))));
out('assets/brand/tray.svg', glyph.off + '\n');
out('assets/brand/tray-on.svg', glyph.on + '\n');
out('docs/img/logo.svg', logo.svg + '\n');
out('docs/img/favicon.png', png(logo.svg, 64));
out('docs/img/apple-touch-icon.png', png(logo.svg, 180));
console.log(`Brand assets written from logo "${logo.name}" and tray "${glyph.name}".`);
