// Draws Mochi into the PNG/ICO set Tauri needs. No dependencies: the icons are
// rasterised here and encoded with node:zlib, so the app icon stays "drawn in
// code" like the character itself.
//
//   node scripts/gen-icons.mjs

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons", "lumen");

// Lumen crystal: violet facets and a warm, luminous core.
const SS = 4; // supersampling factor

const TILE = [12, 10, 17];
const VIOLET_DARK = [31, 24, 55];
const VIOLET = [82, 43, 165];
const VIOLET_LIGHT = [150, 99, 255];
const GOLD = [247, 190, 95];
const CREAM = [255, 239, 195];

function insideRoundRect(x, y, size, radius) {
  const cx = Math.max(radius, Math.min(size - radius, x));
  const cy = Math.max(radius, Math.min(size - radius, y));
  return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2;
}

function insidePoly(x, y, points) {
  let inside = false;
  for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
    const [xi, yi] = points[i];
    const [xj, yj] = points[j];
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

const FACETS = [
  { color: VIOLET_DARK, shape: [[0, -0.30], [-0.13, -0.21], [-0.06, 0.03], [0, -0.04]] },
  { color: VIOLET_LIGHT, shape: [[0, -0.30], [0.13, -0.21], [0.06, -0.04], [0, -0.12]] },
  { color: [53, 34, 93], shape: [[-0.13, -0.21], [-0.21, 0.03], [-0.06, 0.18], [0, -0.04], [-0.06, 0.03]] },
  { color: VIOLET, shape: [[0.13, -0.21], [0.21, 0.03], [0.09, 0.18], [0.06, -0.04], [0, -0.12]] },
  { color: [61, 39, 117], shape: [[-0.06, 0.03], [0, -0.04], [0, 0.30], [-0.06, 0.30], [-0.16, 0.18]] },
  { color: [37, 29, 71], shape: [[0, -0.04], [0.09, 0.18], [0.06, 0.30], [0, 0.30]] },
];

function renderCrystal(size, tile = true) {
  const px = new Uint8Array(size * size * 4);
  const cx = size / 2;
  const cy = size / 2;
  const p = (x, y) => [cx + x * size, cy + y * size];
  const facets = FACETS.map((facet) => ({ color: facet.color, shape: facet.shape.map(([x, y]) => p(x, y)) }));
  const core = [p(0, -0.10), p(0.09, 0), p(0, 0.12), p(-0.09, 0)];
  const tileSize = size * 0.91;
  const tileOffset = (size - tileSize) / 2;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const sum = [0, 0, 0, 0];
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const px = x + (sx + 0.5) / SS;
          const py = y + (sy + 0.5) / SS;
          let color = tile && insideRoundRect(px - tileOffset, py - tileOffset, tileSize, size * 0.18) ? TILE : null;
          for (const facet of facets) if (insidePoly(px, py, facet.shape)) color = facet.color;
          if (insidePoly(px, py, core)) color = px < cx ? CREAM : GOLD;
          if (!color) continue;
          sum[0] += color[0];
          sum[1] += color[1];
          sum[2] += color[2];
          sum[3] += 255;
        }
      }
      const o = (y * size + x) * 4;
      px[o] = Math.round(sum[0] / (SS * SS));
      px[o + 1] = Math.round(sum[1] / (SS * SS));
      px[o + 2] = Math.round(sum[2] / (SS * SS));
      px[o + 3] = Math.round(sum[3] / (SS * SS));
    }
  }
  return px;
}

// ── PNG ───────────────────────────────────────────────────────────────────────

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePNG(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // filter: none
    Buffer.from(rgba.buffer, y * size * 4, size * 4).copy(raw, y * (size * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// ── ICO (PNG-in-ICO, Vista and later) ─────────────────────────────────────────

function encodeICO(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = header.length + dir.length;
  entries.forEach((e, i) => {
    const o = i * 16;
    dir[o] = e.size >= 256 ? 0 : e.size;
    dir[o + 1] = e.size >= 256 ? 0 : e.size;
    dir[o + 2] = 0;
    dir[o + 3] = 0;
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(e.png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += e.png.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.png)]);
}

// ── Go ────────────────────────────────────────────────────────────────────────

mkdirSync(OUT, { recursive: true });

const png = (size) => encodePNG(size, renderCrystal(size));

const files = {
  "32x32.png": png(32),
  "128x128.png": png(128),
  "128x128@2x.png": png(256),
  "icon.png": png(512),
};
for (const [name, data] of Object.entries(files)) {
  writeFileSync(join(OUT, name), data);
  console.log(`${name} — ${data.length} bytes`);
}

const ico = encodeICO([16, 24, 32, 48, 64, 128, 256].map((size) => ({ size, png: png(size) })));
writeFileSync(join(OUT, "icon.ico"), ico);
console.log(`icon.ico — ${ico.length} bytes`);

const tray = encodePNG(32, renderCrystal(32, false));
writeFileSync(join(OUT, "tray.png"), tray);
console.log(`tray.png — ${tray.length} bytes`);
