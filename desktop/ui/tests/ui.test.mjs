import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { COLUMNS, USAGE_ALERT, USAGE_WARN, column, duration, elapsed, glyph, kindColor, label, usageColor } from '../js/format.js';
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
    assert.equal(kindColor(kind), hex, kind);
  }
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

test('every mood of Kai has its animation in the stylesheet', () => {
  const css = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
  for (const mood of MOODS) assert.ok(css.includes(`.kai[data-mood="${mood}"]`), mood);
  assert.ok(css.includes('prefers-reduced-motion'), 'motion can be turned off');
});
