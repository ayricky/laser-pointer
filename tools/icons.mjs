// Regenerates every raster icon from assets/brand/logo.svg:  cd tools && npm install && npm run icons
//   assets/brand/app.ico                         the exe's icon, at every size Windows asks for
//   docs/img/favicon.png, apple-touch-icon.png   the website's icons
// The tray glyphs (assets/brand/tray.svg, tray-on.svg) are drawn by the app itself at runtime.
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
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

const logo = readFileSync(join(root, 'assets', 'brand', 'logo.svg'), 'utf8');
out('assets/brand/app.ico', ico([16, 20, 24, 32, 40, 48, 64, 256].map(size => ({ size, data: png(logo, size) }))));
out('docs/img/logo.svg', logo);
out('docs/img/favicon.png', png(logo, 64));
out('docs/img/apple-touch-icon.png', png(logo, 180));
console.log('Icons written from assets/brand/logo.svg.');
