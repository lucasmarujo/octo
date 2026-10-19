import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import {
  CYCLE, INTRO, SPINNER, USAGE_BAR_WIDTH, USAGE_REFRESH_MS, apply, createOverview, durationShort, fnv1a64, glyph, lines,
  oldestWaiting, projectColorIndex, refreshUsage, slugify, stats, usageCells, usageLines, usageTone, windowTitle,
} from '../assets/tui.js';

const REPO = fileURLToPath(new URL('../../', import.meta.url));
const rust = (file) => readFileSync(join(REPO, file), 'utf8');
const NOW = 1_760_000_000_000;

const snapshot = (overview) => overview.agents.map((a) => `${a.id}:${a.state}:${a.subagents.length}`).join(' ');

function runIntro() {
  const overview = createOverview(NOW);
  apply(overview, INTRO, NOW);
  return overview;
}

test('durationShort reproduz os casos de teste do duration_short em Rust', () => {
  const cases = [...rust('src/tui/format.rs').matchAll(/assert_eq!\(duration_short\(([\d_ *]+)\), "([^"]+)"\)/g)];
  assert.ok(cases.length >= 5, 'casos do Rust não encontrados');
  cases.forEach(([, ms, expected]) => assert.equal(durationShort(new Function(`return ${ms.replaceAll('_', '')}`)()), expected, ms));
});

test('slugify reproduz os casos de teste do slugify em Rust', () => {
  const cases = [...rust('src/tui/format.rs').matchAll(/assert_eq!\(slugify\("([^"]*)", (\d+), (\d+)\), "([^"]*)"\)/g)];
  assert.ok(cases.length >= 4, 'casos do Rust não encontrados');
  cases.forEach(([, text, words, len, expected]) => assert.equal(slugify(text, Number(words), Number(len)), expected, text));
  assert.equal(slugify(INTRO.text), 'add-rate-limiting');
});

test('fnv1a64 segue os vetores oficiais e escolhe a cor do projeto como o theme.rs', () => {
  assert.equal(fnv1a64(''), 0xcbf29ce484222325n);
  assert.equal(fnv1a64('a'), 0xaf63dc4c8601ec8cn);
  assert.equal(fnv1a64('foobar'), 0x85944171f73967e8n);
  const size = rust('src/tui/theme.rs').match(/PROJECT_PALETTE: \[Color; (\d+)\]/)[1];
  assert.equal(projectColorIndex('acme/web'), Number(fnv1a64('acme/web') % BigInt(size)));
});

test('spinner, limites de consumo e barra iguais aos do Rust', () => {
  const theme = rust('src/tui/theme.rs');
  const spinner = theme.match(/SPINNER: \[&str; 10\] = \[([^\]]+)\]/)[1].match(/"([^"]+)"/g).map((s) => s.slice(1, -1));
  assert.deepEqual(SPINNER, spinner);
  assert.equal(usageTone(69.9), 'green');
  assert.equal(usageTone(Number(theme.match(/USAGE_WARN_PERCENT: f64 = ([\d.]+)/)[1])), 'yellow');
  assert.equal(usageTone(Number(theme.match(/USAGE_ALERT_PERCENT: f64 = ([\d.]+)/)[1])), 'red');
  assert.equal(USAGE_BAR_WIDTH, Number(rust('src/tui/render.rs').match(/USAGE_BAR_WIDTH: usize = (\d+)/)[1]));
  assert.equal(usageCells(62, 12), 7);
  assert.equal(usageCells(130, 12), 12);
  assert.equal(usageCells(-5, 12), 0);
});

test('glifos de estado seguem o theme::glyph', () => {
  assert.deepEqual(glyph('claude', 'done', 0), ['✔', 'green']);
  assert.deepEqual(glyph('claude', 'paused', 0), ['‖', 'yellow']);
  assert.deepEqual(glyph('terminal', 'working', 3), ['$', 'text']);
  assert.deepEqual(glyph('codex', 'idle', 0), ['●', 'dim']);
  assert.notEqual(glyph('claude', 'working', 0)[0], glyph('claude', 'working', 1)[0]);
});

test('contadores ignoram terminais e o título mostra esperando e terminados', () => {
  const overview = createOverview(NOW);
  assert.deepEqual(stats(overview), { agents: 7, working: 3, paused: 1, done: 2 });
  assert.equal(windowTitle(overview), '‖1 ✔2 · Orkai');
  overview.agents.forEach((a) => { a.state = 'working'; });
  assert.equal(windowTitle(overview), 'Orkai');
});

