import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, statSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const REPO = join(ROOT, '..');
const SITE = 'https://octo.marujo.dev/';
const read = (file) => readFileSync(join(ROOT, file), 'utf8');
const html = read('index.html');
const css = read('assets/site.css');
const app = read('assets/app.js');
const readme = readFileSync(join(REPO, 'README.md'), 'utf8');
const readmePtBr = readFileSync(join(REPO, 'README.pt-BR.md'), 'utf8');
const head = html.slice(0, html.indexOf('</head>'));
const body = html.slice(html.indexOf('<body>'));
const size = (file) => statSync(join(ROOT, file)).size;
const localPath = (url) => join(ROOT, new URL(url, SITE).pathname);
const decode = (text) => text.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&nbsp;/g, ' ').replace(/&amp;/g, '&');
const stripTags = (text) => decode(text.replace(/<[^>]+>/g, '')).replace(/\s+/g, ' ').trim();
const withoutCode = (markdown) => markdown.replace(/```[\s\S]*?```/g, '');
const headings = (markdown) => [...withoutCode(markdown).matchAll(/^(#{1,6}) (.+)$/gm)].map(([, hashes, text]) => ({ level: hashes.length, text }));

function meta(attr, key) {
  const match = head.match(new RegExp(`<meta ${attr}="${key}" content="([^"]*)">`));
  return match && match[1];
}

function jsonLd() {
  const match = head.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/);
  assert.ok(match, 'JSON-LD ausente no <head>');
  return JSON.parse(match[1])['@graph'];
}

function rustFrame(source, name) {
  return [...source.match(new RegExp(`const ${name}: \\[&str; 16\\] = \\[([\\s\\S]*?)\\];`))[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

function rustPalette(source) {
  return Object.fromEntries([...source.matchAll(/b'(\w)' => Color::Rgb\((\d+), (\d+), (\d+)\)/g)].map(([, code, r, g, b]) => [code, hex(r, g, b)]));
}

function hex(...channels) {
  return `#${channels.map((c) => Number(c).toString(16).padStart(2, '0')).join('')}`;
}

function decodePng(png) {
  const width = png.readUInt32BE(16);
  const height = png.readUInt32BE(20);
  assert.equal(png[24], 8, 'profundidade de 8 bits');
  assert.equal(png[25], 6, 'RGBA');
  const chunks = [];
  for (let at = 8; at < png.length;) {
    const length = png.readUInt32BE(at);
    if (png.toString('ascii', at + 4, at + 8) === 'IDAT') chunks.push(png.subarray(at + 8, at + 8 + length));
    at += length + 12;
  }
  const raw = inflateSync(Buffer.concat(chunks));
  const stride = width * 4;
  const pixels = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    for (let x = 0; x < stride; x += 1) {
      const left = x >= 4 ? pixels[y * stride + x - 4] : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const corner = x >= 4 && y > 0 ? pixels[(y - 1) * stride + x - 4] : 0;
      const predicted = [0, left, up, (left + up) >> 1, paeth(left, up, corner)][filter];
      pixels[y * stride + x] = (raw[y * (stride + 1) + 1 + x] + predicted) & 0xff;
    }
  }
  return { width, height, pixels };
}

function paeth(left, up, corner) {
  const estimate = left + up - corner;
  const [toLeft, toUp, toCorner] = [left, up, corner].map((value) => Math.abs(estimate - value));
  if (toLeft <= toUp && toLeft <= toCorner) return left;
  return toUp <= toCorner ? up : corner;
}

function icoFrames(ico) {
  assert.deepEqual([...ico.subarray(0, 4)], [0, 0, 1, 0]);
  return Array.from({ length: ico.readUInt16LE(4) }, (_, i) => {
    const entry = 6 + i * 16;
    return ico.subarray(ico.readUInt32LE(entry + 12), ico.readUInt32LE(entry + 12) + ico.readUInt32LE(entry + 8));
  });
}

function readmeTable(heading) {
  const start = readme.indexOf(heading);
  assert.ok(start >= 0, `${heading} não encontrado no README`);
  const lines = readme.slice(start).split('\n');
  const first = lines.findIndex((line) => line.startsWith('|'));
  const end = lines.findIndex((line, i) => i > first && !line.startsWith('|'));
  return lines.slice(first + 2, end).map((line) => line.split('|').slice(1, -1).map((cell) => cell.trim().replace(/`/g, '')));
}

test('title é só o produto e o autor, igual ao nome da página no JSON-LD', () => {
  const title = head.match(/<title>([^<]+)<\/title>/)[1];
  assert.equal(title, 'Octo by Lucas Marujo');
  assert.equal(jsonLd().find((node) => node['@type'] === 'WebPage').name, title);
});

test('meta description cita os agentes principais e tem tamanho de snippet', () => {
  const description = meta('name', 'description');
  ['Claude Code', 'Codex', 'Windows', 'Linux', 'macOS'].forEach((word) => assert.ok(description.includes(word), word));
  assert.ok(description.length >= 70 && description.length <= 160, `description com ${description.length} caracteres`);
});

test('canonical, Open Graph e Twitter apontam para o domínio e para uma imagem 1200x630 leve', () => {
  assert.match(head, new RegExp(`<link rel="canonical" href="${SITE}">`));
  assert.equal(meta('property', 'og:url'), SITE);
  const image = meta('property', 'og:image');
  assert.equal(image, `${SITE}og-octo-kai.png`);
  assert.equal(meta('name', 'twitter:image'), image);
  assert.equal(meta('name', 'twitter:card'), 'summary_large_image');
  const png = readFileSync(localPath(image));
  assert.equal(png.readUInt32BE(16), 1200);
  assert.equal(png.readUInt32BE(20), 630);
  assert.ok(png.length < 200 * 1024, `og-image com ${png.length} bytes`);
});

test('favicons referenciados existem e não são data URI', () => {
  const icons = [...head.matchAll(/<link rel="(?:icon|apple-touch-icon)" href="([^"]+)"/g)].map((m) => m[1]);
  assert.ok(icons.length >= 3);
  icons.forEach((href) => {
    assert.ok(!href.startsWith('data:'), href);
    assert.ok(existsSync(localPath(href)), href);
  });
});

test('JSON-LD descreve o software e o vídeo com arquivos que existem', () => {
  const graph = jsonLd();
  const byType = (type) => graph.find((node) => node['@type'] === type);
  const software = byType('SoftwareApplication');
  const video = byType('VideoObject');
  const page = byType('WebPage');
  assert.equal(software.name, 'Octo');
  assert.deepEqual(software.brand, { '@type': 'Brand', name: 'OctoAI' });
  assert.equal(byType('WebSite').name, 'OctoAI');
  ['Windows', 'Linux', 'macOS'].forEach((os) => assert.ok(software.operatingSystem.includes(os), os));
  assert.equal(software.downloadUrl, 'https://github.com/lucasmarujo/octo/releases/latest');
  assert.ok(existsSync(localPath(software.screenshot)), software.screenshot);
  assert.ok(existsSync(localPath(video.contentUrl)), video.contentUrl);
  assert.ok(existsSync(localPath(video.thumbnailUrl)), video.thumbnailUrl);
  assert.equal(video.duration, 'PT60S');
  const ids = new Set(graph.map((node) => node['@id']));
  [page.isPartOf, page.about, page.video].forEach((ref) => assert.ok(ids.has(ref['@id']), ref['@id']));
  assert.match(page.dateModified, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
});

test('sitemap e robots apontam para o domínio e batem com o dateModified', () => {
  const page = jsonLd().find((node) => node['@type'] === 'WebPage');
  const sitemap = read('sitemap.xml');
  assert.match(sitemap, new RegExp(`<loc>${SITE}</loc>`));
  assert.equal(sitemap.match(/<lastmod>([^<]+)<\/lastmod>/)[1], page.dateModified);
  assert.match(read('robots.txt'), /^Sitemap: https:\/\/octo\.marujo\.dev\/sitemap\.xml$/m);
});

test('um único H1, com o texto no HTML estático, sem depender de JavaScript', () => {
  const h1s = html.match(/<h1[\s\S]*?<\/h1>/g);
  assert.equal(h1s.length, 1);
  assert.equal(stripTags(h1s[0]), 'all your agents. one window.');
});

test('a página, os metadados e os textos do app.js estão em inglês', () => {
  assert.match(html, /<html lang="en">/);
  assert.equal(meta('property', 'og:locale'), 'en_US');
  jsonLd().filter((node) => node.inLanguage).forEach((node) => assert.equal(node.inLanguage, 'en', node['@id']));
  const attributes = [...html.matchAll(/\s(?:alt|aria-label|content|data-label)="([^"]*)"/g)].map((m) => m[1]);
  const strings = [...app.matchAll(/'([^'\n]*)'/g)].map((m) => m[1]);
  const text = [stripTags(body), head.match(/<title>([^<]+)<\/title>/)[1], JSON.stringify(jsonLd()), ...attributes, ...strings].join('\n');
  assert.doesNotMatch(text, /[áàâãéêíóôõúç]/i);
  assert.doesNotMatch(text, /\b(?:para|uma|seus|seu|sua|agentes|quem|sem|todos|voce|projetos|tarefa)\b/i);
});

test('os outros mascotes da página são o Kai pixel do app, com as mesmas tiras', () => {
  const mascots = [...html.matchAll(/<span class="kai ([^"]*)"([^>]*)>/g)].map((m) => ({ place: m[1], attrs: m[2] }));
  for (const place of ['brand-mascot', 'top-runner', 'kai-hero hero-mascot', 'finale-mascot', 'diver']) assert.ok(mascots.some((kai) => kai.place === place), place);
  assert.equal(mascots.filter((kai) => kai.place === 'brand-mascot').length, 2, 'no cabeçalho e no rodapé');
  const sprite = '<span class="kai-sprite"><span class="kai-strip"></span></span>';
  const moods = new Set();
  for (const { place, attrs } of mascots) {
    const mood = attrs.match(/data-mood="(\w+)"/);
    assert.ok(mood, `${place} sem data-mood`);
    moods.add(mood[1]);
    const start = html.indexOf(`<span class="kai ${place}"${attrs}>`);
    assert.ok(html.slice(start, start + 2000).includes(sprite), `${place} sem a tira`);
  }
  assert.deepEqual([...moods].sort(), ['idle', 'working']);
  assert.match(html, /<span class="kai kai-hero hero-mascot" data-kai data-mood="working"/, 'o Kai do hero digita nos terminais');
  assert.doesNotMatch(html, /kai-(?:leg|head|face|body|pupils)/, 'nada do Kai antigo em camadas');
  for (const mood of ['idle', 'working', 'done', 'needs', 'sleep', 'lost']) {
    assert.deepEqual(readFileSync(join(ROOT, `assets/kai/${mood}.svg`)), readFileSync(join(REPO, `desktop/ui/kai/${mood}.svg`)), `assets/kai/${mood}.svg`);
    assert.ok(css.includes(`url("/assets/kai/${mood}.svg")`), mood);
  }
  assert.match(css, /\.kai-hero\.cheer \.kai-strip \{[^}]*done\.svg[^}]*steps\(5, jump-none\) forwards/, 'o clique comemora uma vez');
  assert.match(app, /mountKai\(document\.querySelector\('\[data-kai\]'\)\)/);
  assert.match(app, /runnerWidth = runner\.offsetWidth/, 'o Kai da barra para no fim da tela, seja qual for o tamanho dele');
});

test('o Kai só anima o que o compositor roda sozinho, a 60 fps', () => {
  const frames = [...css.matchAll(/@keyframes (kai-[\w-]+) \{(.*)\}\s*$/gm)];
  assert.ok(frames.length >= 4, 'keyframes do Kai');
  for (const [, name, rules] of frames) {
    for (const [, property] of rules.matchAll(/([a-z-]+)\s*:/g)) assert.ok(['transform', 'opacity'].includes(property), `${name} anima ${property}`);
  }
  assert.match(css, /prefers-reduced-motion: reduce\) \{\s*\.kai, \.kai \* \{ animation: none/);
});

test('o ícone do orkai CLI (exe e terminais) é o sprite IDLE em cada tamanho', () => {
  const sprite = readFileSync(join(REPO, 'src/tui/sprite.rs'), 'utf8');
  const palette = rustPalette(sprite);
  const idle = rustFrame(sprite, 'IDLE');
  const ico = readFileSync(join(REPO, 'assets/orkai.ico'));
  assert.deepEqual(readFileSync(join(ROOT, 'orkai.ico')), ico, 'site/orkai.ico');
  const pngs = [...icoFrames(ico), readFileSync(join(REPO, 'assets/orkai.png'))];
  const sizes = pngs.map((png) => decodePng(png).width);
  [16, 32, 48, 256].forEach((size) => assert.ok(sizes.includes(size), `falta ${size}px`));
  for (const png of pngs) {
    const { width, height, pixels } = decodePng(png);
    assert.equal(width, height);
    const scale = height / 16;
    const offset = (width - 14 * scale) / 2;
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const sx = Math.floor((x - offset) / scale);
        const code = sx >= 0 && sx < 14 ? idle[Math.floor(y / scale)][sx] : '.';
        const at = (y * width + x) * 4;
        const actual = pixels[at + 3] === 0 ? null : hex(pixels[at], pixels[at + 1], pixels[at + 2]);
        assert.equal(actual, code === '.' ? null : palette[code], `${width}px, pixel ${x},${y}`);
      }
    }
  }
});

test('o favicon do site é o ícone do Octo, nos tamanhos de aba', () => {
  const ico = readFileSync(join(ROOT, 'octo-kai-corner.ico'));
  assert.deepEqual(readFileSync(join(ROOT, 'favicon.ico')), ico, 'site/favicon.ico');
  const sizes = icoFrames(ico).map((frame) => frame.readUInt32BE(16));
  assert.deepEqual([...sizes].sort((a, b) => a - b), [16, 32, 48]);
  const touch = readFileSync(join(ROOT, 'octo-kai-corner-touch-icon.png'));
  assert.equal(touch.readUInt32BE(16), 180);
  assert.equal(touch.readUInt32BE(20), 180);
  assert.match(head, /<link rel="icon" href="\/octo-kai-corner\.ico" sizes="48x48">/);
  assert.match(head, /<link rel="apple-touch-icon" href="\/octo-kai-corner-touch-icon\.png">/);
});

const macInset = (size) => {
  const shape = Math.round((size * 824) / 1024);
  return { shape, offset: Math.floor((size - shape) / 2) };
};

test('o ícone do Octo é o Kai espiando pelo canto em todo lugar, e o Kai não sai de onde é mascote', () => {
  let tile = null;
  const channels = ({ width, pixels }, x, y) => {
    const { shape, offset } = tile || { shape: width, offset: 0 };
    const at = ((offset + Math.floor(y * shape)) * width + offset + Math.floor(x * shape)) * 4;
    return [...pixels.subarray(at, at + 4)];
  };
  const cell = (x, y) => (x + 0.5) / 48;
  const assertCorner = (png, label, cornerAlpha = 0, mac = false) => {
    const image = decodePng(png);
    tile = null;
    assert.equal(channels(image, 0, 0)[3], cornerAlpha, `${label}: canto`);
    tile = mac ? macInset(image.width) : null;
    if (image.width < 48) return image.width;
    assert.deepEqual(channels(image, cell(24), cell(0)), [0, 0, 0, 255], `${label}: borda da janela`);
    assert.deepEqual(channels(image, cell(4), cell(30)), [0x1c, 0x1c, 0x24, 255], `${label}: fundo da janela`);
    assert.deepEqual(channels(image, cell(13), cell(11)), [0xf0, 0x8a, 0xcb, 255], `${label}: cursor`);
    assert.deepEqual(channels(image, cell(36), cell(32)), [0xe8, 0x5e, 0xb6, 255], `${label}: o Kai no canto, no rosa dele`);
    return image.width;
  };
  const icons = ['desktop/icons/32x32.png', 'desktop/icons/128x128.png', 'desktop/icons/128x128@2x.png', 'desktop/icons/icon.png', 'assets/octo-kai-corner.png'];
  const widths = icons.map((file) => assertCorner(readFileSync(join(REPO, file)), file));
  assert.deepEqual(widths, [32, 128, 256, 512, 512]);
  const desktop = icoFrames(readFileSync(join(REPO, 'desktop/icons/icon.ico'))).map((frame) => assertCorner(frame, 'icon.ico'));
  assert.deepEqual([...desktop].sort((a, b) => a - b), [16, 24, 32, 48, 64, 128, 256]);
  icoFrames(readFileSync(join(ROOT, 'octo-kai-corner.ico'))).forEach((frame) => assertCorner(frame, 'octo-kai-corner.ico'));
  assertCorner(readFileSync(join(ROOT, 'octo-kai-corner-touch-icon.png')), 'touch icon', 255);
  const icns = readFileSync(join(REPO, 'desktop/icons/icon.icns'));
  const mac = [];
  for (let at = 8; at < icns.length; at += icns.readUInt32BE(at + 4)) {
    if (icns.toString('ascii', at, at + 4) !== 'TOC ') mac.push(assertCorner(icns.subarray(at + 8, at + icns.readUInt32BE(at + 4)), 'icon.icns', 0, true));
  }
  assert.deepEqual(new Set(mac), new Set([16, 32, 64, 128, 256, 512, 1024]));
  assert.deepEqual(macInset(1024), { shape: 824, offset: 100 }, 'no Mac a janela ocupa 824 de 1024, como todo ícone do Dock');
  const favicon = read('favicon.svg');
  assert.match(favicon, /viewBox="0 0 48 48" shape-rendering="crispEdges"/, 'a grade de 48 pixels, a mesma do sprite do Kai');
  assert.match(favicon, /fill="#E85EB6"/, 'o Kai no rosa dele');
  assert.match(readFileSync(join(REPO, 'desktop/ui/kai/idle.svg'), 'utf8'), /#E85EB6/, 'o rosa vem da paleta do Kai');
  assert.match(html, /<span class="kai kai-hero hero-mascot"/, 'o mascote da página continua');
  assert.ok(existsSync(join(REPO, 'desktop/ui/kai/idle.svg')), 'o mascote da sidebar continua');
  assert.equal(meta('property', 'og:image'), `${SITE}og-octo-kai.png`, 'a imagem de compartilhamento continua com o mascote');
});

test('o ícone é pixel art: só cores da grade, sem suavização, e todo tamanho sai da grade do favicon', () => {
  const grid = Array.from({ length: 48 }, () => Array(48).fill('00000000'));
  for (const [, d, fill] of read('favicon.svg').matchAll(/<path d="([^"]+)" fill="#([0-9A-F]{6})"\/>/g)) {
    for (const [, x, y, w, h] of d.matchAll(/M(\d+) (\d+)h(\d+)v(\d+)h-\d+z/g)) {
      for (let row = Number(y); row < Number(y) + Number(h); row += 1) {
        for (let column = Number(x); column < Number(x) + Number(w); column += 1) grid[row][column] = `${fill.toLowerCase()}ff`;
      }
    }
  }
  const palette = new Set(grid.flat());
  assert.ok(palette.has('e85eb6ff') && palette.has('1c1c24ff'), 'o rosa do Kai e o fundo da janela');
  const assertPixels = (png, label, background = '00000000', mac = false) => {
    const { width, pixels } = decodePng(png);
    const { shape, offset } = mac ? macInset(width) : { shape: width, offset: 0 };
    const source = (i) => Math.floor(((i - offset + 0.5) * 48) / shape);
    for (let y = 0; y < width; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const color = pixels.subarray((y * width + x) * 4, (y * width + x) * 4 + 4).toString('hex');
        const expected = grid[source(y)]?.[source(x)] ?? '00000000';
        assert.equal(color, expected === '00000000' ? background : expected, `${label}: ${x},${y}`);
      }
    }
  };
  for (const file of ['desktop/icons/32x32.png', 'desktop/icons/128x128.png', 'desktop/icons/128x128@2x.png', 'desktop/icons/icon.png', 'assets/octo-kai-corner.png']) assertPixels(readFileSync(join(REPO, file)), file);
  for (const file of ['desktop/icons/icon.ico', 'site/octo-kai-corner.ico']) icoFrames(readFileSync(join(REPO, file))).forEach((frame) => assertPixels(frame, file));
  assertPixels(readFileSync(join(ROOT, 'octo-kai-corner-touch-icon.png')), 'touch icon', '000000ff');
  const icns = readFileSync(join(REPO, 'desktop/icons/icon.icns'));
  for (let at = 8; at < icns.length; at += icns.readUInt32BE(at + 4)) {
    if (icns.toString('ascii', at, at + 4) !== 'TOC ') assertPixels(icns.subarray(at + 8, at + icns.readUInt32BE(at + 4)), 'icon.icns', '00000000', true);
  }
  assert.equal(readFileSync(join(REPO, 'assets/octo-kai-corner.png')).compare(readFileSync(join(REPO, 'desktop/icons/icon.png'))), 0);
});

test('ícones e imagens trocados ganham nome novo, porque o nginx guarda png e ico por 30 dias', () => {
  const cached = read('nginx.conf').match(/location ~\* \\\.\(\?:([\w|]+)\)\$/)[1].split('|');
  assert.ok(cached.includes('png') && cached.includes('ico'));
  assert.doesNotMatch(html, /\/og-octo\.png|\/octo\.ico|\/octo-touch-icon\.png|\/octo-kai\.ico|\/octo-kai-touch-icon\.png|\/octo-eye\.ico|\/octo-eye-touch-icon\.png|\/octo-eye-pixel\.ico|\/octo-eye-pixel-touch-icon\.png/);
  for (const old of ['og-octo.png', 'octo.ico', 'octo-touch-icon.png', 'octo-kai.ico', 'octo-kai-touch-icon.png', 'octo-eye.ico', 'octo-eye-touch-icon.png', 'octo-eye-pixel.ico', 'octo-eye-pixel-touch-icon.png']) assert.ok(!existsSync(join(ROOT, old)), `${old} ficou para trás`);
});

test('a marca é OctoAI, o produto é Octo e a página não oferece mais o orkai CLI', () => {
  const wordmarks = [...html.matchAll(/<span class="wordmark">([^<]+)<\/span>/g)].map((m) => m[1]);
  assert.deepEqual(wordmarks, ['OctoAI', 'OctoAI']);
  assert.doesNotMatch(stripTags(html.slice(html.indexOf('<main'))), /\borkai\b/i);
  assert.doesNotMatch(html, /id="cli"|href="#cli"|data-tui\b|data-install=/);
  assert.doesNotMatch(read('llms.txt'), /orkai CLI|orkai-setup/);
});

test('o hero mostra a tela real do Octo, com 1x e 2x', () => {
  const hero = html.slice(html.indexOf('<section class="hero"'), html.indexOf('</section>'));
  const img = hero.match(/<div class="shot" data-screen>\s*<img ([^>]+)>/)[1];
  assert.match(img, /src="\/assets\/octo-board\.webp"/);
  assert.ok(!existsSync(join(ROOT, 'assets/octo-board-kai.webp')), 'o board antigo ficou para trás');
  assert.equal(jsonLd().find((node) => node['@type'] === 'SoftwareApplication').screenshot, `${SITE}assets/octo-board.webp`);
  assert.match(css, /@media \(max-width: 760px\) \{[\s\S]*?\.hide-sm \{ display: none; \}/, 'a legenda longa do hero some no celular');
  assert.match(img, /width="1440" height="900"/);
  const srcset = img.match(/srcset="([^"]+)"/)[1].split(', ').map((entry) => entry.split(' '));
  assert.deepEqual(srcset.map(([, density]) => density), ['1x', '2x']);
  srcset.forEach(([file]) => assert.ok(existsSync(join(ROOT, file)), file));
  assert.match(app, /stage\.querySelector\('\[data-screen\]'\)/);
});

test('o problema e o tour mostram as telas reais do Octo, com 1x e 2x, e não a TUI do orkai', () => {
  const chaos = html.match(/<section class="chaos"[\s\S]*?<\/section>/)[0];
  const tour = html.match(/<div class="wrap tour" data-tour>[\s\S]*?<ol class="tour-dots"/)[0];
  const images = (part) => [...part.matchAll(/<img ([^>]+)>/g)].map((m) => m[1]);
  const shots = [...images(chaos), ...images(tour)];
  assert.deepEqual(shots.map((img) => img.match(/src="\/assets\/([^"]+)\.webp"/)[1]), ['octo-list', 'octo-list', 'octo-board', 'octo-pair', 'octo-agent', 'octo-grid']);
  shots.forEach((img) => {
    assert.match(img, /width="1440" height="900"/);
    assert.match(img, /loading="lazy"/);
    assert.match(img, /alt="[^"]{40,}"/);
    const srcset = img.match(/srcset="([^"]+)"/)[1].split(', ').map((entry) => entry.split(' '));
    assert.deepEqual(srcset.map(([, density]) => density), ['1x', '2x']);
    srcset.forEach(([file]) => assert.ok(existsSync(join(ROOT, file)), file));
  });
  assert.doesNotMatch(chaos + tour, /class="tui\b|class="r\b|data-t-/);
  assert.doesNotMatch(css, /\.tui\b|\.c-name|\.tour-tui/);
  assert.doesNotMatch(app, /\bglyph\b|\bslugify\b|\bdurationShort\b|\bHINT\b/);
});

test('a branch das worktrees na página e no llms.txt é a do src/worktree.rs', () => {
  const prefix = readFileSync(join(REPO, 'src/worktree.rs'), 'utf8').match(/pub const BRANCH_PREFIX: &str = "([^"]+)";/)[1];
  assert.ok(html.includes(`<code>${prefix}&lt;name&gt;</code>`), prefix);
  assert.ok(read('llms.txt').includes(`\`${prefix}<name>\``), prefix);
});

test('o uso dos planos segue as cores do app: verde, amarelo a partir de 70% e vermelho a partir de 90%', () => {
  const fills = [...html.matchAll(/<span class="usage-fill is-(\w+)" data-pct="(\d+)"><\/span>/g)].map(([, tone, pct]) => [tone, Number(pct)]);
  assert.ok(fills.length >= 3);
  fills.forEach(([tone, pct]) => assert.equal(tone, pct >= 90 ? 'red' : pct >= 70 ? 'yellow' : 'green', `${pct}%`));
  assert.match(app, /fill\.style\.width = `\$\{fill\.dataset\.pct\}%`/);
});

test('o uso dos planos mostra o ícone de cada agente, o mesmo do app', () => {
  const icons = readFileSync(join(REPO, 'desktop/ui/js/icons.js'), 'utf8');
  const cards = [...html.matchAll(/<div class="usage-provider"><span class="usage-icon">(<svg[\s\S]*?<\/svg>)<\/span><span class="usage-kind k-(\w+)">\2<\/span><\/div>/g)];
  assert.deepEqual(cards.map(([, , kind]) => kind), ['claude', 'codex', 'kimi']);
  cards.forEach(([, svg, kind]) => {
    const source = icons.match(new RegExp(`\\n  ${kind}: \`(<svg[^\`]+)\``))[1];
    const paths = (markup) => [...markup.matchAll(/ d="([^"]+)"/g)].map((m) => m[1]);
    assert.deepEqual(paths(svg), paths(source), kind);
    assert.match(svg, /aria-hidden="true"/);
  });
});

test('sem animação o footer é preto com as infos à mostra; com animação ele continua o rosa até a tinta', () => {
  assert.match(css, /\.finale \{[^}]*background: var\(--pink\);/);
  assert.match(css, /\.foot \{ background: var\(--ink\); color: var\(--text\); \}/);
  assert.match(css, /\.foot\.is-live \{ height: 360vh; background: var\(--pink\); \}/);
  assert.match(css, /\.foot\.is-live \.foot-stage \{ position: sticky; top: 0; height: 100vh;/);
  assert.match(css, /\.foot\.is-live:focus-within \.foot-info \{ opacity: 1; transform: none; pointer-events: auto; \}/, 'quem navega pelo teclado vê os links');
  assert.match(css, /\.foot\.is-live \.foot-info \{[^}]*pointer-events: none;/, 'link invisível não recebe clique');
  const footer = html.match(/<footer class="foot" data-ink>[\s\S]*?<\/footer>/)[0];
  assert.match(footer, /<span class="foot-scene" aria-hidden="true" data-scene><\/span>/);
  assert.equal((footer.match(/<div class="foot-brand">/g) || []).length, 1);
  assert.equal((footer.match(/<h2 class="kicker-dim">/g) || []).length, 2);
  assert.equal((footer.match(/<div class="wrap foot-base"><span>[^<]+<\/span><span[^>]*>[^<]+<\/span><\/div>/g) || []).length, 1);
  assert.match(css, /\.finale-mascot\.kai::before \{[^}]*background: var\(--ink\);/, 'o círculo do Kai do finale é preto, sem o azul');
});

test('o texto escuro do finale vira rosa sobre o rastro, porque a cor de blend é o rosa menos o fundo', () => {
  const color = (name) => css.match(new RegExp(`--${name}: #([0-9a-f]{6});`))[1].match(/../g).map((c) => parseInt(c, 16));
  const [pink, ink, blend] = ['pink', 'ink', 'ink-blend'].map(color);
  assert.deepEqual(blend, pink.map((value, i) => value - ink[i]));
  assert.match(css, /\.finale-title, \.finale-lead \{ color: var\(--ink-blend\); mix-blend-mode: difference; \}/);
  assert.match(css, /\.btn-ink-ghost \{[^}]*background: var\(--pink\);/, 'o botão vazado não some sobre o rastro');
});

test('o rastro pixelado do finale usa a grade da transição, a cor do fundo escuro e respeita quem não quer movimento', () => {
  assert.match(html, /<section class="finale"[^>]*>\s*<canvas class="finale-pixels" data-pixels aria-hidden="true"><\/canvas>\s*<canvas class="finale-trail" data-trail aria-hidden="true"><\/canvas>/);
  const trail = app.slice(app.indexOf('function mountTrail'), app.indexOf('mountReveal();'));
  assert.match(trail, /if \(REDUCED_MOTION \|\| !FINE_POINTER\) return;/);
  assert.match(trail, /getPropertyValue\('--ink'\)/);
  assert.match(trail, /PIXEL_CELL/);
  assert.match(app, /const cell = PIXEL_CELL;/);
  assert.match(trail, /if \(cells\.size\) requestAnimationFrame\(draw\);\s*else running = false;/, 'o laço para quando o rastro some');
  assert.match(trail, /Math\.min\(TRAIL_MAX_STEPS,/, 'movimento rápido não pinta sem limite');
  assert.match(css, /\.finale-trail \{[^}]*pointer-events: none;/);
  assert.match(trail, /canvas\.style\.top = `\$\{-above\}px`;/, 'cobre a transição de pixels acima do finale');
  assert.match(trail, /canvas\.style\.height = `\$\{above \+ section\.offsetHeight\}px`;/, 'e para no fim do finale, onde começa a cena da tinta');
  assert.match(trail, /Math\.ceil\(pixels\.offsetHeight \/ PIXEL_CELL\) \* PIXEL_CELL/, 'na mesma grade do finale');
  assert.match(trail, /addEventListener\('pointermove'/);
  assert.match(app, /mountTrail\(document\.querySelector\('\[data-finale\]'\)\);/);
});

const appConst = (name) => new Function(`return ${app.match(new RegExp(`const ${name} = ([^;]+);`))[1]}`)();

test('o Kai mergulhador tem o tamanho do Kai do finale, desce com a página e pula de lado a cada seção', () => {
  assert.match(html, /<span class="kai diver" aria-hidden="true" data-diver data-mood="idle">/);
  const finaleSize = css.match(/\.finale-mascot\.kai \{ width: (\d+)px; height: \1px; \}/)[1];
  assert.match(css, new RegExp(`\\.diver\\.kai \\{ position: fixed;[^}]*width: ${finaleSize}px; height: ${finaleSize}px;[^}]*pointer-events: none;`));
  const mobileFinale = css.match(/ {2}\.finale-mascot\.kai \{ width: (\d+)px; height: \1px; \}/)[1];
  assert.match(css, new RegExp(` {2}\\.diver\\.kai \\{ width: ${mobileFinale}px; height: ${mobileFinale}px; \\}`));
  const moods = appConst('DIVER_MOODS');
  const ids = Object.keys(moods);
  ids.forEach((id) => assert.match(html, new RegExp(`<section[^>]+id="${id}"`), id));
  Object.values(moods).forEach((mood) => assert.ok(css.includes(`.kai[data-mood="${mood}"] .kai-strip`), mood));
  const working = ids.filter((id) => moods[id] === 'working');
  assert.deepEqual(working, ['recursos', 'agentes'], 'o Kai com os terminais só aparece nas primeiras seções');
  const diver = app.slice(app.indexOf('function mountDiver'), app.indexOf('function sceneFrame'));
  assert.match(diver, /if \(REDUCED_MOTION\) return null;/);
  assert.match(diver, /sections\.indexOf\(entry\.target\.id\) % 2 \? -1 : 1/, 'alterna esquerda e direita');
  assert.match(diver, /hop = Math\.sin\(Math\.PI \* t\) \* DIVER_HOP;/, 'o pulo faz um arco');
  assert.match(diver, /y = top \+ progress \* \(viewport\.h - top - size - DIVER_EDGE\);/, 'desce até o fundo da tela quando chega o footer');
  assert.match(diver, /scroll < footerItem\.top/, 'some quando o Kai da cena da tinta assume');
});

function sceneFrames() {
  const { width, height, pixels } = decodePng(readFileSync(join(ROOT, 'assets/kai/ink.png')));
  const w = appConst('SCENE_W');
  const h = appConst('SCENE_H');
  assert.equal(height, h);
  assert.equal(width, w * appConst('SCENE_FRAMES'));
  return Array.from({ length: width / w }, (_, f) => (x, y) => {
    const at = (y * width + f * w + x) * 4;
    return pixels[at + 3] ? hex(pixels[at], pixels[at + 1], pixels[at + 2]) : null;
  });
}

test('a cena final é uma tira de pixel art própria, com o susto, os oito tentáculos agarrando a tela e a tinta', () => {
  const frames = sceneFrames();
  const w = appConst('SCENE_W');
  const h = appConst('SCENE_H');
  const kai = new Set(['idle', 'working', 'done', 'needs', 'sleep', 'lost'].flatMap((mood) => [...read(`assets/kai/${mood}.svg`).matchAll(/fill="(#[0-9A-Fa-f]{6})"/g)].map((m) => m[1].toLowerCase())));
  const extras = new Set([css.match(/--ink: (#[0-9a-f]{6});/)[1]]);
  const [ox, oy] = appConst('SCENE_KAI');
  const idle = read('assets/kai/idle.svg');
  const idleFrames = [...idle.matchAll(/<svg x="\d+" width="48" height="48" viewBox="0 0 48 48">([\s\S]*?)<\/svg>/g)].map(([, body]) => {
    const grid = Array.from({ length: 48 }, () => Array(48).fill(null));
    for (const [, d, color] of body.matchAll(/<path d="([^"]+)" fill="(#[0-9A-Fa-f]+)"/g)) {
      for (const [, x, y, run] of d.matchAll(/M(\d+) (\d+)h(\d+)v1h-\d+z/g)) for (let i = 0; i < Number(run); i += 1) grid[Number(y)][Number(x) + i] = color.toLowerCase();
    }
    return grid;
  });
  assert.equal(appConst('SCENE_IDLE_FRAMES'), idleFrames.length);
  idleFrames.forEach((grid, f) => {
    for (let y = 0; y < 48; y += 1) for (let x = 0; x < 48; x += 1) assert.equal(frames[f](ox + x, oy + y), grid[y][x], `o quadro ${f} da aproximação é o idle ${f} do app, igual ao Kai que desce a página`);
  });
  const firstInk = appConst('SCENE_STEPS').find(([start]) => start >= appConst('SCENE_GRAB') + appConst('SCENE_GRAB_LENGTH') + 0.04)[1] + 1;
  let hatPixels = 0;
  for (let y = 4; y <= 12; y += 1) for (let x = 13; x <= 34; x += 1) if (idleFrames[0][y][x]) hatPixels += 1;
  for (let f = idleFrames.length; f < firstInk; f += 1) {
    const hat = Math.max(...[0, 1, 2, 3, 4].flatMap((lift) => [0, 1].map((shift) => {
      let same = 0;
      for (let y = 4; y <= 12; y += 1) for (let x = 13; x <= 34; x += 1) if (idleFrames[0][y][x] && frames[f](ox + x + shift, oy + y - lift) === idleFrames[0][y][x]) same += 1;
      return same;
    })));
    assert.equal(hat, hatPixels, `o quadro ${f} mantém o boné do Kai do app, com o símbolo`);
  }
  frames.forEach((pixel, f) => {
    for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) {
      const c = pixel(x, y);
      assert.ok(!c || kai.has(c) || extras.has(c), `quadro ${f} usa ${c}, fora da paleta do Kai`);
    }
  });
  const edges = (pixel) => {
    const hit = { left: false, right: false, top: false, bottom: false };
    for (let y = 0; y < h; y += 1) {
      hit.left ||= Boolean(pixel(0, y));
      hit.right ||= Boolean(pixel(w - 1, y));
    }
    for (let x = 0; x < w; x += 1) {
      hit.top ||= Boolean(pixel(x, 0));
      hit.bottom ||= Boolean(pixel(x, h - 1));
    }
    return hit;
  };
  assert.deepEqual(edges(frames[0]), { left: false, right: false, top: false, bottom: false }, 'calmo, longe das bordas');
  const steps = appConst('SCENE_STEPS');
  const grab = steps.find(([start]) => start === appConst('SCENE_GRAB'))[1];
  const reach = edges(frames[grab]);
  assert.ok(reach.left && reach.right && reach.bottom, 'na agarrada os tentáculos chegam às bordas');
  const yellow = css.match(/--yellow: (#[0-9a-f]{6});/)[1];
  [idleFrames.length, idleFrames.length + 1].forEach((f) => {
    let alarm = false;
    for (let y = 0; y < h && !alarm; y += 1) for (let x = 0; x < w; x += 1) if (frames[f](x, y) === yellow) alarm = true;
    assert.ok(alarm, `quadro ${f} é o susto, com o alerta amarelo`);
  });
  const ink = css.match(/--ink: (#[0-9a-f]{6});/)[1];
  const last = frames[frames.length - 1];
  for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) assert.equal(last(x, y), ink, 'o último quadro é a tela toda de tinta');
  assert.equal(steps[steps.length - 1][1], frames.length - 1);
  steps.slice(1).forEach(([start, frame], i) => assert.ok(start > steps[i][0] && frame === steps[i][1] + 1, 'os quadros seguem em ordem'));
  assert.match(css, new RegExp(`url\\("\\/assets\\/kai\\/ink\\.png"\\) no-repeat 0 0 \\/ ${frames.length * 100}% 100%; image-rendering: pixelated;`));
  assert.doesNotMatch(app, /drawArms|INK_ARMS|kai-strip.*grab/, 'nada é desenhado por cima de um sprite pronto');
});

test('a tinta cobre a tela com a cor do fundo e só então aparecem as infos do footer, com o som de pronto do app', () => {
  const phases = ['SCENE_GROW_END', 'SCENE_STARTLE', 'SCENE_GRAB', 'SCENE_INKED', 'SCENE_INFO'].map(appConst);
  phases.slice(1).forEach((value, i) => assert.ok(value >= phases[i], `fase ${i + 1} fora de ordem`));
  assert.ok(appConst('SCENE_INFO') + appConst('SCENE_INFO_LENGTH') < 1, 'as infos ficam paradas no fim');
  assert.equal(appConst('SCENE_STEPS').at(-1)[0], appConst('SCENE_INKED'), 'a tela fica preta no quadro da tinta toda');
  const ink = app.slice(app.indexOf('function mountInk'), app.indexOf('mountReveal();'));
  assert.match(css, /\.foot\.is-live\.is-inked \.foot-stage \{ background: var\(--ink\); \}/);
  assert.match(ink, /if \(p >= SCENE_INFO \+ SCENE_INFO_LENGTH \/ 2 && !rang\) \{\s*rang = true;\s*play\('done'\);/, 'o som de pronto toca uma vez quando as infos aparecem');
  assert.match(ink, /footer\.classList\.toggle\('is-inked', p >= SCENE_INKED\);/);
  assert.match(app, /import \{ CHIME_DONE, chimeSamples, spitSamples \} from '\.\/chime\.js';/);
  const sound = app.slice(app.indexOf('function mountSound'), app.indexOf('function mountDiver'));
  assert.match(sound, /\['pointerdown', 'keydown', 'touchend'\]\.forEach\(\(type\) => addEventListener\(type, unlock, \{ once: true, passive: true \}\)\)/, 'o áudio só liga depois de um gesto do usuário');
  assert.match(sound, /if \(!audio \|\| audio\.state !== 'running'\) return;/);
  assert.match(sound, /const sounds = \{ done: \(rate\) => chimeSamples\(CHIME_DONE, rate\), spit: spitSamples \};/);
  assert.match(sound, /if \(!buffers\.has\(name\)\)/, 'cada som é gerado uma vez só');
  assert.match(app, /mountInk\(document\.querySelector\('\[data-ink\]'\), mountDiver\([^;]*mountSound\(\)\);/);
});

test('o cuspe de tinta tem som próprio, no quadro em que a tinta sai da boca do Kai', () => {
  const frames = sceneFrames();
  const ink = css.match(/--ink: (#[0-9a-f]{6});/)[1];
  const w = appConst('SCENE_W');
  const h = appConst('SCENE_H');
  const firstInk = frames.findIndex((pixel) => {
    for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) if (pixel(x, y) === ink) return true;
    return false;
  });
  assert.equal(appConst('SCENE_STEPS').find(([, frame]) => frame === firstInk)[0], appConst('SCENE_SPIT'));
  const scene = app.slice(app.indexOf('function mountInk'), app.indexOf('mountReveal();'));
  assert.match(scene, /if \(p >= SCENE_SPIT && !spat\) \{\s*spat = true;\s*play\('spit'\);/, 'toca uma vez por passagem');
  assert.match(scene, /if \(p < SCENE_STARTLE\) \{\s*spat = false;\s*rang = false;/, 'volta a tocar se a pessoa subir e descer de novo');
});

test('a URL do ícone do perfil do Windows Terminal existe no site com o mesmo nome de arquivo', () => {
  const source = readFileSync(join(REPO, 'src/terminal_profile.rs'), 'utf8');
  const url = source.match(/pub const ICON_URL: &str = "([^"]+)";/)[1];
  assert.ok(url.startsWith(SITE), url);
  assert.ok(existsSync(localPath(url)), url);
});

test('a paleta do CSS é a mesma do src/tui/theme.rs', () => {
  const theme = readFileSync(join(REPO, 'src/tui/theme.rs'), 'utf8');
  const constants = Object.fromEntries([...theme.matchAll(/pub const (\w+): Color = Color::Rgb\((\d+), (\d+), (\d+)\);/g)].map(([, name, r, g, b]) => [name, hex(r, g, b)]));
  const tokens = {
    INK: 'ink', TEXT: 'text', MUTED: 'muted', DIM: 'dim', FAINT: 'faint', RULE: 'rule', ACCENT: 'pink', LINK: 'link', GREEN: 'green', YELLOW: 'yellow', RED: 'red',
    PAPER: 'paper', CLAUDE: 'claude', CODEX: 'codex', KILO: 'kilo', OPENCODE: 'opencode', ANTIGRAVITY: 'agy', GROK: 'grok', QWEN: 'qwen', KIMI: 'kimi', TERMINAL: 'terminal', TAB_ACTIVE_BG: 'tab',
  };
  for (const [constant, token] of Object.entries(tokens)) {
    assert.ok(constants[constant], constant);
    assert.match(css, new RegExp(`--${token}: ${constants[constant]};`), `--${token} deveria ser ${constants[constant]} (${constant})`);
  }
  const project = [...theme.match(/PROJECT_PALETTE: \[Color; \d+\] = \[([\s\S]*?)\];/)[1].matchAll(/Color::Rgb\((\d+), (\d+), (\d+)\)/g)].map(([, r, g, b]) => hex(r, g, b));
  project.forEach((color, i) => assert.match(css, new RegExp(`--p${i}: ${color};`), `--p${i}`));
});

test('a tabela de agentes é a do README', () => {
  const expected = readmeTable('## Supported agents');
  const tbody = html.match(/<table class="agents-table"[\s\S]*?<tbody>([\s\S]*?)<\/tbody>/)[1];
  const actual = [...tbody.matchAll(/<tr>([\s\S]*?)<\/tr>/g)].map(([, row]) => [...row.matchAll(/<td[^>]*>([\s\S]*?)<\/td>/g)].map(([, cell]) => stripTags(cell)));
  assert.deepEqual(actual, expected);
});

test('o marquee lista todos os agentes de IA do README', () => {
  const marquee = html.match(/<section class="marquee"[\s\S]*?<\/section>/)[0];
  readmeTable('## Supported agents').map(([who]) => who).filter((who) => who !== 'terminal').forEach((who) => assert.match(marquee, new RegExp(`class="k-${who}"`), who));
});

test('os downloads diretos são os arquivos da tabela do Octo no README', () => {
  const links = (section) => [...section.matchAll(/href="https:\/\/github\.com\/lucasmarujo\/octo\/releases\/(?:download\/latest|latest\/download)\/([^"]+)"/g)].map((m) => m[1]);
  const section = (id) => html.slice(html.indexOf(`<section class="section" id="${id}"`)).split('</section>')[0];
  const octo = readmeTable('| System | Download |').map(([, file]) => file);
  assert.ok(octo.includes('Octo-setup.exe'), 'o README lista o instalador do Octo');
  assert.deepEqual(links(section('instalar')).sort(), [...octo].sort());
  assert.deepEqual(links(html).sort(), [...octo].sort());
  assert.match(readmePtBr, /\| Sistema \| Download \|\n\| --- \| --- \|\n\| Windows 10 e 11 x64 \(instalador\) \| `Octo-setup\.exe` \|\n\| Windows 10 e 11 x64 \(sem instalar\) \| `Octo\.exe` \|/);
});

test('o instalador do Windows é o do Octo e sai da versão mais nova, que nunca muda', () => {
  const versioned = 'https://github.com/lucasmarujo/octo/releases/latest/download/Octo-setup.exe';
  for (const [name, source] of [['index.html', html], ['llms.txt', read('llms.txt')]]) {
    const links = [...source.matchAll(/https:\/\/github\.com\/lucasmarujo\/octo\/releases\/[^\s"()]*-setup\.exe/g)].map((m) => m[0]);
    assert.ok(links.length > 0, `${name} sem link do instalador`);
    assert.deepEqual([...new Set(links)], [versioned], name);
  }
  for (const [name, source] of [['README.md', readme], ['README.pt-BR.md', readmePtBr], ['index.html', html], ['llms.txt', read('llms.txt')]]) {
    assert.doesNotMatch(source, /orkai-setup\.exe|releases\/[^\s"()]*\/orkai-/, `${name} ainda aponta para um download do orkai`);
  }
});

test('nenhum link aponta para a build contínua, que não existe mais', () => {
  for (const [name, source] of [['README.md', readme], ['README.pt-BR.md', readmePtBr], ['index.html', html], ['llms.txt', read('llms.txt')]]) {
    assert.doesNotMatch(source, /releases\/(?:tag|download)\/latest/, name);
  }
});

test('a página e os dois READMEs mostram a code signing policy, que diz que o Windows ainda sai sem assinatura', () => {
  const unsigned = { 'README.md': '`Octo.exe` and `Octo-setup.exe` are not code signed yet.', 'README.pt-BR.md': 'O `Octo.exe` e o `Octo-setup.exe` ainda não são assinados.' };
  for (const [name, source] of [['README.md', readme], ['README.pt-BR.md', readmePtBr]]) {
    assert.ok(source.includes(`## Code signing policy\n\n${unsigned[name]}`), `${name}: a seção diz que ainda não há assinatura`);
    assert.doesNotMatch(source, /Free code signing provided by/, `${name}: atribuição da SignPath sem assinatura`);
  }
  const footer = html.match(/<footer[\s\S]*?<\/footer>/)[0];
  assert.match(footer, /<a href="https:\/\/github\.com\/lucasmarujo\/octo#code-signing-policy">code signing policy<\/a>/);
});

test('o Octo para macOS sai assinado e notarizado, e nada manda abrir com o botão direito', () => {
  const notarized = { 'README.md': 'notarized by Apple, with the ticket\nstapled to the app', 'README.pt-BR.md': 'notarizado pela Apple, com o ticket\ngrampeado no app' };
  for (const [name, source] of [['README.md', readme], ['README.pt-BR.md', readmePtBr]]) {
    const policy = source.split('## Code signing policy')[1];
    assert.ok(policy.includes(notarized[name]), `${name}: a seção diz que o app do Mac é notarizado`);
  }
  const workflow = readFileSync(join(REPO, '.github', 'workflows', 'ci.yml'), 'utf8');
  for (const [name, source] of [['README.md', readme], ['README.pt-BR.md', readmePtBr], ['index.html', html], ['ci.yml', workflow]]) {
    assert.doesNotMatch(source, /not notarized|right-click|botão direito|não é notarizado/i, `${name} ainda manda contornar o Gatekeeper`);
  }
  assert.match(html, /Signed and notarized by Apple/);
});

test('o Octo.app nasce de novo a cada release, porque o macOS só relê o ícone de um bundle com data nova', () => {
  const workflow = readFileSync(join(REPO, '.github', 'workflows', 'ci.yml'), 'utf8');
  assert.match(workflow, /app="target\/bundle\/Octo\.app"\n\s+rm -rf "\$app"\n\s+mkdir -p "\$app\/Contents\/MacOS"/);
});

test('cada release publica um SHA256SUMS de todos os arquivos, conferido antes de sair', () => {
  const workflow = readFileSync(join(REPO, '.github', 'workflows', 'ci.yml'), 'utf8');
  const publish = workflow.slice(workflow.indexOf('\n  publish:'));
  const manifest = publish.indexOf('- name: Write manifest');
  const checksums = publish.indexOf('- name: Write the checksums and the report');
  const release = publish.indexOf('- name: Publish the version');
  assert.ok(manifest > 0 && manifest < checksums && checksums < release, 'o SHA256SUMS sai depois do manifest.json e antes da release');
  const step = publish.slice(checksums, release);
  assert.match(step, /working-directory: dist/);
  assert.match(step, /sha256sum -- \* > "\$RUNNER_TEMP\/SHA256SUMS"/);
  assert.match(step, /sha256sum --check --strict SHA256SUMS/);
  assert.match(step, /\$GITHUB_STEP_SUMMARY/);
  assert.match(publish.slice(release), /gh release create "\$TAG" dist\/\*/);
  for (const [name, source] of [['README.md', readme], ['README.pt-BR.md', readmePtBr]]) {
    assert.match(source, /`SHA256SUMS`/, `${name}: o README cita o SHA256SUMS`);
  }
});

test('o README é em inglês e aponta para o README.pt-BR.md, que aponta de volta', () => {
  assert.match(readme, /<b>English<\/b> · <a href="README\.pt-BR\.md">Português \(Brasil\)<\/a>/);
  assert.match(readmePtBr, /<a href="README\.md">English<\/a> · <b>Português \(Brasil\)<\/b>/);
  const text = stripTags(withoutCode(readme).replace(/<a href="README\.pt-BR\.md">[^<]*<\/a>/, ''));
  assert.doesNotMatch(text, /[áàâãéêíóôõúç]/i);
  assert.doesNotMatch(text, /\b(?:para|uma|seus|seu|sua|agentes|quem|sem|todos|voce|projetos|tarefa|pasta|veja|abaixo)\b/i);
});

test('os dois READMEs têm o mesmo vídeo, a mesma estrutura e os mesmos links externos', () => {
  const videos = (markdown) => markdown.match(/^https:\/\/github\.com\/user-attachments\/assets\/[0-9a-f-]+$/gm);
  assert.equal(videos(readme).length, 1);
  assert.deepEqual(videos(readmePtBr), videos(readme));
  const shape = (markdown) => ({
    headings: headings(markdown).map(({ level }) => level),
    tableRows: (withoutCode(markdown).match(/^\|.*\|$/gm) || []).length,
    codeBlocks: (markdown.match(/^```\w*$/gm) || []).length,
    urls: [...markdown.matchAll(/(?:href|src)="(https?:[^"]+)"|\]\((https?:[^)]+)\)/g)].map((m) => (m[1] || m[2]).replace(/^https:\/\/img\.shields\.io\/badge\/.*/, 'badge')).sort(),
  });
  assert.deepEqual(shape(readmePtBr), shape(readme));
});

test('toda âncora dos READMEs aponta para um título do mesmo arquivo', () => {
  const slug = (text) => text.trim().toLowerCase().replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, '').replace(/\s/g, '-');
  for (const [name, markdown] of [['README.md', readme], ['README.pt-BR.md', readmePtBr]]) {
    const ids = new Set();
    for (const { text } of headings(markdown)) {
      const base = slug(text);
      let id = base;
      for (let n = 1; ids.has(id); n += 1) id = `${base}-${n}`;
      ids.add(id);
    }
    const anchors = [...withoutCode(markdown).matchAll(/(?:href="|\]\()#([^")]+)/g)].map((m) => m[1]);
    assert.ok(anchors.length > 0, name);
    anchors.forEach((anchor) => assert.ok(ids.has(anchor), `${name}: #${anchor}`));
  }
});

test('nenhum recurso de terceiros e nada inline que a CSP bloqueie', () => {
  assert.doesNotMatch(html, /<(?:script|img|video|source|iframe)[^>]+src="https?:/);
  assert.doesNotMatch(html, /<link[^>]+rel="(?:stylesheet|preload)"[^>]+href="https?:/);
  assert.doesNotMatch(html, /\sstyle="/, 'atributo style bloqueado pela CSP');
  assert.doesNotMatch(html, /<style[\s>]/, '<style> bloqueado pela CSP');
  const scripts = [...html.matchAll(/<script([^>]*)>/g)].map((m) => m[1]);
  scripts.forEach((attrs) => assert.ok(/type="application\/ld\+json"/.test(attrs) || /src="\/assets\//.test(attrs), attrs));
  assert.doesNotMatch(css, /url\(["']?https?:/);
  assert.doesNotMatch(css, /@import/);
  assert.doesNotMatch(app, /fetch\(|XMLHttpRequest|https?:\/\//);
});

test('todo arquivo local referenciado existe e toda âncora aponta para um id', () => {
  const refs = [...html.matchAll(/(?:src|href|poster)="(\/[^"#]*)"/g)].map((m) => m[1]).filter((p) => p !== '/');
  refs.forEach((ref) => assert.ok(existsSync(join(ROOT, ref)), ref));
  [...css.matchAll(/url\("([^"]+)"\)/g)].forEach(([, ref]) => assert.ok(existsSync(join(ROOT, ref)), ref));
  const ids = new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));
  [...html.matchAll(/href="#([^"]+)"/g)].forEach(([, id]) => assert.ok(ids.has(id), `#${id}`));
  [...html.matchAll(/aria-(?:controls|labelledby)="([^"]+)"/g)].forEach(([, id]) => assert.ok(ids.has(id), id));
});

test('animações: a cena caos→ordem e o tour têm as peças que o app.js espera', () => {
  const chaos = html.match(/<section class="chaos"[\s\S]*?<\/section>/)[0];
  const windows = (chaos.match(/<div class="win[ "]/g) || []).length;
  const rows = [...chaos.matchAll(/data-row="(\d+)"/g)].map((m) => Number(m[1]));
  const scatter = app.match(/const CHAOS_SCATTER = \[([\s\S]*?)\];\n/)[1].match(/\[[^\]]+\]/g).length;
  assert.equal(windows, 8);
  assert.deepEqual(rows, [...Array(windows).keys()]);
  assert.equal(scatter, windows);
  const steps = (html.match(/class="tour-step"/g) || []).length;
  const dots = html.match(/<ol class="tour-dots"[^>]*>([\s\S]*?)<\/ol>/)[1].match(/<li>/g).length;
  const shots = [...html.matchAll(/data-shot="(\d+)"/g)].map((m) => Number(m[1]));
  const keys = app.match(/const TOUR_KEYS = (\[.*\]);/)[1];
  assert.equal(steps, 5);
  assert.equal(dots, steps);
  assert.deepEqual(shots, [...Array(steps).keys()]);
  assert.equal(JSON.parse(keys.replace(/'/g, '"')).length, steps);
});

test('o vídeo só baixa quando o usuário pede e tem poster', () => {
  const video = html.match(/<video[^>]*>/)[0];
  assert.match(video, /preload="none"/);
  const poster = video.match(/poster="([^"]+)"/)[1];
  assert.ok(existsSync(join(ROOT, poster)));
});

test('o player, o JSON-LD e o arquivo do vídeo têm a mesma duração', () => {
  const seconds = Number(jsonLd().find((node) => node['@type'] === 'VideoObject').duration.match(/^PT(\d+)S$/)[1]);
  const label = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
  assert.match(html, new RegExp(`<span class="kicker">in ${seconds} seconds</span>`));
  assert.match(html, new RegExp(`<video [^>]*aria-label="${seconds}-second video`));
  assert.match(html, new RegExp(`aria-label="Watch the ${seconds}-second video"`));
  assert.match(html, new RegExp(`<span class="player-label">watch · ${label}</span>`));
  const src = html.match(/<video src="([^"]+)"/)[1];
  const mp4 = readFileSync(join(ROOT, src));
  const mvhd = mp4.indexOf('mvhd');
  const version = mp4[mvhd + 4];
  const timescale = mp4.readUInt32BE(mvhd + (version === 1 ? 24 : 16));
  const duration = version === 1 ? Number(mp4.readBigUInt64BE(mvhd + 28)) : mp4.readUInt32BE(mvhd + 20);
  assert.equal(Math.round(duration / timescale), seconds, src);
  assert.ok(mp4.indexOf('moov') < mp4.indexOf('mdat'), 'moov antes do mdat (faststart)');
});

test('orçamento de peso para carregar rápido', () => {
  assert.ok(size('index.html') < 120 * 1024, `index.html ${size('index.html')}`);
  assert.ok(size('assets/site.css') + size('assets/app.js') + size('assets/tui.js') + size('assets/chime.js') < 120 * 1024, 'css + js');
  assert.ok(size('assets/kai/ink.png') < 20 * 1024, 'cena da tinta');
  const fonts = readdirSync(join(ROOT, 'assets/fonts')).reduce((sum, file) => sum + size(`assets/fonts/${file}`), 0);
  assert.ok(fonts < 220 * 1024, `fontes ${fonts}`);
  assert.ok(size('assets/octo-launch-v2.mp4') < 12 * 1024 * 1024, 'vídeo');
  assert.ok(size('assets/octo-launch-v2-poster.webp') < 50 * 1024, 'poster');
  assert.ok(size('assets/overview.webp') < 120 * 1024, 'overview');
  ['octo-board', 'octo-list', 'octo-pair', 'octo-agent', 'octo-grid'].forEach((name) => {
    assert.ok(size(`assets/${name}.webp`) < 100 * 1024, name);
    assert.ok(size(`assets/${name}-2x.webp`) < 180 * 1024, `${name}-2x`);
  });
});

test('nginx: CSP estrita, bloqueio de arquivos internos e compose no padrão da VPS', () => {
  const conf = read('nginx.conf');
  const csp = conf.match(/add_header Content-Security-Policy "([^"]+)" always;/)[1];
  ["default-src 'self'", "frame-ancestors 'none'", "object-src 'none'", "base-uri 'self'"].forEach((rule) => assert.ok(csp.includes(rule), rule));
  assert.doesNotMatch(csp, /unsafe-inline|unsafe-eval|https?:/);
  const dot = new RegExp(conf.match(/location ~ (\S+) \{\s*return 404;/)[1]);
  ['/.git/config', '/.env'].forEach((path) => assert.ok(dot.test(path), path));
  assert.ok(!dot.test('/.well-known/security.txt'));
  const internal = new RegExp(conf.match(/location ~ (\^\/\(README[^ ]+) \{/)[1]);
  ['/README.md', '/docker-compose.yml', '/nginx.conf', '/tests/site.test.mjs'].forEach((path) => assert.ok(internal.test(path), path));
  assert.match(conf, /add_header X-Content-Type-Options "nosniff" always;/);
  assert.match(conf, /location \/ \{\s*expires -1;/);
  const compose = read('docker-compose.yml');
  assert.match(compose, /container_name: octo-site/);
  assert.match(compose, /\.\/nginx\.conf:\/etc\/nginx\/conf\.d\/default\.conf:ro/);
  assert.match(compose, /\.\/:\/usr\/share\/nginx\/html:ro/);
  assert.match(compose, /proxy:\s*\n\s*external: true/);
});
