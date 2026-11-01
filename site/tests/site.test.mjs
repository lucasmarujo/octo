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
    assert.equal(raw[y * (stride + 1)], 0, 'linhas sem filtro');
    raw.copy(pixels, y * stride, y * (stride + 1) + 1, (y + 1) * (stride + 1));
  }
  return { width, height, pixels };
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

test('title começa com o produto e cabe no SERP', () => {
  const title = head.match(/<title>([^<]+)<\/title>/)[1];
  assert.ok(title.startsWith('Octo by OctoAI'), title);
  assert.ok(title.length <= 60, `title com ${title.length} caracteres`);
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
  for (const place of ['brand-mascot', 'top-runner', 'kai-hero hero-mascot', 'finale-mascot']) assert.ok(mascots.some((kai) => kai.place === place), place);
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
  for (const mood of ['idle', 'working', 'done']) {
    assert.deepEqual(readFileSync(join(ROOT, `assets/kai/${mood}.svg`)), readFileSync(join(REPO, `desktop/ui/kai/${mood}.svg`)), `assets/kai/${mood}.svg`);
    assert.ok(css.includes(`url("/assets/kai/${mood}.svg")`), mood);
  }
  assert.match(css, /\.kai-hero\.cheer \.kai-strip \{[^}]*done\.svg[^}]*steps\(5, jump-none\) forwards/, 'o clique comemora uma vez');
  const firstPose = readFileSync(join(REPO, 'desktop/ui/kai/idle.svg'), 'utf8').match(/<svg x="0" width="48" height="48" viewBox="0 0 48 48">([\s\S]*?)<\/svg>/)[1];
  const favicon = read('favicon.svg');
  assert.ok(favicon.includes(`<g shape-rendering="crispEdges">${firstPose}</g>`), 'o favicon é a primeira pose do Kai parado');
  assert.match(favicon, /<rect width="48" height="48" rx="[\d.]+" fill="#212B59"\/>/);
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
  const ico = readFileSync(join(ROOT, 'octo-kai.ico'));
  assert.deepEqual(readFileSync(join(ROOT, 'favicon.ico')), ico, 'site/favicon.ico');
  const sizes = icoFrames(ico).map((frame) => frame.readUInt32BE(16));
  assert.deepEqual([...sizes].sort((a, b) => a - b), [16, 32, 48]);
  const touch = readFileSync(join(ROOT, 'octo-kai-touch-icon.png'));
  assert.equal(touch.readUInt32BE(16), 180);
  assert.equal(touch.readUInt32BE(20), 180);
  assert.match(head, /<link rel="icon" href="\/octo-kai\.ico" sizes="48x48">/);
  assert.match(head, /<link rel="apple-touch-icon" href="\/octo-kai-touch-icon\.png">/);
});

test('ícones e imagens trocados ganham nome novo, porque o nginx guarda png e ico por 30 dias', () => {
  const cached = read('nginx.conf').match(/location ~\* \\\.\(\?:([\w|]+)\)\$/)[1].split('|');
  assert.ok(cached.includes('png') && cached.includes('ico'));
  assert.doesNotMatch(html, /\/og-octo\.png|\/octo\.ico|\/octo-touch-icon\.png/);
  for (const old of ['og-octo.png', 'octo.ico', 'octo-touch-icon.png']) assert.ok(!existsSync(join(ROOT, old)), `${old} ficou para trás`);
});

test('a marca é OctoAI, o produto é Octo e a página não oferece mais o orkai CLI', () => {
  const wordmarks = [...html.matchAll(/<span class="wordmark">([^<]+)<\/span>/g)].map((m) => m[1]);
  assert.deepEqual(wordmarks, ['OctoAI', 'OctoAI']);
  const page = html.slice(html.indexOf('<main')).replace(/<code>orkai\/&lt;name&gt;<\/code>/g, '');
  assert.doesNotMatch(stripTags(page), /\borkai\b/i, 'só a branch orkai/<name> das worktrees cita o orkai');
  assert.doesNotMatch(html, /id="cli"|href="#cli"|data-tui\b|data-install=/);
  assert.doesNotMatch(read('llms.txt'), /orkai CLI|orkai-setup/);
});

test('o hero mostra a tela real do Octo, com 1x e 2x', () => {
  const hero = html.slice(html.indexOf('<section class="hero"'), html.indexOf('</section>'));
  const img = hero.match(/<div class="shot" data-screen>\s*<img ([^>]+)>/)[1];
  assert.match(img, /src="\/assets\/octo-board-kai\.webp"/);
  assert.match(img, /width="1440" height="900"/);
  const srcset = img.match(/srcset="([^"]+)"/)[1].split(', ').map((entry) => entry.split(' '));
  assert.deepEqual(srcset.map(([, density]) => density), ['1x', '2x']);
  srcset.forEach(([file]) => assert.ok(existsSync(join(ROOT, file)), file));
  assert.match(app, /stage\.querySelector\('\[data-screen\]'\)/);
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
  const scenes = app.match(/const scenes = \[([\s\S]*?)\n  \];/)[1].match(/^    async \(mine\) => \{/gm).length;
  assert.equal(steps, 5);
  assert.equal(dots, steps);
  assert.equal(scenes, steps);
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
  assert.ok(size('assets/site.css') + size('assets/app.js') + size('assets/tui.js') < 120 * 1024, 'css + js');
  const fonts = readdirSync(join(ROOT, 'assets/fonts')).reduce((sum, file) => sum + size(`assets/fonts/${file}`), 0);
  assert.ok(fonts < 220 * 1024, `fontes ${fonts}`);
  assert.ok(size('assets/octo-launch.mp4') < 12 * 1024 * 1024, 'vídeo');
  assert.ok(size('assets/octo-launch-poster.webp') < 50 * 1024, 'poster');
  assert.ok(size('assets/overview.webp') < 120 * 1024, 'overview');
  assert.ok(size('assets/octo-board-kai.webp') < 100 * 1024, 'octo-board');
  assert.ok(size('assets/octo-board-kai-2x.webp') < 180 * 1024, 'octo-board-2x');
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