test('a tarefa digitada cria um agente que nasce iniciando e fica selecionado', () => {
  const overview = createOverview(NOW);
  const outcome = apply(overview, INTRO, NOW);
  const agent = overview.agents.at(-1);
  assert.equal(agent.id, 9);
  assert.equal(agent.name, 'add-rate-limiting');
  assert.equal(overview.selected, 9);
  assert.equal(outcome.key, 'enter');
  const row = (at) => lines(overview, at, 0).find((l) => l.id === 9);
  assert.equal(row(NOW + 100).activity, 'starting…');
  assert.equal(row(NOW + 100).mark, '◌');
  assert.equal(row(NOW + 5_000).activity, INTRO.activity);
  const header = lines(overview, NOW, 0).find((l) => l.type === 'header' && l.label === 'acme/web');
  assert.equal(header.count, 3);
});

test('o ciclo é coerente em várias voltas e volta ao estado inicial', () => {
  const overview = runIntro();
  const start = snapshot(overview);
  let now = NOW;
  for (let lap = 0; lap < 3; lap += 1) {
    for (const event of CYCLE) {
      now += 3_600;
      assert.doesNotThrow(() => apply(overview, event, now), JSON.stringify(event));
    }
    assert.equal(snapshot(overview), start, `volta ${lap + 1}`);
  }
});

test('alt+n abre o agente que espera há mais tempo e limpa o aviso', () => {
  const overview = runIntro();
  apply(overview, CYCLE[0], NOW + 1_000);
  apply(overview, CYCLE[1], NOW + 2_000);
  const waiting = apply(overview, CYCLE[2], NOW + 3_000);
  assert.equal(waiting.bell, true);
  assert.equal(overview.notice.text, 'payment-tests is waiting for you · alt+n opens it');
  assert.equal(oldestWaiting(overview).id, 4);
  const outcome = apply(overview, CYCLE[3], NOW + 4_000);
  assert.equal(outcome.key, 'alt+n');
  assert.equal(overview.selected, 4);
  assert.equal(overview.notice, null);
  assert.equal(oldestWaiting(overview).id, 2);
});

test('transições inválidas são recusadas', () => {
  const overview = createOverview(NOW);
  assert.throws(() => apply(overview, { type: 'state', id: 1, from: 'working', to: 'done' }, NOW), /expected working/);
  assert.throws(() => apply(overview, { type: 'altn', id: 2 }, NOW), /expected 2/);
  assert.throws(() => apply(overview, { type: 'nope' }, NOW), /unknown event/);
  assert.throws(() => apply(overview, { type: 'subagent', id: 99 }, NOW), /not found/);
});

test('linhas do overview: agrupamento, cores de tempo e subagent', () => {
  const overview = runIntro();
  apply(overview, CYCLE[0], NOW);
  apply(overview, CYCLE[1], NOW);
  const rows = lines(overview, NOW + 12_000, 2);
  assert.deepEqual(rows.filter((l) => l.type === 'header').map((l) => l.label), ['acme/web', 'acme/mobile', 'studio/game', 'lab/infra', 'orkai/cli']);
  const byId = (id) => rows.find((l) => l.id === id);
  assert.equal(byId(4).activity, 'waiting for you · Resolve the conflict in solver.rs?');
  assert.equal(byId(4).timeTone, 'yellow');
  assert.equal(byId(3).timeTone, 'dim');
  assert.equal(byId(3).nameTone, 'green');
  assert.equal(byId(5).time, '');
  assert.equal(byId(5).mark, '$');
  const sub = rows[rows.findIndex((l) => l.id === 8) + 1];
  assert.equal(sub.type, 'subagent');
  assert.equal(sub.time, '12s');
  assert.equal(sub.activity, 'Find the docker install docs · WebSearch');
});

test('consumo: barras, tons e atualização a cada 2 minutos', () => {
  const overview = createOverview(NOW);
  const usage = usageLines(overview, NOW);
  assert.equal(usage.length, 4);
  assert.deepEqual(usage.map((u) => u.kind), ['claude', '', 'codex', '']);
  assert.deepEqual(usage.map((u) => u.tone), ['green', 'green', 'green', 'yellow']);
  assert.deepEqual(usage.map((u) => u.resets), ['resets 2h14m', 'resets 4d05h', 'resets 3h41m', 'resets 58h00m']);
  usage.forEach((u) => assert.equal(u.filled + u.empty, USAGE_BAR_WIDTH));
  assert.equal(refreshUsage(overview, NOW), false);
  assert.equal(refreshUsage(overview, overview.usageUpdatedAt + USAGE_REFRESH_MS), true);
  assert.equal(overview.usage[0].windows[0].percent, 63);
  assert.equal(usageLines(overview, NOW + 200 * 3_600_000)[0].resets, 'resets now');
});
