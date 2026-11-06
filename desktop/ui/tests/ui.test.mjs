import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { COLUMNS, USAGE_ALERT, USAGE_WARN, column, duration, elapsed, glyph, kindColor, kindHex, label, usageColor } from '../js/format.js';
import { MOODS, moodOf } from '../js/kai.js';

const REPO = fileURLToPath(new URL('../../../', import.meta.url));
const rust = (file) => readFileSync(join(REPO, file), 'utf8');
const NOW = 1_760_000_000_000;

function agent(overrides) {
  return {
    id: 1,
    session: 'default',
    name: 'physics-merge',
    kind: 'claude',
    state: 'idle',
    starting: false,
    activity: '',
    startedAt: NOW - 60_000,
    stateSince: NOW - 30_000,
    turnStarted: null,
    lastTurn: null,
    ...overrides,
  };
}

test('duration reproduces the duration_short cases of the Rust TUI', () => {
  const cases = [...rust('src/tui/format.rs').matchAll(/assert_eq!\(duration_short\(([\d_ *]+)\), "([^"]+)"\)/g)];
  assert.ok(cases.length >= 5, 'Rust cases not found');
  for (const [, ms, expected] of cases) {
    const value = ms.replaceAll('_', '').split('*').reduce((product, factor) => product * Number(factor.trim()), 1);
    assert.equal(duration(value), expected, ms);
  }
});

test('usage turns yellow and red at the same percentages as the TUI', () => {
  const theme = rust('src/tui/theme.rs');
  assert.equal(USAGE_WARN, Number(theme.match(/USAGE_WARN_PERCENT: f64 = ([\d.]+)/)[1]));
  assert.equal(USAGE_ALERT, Number(theme.match(/USAGE_ALERT_PERCENT: f64 = ([\d.]+)/)[1]));
  assert.equal(usageColor(USAGE_WARN - 0.1), 'var(--color-done)');
  assert.equal(usageColor(USAGE_WARN), 'var(--color-waiting)');
  assert.equal(usageColor(USAGE_ALERT), 'var(--color-lost)');
});

test('every agent kind keeps the color it has in the TUI', () => {
  const theme = rust('src/tui/theme.rs');
  const names = { claude: 'CLAUDE', codex: 'CODEX', kilo: 'KILO', opencode: 'OPENCODE', antigravity: 'ANTIGRAVITY', grok: 'GROK', qwen: 'QWEN', kimi: 'KIMI', pi: 'PI', terminal: 'TERMINAL' };
  for (const [kind, constant] of Object.entries(names)) {
    const [, r, g, b] = theme.match(new RegExp(`pub const ${constant}: Color = Color::Rgb\\((\\d+), (\\d+), (\\d+)\\)`));
    const hex = `#${[r, g, b].map((part) => Number(part).toString(16).padStart(2, '0')).join('')}`;
    assert.equal(kindHex(kind), hex, kind);
    assert.equal(kindColor(kind), `var(--kind-${kind})`, kind);
  }
  const all = rust('src/model.rs').match(/pub const ALL: \[AgentKind; \d+\] = \[([\s\S]*?)\];/)[1].match(/AgentKind::\w+/g).map((name) => name.slice(11).toLowerCase());
  assert.deepEqual(Object.keys(names), all, 'every kind the core knows has its color');
});

test('the dark theme keeps the colors of the TUI and the light theme redefines every one', async () => {
  const { BASE16 } = await import('../js/palette.js');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const [dark, light] = [css.split(':root {')[1].split('}')[0], css.split(':root[data-theme="light"] {')[1].split('}')[0]];
  const kinds = ['claude', 'codex', 'kilo', 'opencode', 'antigravity', 'grok', 'qwen', 'kimi', 'pi', 'terminal'];
  for (const kind of kinds) {
    assert.ok(dark.includes(`--kind-${kind}: ${kindHex(kind)};`), kind);
    assert.ok(light.includes(`--kind-${kind}:`), kind);
  }
  BASE16.forEach((color, index) => {
    assert.ok(dark.includes(`--ansi-${index}: ${color};`), `ansi ${index}`);
    assert.ok(light.includes(`--ansi-${index}:`), `ansi ${index}`);
  });
  for (const token of ['--color-bg', '--color-text', '--term-bg', '--term-fg']) assert.ok(light.includes(`${token}:`), token);
});

const grayTheme = () => readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8').split(':root[data-theme="gray"] {')[1].split('}')[0];
const grayTokens = () => Object.fromEntries([...grayTheme().matchAll(/(--[\w-]+): (#[0-9a-f]{6});/g)].map(([, name, color]) => [name, color]));

function contrast(first, second) {
  const luminance = (color) => {
    const [r, g, b] = [1, 3, 5].map((at) => Number.parseInt(color.slice(at, at + 2), 16) / 255).map((value) => (value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const [high, low] = [luminance(first), luminance(second)].sort((a, b) => b - a);
  return (high + 0.05) / (low + 0.05);
}

test('the gray theme is dark, on neutral grays with no tint, and keeps the brand colors of the dark theme', () => {
  const tokens = grayTokens();
  assert.match(grayTheme(), /color-scheme: dark;/);
  assert.equal(tokens['--color-bg'], '#282828');
  assert.equal(tokens['--term-bg'], '#1e1e1e');
  assert.equal(Object.keys(tokens).length, 20, 'every other token is inherited from the dark theme');
  for (const brand of ['--color-accent', '--color-accent-ink', '--color-waiting', '--color-waiting-ink', '--color-done', '--color-link', '--kind-claude', '--ansi-2', '--ansi-15']) assert.ok(!(brand in tokens), brand);
  for (const [name, color] of Object.entries(tokens)) {
    if (name === '--color-lost' || name === '--ansi-1') continue;
    assert.ok(color.slice(1, 3) === color.slice(3, 5) && color.slice(3, 5) === color.slice(5, 7), `${name} ${color} has a tint`);
  }
});

test('the red of the gray theme stays readable on its background and its surface', () => {
  const tokens = grayTokens();
  assert.ok(contrast(tokens['--color-lost'], tokens['--color-bg']) >= 4.5, 'on the background');
  assert.ok(contrast(tokens['--color-lost'], tokens['--color-surface']) >= 4.5, 'on the surface');
  assert.ok(contrast('#f04e5c', tokens['--color-surface']) < 4.5, 'the red of the dark theme is the reason gray has its own');
});

test('the theme picker offers dark, gray, light and system, and only those reach the page', async (context) => {
  const settings = readFileSync(join(REPO, 'desktop/ui/js/settings.js'), 'utf8');
  const offered = [...settings.match(/const THEMES = \[([\s\S]*?)\];/)[1].matchAll(/\['(\w+)', t\('(\w+)'\)\]/g)].map(([, value, text]) => [value, text]);
  assert.deepEqual(offered, [['dark', 'Dark'], ['gray', 'Gray'], ['light', 'Light'], ['system', 'System']]);
  const stored = new Map();
  let systemIsLight = false;
  globalThis.window = {
    localStorage: { getItem: (key) => stored.get(key) ?? null, setItem: (key, value) => stored.set(key, value), removeItem: (key) => stored.delete(key) },
    matchMedia: () => ({ matches: systemIsLight }),
  };
  globalThis.document = { documentElement: { dataset: {} } };
  context.after(() => {
    delete globalThis.window;
    delete globalThis.document;
  });
  const { applyTheme, themeChoice } = await import('../js/settings.js');
  const applied = (choice, light = false) => {
    if (choice === null) stored.delete('orkai.theme');
    else stored.set('orkai.theme', choice);
    systemIsLight = light;
    applyTheme();
    return document.documentElement.dataset.theme;
  };
  assert.equal(applied('gray'), 'gray');
  assert.equal(themeChoice(), 'gray');
  assert.equal(applied('gray', true), 'gray', 'gray does not follow the system');
  assert.equal(applied(null), 'dark', 'nothing stored is the dark theme');
  assert.equal(applied('light'), 'light');
  assert.equal(applied('dark', true), 'dark');
  assert.equal(applied('system', false), 'dark', 'the system resolves to dark');
  assert.equal(applied('system', true), 'light', 'the system resolves to light');
  assert.equal(applied('sepia'), 'dark', 'an unknown choice falls back to dark');
  assert.match(settings, /const THEME_KEY = 'orkai\.theme';/, 'the choice of older versions keeps working');
});

test('the gray theme has its name in Portuguese', async () => {
  const { translate } = await import('../js/i18n.js');
  assert.equal(translate('pt-BR', 'Gray'), 'Cinza');
  assert.equal(translate('en', 'Gray'), 'Gray');
});

test('the agent screen leaves the agent title to the cards and keeps its status bar for alerts', () => {
  const source = readFileSync(join(REPO, 'desktop/ui/js/agent.js'), 'utf8');
  const status = source.slice(source.indexOf('export function renderStatus'), source.indexOf('\n}\n', source.indexOf('export function renderStatus')));
  assert.match(status, /is waiting for you/);
  assert.doesNotMatch(status, /activity/, 'the session title the agent gave itself is not repeated under its terminal');
});

test('Octo draws its own thin scrollbar in both themes, pink while it is dragged', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const [dark, light] = [css.split(':root {')[1].split('}')[0], css.split(':root[data-theme="light"] {')[1].split('}')[0]];
  for (const token of ['--scroll-thumb', '--scroll-thumb-hover', '--scroll-thumb-grab']) {
    assert.ok(dark.includes(`${token}:`), `dark ${token}`);
    assert.ok(light.includes(`${token}:`), `light ${token}`);
  }
  assert.match(css, /::-webkit-scrollbar-button \{ display: none;/, 'no arrow buttons');
  assert.match(css, /::-webkit-scrollbar-thumb \{[^}]*background: var\(--scroll-thumb\) padding-box;/);
  assert.match(css, /:hover::-webkit-scrollbar-thumb \{ background-color: var\(--scroll-thumb-hover\);/);
  assert.match(css, /::-webkit-scrollbar-thumb:active \{[^}]*background-color: var\(--color-accent\);/);
  assert.doesNotMatch(css, /scrollbar-(?:width|color)\s*:/, 'the standard properties make Chromium drop the custom scrollbar');
  assert.match(css, /\.column-body \{[^}]*scrollbar-gutter: stable;/, 'cards keep one width whether their column scrolls or not');
});

test('agents are labelled like the CLI labels them', () => {
  const model = rust('src/model.rs');
  for (const [, kind, text] of model.matchAll(/AgentKind::(\w+) => "([a-z]+)",/g)) {
    assert.equal(label(kind.toLowerCase()), text, kind);
  }
});

test('every agent state lands in exactly one board column', () => {
  const states = [...rust('src/model.rs').matchAll(/AgentState::(\w+) => "([a-z]+)"/g)].map(([, , text]) => text);
  assert.ok(states.length >= 6, 'Rust states not found');
  const keys = COLUMNS.map((def) => def.key);
  for (const state of states) assert.ok(keys.includes(column(agent({ state }))), state);
  assert.equal(column(agent({ state: 'paused' })), 'needs');
  assert.equal(column(agent({ state: 'working' })), 'working');
  assert.equal(column(agent({ state: 'done' })), 'done');
  assert.equal(column(agent({ state: 'lost' })), 'idle');
  assert.equal(column(agent({ state: 'exited' })), 'idle');
  assert.equal(column(agent({ state: 'paused', starting: true })), 'working', 'an agent still starting is busy');
});

test('glyphs match the symbols of the TUI', () => {
  assert.equal(glyph(agent({ state: 'paused' }), 0).text, '‖');
  assert.equal(glyph(agent({ state: 'done' }), 0).text, '✓');
  assert.equal(glyph(agent({ state: 'lost' }), 0).text, '↻');
  assert.equal(glyph(agent({ state: 'exited' }), 0).text, '✗');
  assert.equal(glyph(agent({ state: 'working', kind: 'terminal' }), 0).text, '$');
  assert.ok(glyph(agent({ state: 'working' }), 3).spin);
});

test('a waiting agent counts the time since it started waiting, a terminal its lifetime', () => {
  assert.equal(elapsed(agent({ state: 'paused' }), NOW).text, '30s');
  assert.equal(elapsed(agent({ state: 'working', turnStarted: NOW - 5_000 }), NOW).text, '5s');
  assert.equal(elapsed(agent({ state: 'working', kind: 'terminal' }), NOW).text, '1m00s');
  assert.equal(elapsed(agent({ state: 'exited' }), NOW), null);
});

test('Kai shows the most urgent state among every agent', () => {
  assert.equal(moodOf([]), 'idle');
  assert.equal(moodOf([agent({ state: 'idle' })]), 'idle');
  assert.equal(moodOf([agent({ state: 'lost' }), agent({ state: 'done' })]), 'done');
  assert.equal(moodOf([agent({ state: 'done' }), agent({ state: 'working' })]), 'working');
  assert.equal(moodOf([agent({ state: 'working' }), agent({ state: 'paused' })]), 'needs');
  assert.equal(moodOf([agent({ state: 'lost' })]), 'lost');
  assert.equal(moodOf([agent({ state: 'paused', starting: true })]), 'working');
  for (const mood of ['idle', 'working', 'needs', 'done', 'lost']) assert.ok(MOODS.includes(mood));
});

test('the Kai on the phone picks its state the same way as the one on the desktop', () => {
  const phone = readFileSync(join(REPO, 'src/pwa/web/app.js'), 'utf8');
  const source = phone.match(/function moodOf\(list\) \{[\s\S]*?\n {2}\}/)[0];
  const phoneMoodOf = new Function(`${source}; return moodOf;`)();
  const states = ['idle', 'working', 'paused', 'done', 'lost', 'exited'];
  for (const first of states) {
    for (const second of states) {
      for (const starting of [false, true]) {
        const list = [agent({ state: first, starting }), agent({ state: second })];
        assert.equal(phoneMoodOf(list), moodOf(list), `${first}${starting ? ' starting' : ''} + ${second}`);
      }
    }
  }
  assert.equal(phoneMoodOf([]), moodOf([]));
  const css = readFileSync(join(REPO, 'src/pwa/web/style.css'), 'utf8');
  for (const mood of MOODS) assert.ok(css.includes(`url("/kai/${mood}.svg")`), mood);
});

test('every mood of Kai has its animation in the stylesheet', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  for (const mood of MOODS) assert.ok(css.includes(`.kai[data-mood="${mood}"]`), mood);
  assert.ok(css.includes('prefers-reduced-motion'), 'motion can be turned off');
});

test('Kai only animates what the compositor runs on its own, so it stays at 60 fps', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const frames = [...css.matchAll(/@keyframes (kai-[\w-]+) \{(.*)\}\s*$/gm)];
  assert.ok(frames.length >= 2, 'Kai keyframes not found');
  for (const [, name, body] of frames) {
    const properties = [...body.matchAll(/([a-z-]+)\s*:/g)].map((match) => match[1]);
    for (const property of properties) assert.ok(['transform', 'opacity'].includes(property), `${name} animates ${property}`);
  }
  const animated = [...css.matchAll(/^([^{}@]*\.kai[^{}]*)\{[^}]*animation:\s*kai-/gm)].map((match) => match[1]);
  assert.ok(animated.length >= 6, 'Kai animations not found');
  for (const selector of animated) {
    for (const part of selector.split(',')) assert.ok(/\.kai-[\w-]+\s*$/.test(part.trim()), `animated inside an svg: ${part.trim()}`);
  }
});

test('every state of the pixel Kai plays its own strip at the pace of the design', async () => {
  const { SLEEP } = await import('../js/kai.js');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const pace = { idle: '3.6s', working: '0.56s', needs: '0.9s', done: '1.2s', lost: '1.6s', [SLEEP]: '4s' };
  for (const mood of [...MOODS, SLEEP]) {
    const strip = readFileSync(join(REPO, `desktop/ui/kai/${mood}.svg`), 'utf8');
    const width = Number(strip.match(/^<svg [^>]*viewBox="0 0 (\d+) 48"/)[1]);
    const count = width / 48;
    assert.ok(Number.isInteger(count) && count >= 4, `${mood} has ${count} frames`);
    assert.equal(strip.match(/<svg x="/g).length, count, `${mood} frames`);
    assert.ok(strip.includes('shape-rendering="crispEdges"'), `${mood} keeps the pixels sharp`);
    assert.doesNotMatch(strip, /<(script|image|use)\b|href=/, `${mood} draws only shapes`);
    const rule = css.match(new RegExp(`\\.kai\\[data-mood="${mood}"\\] \\.kai-strip \\{([^}]*)\\}`));
    assert.ok(rule, `no strip rule for ${mood}`);
    assert.ok(rule[1].includes(`width: ${count * 100}%`), `${mood} strip width`);
    assert.ok(rule[1].includes(`url("kai/${mood}.svg")`), `${mood} strip image`);
    assert.ok(rule[1].includes(`kai-strip-${count} ${pace[mood]} steps(${count}, jump-none)`), `${mood} pace`);
    assert.equal(rule[1].includes('infinite'), mood !== 'done', `${mood} loops unless it is the celebration`);
    const shift = ((count - 1) / count) * 100;
    assert.ok(css.includes(`@keyframes kai-strip-${count} { from { transform: translateX(0); } to { transform: translateX(-${shift}%); } }`), `kai-strip-${count} ends on the last frame`);
  }
  assert.match(css, /\.kai\[data-mood="done"\] \.kai-strip \{[^}]*forwards/, 'the celebration stays on its last frame');
  for (const box of ['brand-mascot', 'kai-sleeping']) {
    const [, width, height] = css.match(new RegExp(`\\.${box} \\{[^}]*width: (\\d+)px; height: (\\d+)px`));
    assert.equal(width, height, box);
    assert.equal(Number(width) % 48, 0, `${box} shows whole pixels of the 48 x 48 art`);
  }
  assert.match(css, /\.brand-mascot \{[^}]*width: 96px/, 'the sidebar Kai is twice the art, as asked');
});

test('the colored git diff becomes colored runs without escape codes', async () => {
  const { sgrRuns, BASE16 } = await import('../js/palette.js');
  assert.deepEqual(sgrRuns('plain line'), [{ text: 'plain line', color: null, bold: false, dim: false }]);
  const added = sgrRuns('\x1b[32m+ added\x1b[m');
  assert.deepEqual(added, [{ text: '+ added', color: 'var(--ansi-2)', bold: false, dim: false }]);
  const header = sgrRuns('\x1b[1mdiff --git a/x b/x\x1b[m');
  assert.equal(header[0].bold, true);
  assert.equal(header.map((run) => run.text).join(''), 'diff --git a/x b/x');
  assert.equal(sgrRuns('\x1b[38;5;196mred\x1b[39m')[0].color, 'rgb(255,0,0)');
  assert.equal(sgrRuns('\x1b[38;2;1;2;3mrgb')[0].color, 'rgb(1,2,3)');
  assert.equal(sgrRuns('a\x1b[Kb').map((run) => run.text).join(''), 'ab', 'other escapes are dropped');
  const style = { color: null, bold: false, dim: false };
  sgrRuns('\x1b[31mstarts red', style);
  assert.equal(sgrRuns('and keeps going', style)[0].color, 'var(--ansi-1)', 'a color carries over to the next line');
});

test('alt N opens the agent waiting the longest, then the ones that are done', async () => {
  const { state } = await import('../js/state.js');
  const { nextAttention } = await import('../js/agent.js');
  state.overview = {
    agents: [
      agent({ id: 1, state: 'done', stateSince: NOW - 90_000 }),
      agent({ id: 2, state: 'paused', stateSince: NOW - 10_000 }),
      agent({ id: 3, state: 'paused', stateSince: NOW - 60_000 }),
      agent({ id: 4, state: 'working', stateSince: NOW - 99_000 }),
    ],
  };
  assert.equal(nextAttention(null).id, 3);
  assert.equal(nextAttention(3).id, 2, 'the agent already open is skipped');
  state.overview.agents = state.overview.agents.filter((a) => a.state !== 'paused');
  assert.equal(nextAttention(null).id, 1);
  state.overview.agents = [agent({ id: 5, state: 'idle' })];
  assert.equal(nextAttention(null), null);
});

test('the list groups agents by project in the order the core sends them', async () => {
  const { groupByProject } = await import('../js/format.js');
  const groups = groupByProject([
    agent({ id: 1, project: 'studio/game', color: '#baa6ff' }),
    agent({ id: 2, project: 'acme/web', color: '#8ce2d6' }),
    agent({ id: 3, project: 'studio/game', color: '#baa6ff' }),
  ]);
  assert.deepEqual(
    groups.map((group) => [group.project, group.color, group.agents.map((member) => member.id)]),
    [
      ['studio/game', '#baa6ff', [1, 3]],
      ['acme/web', '#8ce2d6', [2]],
    ],
  );
  assert.deepEqual(groupByProject([]), []);
});

test('the project filter shows one project and falls back to all of them when it is gone', async () => {
  const { state, visible, currentProject, setProject, projectsInSession } = await import('../js/state.js');
  state.session = 'default';
  state.overview = {
    agents: [
      agent({ id: 1, project: 'acme/web' }),
      agent({ id: 2, project: 'studio/game' }),
      agent({ id: 3, project: 'acme/web' }),
      agent({ id: 4, project: 'lab/infra', session: 'weekend' }),
    ],
  };
  assert.deepEqual(projectsInSession(), ['acme/web', 'studio/game']);
  assert.deepEqual(visible().map((a) => a.id), [1, 2, 3]);
  setProject('acme/web');
  assert.equal(currentProject(), 'acme/web');
  assert.deepEqual(visible().map((a) => a.id), [1, 3]);
  state.overview.agents = state.overview.agents.filter((a) => a.project !== 'acme/web');
  assert.equal(currentProject(), null, 'a project without agents is no longer a filter');
  assert.deepEqual(visible().map((a) => a.id), [2]);
  setProject(null);
});

test('the grid asks for the screens of the running agents, as many as the core answers', async () => {
  const { MAX_PEEK_AGENTS, peekRequest } = await import('../js/grid.js');
  const { MAX_PREVIEW_ROWS } = await import('../js/format.js');
  const actions = rust('src/pwa/actions.rs');
  assert.ok(actions.includes(`MAX_PEEK_AGENTS: usize = ${MAX_PEEK_AGENTS};`), 'same agent limit as the core');
  assert.equal(MAX_PREVIEW_ROWS, Number(actions.match(/MAX_PEEK_ROWS: u16 = (\d+);/)[1]), 'same row limit as the core');
  const list = [
    agent({ id: 1, state: 'working' }),
    agent({ id: 2, state: 'exited' }),
    agent({ id: 3, state: 'lost' }),
    agent({ id: 4, state: 'idle', starting: true }),
    agent({ id: 5, state: 'paused' }),
  ];
  assert.deepEqual(peekRequest(list, 18), { type: 'peek', agents: [1, 5], rows: 18 });
  assert.equal(peekRequest(list.slice(1, 4), 18), null, 'nothing running, nothing to ask');
  const many = Array.from({ length: MAX_PEEK_AGENTS + 6 }, (_, index) => agent({ id: index + 1, state: 'idle' }));
  assert.equal(peekRequest(many, 18).agents.length, MAX_PEEK_AGENTS);
});

test('the grid shows as many agents per screen as the user picks, each with as many lines as fit', async () => {
  const { DEFAULT_GRID_SIZE, GRID_SIZES, MAX_PREVIEW_ROWS, gridShape, previewRows } = await import('../js/format.js');
  assert.deepEqual(GRID_SIZES, ['4', '6', '8', '10', '12', 'all']);
  assert.ok(GRID_SIZES.includes(DEFAULT_GRID_SIZE));
  for (const size of GRID_SIZES.filter((size) => size !== 'all')) {
    const { cols, rows } = gridShape(size);
    assert.equal(cols * rows, Number(size), size);
    assert.ok(cols >= rows, `${size} is laid out wider than tall`);
  }
  assert.equal(gridShape('all'), null, 'all lets the tiles flow');
  assert.equal(gridShape('7'), null);
  assert.equal(previewRows(300, 15), 20);
  assert.equal(previewRows(9000, 15), MAX_PREVIEW_ROWS, 'never more than the core sends');
  assert.equal(previewRows(4, 15), 1);
  assert.equal(previewRows(0, 15), MAX_PREVIEW_ROWS, 'before the first layout it asks for the most');
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  const picker = [...html.match(/<div id="grid-size"[\s\S]*?<\/div>/)[0].matchAll(/data-size="([^"]+)"/g)].map((match) => match[1]);
  assert.deepEqual(picker, GRID_SIZES);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.grid \{[^}]*grid-template-columns: repeat\(var\(--grid-cols\)/);
  assert.match(css, /\.grid \{[^}]*grid-auto-rows: calc\(/, 'the picked rows fill the height of the screen');
  assert.doesNotMatch(css, /\.tile-foot/, 'a tile is the terminal with a single line on top');
});

test('grid tiles sit edge to edge and only the title bar carries the status color', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const grid = readFileSync(join(REPO, 'desktop/ui/js/grid.js'), 'utf8');
  assert.match(css, /\.grid \{[^}]*--grid-gap: 1px;/, 'a hairline between tiles instead of a margin');
  assert.doesNotMatch(css.match(/\n\.tile \{[^}]*\}/)[0], /border-radius|border:/, 'a tile is no longer a framed card');
  assert.match(css, /\.tile \{ --tile-tone: var\(--color-text-muted\);/, 'idle and stopped agents stay gray');
  assert.match(css, /\.tile\[data-column="working"\], \.tile\[data-column="needs"\] \{ --tile-tone: var\(--color-waiting\); \}/);
  assert.match(css, /\.tile\[data-column="done"\] \{ --tile-tone: var\(--color-done\); \}/);
  assert.match(css, /\.tile-head \{[^}]*background: color-mix\(in srgb, var\(--tile-tone\)/);
  assert.match(css, /\.tile-name \{[^}]*color: var\(--tile-tone\)/);
  assert.match(css, /\.tile\.live \{[^}]*outline: 1px solid var\(--color-accent\); outline-offset: -1px;/, 'the live ring stays visible on the edge of the grid');
  assert.match(grid, /dataset: \{ agent: String\(agent\.id\), column: column\(agent\) \}/);
  assert.match(grid, /function tileSignature[\s\S]*?agent\.state, agent\.starting/, 'a new status rebuilds the tile with its color');
});

test('arguments typed in the settings become the list the config keeps', async () => {
  const { joinArgs, splitArgs } = await import('../js/format.js');
  assert.deepEqual(splitArgs('--model opus'), ['--model', 'opus']);
  assert.deepEqual(splitArgs('  -c   curl -s -d "$ORKAI_MESSAGE" ntfy.sh/my-topic '), ['-c', 'curl', '-s', '-d', '$ORKAI_MESSAGE', 'ntfy.sh/my-topic']);
  assert.deepEqual(splitArgs(`--title 'two words' ""`), ['--title', 'two words', '']);
  assert.deepEqual(splitArgs(''), []);
  for (const args of [['--model', 'opus'], ['-c', 'echo "hi there"'], ['a b', ''], []]) {
    assert.deepEqual(splitArgs(joinArgs(args)).length, args.length, JSON.stringify(args));
  }
  assert.equal(joinArgs(['-m', 'opencode/big-pickle']), '-m opencode/big-pickle');
  assert.equal(joinArgs(['--title', 'two words']), '--title "two words"');
});

test('the help says the same in English and in Portuguese, and English comes first', async () => {
  const { HELP } = await import('../js/help.js');
  const { LANGUAGES, language } = await import('../js/i18n.js');
  assert.equal(LANGUAGES[0].code, 'en');
  assert.equal(language(), 'en', 'English unless the person picks Portuguese');
  const [en, pt] = [HELP.en, HELP['pt-BR']];
  assert.equal(pt.steps.length, en.steps.length);
  assert.equal(pt.legend.length, en.legend.length);
  assert.deepEqual(pt.legend.map(([mark]) => mark), en.legend.map(([mark]) => mark));
  assert.equal(pt.groups.length, en.groups.length);
  const translated = { '!command': '!comando', 'ctrl click': 'ctrl clique', drag: 'arrastar' };
  en.groups.forEach(([, rows], index) => {
    const keys = pt.groups[index][1].map(([key]) => key);
    assert.deepEqual(keys, rows.map(([key]) => translated[key] || key));
  });
});

test('the chips of the task box show only the agent and the folder, and still name themselves to a screen reader', async () => {
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  assert.doesNotMatch(composer, /class: 'key'/, 'no label next to the value');
  assert.doesNotMatch(composer, /t\('(who|where)'\)/);
  assert.match(composer, /\$\('who'\)\.setAttribute\('aria-label', `\$\{t\('Who runs the task'\)\}: \$\{label\(who\)\}`\);\n\s*\$\('who'\)\.title = t\('Who runs the task'\);/);
  assert.match(composer, /\$\('where'\)\.setAttribute\('aria-label', `\$\{t\('Where the task runs'\)\}: \$\{currentWhere\(\)\}`\);\n\s*\$\('where'\)\.title = currentWhere\(\);/);
  assert.doesNotMatch(readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8'), /\.chip \.key/);
  const { PT } = await import('../js/i18n.js');
  assert.equal(PT['Who runs the task'], 'Quem roda a tarefa');
  assert.equal(PT['Where the task runs'], 'Onde a tarefa roda');
  for (const gone of ['who', 'where']) assert.ok(!Object.hasOwn(PT, gone), `${gone} is no longer a text of the app`);
});

test('the help and the settings no longer point at the who and where labels of the task box', async () => {
  const help = readFileSync(join(REPO, 'desktop/ui/js/help.js'), 'utf8');
  assert.doesNotMatch(help, /key\('(who|where|quem|onde)'\)/);
  assert.ok(help.includes("['Click the folder below the task or press ', key('ctrl L'), '. Each session remembers its own folder.']"));
  assert.ok(help.includes("['Clique na pasta abaixo da tarefa ou aperte ', key('ctrl L'), '. Cada sessão lembra a própria pasta.']"));
  assert.match(readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8'), /\.help-key \{[^}]*white-space: nowrap;/, 'the longer step never breaks ctrl L across two lines');
  const settings = readFileSync(join(REPO, 'desktop/ui/js/settings.js'), 'utf8');
  assert.doesNotMatch(settings, /mono\(t\('who'\)\)/);
  const { PT } = await import('../js/i18n.js');
  const sentence = 'The program Octo runs for each agent and the extra arguments it passes. Only installed agents can be picked in the task box.';
  assert.ok(settings.includes(`t('${sentence}')`));
  assert.equal(PT[sentence], 'O programa que o Octo roda para cada agente e os argumentos extras que ele passa. Só os agentes instalados podem ser escolhidos na caixa de tarefa.');
  assert.ok(!Object.hasOwn(PT, 'The program Octo runs for each agent and the extra arguments it passes. Only installed agents show up in'));
});

test('the accent of every surface is the pink of Kai', () => {
  const pink = '#e85eb6';
  assert.match(readFileSync(join(REPO, 'desktop/ui/kai/idle.svg'), 'utf8').toLowerCase(), new RegExp(pink));
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.equal(css.match(/--color-accent: #[0-9a-f]{6};/g).length, 2);
  assert.ok(css.match(/--color-accent: #[0-9a-f]{6};/g).every((rule) => rule.includes(pink)));
  assert.match(readFileSync(join(REPO, 'src/pwa/web/style.css'), 'utf8'), new RegExp(`--pink: ${pink};`));
  assert.match(readFileSync(join(REPO, 'site/assets/site.css'), 'utf8'), new RegExp(`--pink: ${pink};`));
  assert.match(rust('src/tui/theme.rs'), /ACCENT: Color = Color::Rgb\(232, 94, 182\);/);
  for (const file of ['desktop/ui/styles.css', 'src/pwa/web/style.css', 'site/assets/site.css', 'site/index.html']) {
    assert.doesNotMatch(readFileSync(join(REPO, file), 'utf8'), /ff5f87/i, file);
  }
});

test('a long line of the help wraps inside its column instead of running over the next one', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const rule = (selector) => css.split(`\n${selector} {`)[1].split('}')[0];
  assert.doesNotMatch(rule('.help-row-text'), /white-space: nowrap/);
  assert.match(rule('.help-row'), /min-height: 30px/);
  assert.doesNotMatch(rule('.help-row'), /[^-]height: 30px/);
});

test('the changes and the test of an agent stay on one line in the list', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const rule = css.split('\n.cell-worktree { display: flex;')[1].split('}')[0];
  assert.match(rule, /white-space: nowrap/);
  const longest = '12 files +1204 −333 ✓ test'.length;
  const monoWidth = 7.2;
  const gaps = 3 * 7;
  assert.ok(Number(rule.match(/ width: (\d+)px/)[1]) >= longest * monoWidth - 3 * monoWidth + gaps, 'room for four digit counts');
});

test('the time on a card and on a tile takes the color of the state of the agent', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  for (const selector of ['.card-time', '.tile-time']) {
    assert.doesNotMatch(css.split(`\n${selector} {`)[1].split('}')[0], /[^-]color:/, selector);
  }
  assert.equal(elapsed(agent({ state: 'paused', lastTurn: 1 }), NOW).tone, 't-yellow');
  assert.equal(elapsed(agent({ state: 'idle', lastTurn: 1 }), NOW).tone, 't-muted');
  assert.equal(elapsed(agent({ state: 'idle', kind: 'terminal' }), NOW).tone, 't-muted');
});

test('the small buttons keep the font of the buttons inside the monospaced foot of a card', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css.split('\n.card-foot {')[1].split('}')[0], /font-family: var\(--font-mono\)/);
  assert.match(css.split('\n.button.tiny {')[1].split('}')[0], /font-family: var\(--font-sans\)/);
});

test('an agent that starts waiting or finishes raises one alert, like in the TUI', async () => {
  const { ALERT_REPEAT_MS, detectAlerts } = await import('../js/alerts.js');
  const app = rust('src/tui/app.rs');
  assert.ok(app.includes(`const ALERT_REPEAT_MS: u64 = ${ALERT_REPEAT_MS.toLocaleString('en').replaceAll(',', '_')};`), 'same repeat window as the TUI');
  const last = new Map();
  const first = detectAlerts(new Map(), [agent({ id: 1, state: 'working' })], null, NOW, last);
  assert.deepEqual(first.alerts, [], 'nothing to compare with yet');
  const agents = [
    agent({ id: 1, state: 'paused' }),
    agent({ id: 2, state: 'done' }),
    agent({ id: 3, state: 'done', kind: 'terminal' }),
    agent({ id: 4, state: 'done' }),
    agent({ id: 5, state: 'done' }),
  ];
  const before = new Map([[1, 'working'], [2, 'working'], [3, 'working'], [4, 'working'], [5, 'done']]);
  const { alerts, states } = detectAlerts(before, agents, 4, NOW, last);
  assert.deepEqual(alerts.map((alert) => [alert.id, alert.state]), [[1, 'paused'], [2, 'done']], 'terminals, the open agent and agents already done stay quiet');
  assert.equal(states.get(5), 'done');
  const again = detectAlerts(new Map([[1, 'working']]), [agent({ id: 1, state: 'paused' })], null, NOW + 1000, last);
  assert.deepEqual(again.alerts, [], 'the same alert is not repeated right away');
  const later = detectAlerts(new Map([[1, 'working']]), [agent({ id: 1, state: 'paused' })], null, NOW + ALERT_REPEAT_MS, last);
  assert.equal(later.alerts.length, 1);
});

test('the window title counts who waits and who finished, like the terminal title of the TUI', async () => {
  const { windowTitle } = await import('../js/alerts.js');
  assert.equal(windowTitle([]), 'Octo');
  const agents = [
    agent({ id: 1, state: 'paused' }),
    agent({ id: 2, state: 'paused' }),
    agent({ id: 3, state: 'done' }),
    agent({ id: 4, state: 'done', kind: 'terminal' }),
    agent({ id: 5, state: 'working' }),
  ];
  assert.equal(windowTitle(agents), '‖2 ✔1 · Octo');
  assert.equal(windowTitle([agent({ state: 'done' })]), '✔1 · Octo');
});

test('the pairing code is drawn from the modules the core encodes', async () => {
  const { qrPath } = await import('../js/pair.js');
  assert.equal(qrPath(['110', '001']), 'M0 0h2v1h-2zM2 1h1v1h-1z');
  assert.equal(qrPath(['000']), '');
});

test('the pairing status and the waiting line follow the bridge', async () => {
  const { phoneStatus, waiting } = await import('../js/pair.js');
  assert.equal(phoneStatus({ enabled: false }), 'off');
  assert.equal(phoneStatus({ enabled: true, phase: 'ready', phones: 2 }), 'connected');
  assert.equal(phoneStatus({ enabled: true, phase: 'ready', phones: 0 }), 'ready');
  assert.equal(phoneStatus({ enabled: true, phase: 'failed' }), 'failed');
  assert.equal(phoneStatus({ enabled: true, phase: 'connecting' }), 'pending');
  assert.match(waiting({ phase: null }).text, /Starting/);
  assert.match(waiting({ phase: 'installing' }).text, /cloudflared/);
  assert.equal(waiting({ phase: 'connecting', tunnel: 'tailscale' }).text, 'Opening the tailscale tunnel…');
  assert.equal(waiting({ phase: 'failed', error: 'no network' }).text, 'no network · retrying');
});

test('the pairing dialog names connected devices and their connection age', async () => {
  const { describeDevice, connectedFor, phoneStatus } = await import('../js/pair.js');
  assert.deepEqual(describeDevice({ agent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Mobile Safari/604.1' }),
    { name: 'iPhone', os: 'iOS 17.2', browser: 'Safari 17' });
  assert.deepEqual(describeDevice({ agent: 'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 Chrome/120.0 Mobile Safari/537.36', model: 'Pixel 8', osVersion: '14.0' }),
    { name: 'Pixel 8', os: 'Android 14', browser: 'Chrome 120' });
  assert.deepEqual(describeDevice({ agent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/122.0' }),
    { name: 'Windows computer', os: 'Windows', browser: 'Chrome 122' });
  assert.equal(connectedFor(1000, 1000), 'just now');
  assert.equal(connectedFor(1000, 1000 + 61_000), '1 min');
  assert.equal(connectedFor(1000, 1000 + 3_661_000), '1 h 01 min');
  assert.equal(connectedFor(1000, 1000 + 90_061_000), '1 d 1 h');
  assert.equal(phoneStatus({ enabled: true, phase: 'ready', phones: 1 }), 'connected');
});

test('a phone tab that another tab of the same browser replaced waits for a tap instead of reconnecting', () => {
  const code = rust('src/pwa/server.rs').match(/pub const REPLACED: u16 = (\d+);/)[1];
  assert.match(rust('src/pwa/web/wire.js'), new RegExp(`const REPLACED = ${code};`), 'the phone knows the code the bridge closes with');
  const app = rust('src/pwa/web/app.js');
  const replaced = app.match(/if \(code === Wire\.REPLACED\) \{([\s\S]*?)\n {4}\}/)[1];
  assert.match(replaced, /setLink\('replaced'\);[\s\S]*showPair\([^)]*, true, false\);\s+return;/);
  assert.doesNotMatch(replaced, /setTimeout|connect\(/, 'reconnecting would replace the other tab in turn, forever');
  assert.match(app, /\['unpaired', 'replaced'\]\.includes\(el\.link\.dataset\.state\)/, 'coming back to the tab or going online does not take the session back either');
});

test('the test line of an agent says what the TUI says about its last run', async () => {
  const { testLine } = await import('../js/agent.js');
  const base = { command: 'cargo test', startedAt: NOW - 50_000, finishedAt: NOW - 41_000, code: 0 };
  assert.deepEqual(testLine({ ...base, state: 'passed' }, NOW), { mark: '✓', tone: 't-green', text: 'Passed after the last turn · 41s ago' });
  assert.equal(testLine({ ...base, state: 'failed', code: 101 }, NOW).text, 'Failed (exit 101) after the last turn · 41s ago');
  assert.equal(testLine({ ...base, state: 'failed', code: null }, NOW).text, 'Failed after the last turn · 41s ago');
  assert.equal(testLine({ ...base, state: 'running', finishedAt: null }, NOW).tone, 't-yellow');
  assert.match(testLine({ ...base, state: 'untrusted' }, NOW).text, /trusted/);
});

test('paths under the home folder are shown from ~ like in the TUI', async () => {
  const { shortPath } = await import('../js/agent.js');
  const { state } = await import('../js/state.js');
  state.home = 'C:\\Users\\ana';
  assert.equal(shortPath('C:\\Users\\ana\\orkai\\worktrees\\3\\studio'), '~/orkai/worktrees/3/studio');
  assert.equal(shortPath('D:\\work\\app'), 'D:/work/app');
  state.home = '/home/ana';
  assert.equal(shortPath('/home/ana/dev/app'), '~/dev/app');
  state.home = null;
  assert.equal(shortPath('/srv/app'), '/srv/app');
});

test('the composer knows the same slash commands as the TUI, in the same order', async () => {
  const { COMMANDS } = await import('../js/commands.js');
  const complete = rust('src/tui/complete.rs');
  const list = complete.slice(complete.indexOf('pub const COMMANDS'), complete.indexOf('];', complete.indexOf('pub const COMMANDS')));
  const usages = [...list.matchAll(/"(\/[^"]+)"/g)].map((match) => match[1]);
  assert.ok(usages.length >= 20, 'TUI commands not found');
  assert.deepEqual(COMMANDS.map(([usage]) => usage), usages);
});

test('tab fills the suggestion selected in the palette and the arrows move the selection', async () => {
  const { acceptedSuggestion, nextSuggestion } = await import('../js/commands.js');
  assert.equal(acceptedSuggestion('/where <folder>', 'command'), '/where ', 'a command that takes arguments leaves room for them');
  assert.equal(acceptedSuggestion('/quit', 'command'), '/quit');
  assert.equal(acceptedSuggestion('/maestri-chat', 'skill'), '/maestri-chat ', 'a skill always takes arguments');
  assert.equal(nextSuggestion(0, 1, 3), 1);
  assert.equal(nextSuggestion(2, 1, 3), 0, 'down from the last goes back to the first');
  assert.equal(nextSuggestion(0, -1, 3), 2, 'up from the first goes to the last');
  assert.equal(nextSuggestion(0, 1, 0), 0);
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  assert.match(composer, /if \(paletteItems\.length\) acceptSuggestion\(paletteItems\[paletteIndex\]\);\n\s*return true;/, 'tab takes the selected one, even when several match');
  assert.match(composer, /onclick: \(\) => acceptSuggestion\(item\)/, 'a click fills the same way');
  assert.match(composer, /class: `palette-item \$\{item\.kind\}\$\{index === paletteIndex \? ' active' : ''\}`/);
  assert.match(composer, /\(event\.key === 'ArrowDown' \|\| event\.key === 'ArrowUp'\) && !event\.ctrlKey && !\$\('palette'\)\.hidden/, 'ctrl+arrows still walk the history');
  assert.match(composer, /if \(word !== paletteWord\) \{\n\s*paletteWord = word;\n\s*paletteIndex = 0;/, 'typing more selects the first match again');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.palette-item:hover, \.palette-item\.active \{ background: var\(--color-border\); \}/);
});

test('a skill in the palette takes the color of the selected agent and a command keeps the text color', () => {
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  assert.match(composer, /kind: 'skill', color: kindColor\(currentWho\(\)\) \}/);
  assert.match(composer, /h\('span', \{ class: 'palette-usage', style: item\.color && `color:\$\{item\.color\}` \}, item\.text\)/);
  assert.doesNotMatch(composer, /kind: 'command', color/);
  assert.match(composer, /setWho\(kinds\[[^\n]*\n\s*renderComposer\(\);\n\s*renderPalette\(\);/, 'shift+tab with the palette open repaints the skills');
  assert.match(composer, /setWho\(kind\);\n\s*closeMenu\(\);\n\s*renderComposer\(\);\n\s*renderPalette\(\);/, 'the who menu with the palette open repaints the skills');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.doesNotMatch(css, /\.palette-item\.skill/, 'the accent no longer paints a skill');
  assert.match(css, /\.palette-usage \{[^}]*color: var\(--color-text\); \}/);
});

test('a slash command is read with its argument, and agent names take the TUI aliases', async () => {
  const { parseCommand, parseKind } = await import('../js/commands.js');
  assert.deepEqual(parseCommand('/race cc,codex fix the login'), { name: 'race', word: 'race', arg: 'cc,codex fix the login' });
  assert.deepEqual(parseCommand('/cd ~/dev'), { name: 'where', word: 'cd', arg: '~/dev' });
  assert.equal(parseCommand('/rm').name, 'kill');
  assert.equal(parseCommand('fix the bug'), null);
  assert.equal(parseCommand('/review\nsecond line'), null, 'a task with several lines is a task');
  const model = rust('src/model.rs');
  for (const [, names, kind] of model.matchAll(/((?:"[a-z]+" \| )*"[a-z]+") => Some\(AgentKind::(\w+)\)/g)) {
    for (const alias of names.split(' | ').map((name) => name.replaceAll('"', ''))) {
      assert.equal(parseKind(alias), kind.toLowerCase(), alias);
    }
  }
});

test('a card sums up its worktree and its tests like the overview of the TUI', async () => {
  const { changeParts, subagentLine, testMark } = await import('../js/format.js');
  assert.equal(changeParts(null), null);
  assert.equal(changeParts({ files: 0, insertions: 0, deletions: 0 }), null, 'nothing changed yet shows the branch');
  assert.deepEqual(changeParts({ files: 4, insertions: 12, deletions: 3 }).map((part) => part.text), ['4 files', '+12', '−3']);
  assert.equal(changeParts({ files: 1, insertions: 2, deletions: 0 })[0].text, '1 file');
  assert.deepEqual(testMark('passed'), { text: '✓ test', tone: 't-green' });
  assert.equal(testMark('failed').tone, 't-red');
  assert.equal(testMark(null), null);
  assert.deepEqual(subagentLine({ role: 'Explore', description: 'Find callers', activity: 'Grep', startedAt: NOW - 12_000 }, NOW), { role: 'Explore', time: '12s', text: 'Find callers · Grep' });
});

test('a card on the board lists every subagent of its agent', () => {
  const board = readFileSync(join(REPO, 'desktop/ui/js/board.js'), 'utf8');
  assert.match(board, /const subagents = \(agent\.subagents \|\| \[\]\)\.map\(/);
});

test('the agent terminal reads the raw frames the holder bridge sends, in the same tags as the Rust side', async () => {
  const { ENDED, OUTPUT, SNAPSHOT, decodeFrame } = await import('../js/termwire.js');
  const source = rust('desktop/src/term.rs');
  for (const [name, value] of Object.entries({ OUTPUT, SNAPSHOT, ENDED })) assert.match(source, new RegExp(`pub const ${name}: u8 = ${value};`), name);
  const output = decodeFrame(new Uint8Array([OUTPUT, 0x68, 0x69]).buffer);
  assert.equal(output.kind, 'output');
  assert.deepEqual([...output.data], [0x68, 0x69]);
  const snapshot = decodeFrame([SNAPSHOT, 30, 0, 44, 1, 0x61]);
  assert.deepEqual({ ...snapshot, data: [...snapshot.data] }, { kind: 'snapshot', rows: 30, cols: 300, data: [0x61] });
  assert.deepEqual(decodeFrame(new Uint8Array([ENDED])), { kind: 'ended' });
  assert.equal(decodeFrame(new Uint8Array([SNAPSHOT, 1])), null, 'a cut snapshot is ignored');
  assert.equal(decodeFrame(new Uint8Array([])), null);
  assert.equal(decodeFrame('text'), null);
});

test('links the agent shows by their title survive in the snapshot and are found on screen', async () => {
  const { LINK_RECORD_OSC, findLinks, parseLinkRecord } = await import('../js/termwire.js');
  assert.match(rust('src/termlink.rs'), new RegExp(`pub const LINK_RECORD_OSC: &str = "${LINK_RECORD_OSC}";`));
  const encode = (text) => Buffer.from(text, 'utf8').toString('base64');
  const record = parseLinkRecord(`${encode('https://example.com/ünï')};${encode('the guide ✓')}`);
  assert.deepEqual(record, { url: 'https://example.com/ünï', text: 'the guide ✓' });
  assert.equal(parseLinkRecord('only-one-part'), null);
  assert.equal(parseLinkRecord('%%%;%%%'), null);
  assert.deepEqual(findLinks('read the guide ✓ and the guide ✓ again', [record]), [
    { start: 5, end: 16, url: 'https://example.com/ünï' },
    { start: 21, end: 32, url: 'https://example.com/ünï' },
  ]);
  assert.deepEqual(findLinks('nothing here', [record]), []);
});

test('keys in the agent terminal copy a selection, paste through the browser and reach the agent otherwise', async () => {
  const { keyRoute } = await import('../js/termwire.js');
  const key = (key, mods = {}) => ({ key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });
  assert.equal(keyRoute(key('c', { ctrlKey: true }), true), 'copy');
  assert.equal(keyRoute(key('c', { ctrlKey: true }), false), 'terminal', 'without a selection ctrl+c interrupts the agent');
  assert.equal(keyRoute(key('C', { ctrlKey: true, shiftKey: true }), true), 'terminal');
  assert.equal(keyRoute(key('v', { ctrlKey: true }), false), 'paste');
  assert.equal(keyRoute(key('Insert', { shiftKey: true }), false), 'paste');
  assert.equal(keyRoute(key('a'), true), 'terminal');
});

test('the agent terminal takes its colors from the theme tokens', async () => {
  const { terminalTheme } = await import('../js/termwire.js');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const dark = css.split(':root {')[1].split('}')[0];
  const read = (name) => dark.match(new RegExp(`${name}: (#[0-9a-f]{6});`))[1];
  const theme = terminalTheme(read);
  assert.equal(theme.background, read('--term-bg'));
  assert.equal(theme.foreground, read('--term-fg'));
  assert.equal(theme.red, read('--ansi-1'));
  assert.equal(theme.brightWhite, read('--ansi-15'));
  assert.match(theme.selectionBackground, /^#[0-9a-f]{8}$/);
  assert.equal(Object.keys(theme).filter((name) => /^(bright)?[A-Za-z]+$/.test(name) && !['background', 'foreground', 'cursor', 'cursorAccent', 'selectionBackground'].includes(name)).length, 16);
});

test('the real terminal ships inside the app, licensed, and loads before the interface', () => {
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  const scripts = [...html.matchAll(/<script(?: type="module")? src="([^"]+)"><\/script>/g)].map((match) => match[1]);
  assert.deepEqual(scripts.slice(0, -1), ['vendor/xterm/xterm.js', 'vendor/xterm/addon-fit.js', 'vendor/xterm/addon-webgl.js', 'vendor/xterm/addon-web-links.js', 'vendor/xterm/addon-unicode11.js']);
  assert.equal(scripts.at(-1), 'js/main.js');
  assert.match(html, /<link rel="stylesheet" href="vendor\/xterm\/xterm.css">\s*<link rel="stylesheet" href="styles.css">/);
  for (const file of [...scripts, 'vendor/xterm/xterm.css', 'vendor/xterm/LICENSE']) assert.ok(existsSync(join(REPO, 'desktop/ui', file)), file);
  assert.match(readFileSync(join(REPO, 'desktop/ui/vendor/xterm/LICENSE'), 'utf8'), /@xterm\/xterm 5\.5\.0[\s\S]*MIT License/);
  const csp = JSON.parse(rust('desktop/tauri.conf.json')).app.security.csp;
  assert.match(csp, /script-src 'self'(;|$)/, 'the terminal scripts are local files, allowed by the CSP');
  assert.doesNotMatch(html, /class="terminal"/, 'xterm names its own root .terminal, so the frame uses another class');
});

test('every terminal names its session, so a late close never ends the terminal that replaced it', () => {
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  for (const command of ['term_input', 'term_resize', 'term_close']) assert.match(main, new RegExp(`invoke\\('${command}', \\{ session`), command);
  const terminal = readFileSync(join(REPO, 'desktop/ui/js/terminal.js'), 'utf8');
  assert.match(terminal, /this\.handlers\.close\(this\.session\)/);
  assert.match(terminal, /else this\.handlers\.close\(session\)|this\.handlers\.close\(session\);\n\s*return;/, 'a terminal closed while it was opening lets its new session go');
  assert.match(rust('desktop/src/term.rs'), /pub fn term_close\(console: State<'_, Console>, session: SessionId\)/);
});

test('a terminal lets go of its own session when it opens another and never of one it does not own', () => {
  const terminal = readFileSync(join(REPO, 'desktop/ui/js/terminal.js'), 'utf8');
  const open = terminal.slice(terminal.indexOf('async open('), terminal.indexOf('\n  }\n', terminal.indexOf('async open(')));
  assert.match(open, /const previous = this\.session;[\s\S]*?if \(previous !== null\) this\.handlers\.close\(previous\);/);
  assert.match(open, /this\.live\.hidden = true;/, 'the back to live button of the last screen does not stay');
  const console = rust('desktop/src/term.rs');
  assert.doesNotMatch(console, /close_current/, 'opening a terminal never ends the one another terminal holds');
  assert.match(console, /sessions: Sessions/);
  assert.match(rust('desktop/src/link.rs'), /PageLoadEvent::Started[\s\S]*?close_all\(\)/, 'a reloaded page lets go of the terminals it left open');
});

test('a terminal whose session ends before any screen arrives opens it again at once', () => {
  const terminal = readFileSync(join(REPO, 'desktop/ui/js/terminal.js'), 'utf8');
  assert.match(terminal, /if \(this\.received\) return;\n\s*this\.host\.dataset\.status = 'ended';\n\s*if \(this\.retry\) \{[\s\S]*?this\.unstick\(this\.generation\);/);
  assert.match(terminal, /unstick\(generation\) \{\n\s*if \(generation !== this\.generation \|\| this\.received\) return;\n\s*this\.open\(this\.agent, false\)/, 'only once, and never over a screen it already shows');
  assert.equal(terminal.match(/this\.received = true;/g).length, 2, 'a snapshot or any output counts as a screen');
});

test('an agent terminal says it is connecting instead of staying black, retries once and redraws once shown', () => {
  const terminal = readFileSync(join(REPO, 'desktop/ui/js/terminal.js'), 'utf8');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(terminal, /dataset\.status = 'connecting'/);
  assert.match(css, /\[data-status="connecting"\] > \.term-status \{ display: grid; \}/);
  assert.match(terminal, /CONNECT_RETRY_MS = \d+/);
  assert.match(terminal, /unstick\(generation\)/);
  assert.match(terminal, /requestAnimationFrame\(\(\) => this\.redraw\(\)\)/, 'the first screen is drawn again on the next frame');
  assert.match(terminal, /visibilitychange/, 'a window shown again is redrawn');
  assert.match(terminal, /onContextLoss\(\(\) => \{[\s\S]*?this\.xterm\.refresh/, 'losing WebGL falls back and draws at once');
});

test('a grid tile runs its agent in place: a click makes it the live terminal and a tab opens only from its button', () => {
  const grid = readFileSync(join(REPO, 'desktop/ui/js/grid.js'), 'utf8');
  assert.match(grid, /onclick: \(\) => activate\(agent, handlers\)/);
  assert.equal(grid.match(/handlers\.open\(agent\)/g).length, 1, 'only the open-in-a-tab button opens the tab');
  assert.match(grid, /new Terminal\(live\.host, handlers\.term/);
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /event\.target\.closest\('\.xterm'\)\) return;/, 'keys typed into a terminal never reach the overview shortcuts');
  assert.match(main, /function gridShortcut[\s\S]*?event\.key === '\]'[\s\S]*?deactivateGrid\(\)/, 'ctrl+] lets go of the tile');
  for (const place of ['function openAgent', 'function openPage', 'function renderView']) {
    const body = main.slice(main.indexOf(place), main.indexOf('\n}\n', main.indexOf(place)));
    assert.match(body, /deactivateGrid\(\)/, `${place} lets the live tile go`);
  }
});

test('the idle column stops at once only the idle agents that are still running', async () => {
  const { idleRunning } = await import('../js/format.js');
  const list = [
    agent({ id: 1, state: 'idle' }),
    agent({ id: 2, state: 'exited' }),
    agent({ id: 3, state: 'lost' }),
    agent({ id: 4, state: 'working' }),
    agent({ id: 5, state: 'idle', starting: true }),
    agent({ id: 6, state: 'idle' }),
  ];
  assert.deepEqual(idleRunning(list).map((entry) => entry.id), [1, 6]);
  assert.deepEqual(idleRunning([]), []);
  const board = readFileSync(join(REPO, 'desktop/ui/js/board.js'), 'utf8');
  assert.match(board, /dataset: \{ action: 'stop-all' \}, onclick: stopIdle/);
  assert.match(board, /async function stopIdle\(\) \{[\s\S]*?const sure = await confirm\(\{[\s\S]*?danger: true[\s\S]*?for \(const agent of targets\) send\(\{ type: 'stop', agent: agent\.id \}\)/, 'it asks before stopping');
});

test('once stopped, the idle column removes every stopped agent at once, after asking', async () => {
  const { idleStopped } = await import('../js/format.js');
  const list = [
    agent({ id: 1, state: 'idle' }),
    agent({ id: 2, state: 'exited' }),
    agent({ id: 3, state: 'lost' }),
    agent({ id: 4, state: 'done' }),
    agent({ id: 5, state: 'exited', branch: 'orkai/fix' }),
    agent({ id: 6, state: 'paused' }),
  ];
  assert.deepEqual(idleStopped(list).map((entry) => entry.id), [2, 5], 'interrupted agents stay, they can still be recovered');
  assert.deepEqual(idleStopped([]), []);
  const board = readFileSync(join(REPO, 'desktop/ui/js/board.js'), 'utf8');
  assert.match(board, /dataset: \{ action: 'remove-all' \}, onclick: removeStopped/);
  assert.match(board, /removeAll\.hidden = idleStopped\(members\)\.length === 0/, 'it shows up only when something stopped');
  assert.match(board, /async function removeStopped\(\) \{[\s\S]*?const sure = await confirm\(\{[\s\S]*?danger: true[\s\S]*?for \(const agent of targets\) send\(\{ type: 'remove', agent: agent\.id \}\)/, 'it asks before removing');
});

test('an agent outside a worktree shows what is pending in its folder and commits it instead of merging', () => {
  const source = readFileSync(join(REPO, 'desktop/ui/js/agent.js'), 'utf8');
  assert.match(source, /if \(agent\.branch \|\| \(info && info\.changes\)\) parts\.push\(changesSection\(agent, info\)\)/, 'the core sends changes only for a folder inside git');
  assert.match(source, /export async function merge\(agent\) \{\n\s*if \(!agent\.branch\) return commit\(agent\);/, '/merge commits for an agent without a branch');
  assert.match(source, /async function commit\(agent\) \{[\s\S]*?const sure = await confirm\(\{[\s\S]*?if \(sure\) send\(\{ type: 'merge', agent: agent\.id \}\)/, 'it asks before committing');
  assert.match(source, /disabled: busy \|\| !files\.length, onclick: \(\) => commit\(agent\)/, 'a busy agent or a clean folder keeps Commit off');
  assert.match(source, /disabled: busy, onclick: \(\) => merge\(agent\) \}, t\('Merge'\)/, 'an agent in a worktree still merges its branch');
});

test('the agent panel lists the latest commits under its changes, like git log, marking the ones of its branch', () => {
  const source = readFileSync(join(REPO, 'desktop/ui/js/agent.js'), 'utf8');
  assert.match(source, /parts\.push\(changesSection\(agent, info\)\);\n\s*if \(info && info\.commits\) parts\.push\(commitsSection\(agent, info\.commits\)\);/, 'right under the changes, for a worktree or a plain folder');
  const section = source.slice(source.indexOf('function commitsSection'), source.indexOf('\n}\n', source.indexOf('function commitsSection')));
  assert.match(section, /commits\.map\(\(commit\) =>/, 'in the order the core reads them, newest first');
  assert.match(section, /commit\.onBranch \? 'on-branch' : ''/);
  assert.match(section, /agent\.branch && own \? t\('\{count\} on this branch', \{ count: own \}\) : null/);
  assert.match(section, /t\('\{time\} ago', \{ time: duration\(now\(\) - commit\.at\) \}\)/);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.commit\.on-branch \.commit-hash \{ color: var\(--color-accent\); \}/);
  assert.match(css, /\.commit-subject \{[^}]*text-overflow: ellipsis;/);
  assert.match(rust('src/worktree.rs'), /pub const MAX_LOG_COMMITS: usize = \d+;/, 'the core reads a bounded log');
});

test('hovering a commit shows its files like VS Code and a click opens it on the web once it is on the remote', () => {
  const source = readFileSync(join(REPO, 'desktop/ui/js/agent.js'), 'utf8');
  const section = source.slice(source.indexOf('function commitsSection'), source.indexOf('\n}\n', source.indexOf('function commitsSection')));
  assert.match(section, /dataset: \{ commit: commit\.hash \},\n\s*onmouseenter: \(\) => showCommitTip\(agent, commit\),\n\s*onmouseleave: hideCommitTip,\n\s*onclick: \(\) => openCommit\(agent, commit\),/);
  assert.doesNotMatch(section, /title:/, 'no native tooltip covers the files');
  assert.match(source, /invoke\('commit_info', \{ agent: agent\.id, hash: commit\.hash \}\)/, 'the files are read only when a commit is hovered');
  assert.match(source, /if \(!commitInfo\.has\(key\)\)/, 'once per commit');
  assert.match(source, /commitInfo\.delete\(key\);/, 'a commit that could not be read is read again on the next hover');
  assert.match(source, /const url = loaded\.info && loaded\.info\.url;\n\s*if \(url\) invoke\('open_link', \{ url \}\)/, 'only a commit already on the remote opens');
  assert.match(source, /url \? t\('Click to open it on \{host\}', \{ host: new URL\(url\)\.host \}\) : t\('Not on the remote yet\.'\)/);
  assert.match(source, /`tip-status status-\$\{file\.status\.toLowerCase\(\)\}`/);
  assert.match(source, /file\.from \? `\$\{file\.from\} → \$\{file\.path\}` : file\.path/, 'a rename shows where the file came from');
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  assert.match(html, /<div id="commit-tip" class="commit-tip" role="tooltip" hidden><\/div>/);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.commit-tip\[hidden\] \{ display: none; \}/, 'the flex tooltip still hides');
  assert.match(css, /\.commit-tip \{[^}]*pointer-events: none;/, 'the tooltip never steals the hover of the row under it');
  for (const status of ['a', 'm', 'd']) assert.match(css, new RegExp(`\\.tip-status\\.status-${status} \\{ color: var\\(--color-`), status);
  assert.match(css, /\.commit\.linked \{ cursor: pointer; \}/);
  const link = rust('desktop/src/link.rs');
  assert.match(link, /#\[tauri::command\(async\)\]\nfn commit_info\(/, 'git runs off the main thread');
  assert.match(link, /generate_handler!\[[\s\S]*?commit_info,/);
  assert.match(rust('src/worktree.rs'), /pub const MAX_COMMIT_FILES: usize = \d+;/, 'a huge commit lists a bounded number of files');
});

test('a Mac keeps its own traffic lights and Octo draws window controls only where the system draws none', () => {
  const TRAFFIC_LIGHTS_WIDTH = 60;
  const LIGHT_CENTER_BELOW_Y = { tahoe: -2.5, beforeTahoe: 1.5 };
  const [base] = JSON.parse(rust('desktop/tauri.conf.json')).app.windows;
  const macWindows = JSON.parse(rust('desktop/tauri.macos.conf.json')).app.windows;
  assert.equal(macWindows.length, 1);
  const { decorations, titleBarStyle, hiddenTitle, create, ...macRest } = macWindows[0];
  const { decorations: baseDecorations, ...baseRest } = base;
  assert.equal(baseDecorations, false, 'Windows and Linux get the controls Octo draws');
  assert.deepEqual({ decorations, titleBarStyle, hiddenTitle }, { decorations: true, titleBarStyle: 'Overlay', hiddenTitle: true });
  assert.equal(create, false, 'the app creates the Mac window itself to place the traffic lights for the running macOS');
  assert.deepEqual(macRest, baseRest, 'the macOS file replaces the whole window list, so it repeats every other setting');
  const window = rust('desktop/src/window.rs');
  const constant = (name) => Number(window.match(new RegExp(`const ${name}: f64 = ([\\d.]+);`))[1]);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const inset = Number(css.match(/:root\[data-window-controls="native"\] \.titlebar \{ padding-left: (\d+)px; \}/)[1]);
  assert.ok(inset >= constant('TRAFFIC_LIGHTS_X') + TRAFFIC_LIGHTS_WIDTH, 'the tabs start after the traffic lights');
  const [, height, border] = css.match(/\.titlebar \{[^}]*\bheight: (\d+)px;[^}]*border-bottom: (\d+)px/).map(Number);
  const middle = (height - border) / 2;
  assert.equal(constant('TRAFFIC_LIGHTS_Y') + LIGHT_CENTER_BELOW_Y.tahoe, middle, 'centered beside the tabs on macOS 26 and later');
  assert.equal(constant('TRAFFIC_LIGHTS_Y_BEFORE_TAHOE') + LIGHT_CENTER_BELOW_Y.beforeTahoe, middle, 'centered beside the tabs before macOS 26');
  assert.match(rust('desktop/src/link.rs'), /#\[cfg\(target_os = "macos"\)\]\n\s*crate::window::open\(app\)\?;/);
  assert.match(css, /\.window-controls \{ display: none;/, 'no Windows controls flash on a Mac before the answer comes');
  assert.match(css, /:root\[data-window-controls="app"\] \.window-controls \{ display: flex; \}/);
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /win\.isDecorated\(\)\.catch\(\(\) => false\)\.then\(\(native\) => \{\n\s*document\.documentElement\.dataset\.windowControls = native \? 'native' : 'app';/);
  assert.ok(JSON.parse(rust('desktop/capabilities/default.json')).permissions.includes('core:default'), 'core:default lets the window ask whether the system decorates it');
});

test('every text the app shows has a Portuguese version', async () => {
  const { PT } = await import('../js/i18n.js');
  const ui = join(REPO, 'desktop/ui');
  const sources = readdirSync(join(ui, 'js')).filter((file) => file.endsWith('.js')).map((file) => readFileSync(join(ui, 'js', file), 'utf8'));
  const keys = new Set();
  for (const source of sources) {
    assert.doesNotMatch(source, /\bt\(\s*`/, 'a translated text is a literal, never a template');
    for (const [, key] of source.matchAll(/\bt\(\s*'((?:[^'\\]|\\.)*)'/g)) keys.add(key);
    for (const [, key] of source.matchAll(/\bt\(\s*"((?:[^"\\]|\\.)*)"/g)) keys.add(key);
  }
  const html = readFileSync(join(ui, 'index.html'), 'utf8');
  for (const [, key] of html.matchAll(/data-i18n(?:-placeholder|-title|-label)?="([^"]*)"/g)) keys.add(key.replaceAll('&amp;', '&'));
  assert.ok(keys.size > 200, `only ${keys.size} texts found`);
  for (const key of keys) assert.ok(Object.hasOwn(PT, key), `no Portuguese for: ${key}`);
  const shown = [
    ...['Appearance', 'Agents', 'Alerts', 'Phone', 'Updates'],
    ...['running', 'passed', 'failed', 'untrusted'],
    ...[...rust('src/tui/render.rs').matchAll(/USAGE_(?:WEEKLY|MONTHLY)_LABEL: &str = "([^"]+)"/g)].map((match) => match[1]),
    ...[...rust('src/usage.rs').matchAll(/Problem::\w+ => "([^"]+)"/g)].map((match) => match[1]),
    ...[...rust('desktop/src/settings.rs').match(/SHELL_RULE: &str = if cfg!\(windows\) \{ "([^"]+)" \} else \{ "([^"]+)" \}/).slice(1)],
    rust('desktop/src/updates.rs').match(/TRANSLOCATED: &str =\s*"([^"]+)"/)[1],
  ];
  for (const text of shown) assert.ok(Object.hasOwn(PT, text), `no Portuguese for the backend text: ${text}`);
});

test('a Portuguese text keeps every placeholder of the English one', async () => {
  const { PT, translate } = await import('../js/i18n.js');
  const names = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
  for (const [en, pt] of Object.entries(PT)) {
    assert.deepEqual(names(pt), names(en), en);
    assert.notEqual(pt.trim(), '', en);
  }
  assert.equal(translate('pt-BR', 'Stop {name}?', { name: 'kai-1' }), 'Parar kai-1?');
  assert.equal(translate('en', 'Stop {name}?', { name: 'kai-1' }), 'Stop kai-1?');
  assert.equal(translate('pt-BR', 'Nothing translated here'), 'Nothing translated here');
  assert.equal(translate('pt-BR', 'Done {time} ago', {}), 'Terminou há {time}');
});

test('every notice the hub sends reads in Portuguese and stays as is in English', async () => {
  const { translateHub } = await import('../js/i18n.js');
  const sources = ['src/pwa/actions.rs', 'src/pwa/server.rs'].map(rust).join('\n');
  const notices = [...sources.matchAll(/notice\([^;]*?Tone::\w+,\s*(?:format!\()?"((?:[^"\\]|\\.)*)"/g)].map((match) => match[1].replace(/\{[^}]*\}/g, 'alpha'));
  assert.ok(notices.length >= 40, `only ${notices.length} notices found`);
  const outcomes = ['alpha had nothing new for main', 'alpha merged into main', 'exited with code 1', 'exited', 'failed: boom'];
  for (const text of [...notices, ...outcomes]) {
    assert.notEqual(translateHub('pt-BR', text), text, `no Portuguese for the notice: ${text}`);
    assert.equal(translateHub('en', text), text);
  }
  assert.equal(translateHub('pt-BR', 'starting claude fix-login in a new worktree with permissions bypassed…'), 'iniciando claude fix-login num worktree novo sem pedir permissões…');
  assert.equal(translateHub('pt-BR', 'starting terminal npm-dev…'), 'iniciando terminal npm-dev…');
  assert.equal(translateHub('pt-BR', 'session work is now home'), 'a sessão work agora se chama home');
  assert.equal(translateHub('pt-BR', 'fix-login is now login'), 'fix-login agora se chama login');
  assert.equal(translateHub('pt-BR', 'exited with code 3'), 'encerrado com código 3');
  assert.equal(translateHub('pt-BR', 'a text the hub never sends'), 'a text the hub never sends');
});

test('Portuguese is chosen once, kept, and reopens the page where it was picked', () => {
  const i18n = readFileSync(join(REPO, 'desktop/ui/js/i18n.js'), 'utf8');
  assert.match(i18n, /storage\.set\(LANGUAGE_KEY, code === 'en' \? null : code\)/, 'English is the default and is not stored');
  assert.match(i18n, /window\.location\.reload\(\)/);
  const settings = readFileSync(join(REPO, 'desktop/ui/js/settings.js'), 'utf8');
  assert.match(settings, /setLanguage\(code, 'settings'\)/, 'settings has the language picker');
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /applyStatic\(\);\n\s*pageAfterReload = pageToReopen\(\);/);
  assert.match(main, /invoke\('alert', \{ alerts, language: language\(\) \}\)/, 'desktop notifications follow the language');
  assert.match(main, /toast\(hubText\(message\.text\), message\.tone\)/);
});

test('an update that cannot install says why in the language of the app', async () => {
  const { translate } = await import('../js/i18n.js');
  const settings = readFileSync(join(REPO, 'desktop/ui/js/settings.js'), 'utf8');
  assert.match(settings, /t\('Could not check for updates: \{error\}', \{ error: t\(String\(err\)\) \}\)/);
  const translocated = rust('desktop/src/updates.rs').match(/TRANSLOCATED: &str =\s*"([^"]+)"/)[1];
  assert.match(translate('pt-BR', translocated), /Arraste o Octo para Aplicativos/);
  assert.equal(translate('pt-BR', 'error sending request'), 'error sending request', 'any other error reaches the toast as is');
});

test('a right click never opens the menu of the webview, with its inspect, save as and print', () => {
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /document\.addEventListener\('contextmenu', \(event\) => event\.preventDefault\(\)\);/);
  assert.doesNotMatch(rust('desktop/Cargo.toml'), /devtools/, 'a release build never turns the inspector on');
  const windows = ['desktop/tauri.conf.json', 'desktop/tauri.macos.conf.json'].flatMap((file) => JSON.parse(rust(file)).app.windows);
  assert.ok(windows.every((window) => window.devtools !== true), 'no window turns the inspector on');
});

test('the settings button shows a gear, not the sun of the light theme', async () => {
  const { ICON_SETTINGS } = await import('../js/icons.js');
  assert.equal(ICON_SETTINGS.match(/A5 5 0 0 1/g).length, 8, 'eight teeth around the gear');
  assert.match(ICON_SETTINGS, /Z" stroke="currentColor"/, 'one closed outline');
  assert.doesNotMatch(ICON_SETTINGS, /stroke-linecap="round"/, 'no loose rays, like the ones of a sun');
});

test('when the session on screen is gone the app moves to one that exists, even if default was deleted', () => {
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /selectSession\(names\.includes\(state\.overview\.session\) \? state\.overview\.session : names\[0\] \|\| null\);/);
  assert.doesNotMatch(rust('src/store.rs'), /default session cannot be deleted/);
});

test('plan usage lists the providers of the running agents first and folds to two until opened', async () => {
  const { USAGE_FOLDED, usageOrder, usageShown } = await import('../js/format.js');
  const entry = (kind, overrides) => ({ kind, windows: [{ label: '5h', used: 10, resetsAt: null }], credits: null, problem: null, ...overrides });
  const entries = [entry('claude'), entry('codex', { windows: [], problem: 'login expired' }), entry('kilo', { windows: [], credits: 3 }), entry('opencode'), entry('grok')];
  assert.deepEqual(usageOrder(entries, ['opencode', 'codex', 'terminal']).map((item) => item.kind), ['opencode', 'codex', 'claude', 'kilo', 'grok']);
  const idle = usageOrder(entries, []);
  assert.deepEqual(idle.map((item) => item.kind), ['claude', 'kilo', 'opencode', 'grok', 'codex'], 'nothing measured goes last, the rest keep the hub order');
  assert.deepEqual(entries.map((item) => item.kind), ['claude', 'codex', 'kilo', 'opencode', 'grok'], 'ordering leaves the hub list alone');
  assert.equal(USAGE_FOLDED, 2);
  assert.deepEqual(usageShown(idle, false), idle.slice(0, 2));
  assert.equal(usageShown(idle, true), idle);
  assert.deepEqual(usageShown(idle.slice(0, 1), false), idle.slice(0, 1));
  assert.deepEqual(usageShown([], false), []);
});

test('plan usage stays opened or folded as it was left, and only offers the arrow when something is folded away', async () => {
  const { state, toggleUsage } = await import('../js/state.js');
  const before = state.usageOpen;
  toggleUsage();
  assert.equal(state.usageOpen, !before);
  toggleUsage();
  assert.equal(state.usageOpen, before);
  const source = readFileSync(join(REPO, 'desktop/ui/js/state.js'), 'utf8');
  assert.match(source, /usageOpen: storage\.get\(USAGE_KEY\) === 'open'/);
  assert.match(source, /storage\.set\(USAGE_KEY, state\.usageOpen \? 'open' : null\)/, 'folded is the default and is not stored');
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.match(sidebar, /const toggle = total > USAGE_FOLDED/);
  assert.match(sidebar, /keep\(usageFrame, 'head', JSON\.stringify\(\[state\.usageOpen, entries\.length, hidden\]\)/, 'the arrow survives the tick that redraws the sidebar every second');
  assert.match(sidebar, /keep\(usageCards, entry\.kind, JSON\.stringify\(entry\)/, 'a card is rebuilt only when its numbers change');
  assert.match(sidebar, /node\.textContent = t\('resets in \{time\}'/, 'the reset countdown moves without rebuilding the card');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.usage \{[^}]*flex-shrink: 0/, 'folded usage never gets squeezed by a long session list');
  assert.match(css, /\.usage\.open \{ flex-shrink: 1; min-height: 0; \}/, 'opened usage grows upward and stops at the sessions');
  assert.match(css, /\.usage-list \{[^}]*overflow-y: auto/);
});

test('every provider with plan usage has its icon in the real colors of its brand', async () => {
  const { PROVIDER_ICONS } = await import('../js/icons.js');
  const providers = rust('src/usage.rs').match(/pub const PROVIDERS: \[AgentKind; \d+\] = \[([\s\S]*?)\];/)[1].match(/AgentKind::\w+/g).map((name) => name.slice(11).toLowerCase());
  assert.ok(providers.length >= 7, 'Rust providers not found');
  assert.deepEqual(Object.keys(PROVIDER_ICONS).sort(), [...providers].sort());
  for (const [kind, icon] of Object.entries(PROVIDER_ICONS)) {
    assert.match(icon, /^<svg width="(14|22)" height="\1" viewBox="0 0 24 24"[^>]*aria-hidden="true">[\s\S]+<\/svg>$/, kind);
    assert.doesNotMatch(icon, /<title|<script|href=|url\(http/i, kind);
  }
  const brand = { claude: '#D97757', codex: '#3941FF', kilo: '#F8F676', antigravity: '#3186FF', kimi: '#1783FF' };
  for (const [kind, color] of Object.entries(brand)) assert.ok(PROVIDER_ICONS[kind].includes(`"${color}"`), `${kind} keeps ${color}`);
  for (const kind of ['grok', 'opencode']) assert.match(PROVIDER_ICONS[kind], /^<svg [^>]*fill="currentColor"/, `${kind} is black or white, like its logo`);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.usage-icon \{[^}]*overflow: hidden;[^}]*color: var\(--color-text\); background: var\(--color-surface-raised\); \}/, 'a neutral tile, so no brand gets tinted');
  assert.ok(existsSync(join(REPO, 'desktop/ui/vendor/lobehub-icons/LICENSE')), 'the icon set keeps its MIT notice');
});

test('a line of the sidebar shows the provider of its agent, with a mark for those without an icon', async () => {
  const { PROVIDER_ICONS, providerIcon } = await import('../js/icons.js');
  const { kindMark } = await import('../js/format.js');
  for (const kind of Object.keys(PROVIDER_ICONS)) assert.match(providerIcon(kind), /^<svg /, kind);
  for (const kind of ['terminal', 'not-a-kind', 'constructor']) assert.equal(providerIcon(kind), null, kind);
  assert.equal(kindMark('terminal'), '$');
  assert.equal(kindMark('qwen'), 'Q');
  assert.equal(kindMark('pi'), 'P');
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.match(sidebar, /icon \? svg\(icon\) : h\('span', \{ class: 'summary-mark', style: `color:\$\{kindColor\(kind\)\}` \}, kindMark\(kind\)\)/);
  assert.match(sidebar, /summaryIcon\(list\[0\]\.kind\)/);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.summary-icon \{[^}]*width: 12px; height: 12px; flex-shrink: 0;/, 'every line keeps the text in the same column');
  assert.match(css, /\.summary-text \{ flex: 1; min-width: 0;[^}]*text-overflow: ellipsis;/, 'the name gives way first');
  assert.match(css, /\.summary-model \{ flex-shrink: 0; max-width: \d+px;[^}]*text-overflow: ellipsis;/, 'the model keeps its place up to a limit');
  assert.match(css, /\.session-list \{[^}]*container-type: inline-size;/);
  assert.match(css, /@container \(max-width: \d+px\) \{ \.summary-model \{ display: none; \} \}/, 'a narrow sidebar leaves the model to the tooltip');
});

test('every copy of a provider icon gets its own ids, so hiding one never blanks another', async () => {
  const { PROVIDER_ICONS, providerIcon } = await import('../js/icons.js');
  const idsOf = (icon) => [...icon.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
  const refsOf = (icon) => [...icon.matchAll(/url\(#([^)]+)\)/g)].map((match) => match[1]);
  for (const kind of ['codex', 'antigravity']) {
    assert.ok(idsOf(PROVIDER_ICONS[kind]).length > 0, kind);
    const [first, second] = [providerIcon(kind), providerIcon(kind)];
    assert.equal(idsOf(first).filter((id) => idsOf(second).includes(id)).length, 0, `${kind} repeats no id`);
    for (const icon of [first, second]) {
      for (const ref of refsOf(icon)) assert.ok(idsOf(icon).includes(ref), `${kind} points inside itself: ${ref}`);
    }
    const sameLook = (icon) => icon.replace(/\b(id="|url\(#)[^")]+/g, '$1');
    assert.equal(sameLook(first), sameLook(PROVIDER_ICONS[kind]), `${kind} looks the same`);
  }
  assert.equal(providerIcon('claude'), PROVIDER_ICONS.claude, 'an icon without ids stays as it is');
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.doesNotMatch(sidebar, /PROVIDER_ICONS/, 'the sidebar never repeats an id');
});

test('a model becomes a short name by one rule and the tooltip keeps the full id', async () => {
  const { agentTip, agentsTip, shortModel } = await import('../js/format.js');
  const cases = {
    'claude-opus-5-5': 'opus',
    'claude-sonnet-4-5-20250929': 'sonnet',
    'claude-3-5-haiku-20241022': 'haiku',
    'anthropic/claude-sonnet-4-5': 'sonnet',
    'us.anthropic.claude-opus-4-1-20250805-v1:0': 'opus',
    opus: 'opus',
    'sonnet[1m]': 'sonnet',
    'gpt-5': 'gpt-5',
    'gpt-5-codex': 'gpt-5',
    'gpt-5.1-codex-max': 'gpt-5.1',
    'opencode/big-pickle': 'big-pickle',
    'qwen3-coder-plus': 'qwen3-coder',
    o3: 'o3',
  };
  for (const [id, short] of Object.entries(cases)) assert.equal(shortModel(id), short, id);
  for (const missing of [undefined, null, '']) assert.equal(shortModel(missing), '', 'no model, no text');

  const claude = agent({ name: 'one', kind: 'claude', model: 'claude-opus-5-5' });
  const codex = agent({ name: 'two', kind: 'codex', model: 'gpt-5.1-codex-max' });
  const qwen = agent({ name: 'three', kind: 'qwen' });
  const shell = agent({ name: 'four', kind: 'terminal', model: null });
  const agy = agent({ name: 'five', kind: 'antigravity' });
  assert.equal(agentTip(claude), 'one · claude · claude-opus-5-5');
  assert.equal(agentTip(qwen), 'three · qwen', 'a missing model is left out');
  assert.equal(agentTip(agy), 'five · agy');
  assert.equal(agentsTip([claude, codex, qwen, shell]), 'one · claude · claude-opus-5-5\ntwo · codex · gpt-5.1-codex-max\nthree · qwen\nfour · terminal');
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.match(sidebar, /title: agentsTip\(list\)/, 'the +N of a line are in its tooltip');
  assert.match(sidebar, /model \? h\('span', \{ class: 'summary-model' \}, model\) : null/, 'no element for a missing model');
});

test('the options button tints like the most severe option that is on, so bypass is never silently on', async () => {
  const { optionsTone } = await import('../js/format.js');
  assert.equal(optionsTone({ worktree: false, remote: false, bypass: false }), 'neutral');
  assert.equal(optionsTone({ worktree: true, remote: false, bypass: false }), 'default');
  assert.equal(optionsTone({ worktree: false, remote: true, bypass: false }), 'caution');
  assert.equal(optionsTone({ worktree: true, remote: true, bypass: false }), 'caution');
  assert.equal(optionsTone({ worktree: false, remote: false, bypass: true }), 'danger');
  assert.equal(optionsTone({ worktree: true, remote: true, bypass: true }), 'danger');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.toggle\[aria-pressed="true"\] \{ color: var\(--color-link\); border-color: color-mix\(in srgb, var\(--color-link\) 50%, transparent\); background: color-mix\(in srgb, var\(--color-link\) 10%, transparent\); \}/);
  assert.match(css, /\.toggle\.caution\[aria-pressed="true"\] \{ color: var\(--color-waiting\);/);
  assert.match(css, /\.toggle\.danger\[aria-pressed="true"\] \{ color: var\(--color-lost\);/);
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  assert.match(composer, /button\.classList\.toggle\('caution', tone === 'caution'\);\n\s*button\.classList\.toggle\('danger', tone === 'danger'\);\n\s*button\.setAttribute\('aria-pressed', String\(tone !== 'neutral'\)\);/);
  assert.match(composer, /remote: state\.remote, bypass: state\.bypass/, 'remote control shows whichever agent is selected');
  assert.match(composer, /button\.title = name;\n\s*button\.setAttribute\('aria-label', name\);/);
  assert.match(composer, /`\$\{t\('Task options'\)\} · \$\{active\.join\(', '\)\}`/, 'the name lists what is on');
  const { PT } = await import('../js/i18n.js');
  assert.equal(PT['Task options'], 'Opções da tarefa');
});

test('the three options left the row of the task box for a small button right after the folder', async () => {
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  assert.doesNotMatch(html, /id="(worktree|remote|bypass)"/);
  const row = html.split('<div class="composer-options">')[1].split('</form>')[0];
  const ids = [...row.matchAll(/<button id="(\w+)"/g)].map((match) => match[1]);
  assert.deepEqual(ids, ['who', 'where', 'options', 'launch']);
  assert.match(html, /<button id="options" class="toggle options-button" type="button" aria-haspopup="menu" aria-expanded="false" aria-pressed="false"><\/button>/);
  const { ICON_OPTIONS } = await import('../js/icons.js');
  assert.match(ICON_OPTIONS, /d="M2 3h8M2 6h8M2 9h8"/, 'three horizontal bars');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.options-button \{ width: 30px; padding: 0; justify-content: center; flex-shrink: 0; \}/);
});

test('the options open above their button as rows of checkboxes with a small switch in the tone of each one', () => {
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  const order = [...composer.split('const TASK_OPTIONS = [')[1].split('];')[0].matchAll(/name: t\('([^']+)'\), icon: (\w+), tone: 'var\((--color-\w+)\)'/g)].map((match) => match.slice(1));
  assert.deepEqual(order, [
    ['Worktree', 'ICON_BRANCH', '--color-link'],
    ['Remote Control', 'ICON_REMOTE', '--color-waiting'],
    ['Bypass', 'ICON_SHIELD', '--color-lost'],
  ]);
  for (const title of ['Each new AI agent runs in its own git worktree', 'New Claude agents start with Remote Control', 'New AI agents run tools, commands and edits without asking']) assert.ok(composer.split('const TASK_OPTIONS = [')[1].split('];')[0].includes(title), title);
  assert.match(composer, /role: 'menuitemcheckbox'/);
  assert.match(composer, /h\('span', \{ class: 'switch', 'aria-hidden': 'true' \}, h\('span', \{ class: 'switch-knob' \}\)\)/);
  assert.match(composer, /row\.setAttribute\('aria-checked', on\);\n\s*row\.querySelector\('\.switch'\)\.setAttribute\('aria-checked', on\);/);
  assert.match(composer, /openMenu\(\$\('options'\), optionRows, true, optionsClosed\)/);
  assert.match(composer, /\$\('options'\)\.setAttribute\('aria-expanded', 'true'\)/);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.option-item \{ font-family: var\(--font-sans\); font-size: var\(--text-sm\); font-weight: 500; color: var\(--color-text-muted\); \}/, 'unlike the mono menu of agents, and muted while off');
  assert.match(css, /\.option-item\[aria-checked="true"\] \{ color: var\(--tone\); \}/);
  assert.match(css, /\.option-item \.switch\[aria-checked="true"\] \{ background: var\(--tone\); \}/);
  assert.ok(css.indexOf('.option-item .switch-knob') > css.indexOf('.switch[aria-checked="true"]:active .switch-knob'), 'the small switch comes after the rules it overrides');
});

test('flipping an option keeps the menu open: the rows change in place, so no click lands on a detached node', () => {
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  const flip = composer.slice(composer.indexOf('function flipOption'), composer.indexOf('function optionRow'));
  assert.doesNotMatch(flip, /closeMenu|openMenu|replaceChildren/);
  const rows = composer.slice(composer.indexOf('function renderOptionRows'), composer.indexOf('function optionsClosed'));
  assert.doesNotMatch(rows, /replaceChildren|openMenu/, 'an overview arriving mid-click must not swap the row under the mouse');
  assert.match(composer.split('export function renderComposer')[1], /renderOptionRows\(\);\n\}/, 'any change of state refreshes an open menu');
  assert.match(composer, /if \(!\$\('menu'\)\.contains\(event\.target\)\) closeMenu\(\);/);
  assert.match(composer, /\$\('options'\)\.addEventListener\('click', \(event\) => \{\n\s*event\.stopPropagation\(\);\n\s*if \(!menuShown\(\)\) openOptionsMenu\(event\);\n\s*else closeMenu\(\);/);
});

test('a menu tells who opened it when it goes away, by closing or by giving way to another menu', () => {
  const dialogs = readFileSync(join(REPO, 'desktop/ui/js/dialogs.js'), 'utf8');
  assert.match(dialogs, /export function openMenu\(anchor, items, above, onClose = null\) \{\n\s*const menu = \$\('menu'\);\n\s*menuEnded\(\);\n\s*menuOnClose = onClose;/);
  assert.match(dialogs, /if \(menu\.hidden \|\| menuClosing !== null\) return;\n\s*menuEnded\(\);/);
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  const closed = composer.slice(composer.indexOf('function optionsClosed'), composer.indexOf('function openOptionsMenu'));
  assert.match(closed, /optionRows = null;/);
  assert.match(closed, /\$\('options'\)\.setAttribute\('aria-expanded', 'false'\)/);
  assert.match(closed, /\$\('task'\)\.focus\(\)/);
  assert.match(closed, /focused === document\.body \|\| focused === \$\('options'\) \|\| \$\('menu'\)\.contains\(focused\)/, 'a click on another field keeps its focus');
});

test('ctrl T flips the worktree through the state, not through a button that left the page', () => {
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /else if \(plain && key === 't'\) toggleWorktree\(\);/);
  assert.match(main, /import \{ initComposer, renderComposer, renderPalette, setCommandRunner, toggleWorktree \} from '\.\/composer\.js';/);
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  assert.match(composer, /export function toggleWorktree\(\) \{\n\s*setWorktree\(!worktreeOn\(\)\);\n\s*renderComposer\(\);\n\}/);
  const ui = join(REPO, 'desktop/ui/js');
  for (const file of readdirSync(ui)) assert.doesNotMatch(readFileSync(join(ui, file), 'utf8'), /\$\('(worktree|remote|bypass)'\)|#(worktree|remote|bypass)\b/, file);
});

test('the phone button shows a phone and a green outline while a phone is connected', async () => {
  const { phoneConnected } = await import('../js/pair.js');
  assert.equal(phoneConnected({ enabled: true, phase: 'ready', phones: 1 }), true);
  assert.equal(phoneConnected({ enabled: true, phase: 'ready', phones: 0 }), false, 'paired but not open on the phone');
  assert.equal(phoneConnected({ enabled: false, phase: 'ready', phones: 1 }), false, 'turned off');
  assert.equal(phoneConnected({ enabled: true, phase: 'connecting', phones: 1 }), false, 'the tunnel is down');
  assert.equal(phoneConnected({ enabled: true, phase: null, phones: 0 }), false);
  const { ICON_PAIR } = await import('../js/icons.js');
  assert.match(ICON_PAIR, /<rect x="3\.4" y="1" width="7\.2" height="12" rx="1\.8"/, 'a phone, not a QR code');
  const pair = readFileSync(join(REPO, 'desktop/ui/js/pair.js'), 'utf8');
  assert.match(pair, /\$\('pair'\)\.classList\.toggle\('connected', phoneConnected\(pairing\)\)/);
  assert.match(pair, /function render\(pairing\) \{\n\s*markPhone\(pairing\);/, 'the open pairing dialog keeps the outline fresh too');
  assert.match(pair, /setInterval\(check, PHONE_POLL_MS\)/);
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /watchPhone\(\);/);
  assert.match(rust('desktop/src/pair.rs'), /#\[tauri::command\(async\)\]\npub fn pairing\(/, 'polled from the sidebar, so it stays off the window thread');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.phone-status\.connected \{[^}]*box-shadow: inset 0 0 0 1px color-mix\(in srgb, var\(--color-done\) 70%, transparent\); \}/, 'an inset outline, so the footer does not move');
});

test('the phone button is a mini icon to the right of settings, and its status stays on the icon', async () => {
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  const footer = html.match(/<footer class="sidebar-footer">([\s\S]*?)<\/footer>/)[1];
  assert.deepEqual([...footer.matchAll(/<button id="([\w-]+)" class="footer-icon[ "]/g)].map((match) => match[1]), ['open-help', 'open-settings', 'pair']);
  assert.match(footer, /<button id="pair" [^>]*aria-label="Pair phone"[^>]*><\/button>/, 'no text child, and a name before the first status arrives');
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /\$\('pair'\)\.replaceChildren\(svg\(ICON_PAIR\)\);/, 'the icon alone');
  const pair = readFileSync(join(REPO, 'desktop/ui/js/pair.js'), 'utf8');
  assert.doesNotMatch(pair, /phone-text|ICON_CHEVRON_RIGHT/, 'no text and no chevron come back with the status');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.phone-status\.connected \{ color: var\(--color-done\); background: color-mix\(in srgb, var\(--color-done\) 7%, transparent\);/);
  assert.match(css, /\.phone-status\[data-state="failed"\] \{ box-shadow: inset 0 0 0 1px var\(--color-lost\); \}/);
  assert.doesNotMatch(css, /\.footer-pair|\.phone-text/, 'the big card is gone');
  assert.match(css, /\.sidebar-footer \{ display: flex; align-items: center;/, 'one row, nothing wraps');

  const { renderPhoneStatus } = await import('../js/pair.js');
  const button = { dataset: {}, title: '', attributes: {}, setAttribute(name, value) { this.attributes[name] = value; } };
  globalThis.document = { getElementById: () => button };
  try {
    const iphone = { agent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) Version/18.1 Mobile/15E148 Safari/604.1' };
    const cases = [
      [{ enabled: false, phones: 0 }, 'off', 'Pair phone · Phone access is off'],
      [{ enabled: true, phase: 'connecting', phones: 0 }, 'pending', 'Pair phone · Starting…'],
      [{ enabled: true, phase: 'ready', phones: 0 }, 'ready', 'Pair phone · Waiting for a phone'],
      [{ enabled: true, phase: 'failed', phones: 0 }, 'failed', 'Pair phone · Pairing needs attention'],
      [{ enabled: true, phase: 'ready', phones: 1, devices: [iphone] }, 'connected', 'iPhone · Live · end-to-end encrypted'],
      [{ enabled: true, phase: 'ready', phones: 2, devices: [iphone, iphone] }, 'connected', '2 connected · Live · end-to-end encrypted'],
    ];
    for (const [pairing, status, words] of cases) {
      renderPhoneStatus(pairing);
      assert.equal(button.dataset.state, status);
      assert.equal(button.attributes['aria-label'], words);
      assert.equal(button.title, `${words} (ctrl P)`, 'the tooltip names the shortcut too');
    }
  } finally {
    delete globalThis.document;
  }
});

test('remote control is off until chosen and only for Claude', async () => {
  const { state, setRemote } = await import('../js/state.js');
  assert.equal(state.remote, false, 'off by default');
  setRemote(true);
  assert.equal(state.remote, true);
  setRemote(false);
  const source = readFileSync(join(REPO, 'desktop/ui/js/state.js'), 'utf8');
  assert.match(source, /remote: storage\.get\(REMOTE_KEY\) === 'on'/);
  assert.match(source, /storage\.set\(REMOTE_KEY, on \? 'on' : null\)/);
  const composer = readFileSync(join(REPO, 'desktop/ui/js/composer.js'), 'utf8');
  assert.match(composer, /remoteControl: state\.remote && kind === 'claude'/);
  const commands = readFileSync(join(REPO, 'desktop/ui/js/commands.js'), 'utf8');
  assert.match(commands, /type: 'race'[^}]*remoteControl: state\.remote/);
  assert.match(commands, /type: 'skill'[^}]*remoteControl: state\.remote/);
  const actions = rust('src/pwa/actions.rs');
  assert.match(actions, /let remote_control = remote_control && kind == AgentKind::Claude;/);
  assert.match(actions, /let remote_control = remote_control && draft\.kind == AgentKind::Claude;/);
  assert.match(rust('src/launch.rs'), /if record\.remote_control \{\n\s*args\.push\("--remote-control"\.to_string\(\)\);/);
});

test('every control, overlay and view moves with the same motion tokens, and reduced motion turns it all off', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  for (const name of ['--ease-out', '--ease-in', '--ease-spring', '--motion-fast', '--motion-base', '--motion-slow']) assert.match(css, new RegExp(`${name}: `), name);
  const animated = css.match(/^\.button, \.toggle, \.chip, [^{]+\{/m)[0];
  for (const control of ['.button', '.toggle', '.chip', '.filter', '.segmented button', '.tab', '.session-row', '.menu-item', '.palette-item', '.card', '.tile', '.icon-button', '.footer-icon', '.field']) {
    assert.ok(animated.includes(`${control},`) || animated.includes(`${control} {`), `${control} has transitions`);
  }
  assert.match(css, /\.button:active:not\(:disabled\)[^{]*\{ transform: scale\(0\.96\)/, 'buttons sink when pressed');
  for (const overlay of [/\.menu:not\(\[hidden\]\) \{ animation: pop-in/, /\.menu\.closing \{ animation: pop-out/, /\.palette:not\(\[hidden\]\) \{[^}]*animation: pop-in/, /\.dialog\[open\] \{ animation: dialog-in/, /\.dialog\[open\]::backdrop \{ animation: fade-in/, /\.toast \{ animation: toast-in/, /\.toast\.leaving \{ animation: toast-out/, /\.tip\.shown \{ opacity: 1;/]) {
    assert.match(css, overlay);
  }
  assert.match(css, /\.overview:not\(\[hidden\]\), \.help:not\(\[hidden\]\), \.settings-view:not\(\[hidden\]\) \{ animation: view-in/);
  assert.match(css, /\.agent-view:not\(\[hidden\]\) \{ animation: fade-in/, 'the terminal only fades, so xterm never measures a moving box');
  assert.match(css, /\.card-tools \{[^}]*opacity: 0; visibility: hidden;/, 'hover tools fade instead of popping');
  assert.match(css, /\.switch\[aria-checked="true"\] \.switch-knob \{ transform: translateX\(16px\); \}/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\) \{\n\s*\*, \*::before, \*::after \{ animation-duration: 1ms !important;[^}]*transition-duration: 1ms !important;/);
});

test('menus close with an animation, toasts slide out, and tooltips replace the native title', () => {
  const dialogs = readFileSync(join(REPO, 'desktop/ui/js/dialogs.js'), 'utf8');
  assert.match(dialogs, /export function menuShown\(\) \{\n\s*return !\$\('menu'\)\.hidden && menuClosing === null;/);
  assert.match(dialogs, /menu\.classList\.add\('closing'\);\n\s*menuClosing = setTimeout/);
  assert.match(dialogs, /clearTimeout\(menuClosing\);\n\s*menuClosing = null;\n\s*menu\.classList\.remove\('closing'\);/, 'a menu opened while closing comes back');
  for (const file of ['composer.js', 'main.js']) {
    const source = readFileSync(join(REPO, 'desktop/ui/js', file), 'utf8');
    assert.doesNotMatch(source, /\$\('menu'\)\.hidden/, `${file} asks menuShown, so a click during the close reopens`);
  }
  const toast = readFileSync(join(REPO, 'desktop/ui/js/toast.js'), 'utf8');
  assert.match(toast, /node\.classList\.add\('leaving'\);\n\s*setTimeout\(\(\) => node\.remove\(\), motionMs\('--motion-base'\)\);/);
  const motion = readFileSync(join(REPO, 'desktop/ui/js/motion.js'), 'utf8');
  assert.match(motion, /node\.dataset\.tip = title;\n\s*node\.removeAttribute\('title'\);/, 'the native tooltip never shows on top');
  assert.match(motion, /target\.closest\('dialog\[open\]'\) \|\| document\.body/, 'tooltips inside a dialog stay above it');
  assert.match(motion, /return reducedMotion\(\) \? 0 :/);
  assert.doesNotMatch(motion.split('\nexport function reducedMotion')[0], /window\.|document\./, 'importing motion.js touches nothing until used');
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /initTooltips\(\);/);
  assert.match(readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8'), /<div id="tip" class="tip" role="tooltip"><\/div>/);
});

test('board cards glide only when the columns change, and settings controls finish moving before the page redraws', () => {
  const board = readFileSync(join(REPO, 'desktop/ui/js/board.js'), 'utf8');
  assert.match(board, /const moved = shownLayout !== '' && layout !== shownLayout && shownSession === state\.session && !reducedMotion\(\);/, 'no measuring on every tick, on the first paint or on a session switch');
  assert.match(board, /if \(!was \|\| was\.column !== node\.closest\('\.column'\)\.dataset\.column\) rise\(node, options\);\n\s*else glide\(node, was\.box, options\);/);
  assert.match(board, /slideThumb\(\$\('views'\)\);\n\s*slideThumb\(\$\('grid-size'\)\);/);
  const settings = readFileSync(join(REPO, 'desktop/ui/js/settings.js'), 'utf8');
  assert.match(settings, /choose\(event\.currentTarget, \(\) => onPick\(option\)\)/);
  assert.match(settings, /event\.currentTarget\.setAttribute\('aria-checked', String\(!on\)\);\n\s*settle\(\(\) => onFlip\(!on\)\);/);
  assert.match(settings, /for \(const group of host\.querySelectorAll\('\.segmented'\)\) slideThumb\(group\);/);
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.match(css, /\.segmented\.sliding::before \{[^}]*transform: translateX\(var\(--thumb-x\)\);[^}]*\}/);
});

test('a right click on a session in the sidebar offers to rename or delete that session', () => {
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.match(sidebar, /oncontextmenu: \(event\) => sessionMenu\(event, session\.name\)/);
  assert.match(sidebar, /openMenu\(event\.currentTarget, \[item\(t\('Rename'\), renameSession\), item\(t\('Delete'\), deleteSession\)\], false\);/);
  assert.match(sidebar, /closeMenu\(\);\n\s*act\(name\);/, 'the menu closes before the question opens');
  const commands = readFileSync(join(REPO, 'desktop/ui/js/commands.js'), 'utf8');
  assert.match(commands, /export async function renameSession\(name, arg\) \{[\s\S]*?value: name,[\s\S]*?if \(name === state\.session\) state\.pendingSession = to\.trim\(\);\n\s*send\(\{ type: 'renameSession', name, to \}\);/, 'renaming another session keeps the one on screen');
  assert.match(commands, /export async function deleteSession\(name\) \{[\s\S]*?send\(\{ type: 'deleteSession', name \}\)/);
  assert.match(commands, /case 'rename-session':\n\s*renameSession\(state\.session, arg\)/, 'the slash command still renames the session on screen');
  assert.match(commands, /case 'delete-session':\n\s*deleteSession\(arg \|\| state\.session\);/);
});

test('the sidebar is as wide as the user left it, within limits, on every screen', async () => {
  const { SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN, sidebarWidth } = await import('../js/format.js');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  assert.equal(SIDEBAR_DEFAULT, Number(css.match(/--sidebar: (\d+)px;/)[1]), 'the first paint has the same width');
  assert.equal(sidebarWidth(null), SIDEBAR_DEFAULT, 'nothing saved yet');
  assert.equal(sidebarWidth('not a number'), SIDEBAR_DEFAULT);
  assert.equal(sidebarWidth(''), SIDEBAR_DEFAULT);
  assert.equal(sidebarWidth('320'), 320);
  assert.equal(sidebarWidth(320.6), 321);
  assert.equal(sidebarWidth(0), SIDEBAR_MIN);
  assert.equal(sidebarWidth(9999), SIDEBAR_MAX);
  const windows = JSON.parse(rust('desktop/tauri.conf.json')).app.windows;
  assert.ok(windows.every((window) => window.minWidth - SIDEBAR_MAX >= 600), 'the widest sidebar still leaves room for the screen beside it');
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  assert.match(html, /<\/aside>\s*<div id="sidebar-resize"[^>]*role="separator"[^>]*tabindex="0"[^>]*><\/div>\s*<main class="main">/, 'beside the sidebar, outside of any one view');
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.match(sidebar, /handle\.setPointerCapture\(event\.pointerId\)/, 'a drag over a terminal keeps resizing');
  assert.match(sidebar, /storage\.set\(key, width === fallback \? null : String\(width\)\)/);
  assert.match(sidebar, /handle\.addEventListener\('dblclick', \(\) => \{\n\s*apply\(fallback\);/);
  assert.match(sidebar, /event\.key !== 'ArrowLeft' && event\.key !== 'ArrowRight'/, 'it also works from the keyboard');
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /initSidebarWidth\(\);/);
});

test('the grid keeps its tiles in place until an agent asks for attention or finishes', async () => {
  const { arrangeGrid } = await import('../js/format.js');
  const working = (id) => agent({ id, state: 'working' });
  const first = arrangeGrid([], new Map(), [working(1), working(2), working(3)]);
  assert.deepEqual(first.order, [1, 2, 3], 'the first time takes the order it gets');
  const clicked = arrangeGrid(first.order, first.columns, [working(2), working(1), working(3)]);
  assert.deepEqual(clicked.order, [1, 2, 3], 'the hub moving the agent someone clicked or typed into moves nothing');
  const needs = arrangeGrid(clicked.order, clicked.columns, [working(1), working(2), agent({ id: 3, state: 'paused' })]);
  assert.deepEqual(needs.order, [3, 1, 2], 'an agent that starts waiting comes first');
  const done = arrangeGrid(needs.order, needs.columns, [working(1), agent({ id: 2, state: 'done' }), agent({ id: 3, state: 'paused' })]);
  assert.deepEqual(done.order, [2, 3, 1], 'so does one that finishes');
  const still = arrangeGrid(done.order, done.columns, [working(1), agent({ id: 2, state: 'done' }), agent({ id: 3, state: 'paused' })]);
  assert.deepEqual(still.order, [2, 3, 1], 'it moves once, on the change');
  const seen = arrangeGrid(still.order, still.columns, [working(1), agent({ id: 2, state: 'idle' }), working(3)]);
  assert.deepEqual(seen.order, [2, 3, 1], 'going back to work or being seen moves nothing');
  const reopened = arrangeGrid([2, 3, 1], new Map(), [agent({ id: 1, state: 'done' }), working(2), working(3)]);
  assert.deepEqual(reopened.order, [2, 3, 1], 'reopening the app raises nothing');
  const fresh = arrangeGrid(seen.order, seen.columns, [working(1), working(2), working(4)]);
  assert.deepEqual(fresh.order, [2, 1, 4], 'a removed agent closes its gap and a new one takes the free place at the end');
  const grid = readFileSync(join(REPO, 'desktop/ui/js/grid.js'), 'utf8');
  assert.match(grid, /const list = gridAgents\(\);/);
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /peekRequest\(gridAgents\(\), rowsOnScreen\(\)\)/, 'previews follow the tiles on screen');
});

test('a double click on the title of a session renames it from the board, the list and the grid', async () => {
  const { PT } = await import('../js/i18n.js');
  const main = readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8');
  assert.match(main, /\$\('session-title'\)\.addEventListener\('dblclick', \(\) => state\.session && renameSession\(state\.session\)\);/, 'the same rename as the sidebar');
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  assert.match(html, /<header class="overview-head">[\s\S]*?<h1 id="session-title"[^>]*title="Double click to rename"[^>]*>[\s\S]*?<div id="board"/, 'one title above all three views');
  assert.equal(PT['Double click to rename'], 'Clique duplo para renomear');
});

test('the panel of an agent is as wide as the user left it and never scrolls sideways', async () => {
  const { DETAILS_DEFAULT, DETAILS_MAX, DETAILS_MIN, detailsWidth } = await import('../js/format.js');
  const { PT } = await import('../js/i18n.js');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const rule = (selector) => css.split(`\n${selector} {`)[1].split('}')[0];
  assert.equal(DETAILS_DEFAULT, Number(css.match(/--details: (\d+)px;/)[1]), 'the first paint has the same width');
  assert.equal(detailsWidth(null), DETAILS_DEFAULT);
  assert.equal(detailsWidth('not a number'), DETAILS_DEFAULT);
  assert.equal(detailsWidth('320'), 320);
  assert.equal(detailsWidth(0), DETAILS_MIN);
  assert.equal(detailsWidth(9999), DETAILS_MAX);
  assert.match(rule('.details'), /width: var\(--details\)/);
  assert.match(rule('.details'), /max-width: 55%/, 'the terminal keeps its room on a small window');
  assert.match(rule('.details'), /overflow-x: hidden/);
  assert.match(rule('.subagent-meta'), /min-width: 0/, 'a long activity shrinks instead of pushing the panel');
  assert.match(rule('.subagent-meta'), /text-overflow: ellipsis/);
  assert.match(rule('.subagent-text'), /overflow-wrap: anywhere/, 'a long path wraps');
  const html = readFileSync(join(REPO, 'desktop/ui/index.html'), 'utf8');
  assert.match(html, /<div id="terminal"[^>]*><\/div>\s*<div id="details-resize"[^>]*role="separator"[^>]*tabindex="0"[^>]*><\/div>\s*<aside id="details"/, 'between the terminal and the panel');
  const sidebar = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.equal(sidebar.match(/setPointerCapture/g).length, 1, 'both panels resize through the same code');
  assert.match(sidebar, /growKey: 'ArrowLeft'/, 'this panel grows to the left');
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /initDetailsWidth\(\);/);
  assert.equal(PT['Resize the agent panel'], 'Redimensionar o painel do agente');
});

test('an agent moves by dragging it onto a session of the sidebar, and a drag never opens it', async () => {
  const { canMove } = await import('../js/format.js');
  assert.equal(canMove(agent({ session: 'default' }), 'work'), true);
  assert.equal(canMove(agent({ session: 'work' }), 'work'), false, 'it is already there');
  assert.equal(canMove(agent({ session: 'default', starting: true }), 'work'), false, 'the launcher still holds its own copy of the record');
  assert.equal(canMove(agent({ session: 'default', state: 'exited' }), 'work'), true, 'a stopped agent moves too');
  assert.equal(canMove(null, 'work'), false);
  assert.equal(canMove(agent({}), null), false);
  const drag = readFileSync(join(REPO, 'desktop/ui/js/drag.js'), 'utf8');
  assert.match(drag, /send\(\{ type: 'moveAgent', agent: moving\.agent\.id, session \}\)/);
  assert.match(drag, /#board \.card, #list-body \.row, #grid \.tile-head, #sessions \.summary/, 'a tile drags by its head: its screen is a terminal to select from');
  assert.match(drag, /closest\('\.session'\)\?\.querySelector\('\.session-row\[data-session\]'\)/, 'a drop counts anywhere on an expanded session, not only on its row');
  assert.doesNotMatch(drag, /addEventListener\('blur'/, 'the capture of the pointer says when a drag is lost');
  const sidebarCode = readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8');
  assert.match(sidebarCode, /class: `summary\$\{entry\.waiting \? ' waiting' : ''\}`, dataset: \{ agent: String\(list\[0\]\.id\) \}/, 'an agent listed in the sidebar drags from there too');
  assert.match(sidebarCode, /\[agent\.id, agent\.name, agent\.state, agent\.starting, agent\.kind, agent\.model\]/, 'a line never keeps the id of another agent with the same name, nor an old provider or model');
  assert.match(drag, /\.xterm/, 'nothing drags out of a terminal');
  assert.match(drag, /addEventListener\('click', swallow, \{ capture: true, once: true \}\)/);
  assert.match(drag, /event\.key === 'Escape'/, 'a drag can be called off');
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/sidebar.js'), 'utf8'), /class: 'session-row', type: 'button', dataset: \{ session: session\.name \}/);
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/main.js'), 'utf8'), /initAgentDrag\(\);/);
  assert.match(rust('src/pwa/actions.rs'), /PhoneMsg::MoveAgent \{ agent, session \} => move_agent\(hub, agent, &session, reply\)/);
});

test('settings show one section at a time, so every entry of the nav always does something', () => {
  const settings = readFileSync(join(REPO, 'desktop/ui/js/settings.js'), 'utf8');
  assert.match(settings, /const SECTIONS = \{ Appearance: appearanceSection, Agents: agentsSection, Alerts: alertsSection, Phone: phoneSection, Updates: updatesSection \};/);
  assert.match(settings, /h\('div', \{ class: 'settings-content' \}, SECTIONS\[shownSection\]\(\)\)/, 'only the chosen section is on the page');
  assert.match(settings, /'aria-current': String\(name === shownSection\), onclick: \(\) => showSection\(name\)/);
  assert.doesNotMatch(settings, /scrollIntoView|addEventListener\('scroll'/, 'the nav depends on nothing that a short page cannot do');
  const save = settings.match(/function render\(\) \{[\s\S]*?\n\}/)[0];
  assert.match(save, /host\.scrollTop = scroll;/, 'saving a setting keeps the place on the page');
});

test('the folder picker ranks matches like src/folders.rs and keeps the matched letters', async () => {
  const { fuzzy } = await import('../js/folders.js');
  assert.equal(fuzzy('octo', 'octo').score, 1000, 'an exact name wins');
  assert.ok(fuzzy('oc', 'octo').score > fuzzy('oc', 'my-octo').score, 'a prefix beats a word inside the name');
  assert.ok(fuzzy('oc', 'my-octo').score > fuzzy('oc', 'doctor').score, 'a word start beats the middle of a word');
  assert.deepEqual(fuzzy('mo', 'my-octo').marks, [0, 3], 'letters in order mark where they matched');
  assert.equal(fuzzy('AP', 'api').whole, true, 'case is ignored');
  assert.equal(fuzzy('zz', 'octo'), null);
  assert.equal(fuzzy('ox', 'o-aaaaaaaaaaaaaaaaaax'), null, 'letters spread too far apart are not a match');
});

test('the folder picker splits a path into clickable crumbs and completes typed paths', async () => {
  const { folderCrumbs, splitTyped } = await import('../js/folders.js');
  assert.deepEqual(folderCrumbs('/Users/ana/dev', '/Users/ana'), [{ name: '~', path: '/Users/ana' }, { name: 'dev', path: '/Users/ana/dev' }]);
  assert.deepEqual(folderCrumbs('/opt/x', '/Users/ana').map((crumb) => crumb.path), ['/', '/opt', '/opt/x']);
  assert.deepEqual(folderCrumbs('C:\\Users\\ana', null).map((crumb) => crumb.name), ['C:', 'Users', 'ana'], 'a Windows drive is the first crumb');
  assert.deepEqual(splitTyped('~/Doc'), ['~/', 'Doc']);
  assert.deepEqual(splitTyped('~'), ['~', '']);
  assert.deepEqual(splitTyped('C:\\Users\\a'), ['C:\\Users\\', 'a']);
});

test('a typed path to a folder without subfolders says it is empty instead of quoting an empty name', async () => {
  const { emptyTypedNote } = await import('../js/folders.js');
  assert.deepEqual(emptyTypedNote('', 'C:/work/'), ['No folders inside', 'You can still use this folder.']);
  assert.deepEqual(emptyTypedNote('Doc', '~/Doc'), ['No folder starts with “Doc” here', 'Enter goes to ~/Doc']);
});

test('the folder picker warns that AI agents cannot start outside a repository while worktree is on', async () => {
  const { folderNote } = await import('../js/folders.js');
  const blocked = folderNote(null, false, true);
  assert.equal(blocked.tone, 'blocked');
  assert.equal(blocked.text, 'Not a git repository · turn worktree off to start AI agents here');
  assert.equal(folderNote(null, false, false).tone, 'plain', 'without worktree agents run right in the folder');
  assert.equal(folderNote('/w/octo', false, true).text, 'octo is a git repository · each agent gets its own worktree');
  assert.equal(folderNote('/w/octo', false, false).tone, 'git');
  assert.equal(folderNote(null, true, true).tone, 'missing', 'a folder that does not exist says so first');
  assert.match(readFileSync(new URL('../styles.css', import.meta.url), 'utf8'), /\.folder-status\[data-tone="blocked"\] \{ color: var\(--color-waiting\); \}/);
});

test('the folder picker numbers every listing it asks for and drops a reply to an older one', () => {
  const picker = readFileSync(new URL('../js/folders.js', import.meta.url), 'utf8');
  const asked = [...picker.matchAll(/send\(\{ type: '(folders|folderSearch)'[^}]*\}\)/g)].map(([call]) => call);
  assert.equal(asked.length, 2, 'one place asks for listings and one for searches');
  for (const call of asked) assert.match(call, /seq: pick\.(listed|search\.seq)/);
  assert.match(picker, /pick\.listed = \+\+requests;/, 'listings and searches share one counter, so a reopened picker never takes an old reply');
  assert.match(picker, /seq: \+\+requests \}/);
  assert.match(picker, /pick\.onListing = \(message\) => \{\n\s+if \(message\.seq !== pick\.listed\) return;/);
  assert.match(rust('src/pwa/protocol.rs'), /Folders \{\n\s+path: String,[\s\S]*?#\[serde\(default\)\]\n\s+seq: u64,\n\s+\},/, 'the phone keeps sending listings without a number');
});

test('every kind of place the core sends has its icon in the folder picker', () => {
  const variants = rust('src/folders.rs').match(/pub enum PlaceKind \{([^}]+)\}/)[1].match(/\w+/g).map((name) => name.toLowerCase());
  const picker = readFileSync(new URL('../js/folders.js', import.meta.url), 'utf8');
  const icons = [...picker.match(/const PLACE_ICONS = \{([^}]+)\}/)[1].matchAll(/(\w+):/g)].map(([, kind]) => kind);
  assert.deepEqual(icons.sort(), variants.sort());
});
