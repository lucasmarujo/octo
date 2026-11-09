const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const MOODS = ['idle', 'working', 'needs', 'done', 'lost'];
const GRID = 48;
const SCALE = 8;
const SOURCE = path.join(__dirname, '..', '..', 'desktop', 'ui', 'kai');
const TARGET = path.join(__dirname, '..', 'assets', 'kai');
const FRAME = /<svg x="(\d+)"[^>]*>([\s\S]*?)<\/svg>/g;
const SHAPE = /<path d="([^"]+)" fill="#([0-9A-Fa-f]{6})"\/>/g;
const RECT = /M(\d+) (\d+)h(\d+)v(\d+)h-\d+z/g;
const PNG_SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const RGBA = 6;

function chunk(type, data) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, 'latin1');
  const tail = Buffer.alloc(4);
  tail.writeUInt32BE(zlib.crc32(Buffer.concat([head.subarray(4), data])), 0);
  return Buffer.concat([head, data, tail]);
}

function encode(width, height, pixels) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = RGBA;
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y += 1) pixels.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  return Buffer.concat([PNG_SIGNATURE, chunk('IHDR', header), chunk('IDAT', zlib.deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

function strip(svg) {
  const frames = [...svg.matchAll(FRAME)];
  if (!frames.length) throw new Error('the sprite has no frames');
  const width = frames.length * GRID * SCALE;
  const height = GRID * SCALE;
  const pixels = Buffer.alloc(width * height * 4);
  for (const [, offset, body] of frames) {
    const shapes = [...body.matchAll(SHAPE)];
    if (!shapes.length) throw new Error('a frame of the sprite has no shapes');
    for (const [, d, hex] of shapes) {
      const color = [0, 2, 4].map((at) => parseInt(hex.slice(at, at + 2), 16));
      for (const [, x, y, w, h] of d.matchAll(RECT)) {
        const left = (Number(offset) + Number(x)) * SCALE;
        const top = Number(y) * SCALE;
        for (let row = top; row < top + Number(h) * SCALE; row += 1) {
          for (let col = left; col < left + Number(w) * SCALE; col += 1) {
            const at = (row * width + col) * 4;
            pixels[at] = color[0];
            pixels[at + 1] = color[1];
            pixels[at + 2] = color[2];
            pixels[at + 3] = 255;
          }
        }
      }
    }
  }
  return { frames: frames.length, width, height, png: encode(width, height, pixels) };
}

function source(mood) {
  return fs.readFileSync(path.join(SOURCE, `${mood}.svg`), 'utf8');
}

function target(mood) {
  return path.join(TARGET, `${mood}.png`);
}

function write() {
  fs.mkdirSync(TARGET, { recursive: true });
  for (const mood of MOODS) fs.writeFileSync(target(mood), strip(source(mood)).png);
}

module.exports = { MOODS, GRID, SCALE, strip, source, target };

if (require.main === module) write();
