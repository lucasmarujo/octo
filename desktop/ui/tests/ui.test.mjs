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
  const names = { claude: 'CLAUDE', codex: 'CODEX', kilo: 'KILO', opencode: 'OPENCODE', antigravity: 'ANTIGRAVITY', grok: 'GROK', qwen: 'QWEN', kimi: 'KIMI', terminal: 'TERMINAL' };
  for (const [kind, constant] of Object.entries(names)) {
    const [, r, g, b] = theme.match(new RegExp(`pub const ${constant}: Color = Color::Rgb\\((\\d+), (\\d+), (\\d+)\\)`));
    const hex = `#${[r, g, b].map((part) => Number(part).toString(16).padStart(2, '0')).join('')}`;
    assert.equal(kindHex(kind), hex, kind);
    assert.equal(kindColor(kind), `var(--kind-${kind})`, kind);
  }
});

test('the dark theme keeps the colors of the TUI and the light theme redefines every one', async () => {
  const { BASE16 } = await import('../js/palette.js');
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  const [dark, light] = [css.split(':root {')[1].split('}')[0], css.split(':root[data-theme="light"] {')[1].split('}')[0]];
  const kinds = ['claude', 'codex', 'kilo', 'opencode', 'antigravity', 'grok', 'qwen', 'kimi', 'terminal'];
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
  const translated = { '!command': '!comando', 'ctrl click': 'ctrl clique' };
  en.groups.forEach(([, rows], index) => {
    const keys = pt.groups[index][1].map(([key]) => key);
    assert.deepEqual(keys, rows.map(([key]) => translated[key] || key));
  });
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

test('the pairing badge and the waiting line follow the bridge like in the TUI', async () => {
  const { badge, waiting } = await import('../js/pair.js');
  assert.deepEqual(badge({ enabled: false }), { text: 'off', tone: 't-faint' });
  assert.equal(badge({ enabled: true, phase: 'ready', phones: 2 }).text, '● 2');
  assert.equal(badge({ enabled: true, phase: 'ready', phones: 0 }).text, 'on');
  assert.equal(badge({ enabled: true, phase: 'failed' }).tone, 't-red');
  assert.equal(badge({ enabled: true, phase: 'connecting' }).text, '…');
  assert.match(waiting({ phase: null }).text, /Starting/);
  assert.match(waiting({ phase: 'installing' }).text, /cloudflared/);
  assert.equal(waiting({ phase: 'connecting', tunnel: 'tailscale' }).text, 'Opening the tailscale tunnel…');
  assert.equal(waiting({ phase: 'failed', error: 'no network' }).text, 'no network · retrying');
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

test('tab completes a slash command the way the TUI does', async () => {
  const { completeCommand } = await import('../js/commands.js');
  assert.equal(completeCommand('/whe', []), '/where ');
  assert.equal(completeCommand('/qu', []), '/quit');
  assert.equal(completeCommand('/wh', []), null);
  assert.equal(completeCommand('/ren', []), '/rename');
  assert.equal(completeCommand('/cro', []), '/cross-review ');
  assert.equal(completeCommand('/rev', ['/review']), '/review ');
  assert.equal(completeCommand('/mae', ['/maestri', '/maestri-chat', '/maestri-manager']), '/maestri');
  assert.equal(completeCommand('/maestri', ['/maestri', '/maestri-chat']), null);
  assert.equal(completeCommand('/s', ['/stripe:docs']), null);
  assert.equal(completeCommand('/zzz', []), null);
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
  assert.match(board, /def\.key === 'idle' \? h\('button'/);
  assert.match(board, /const sure = await confirm\(\{[\s\S]*?danger: true/, 'it asks before stopping');
  assert.match(board, /for \(const agent of targets\) send\(\{ type: 'stop', agent: agent\.id \}\)/);
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
