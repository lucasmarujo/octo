import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';
import { SCENE_FRAMES, SCENE_IDLE_FRAMES, SCENE_INKED, SCENE_STARTLE, SCENE_STEPS, kaiCorner, pinned, sceneFrame } from '../assets/footer.js';
import { PROVIDER_ICONS, agentIcon } from '../assets/icons.js';
import { MODES, MOODS, ORCH_EDGES, ORCH_NODES, ORCH_STATE_TEXT, ORCH_STEPS, OS_LABELS, detectOs, formatStars, progressFor, stepAt } from '../assets/session.js';
import { AGENT_STATES, HOLD_MS, HOST, INTRO, KINDS, MAIN_SESSION, PREFERENCES, SCRIPT, SIDE_SESSION, VIEWS, agentName, applyStep, createHub, finalState, initialState, orchestratorMessage, projectColor, projectLabel, repliesTo, stateMessage, usageMessage } from '../app/demo.js';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const REPO = join(ROOT, '..');
const read = (file) => readFileSync(join(ROOT, file), 'utf8');
const html = read('index.html');
const css = read('assets/site.css');
const app = read('assets/app.js');
const footerSource = read('assets/footer.js');
const wrapper = read('app/index.html');
const bridge = read('app/bridge.js');
const demo = read('app/demo.js');
const readme = readFileSync(join(REPO, 'README.md'), 'utf8');
const desktopCss = readFileSync(join(REPO, 'desktop/ui/styles.css'), 'utf8');
const help = readFileSync(join(REPO, 'desktop/ui/js/help.js'), 'utf8');
const RELEASE = 'https://github.com/lucasmarujo/octo/releases/latest/download/';
const NOW = 1_800_000_000_000;

function rootToken(source, name) {
  const root = source.slice(source.indexOf(':root {'), source.indexOf('}', source.indexOf(':root {')));
  const match = root.match(new RegExp(`--${name}: (#[0-9a-fA-F]{6});`));
  return match && match[1].toLowerCase();
}

test('the page loads nothing from third parties', () => {
  const sources = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
  const loaded = sources.filter((url) => /\.(css|js|woff2|svg|png|webp|mp4)(\?|$)/.test(url));
  assert.ok(loaded.length > 0);
  for (const url of loaded) assert.ok(url.startsWith('/') && !url.startsWith('//'), `${url} is not local`);
  assert.doesNotMatch(css, /url\("?https?:/);
  assert.doesNotMatch(footerSource, /https?:\/\//);
  assert.deepEqual([...app.matchAll(/https?:\/\/[^'"`\s]+/g)].map((match) => match[0]), ['https://api.github.com/repos/lucasmarujo/octo']);
  assert.deepEqual([...html.matchAll(/<iframe[^>]* src="([^"]+)"/g)].map((match) => match[1]), ['/app/']);
  for (const source of [wrapper, bridge, read('app/ui/index.html')]) assert.doesNotMatch(source, /https?:\/\//);
  assert.deepEqual([...demo.matchAll(/https?:\/\/[^'"`\s]+/g)].map((match) => match[0]), ['http://localhost:5173/']);
  assert.doesNotMatch(desktopCss, /url\("?https?:/);
  assert.doesNotMatch(bridge + demo, /\b(?:XMLHttpRequest|WebSocket|EventSource|sendBeacon)\b/);
  assert.deepEqual([...bridge.matchAll(/fetch\(([^)]*)\)/g)].map((match) => match[1]), ['file']);
});

test('every local file the page references exists', () => {
  const fromHtml = [...html.matchAll(/(?:src|href)="(\/assets\/[^"]+)"/g)].map((match) => match[1].slice(1));
  const fromCss = [...css.matchAll(/url\("([^"]+)"\)/g)].map((match) => join('assets', match[1]));
  for (const file of [...fromHtml, ...fromCss]) assert.ok(existsSync(join(ROOT, file)), `${file} is missing`);
  const fromWrapper = [...wrapper.matchAll(/(?:src|href)="\/(app\/[^"]+)"/g)].map((match) => match[1]);
  const fromApp = [...read('app/ui/index.html').matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => join('app/ui', match[1]));
  const fromAppCss = [...desktopCss.matchAll(/url\("([^"]+)"\)/g)].map((match) => join('app/ui', match[1]));
  assert.ok(fromWrapper.length === 2 && fromApp.length >= 7 && fromAppCss.length >= 10);
  for (const file of [...fromWrapper, ...fromApp, ...fromAppCss, 'app/index.html', 'app/demo.js']) assert.ok(existsSync(join(ROOT, file)), `${file} is missing`);
});

test('every in-page link points to a section that exists', () => {
  const ids = new Set([...html.matchAll(/ id="([^"]+)"/g)].map((match) => match[1]));
  const anchors = [...html.matchAll(/href="#([^"]+)"/g)].map((match) => match[1]);
  assert.ok(anchors.length > 5);
  for (const anchor of anchors) assert.ok(ids.has(anchor), `#${anchor} has no target`);
});

test('there is one h1 and every section is labelled by a heading that exists', () => {
  assert.equal([...html.matchAll(/<h1[ >]/g)].length, 1);
  for (const [, id] of html.matchAll(/aria-labelledby="([^"]+)"/g)) assert.match(html, new RegExp(`<h[12][^>]* id="${id}"`));
});

function themeToken(theme, name) {
  const start = desktopCss.indexOf(`:root[data-theme="${theme}"] {`);
  const block = desktopCss.slice(start, desktopCss.indexOf('}', start));
  const match = block.match(new RegExp(`--${name}: (#[0-9a-fA-F]{6});`));
  return match ? match[1].toLowerCase() : rootToken(desktopCss, name);
}

test('the page wears the black theme of the app, and Kai keeps his pink', () => {
  const pairs = { bg: 'color-bg', surface: 'color-surface', raised: 'color-surface-raised', border: 'color-border', 'border-strong': 'color-border-strong', text: 'color-text', muted: 'color-text-muted', faint: 'color-text-faint', well: 'term-bg', waiting: 'color-waiting', done: 'color-done', lost: 'color-lost' };
  for (const [site, desktop] of Object.entries(pairs)) assert.equal(rootToken(css, site), rootToken(desktopCss, desktop), `--${site}`);
  assert.equal(rootToken(css, 'kai'), themeToken('old', 'color-accent'));
  assert.equal(rootToken(css, 'kai-ink'), themeToken('old', 'color-accent-ink'));
  for (const kind of ['claude', 'codex', 'kilo', 'opencode', 'antigravity', 'grok', 'qwen', 'kimi', 'pi', 'cursor', 'terminal']) {
    assert.equal(rootToken(css, `kind-${kind}`), rootToken(desktopCss, `kind-${kind}`), `--kind-${kind}`);
  }
});

test('the downloads are the files of the README table', () => {
  const table = readme.slice(readme.indexOf('| System | Download |'), readme.indexOf('`Octo-setup.exe` installs Octo'));
  const files = [...table.matchAll(/\| `([^`]+)` \|/g)].map((match) => match[1]);
  assert.equal(files.length, 4);
  const linked = [...html.matchAll(new RegExp(`href="${RELEASE.replace(/[.]/g, '\\.')}([^"]+)"`, 'g'))].map((match) => match[1]);
  assert.deepEqual([...new Set(linked)].sort(), [...files].sort());
});

test('every agent the README supports is named on the page', () => {
  const faqAgents = html.match(/Which agents are supported\?[\s\S]*?<p>([^<]+)<\/p>/)[1];
  assert.doesNotMatch(html, /marquee/);
  for (const name of ['Claude Code', 'Codex', 'Cursor', 'OpenCode', 'Kilo', 'Antigravity', 'Grok Build', 'Qwen Code', 'Kimi Code', 'Pi']) {
    assert.ok(readme.includes(name.replace(' Build', '')), `${name} left the README`);
    assert.ok(faqAgents.includes(name), `${name} is missing from the FAQ`);
  }
});

test('the orchestrator modes say what the app says', () => {
  for (const [name, mode] of Object.entries(MODES)) {
    const title = name[0].toUpperCase() + name.slice(1);
    assert.ok(help.includes(`['${title}', '${mode.text}']`), `${title} differs from help.js`);
    assert.match(html, new RegExp(`data-mode="${name}"[^>]*>${title}<`));
  }
  assert.ok(html.includes(MODES.moderate.text));
  assert.equal(MODES.bypass.thread.some((entry) => entry.type === 'card'), false);
  assert.equal(MODES.observe.thread.some((entry) => entry.type === 'summary'), false);
});

function filesUnder(dir, base = dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? filesUnder(path, base) : [relative(base, path).replaceAll('\\', '/')];
  });
}

function rustFields(source, name) {
  const start = source.indexOf(`pub struct ${name} {`);
  assert.notEqual(start, -1, name);
  const body = source.slice(start, source.indexOf('\n}', start));
  return [...body.matchAll(/pub (\w+):/g)].map((match) => match[1].replace(/_(\w)/g, (whole, letter) => letter.toUpperCase())).sort();
}

function invokedCommands() {
  const names = new Set();
  for (const file of readdirSync(join(REPO, 'desktop/ui/js'))) {
    const source = readFileSync(join(REPO, 'desktop/ui/js', file), 'utf8');
    for (const match of source.matchAll(/\b(?:invoke|act)\(\s*'([a-z_]+)'/g)) names.add(match[1]);
  }
  return [...names].sort();
}

function columnOf(agent) {
  if (agent.starting || agent.state === 'working') return 'working';
  if (agent.state === 'paused') return 'needs';
  return agent.state === 'done' ? 'done' : 'idle';
}

function attachedHub() {
  const pushed = [];
  const hub = createHub({ now: () => NOW, later: (work) => work() });
  hub.commands.attach({ channel: { onmessage: (message) => pushed.push(message) } });
  return { hub, pushed };
}

test('the preview runs an exact copy of the desktop app', () => {
  const copied = filesUnder(join(ROOT, 'app/ui')).sort();
  const source = filesUnder(join(REPO, 'desktop/ui')).filter((file) => !file.startsWith('tests/') && file !== 'island.html').sort();
  assert.deepEqual(copied, source);
  assert.ok(copied.length > 60);
  for (const file of copied) {
    assert.ok(readFileSync(join(ROOT, 'app/ui', file)).equals(readFileSync(join(REPO, 'desktop/ui', file))), `${file} differs from desktop/ui`);
  }
});

test('the fake backend is injected around the app, never into it', () => {
  assert.match(wrapper, /<base href="\/app\/ui\/">\s*<script type="module" src="\/app\/bridge\.js"><\/script>/);
  assert.equal([...wrapper.matchAll(/<script/g)].length, 1);
  assert.doesNotMatch(wrapper, /<style|style="/);
  assert.match(bridge, /window\.__TAURI__ = \{/);
  for (const api of ['core:', 'event:', 'webview:', 'window:', 'invoke:', 'Channel', 'listen:', 'onDragDropEvent:', 'isDecorated:', 'setTitle:']) assert.ok(bridge.includes(api), api);
  const used = new Set();
  for (const file of readdirSync(join(REPO, 'desktop/ui/js'))) {
    for (const match of readFileSync(join(REPO, 'desktop/ui/js', file), 'utf8').matchAll(/__TAURI__\.(\w+)/g)) used.add(match[1]);
  }
  for (const api of used) assert.ok(bridge.includes(`${api}:`), `window.__TAURI__.${api} is missing`);
  assert.ok(bridge.indexOf('  installTauri(hub);') < bridge.indexOf('await mount()'));
  assert.doesNotMatch(bridge, /\b(?:alert|confirm|prompt)\(/);
  assert.doesNotMatch(demo, /\b(?:window|document|localStorage|navigator)\./);
});

test('the preview starts in the gray theme, in English, on the board of an expanded session', () => {
  assert.equal(PREFERENCES['orkai.theme'], 'gray');
  assert.ok(desktopCss.includes(':root[data-theme="gray"] {'));
  assert.match(readFileSync(join(REPO, 'desktop/ui/js/theme.js'), 'utf8'), /THEME_KEY = 'orkai\.theme'/);
  assert.equal(PREFERENCES['orkai.view'], 'board');
  assert.equal(PREFERENCES['orkai.session'], MAIN_SESSION);
  assert.deepEqual(JSON.parse(PREFERENCES['orkai.expanded']), [MAIN_SESSION]);
  assert.deepEqual(JSON.parse(PREFERENCES['orkai.tabs']), ['overview', 'octo']);
  assert.equal(PREFERENCES['octo.octoTab'], 'on');
  assert.equal(Object.hasOwn(PREFERENCES, 'orkai.help.language'), false);
  const state = readFileSync(join(REPO, 'desktop/ui/js/state.js'), 'utf8') + readFileSync(join(REPO, 'desktop/ui/js/octo.js'), 'utf8') + readFileSync(join(REPO, 'desktop/ui/js/theme.js'), 'utf8');
  for (const key of Object.keys(PREFERENCES)) assert.ok(state.includes(`'${key}'`), `${key} is not a key of the app`);
  assert.match(bridge, /memoryStorage\(PREFERENCES\)/);
  assert.ok(bridge.indexOf('  installStorage();') < bridge.indexOf('await mount()'));
});

test('the simulated session has agents of many kinds in every column, in two sessions', () => {
  const state = initialState(NOW);
  assert.ok(state.agents.length >= 8 && state.agents.length <= 10);
  assert.equal(new Set(state.agents.map((agent) => agent.id)).size, state.agents.length);
  assert.equal(new Set(state.agents.map((agent) => agent.name)).size, state.agents.length);
  for (const column of ['needs', 'working', 'done', 'idle']) assert.ok(state.agents.some((agent) => columnOf(agent) === column), column);
  assert.ok(new Set(state.agents.map((agent) => agent.kind)).size >= 6);
  assert.ok(state.agents.some((agent) => agent.kind === 'terminal'));
  assert.ok(state.agents.some((agent) => agent.kind === 'claude' && agent.subagents.length >= 2));
  assert.deepEqual(state.sessions, [MAIN_SESSION, SIDE_SESSION]);
  for (const agent of state.agents) {
    assert.ok(state.sessions.includes(agent.session), agent.name);
    assert.ok(KINDS.includes(agent.kind), agent.kind);
    assert.ok(AGENT_STATES.includes(agent.state), agent.state);
    assert.ok(agent.startedAt < NOW && agent.stateSince <= NOW, agent.name);
    assert.match(agent.color, /^#[0-9a-f]{6}$/);
    assert.equal(agent.project, projectLabel(agent.cwd));
  }
  assert.ok(new Set(state.agents.map((agent) => agent.project)).size >= 4);
});

test('the messages of the fake hub have the fields the real hub serializes', () => {
  const protocol = readFileSync(join(REPO, 'src/pwa/protocol.rs'), 'utf8');
  const orchestrator = readFileSync(join(REPO, 'src/orchestrator/mod.rs'), 'utf8');
  const keys = (value) => Object.keys(value).sort();
  const state = finalState(NOW);
  const { type, ...overview } = stateMessage(initialState(NOW), NOW);
  assert.equal(type, 'state');
  assert.deepEqual(keys(overview), rustFields(protocol, 'Overview'));
  assert.deepEqual(keys(overview.sessions[0]), rustFields(protocol, 'SessionView'));
  for (const agent of state.agents) assert.deepEqual(keys(agent), rustFields(protocol, 'AgentView'), agent.name);
  assert.deepEqual(keys(initialState(NOW).agents[1].subagents[0]), rustFields(protocol, 'SubagentView'));
  const usage = usageMessage(NOW);
  assert.deepEqual(keys(usage.entries[0]), rustFields(protocol, 'UsageEntry'));
  assert.deepEqual(keys(usage.entries[0].windows[0]), rustFields(protocol, 'UsageWindow'));
  assert.deepEqual(usage.entries.map((entry) => entry.kind), ['claude', 'codex']);
  const { type: kind, ...view } = orchestratorMessage(initialState(NOW));
  assert.equal(kind, 'orchestrator');
  assert.deepEqual(keys(view), rustFields(orchestrator, 'OrchestratorView'));
  assert.deepEqual(keys(view.decisions[0]), rustFields(orchestrator, 'Decision'));
  assert.deepEqual(keys(view.handled[0]), rustFields(orchestrator, 'ActivityEntry'));
  assert.deepEqual(keys(view.chat[0]), rustFields(orchestrator, 'ChatEntry'));
  assert.deepEqual(keys(state.orchestrator.handled[0]), rustFields(orchestrator, 'ActivityEntry'));
  const [details] = repliesTo(initialState(NOW), { type: 'details', agent: 2 });
  assert.deepEqual(keys(details).filter((key) => key !== 'type'), rustFields(protocol, 'Details'));
  assert.deepEqual(keys(details.changes[0]), rustFields(protocol, 'FileView'));
});

test('every step of the script changes the session and none mutates what it receives', () => {
  let state = initialState(NOW);
  const steps = SCRIPT.filter((beat) => beat.step);
  assert.ok(steps.length >= 6);
  for (const types of [['answer'], ['finish'], ['launch'], ['ask'], ['chat', 'reply'], ['approve']]) {
    for (const name of types) assert.ok(steps.some((beat) => beat.step.type === name), name);
  }
  for (const beat of steps) {
    const copy = structuredClone(state);
    const next = applyStep(state, beat.step, NOW + 1000);
    assert.deepEqual(state, copy, beat.step.type);
    assert.notDeepEqual(next, state, beat.step.type);
    assert.deepEqual(applyStep(state, beat.step, NOW + 1000), next, beat.step.type);
    state = next;
  }
  assert.equal(applyStep(state, { type: 'unknown' }, NOW), state);
});

test('the session ends with nobody waiting and one more agent, then loops to where it began', () => {
  const end = finalState(NOW);
  const begin = initialState(NOW);
  assert.equal(end.agents.length, begin.agents.length + 1);
  assert.equal(new Set(end.agents.map((agent) => agent.id)).size, end.agents.length);
  assert.equal(end.agents.filter((agent) => columnOf(agent) === 'needs').length, 0);
  assert.equal(end.orchestrator.decisions.length, 0);
  assert.equal(end.orchestrator.thinking, false);
  assert.equal(end.orchestrator.handledToday, begin.orchestrator.handledToday + 2);
  assert.equal(end.orchestrator.handled.length, end.orchestrator.handledToday);
  for (const agent of end.agents) assert.ok(AGENT_STATES.includes(agent.state) && !agent.starting, agent.name);
  const launched = end.agents.at(-1);
  assert.equal(launched.name, 'fix-flaky-checkout');
  assert.equal(launched.branch, 'octo/fix-flaky-checkout');
  assert.equal(launched.cwd, begin.agents[0].cwd);
  const { hub, pushed } = attachedHub();
  for (const beat of SCRIPT) if (beat.step) hub.play(beat.step);
  assert.deepEqual(hub.state(), end);
  hub.reset();
  assert.deepEqual(hub.state(), begin);
  assert.deepEqual(pushed.at(-2), stateMessage(begin, NOW));
  assert.deepEqual(pushed.at(-1), orchestratorMessage(begin));
});

test('the chat with Octo is a short conversation in Moderate with one decision waiting for you', () => {
  let state = initialState(NOW);
  assert.equal(state.orchestrator.enabled, true);
  assert.equal(state.orchestrator.mode, 'moderate');
  assert.deepEqual(state.orchestrator.chat.map((entry) => entry.role), ['you', 'octo']);
  assert.ok(state.orchestrator.chat[1].actions.length >= 2);
  const reply = SCRIPT.findIndex((beat) => beat.step && beat.step.type === 'reply');
  for (const beat of SCRIPT.slice(0, reply + 1)) if (beat.step) state = applyStep(state, beat.step, NOW);
  assert.deepEqual(state.orchestrator.chat.map((entry) => entry.role), ['you', 'octo', 'you', 'octo']);
  const waiting = state.orchestrator.decisions.filter((decision) => decision.status === 'needsYou');
  assert.equal(waiting.length, 1);
  assert.equal(waiting[0].name, 'api-rate-limit');
  assert.equal(state.agents.find((agent) => agent.id === waiting[0].agent).state, 'paused');
  const view = SCRIPT.slice(0, reply + 1).at(-1).view;
  assert.equal(view, 'chat');
});

test('Kai narrates every beat of the tour, which goes board, list, grid, chat and back to the board', () => {
  for (const beat of SCRIPT) {
    assert.ok(MOODS.includes(beat.mood), beat.mood);
    assert.ok(beat.say.length > 15 && beat.say.length < 90, beat.say);
    assert.ok(VIEWS.includes(beat.view), beat.view);
    assert.ok(beat.wait >= 2000 && beat.wait <= 6000, beat.say);
  }
  assert.deepEqual(SCRIPT[0].say, INTRO.say);
  const stops = SCRIPT.map((beat) => beat.view).filter((view, index, views) => view !== views[index - 1]);
  assert.deepEqual(stops, ['board', 'list', 'grid', 'chat', 'board']);
  assert.equal(HOLD_MS, 20000);
  assert.match(html, /data-perch-kai/);
  assert.match(html, /data-perch-say/);
  assert.match(bridge, /postMessage\(\{ source: DEMO, say: line\.say, mood: line\.mood \}, window\.location\.origin\)/);
  assert.match(bridge, /event\.origin !== window\.location\.origin \|\| event\.source !== window\.parent/);
  assert.match(bridge, /if \(event\.isTrusted\) heldUntil = Date\.now\(\) \+ HOLD_MS;/);
  assert.match(bridge, /if \(reduced\) narrate\(INTRO\);\s*else tour\(hub\);/);
  assert.match(app, /event\.origin !== window\.location\.origin \|\| event\.source !== frame\.contentWindow/);
  assert.match(app, /line\.source !== APP_DEMO/);
  assert.match(app, /postMessage\(\{ source: APP_PAGE, visible \}, window\.location\.origin\)/);
});

test('the bridge answers every command the app can invoke', async () => {
  const invoked = invokedCommands();
  assert.ok(invoked.length >= 30);
  const { hub } = attachedHub();
  for (const name of invoked) assert.ok(Object.hasOwn(hub.commands, name), `${name} has no answer`);
  const refused = ['add_device', 'add_server'];
  for (const name of Object.keys(hub.commands)) {
    const answer = hub.invoke(name, name === 'term_open' ? { agent: 1, rows: 24, cols: 80, channel: {} } : undefined);
    if (refused.includes(name)) await assert.rejects(answer, name);
    else await assert.doesNotReject(answer, name);
  }
  assert.equal(await hub.invoke('never_heard_of'), null);
  assert.equal((await hub.invoke('settings')).config.notify_sound, false);
  assert.equal(await hub.invoke('play_chime', { sound: 'done' }), false);
  assert.equal((await hub.invoke('set_phone', { on: true })).phones, 1);
  assert.equal((await hub.invoke('set_phone', { on: false })).enabled, false);
  assert.equal((await hub.invoke('devices')).here.agents, initialState(NOW).agents.length);
  assert.deepEqual(await hub.invoke('agent_updates'), []);
});

test('the hub greets the app with its state and answers what the views ask for', () => {
  const { hub, pushed } = attachedHub();
  assert.deepEqual(pushed.map((message) => message.type), ['welcome', 'state', 'usage']);
  assert.equal(pushed[0].host, HOST);
  hub.commands.send({ message: { type: 'orchestrator' } });
  assert.equal(pushed.at(-1).type, 'orchestrator');
  hub.commands.send({ message: { type: 'peek', agents: [1, 2, 7, 99], rows: 6 } });
  const peek = pushed.at(-1);
  assert.equal(peek.type, 'peek');
  assert.deepEqual(peek.previews.map((preview) => preview.agent), [1, 2, 7]);
  for (const preview of peek.previews) {
    assert.ok(preview.lines.length > 2 && preview.lines.length <= 6);
    for (const line of preview.lines) {
      for (const run of line) {
        assert.equal(run.length, 4);
        assert.equal(typeof run[0], 'string');
        assert.ok(Number.isInteger(run[1]) && run[1] >= -1 && run[1] < 16 && run[2] === -1 && Number.isInteger(run[3]));
      }
    }
  }
  const frames = [];
  const session = hub.commands.term_open({ agent: 2, rows: 30, cols: 100, channel: { onmessage: (frame) => frames.push(frame) } });
  assert.equal(frames.length, 1);
  assert.deepEqual(frames[0].slice(0, 5), [1, 30, 0, 100, 0]);
  assert.ok(new TextDecoder().decode(Uint8Array.from(frames[0].slice(5))).includes('checkout'));
  hub.commands.send({ message: { type: 'stop', agent: 2 } });
  assert.equal(frames.length, 2);
  hub.commands.term_close({ session });
  hub.commands.send({ message: { type: 'restart', agent: 2 } });
  assert.equal(frames.length, 2);
  for (const type of ['models', 'skills', 'folders', 'folderSearch', 'makeFolder', 'orchestratorInstructions', 'orchestratorActivity']) {
    hub.commands.send({ message: { type, kind: 'claude', cwd: '~', path: '~', root: '~', query: 'x', parent: '~', name: 'new', project: null, seq: 7 } });
    assert.equal(pushed.at(-1).type, type === 'makeFolder' ? 'folderMade' : type);
  }
});

test('what a visitor does in the composer and on the cards changes the simulated session', () => {
  const { hub, pushed } = attachedHub();
  const count = hub.state().agents.length;
  hub.commands.send({ message: { type: 'start', kind: 'codex', session: MAIN_SESSION, cwd: '~/dev/shop', task: 'Please write the changelog', worktree: true } });
  const fresh = hub.state().agents.at(-1);
  assert.equal(hub.state().agents.length, count + 1);
  assert.deepEqual([fresh.name, fresh.kind, fresh.state, fresh.starting, fresh.project, fresh.branch], ['write-changelog', 'codex', 'working', false, 'dev/shop', 'octo/write-changelog']);
  hub.commands.send({ message: { type: 'start', kind: 'terminal', session: MAIN_SESSION, cwd: '~/dev/shop', task: '', worktree: false } });
  assert.equal(hub.state().agents.at(-1).name, 'term-1');
  hub.commands.send({ message: { type: 'stop', agent: fresh.id } });
  assert.equal(hub.state().agents.find((agent) => agent.id === fresh.id).state, 'exited');
  hub.commands.send({ message: { type: 'remove', agent: fresh.id } });
  assert.equal(hub.state().agents.some((agent) => agent.id === fresh.id), false);
  hub.commands.send({ message: { type: 'rename', agent: 3, name: 'physics' } });
  assert.equal(hub.state().agents.find((agent) => agent.id === 3).name, 'physics');
  hub.commands.send({ message: { type: 'session', name: 'experiments' } });
  hub.commands.send({ message: { type: 'moveAgent', agent: 3, session: 'experiments' } });
  assert.equal(pushed.findLast((message) => message.type === 'state').sessions.find((session) => session.name === 'experiments').agents, 1);
  hub.commands.send({ message: { type: 'deleteSession', name: 'experiments' } });
  assert.equal(hub.state().agents.some((agent) => agent.id === 3), false);
  hub.commands.send({ message: { type: 'orchestratorSet', mode: 'observe' } });
  assert.equal(hub.state().orchestrator.mode, 'observe');
  hub.commands.send({ message: { type: 'orchestratorChat', text: 'status?' } });
  const chat = hub.state().orchestrator.chat;
  assert.deepEqual(chat.slice(-2).map((entry) => entry.role), ['you', 'octo']);
  assert.equal(hub.state().orchestrator.thinking, false);
  hub.commands.send({ message: { type: 'orchestratorClearChat' } });
  assert.equal(hub.state().orchestrator.chat.length, 0);
  const before = hub.state();
  for (const message of [{ type: 'tests', agent: 1 }, { type: 'nonsense' }, null, 'text']) hub.commands.send({ message });
  assert.equal(hub.state(), before);
  assert.equal(agentName('claude', 'the of and', new Set(['claude-1'])), 'claude-2');
  assert.equal(projectColor('dev/shop'), projectColor('dev/shop'));
});

test('the real app sits under the hero in a frame scaled to the stage', () => {
  const frame = html.match(/<div class="app-fit" data-app-fit>\s*<iframe class="app-frame" data-app-frame src="\/app\/" width="1440" height="900" loading="lazy" tabindex="-1" title="([^"]{40,})"><\/iframe>\s*<\/div>/);
  assert.ok(frame);
  assert.ok(html.indexOf('data-perch') < html.indexOf('data-app-frame'));
  assert.match(css, /\.app-fit \{[^}]*height: calc\(var\(--s, 1\) \* 900px\);/);
  assert.match(css, /\.app-frame \{[^}]*width: 1440px; height: 900px;[^}]*border-radius: 14px; border: 1px solid #2a3133;[^}]*transform: scale\(var\(--s, 1\)\);[^}]*box-shadow:/);
  assert.equal(css.match(/\.app-frame \{[^}]*background: (#[0-9a-f]{6});/)[1], themeToken('gray', 'color-bg'));
  assert.match(app, /const APP_WIDTH = 1440;/);
  assert.match(app, /fit\.clientWidth \/ APP_WIDTH/);
  assert.doesNotMatch(app, /APP_SHELL/);
  assert.doesNotMatch(css, /^\.a-[a-z]+/m);
  assert.doesNotMatch(html, /data-app[ >]/);
});

test('the system is detected from the browser, and phones get no direct download', () => {
  assert.equal(detectOs('Win32', 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'), 'windows');
  assert.equal(detectOs('MacIntel', 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'), 'mac');
  assert.equal(detectOs('Linux x86_64', 'Mozilla/5.0 (X11; Linux x86_64)'), 'linux');
  assert.equal(detectOs('Linux armv8l', 'Mozilla/5.0 (Linux; Android 14)'), null);
  assert.equal(detectOs('iPhone', 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)'), null);
  assert.equal(detectOs('', ''), null);
  for (const os of Object.keys(OS_LABELS)) assert.match(html, new RegExp(`data-os="${os}"`));
});

test('motion is switched off for people who ask for less of it', () => {
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(app, /prefers-reduced-motion: reduce/);
});

test('the star count is short and hides when there is nothing to show', () => {
  assert.equal(formatStars(0), '0');
  assert.equal(formatStars(26), '26');
  assert.equal(formatStars(999), '999');
  assert.equal(formatStars(1000), '1k');
  assert.equal(formatStars(1249), '1.2k');
  assert.equal(formatStars(18950), '18.9k');
  assert.equal(formatStars(undefined), '');
  assert.equal(formatStars(-1), '');
  assert.equal(formatStars(NaN), '');
  assert.match(html, /<span class="gh-count" data-stars-box hidden><svg [^>]*>[\s\S]*?<\/svg><span data-stars><\/span><\/span>/);
});

test('every question of the FAQ is numbered in order and has an answer', () => {
  const faq = html.slice(html.indexOf('data-faq'), html.indexOf('</main>'));
  const items = [...faq.matchAll(/<details class="faq-item" data-reveal>\s*<summary><span class="faq-n">(\d\d)<\/span><span class="faq-q">([^<]+)<\/span><span class="faq-icon" aria-hidden="true"><\/span><\/summary>\s*<div class="faq-a"><p>([\s\S]*?)<\/p><\/div>\s*<\/details>/g)];
  assert.equal(items.length, [...faq.matchAll(/<details/g)].length);
  assert.ok(items.length >= 5);
  items.forEach(([, number, question, answer], index) => {
    assert.equal(number, String(index + 1).padStart(2, '0'));
    assert.ok(question.trim().endsWith('?'), question);
    assert.ok(answer.trim().length > 60, question);
  });
  assert.doesNotMatch(faq, /<details[^>]* open/);
});

test('Kai is the one of the app, frame by frame', () => {
  const strips = readdirSync(join(REPO, 'desktop/ui/kai')).filter((file) => file.endsWith('.svg'));
  assert.ok(strips.length >= 5);
  for (const file of strips) {
    assert.ok(readFileSync(join(ROOT, 'assets/kai', file)).equals(readFileSync(join(REPO, 'desktop/ui/kai', file))), file);
  }
  for (const [, mood] of css.matchAll(/\.kai\[data-mood="(\w+)"\]/g)) assert.ok(MOODS.includes(mood), mood);
});

test('Kai introduces every section after the hero', () => {
  const sections = [...html.matchAll(/<section class="section[^"]*" id="([^"]+)"[\s\S]*?<\/section>/g)];
  assert.ok(sections.length >= 7);
  for (const [block, id] of sections) {
    const guide = block.match(/<div class="guide" data-reveal data-guide>\s*<span class="kai" data-mood="(\w+)" aria-hidden="true"><span class="kai-strip"><\/span><\/span>\s*<p class="guide-say"><b>Kai<\/b><span data-say>([^<]+)<\/span><\/p>/);
    assert.ok(guide, id);
    assert.ok(MOODS.includes(guide[1]), id);
    assert.ok(guide[2].length > 30, id);
  }
});

test('the wordmark spells octo with the O of the icon as its first letter only, next to Kai', () => {
  const brands = [...html.matchAll(/<a class="brand"[^>]*aria-label="Octo, back to top">([\s\S]*?)<\/a>/g)];
  for (const [, brand] of brands) assert.equal([...brand.matchAll(/<span class="mark wordmark-o"><\/span>/g)].length, 1);
  assert.equal(brands.length, 2);
  for (const [, brand] of brands) {
    assert.match(brand, /<span class="kai kai-brand" data-mood="idle" aria-hidden="true">/);
    assert.match(brand, /<span class="wordmark" aria-hidden="true"><span class="mark wordmark-o"><\/span><svg class="wordmark-cto"/);
    assert.equal([...brand.matchAll(/<path /g)].length, 2);
    assert.equal([...brand.matchAll(/<circle /g)].length, 1);
    assert.match(brand, /<circle cx="232" cy="50" r="42\.4"\/><\/svg><\/span>/);
  }
  assert.match(css, /\.mark \{[^}]*octo-mark\.svg/);
});

test('the headline writes Octo with the wordmark and still reads as text', () => {
  const title = html.match(/<h1[^>]*id="hero-title"[^>]*>([\s\S]*?)<\/h1>/)[1];
  assert.match(title, /<span class="sr">Octo<\/span><span class="wordmark wordmark-inline" aria-hidden="true"><span class="mark wordmark-o"><\/span><svg class="wordmark-cto"/);
  assert.equal(title.replace(/<svg[\s\S]*?<\/svg>/g, '').replace(/<[^>]+>/g, '').replace(/\s+/g, ' ').trim(), 'Your IDE was built for typing. Octo is built for agents.');
  assert.match(css, /\.sr \{[^}]*clip-path: inset\(50%\)/);
});

test('the footer is the ink scene of the current site, with its sprite and sounds', () => {
  for (const file of ['assets/kai/ink.png', 'assets/chime.js']) {
    assert.ok(readFileSync(join(ROOT, file)).equals(readFileSync(join(REPO, 'site', file))), file);
  }
  const png = readFileSync(join(ROOT, 'assets/kai/ink.png'));
  assert.equal(png.readUInt32BE(16), 128 * SCENE_FRAMES);
  assert.equal(png.readUInt32BE(20), 96);
  assert.match(css, new RegExp(`kai/ink\\.png"\\) no-repeat 0 0 / ${SCENE_FRAMES}00% 100%`));
  const footer = html.slice(html.indexOf('<footer'));
  assert.match(footer, /<footer class="foot" data-ink>\s*<div class="foot-stage" data-ink-stage>\s*<span class="foot-scene" aria-hidden="true" data-scene><\/span>/);
  assert.ok([...footer.matchAll(/<a href="/g)].length >= 9);
});

test('Kai idles, shivers, then spits ink as the footer scrolls', () => {
  assert.equal(sceneFrame(0, 0), 0);
  assert.equal(sceneFrame(0.1, 900 * 3), 3);
  assert.equal(sceneFrame(0.1, 900 * 4), 0);
  assert.ok([SCENE_IDLE_FRAMES, SCENE_IDLE_FRAMES + 1].includes(sceneFrame(SCENE_STARTLE, 0)));
  let last = 0;
  for (const [start, frame] of SCENE_STEPS) {
    assert.equal(sceneFrame(start, 0), frame);
    assert.ok(frame > last);
    last = frame;
  }
  assert.equal(sceneFrame(SCENE_INKED, 0), SCENE_FRAMES - 1);
  assert.equal(sceneFrame(1, 12345), SCENE_FRAMES - 1);
});

test('the footer scene is driven by how far the footer has been scrolled', () => {
  assert.equal(pinned(5000, 3600, 1000, 4000), 0);
  assert.equal(pinned(5000, 3600, 1000, 5000), 0);
  assert.equal(pinned(5000, 3600, 1000, 6300), 0.5);
  assert.equal(pinned(5000, 3600, 1000, 9000), 1);
  assert.deepEqual(kaiCorner(1440, 900), { x: 1236, y: 696, size: 192 });
  assert.deepEqual(kaiCorner(390, 800), { x: 234, y: 644, size: 144 });
});

test('the header is a plain square bar, with no rounded or cut corners', () => {
  const rules = [...css.matchAll(/^(\.nav[^{]*|\.gh[^{]*) \{([^}]*)\}/gm)];
  assert.ok(rules.length >= 10);
  for (const [, selector, body] of rules) {
    assert.doesNotMatch(body, /clip-path/, selector);
    assert.doesNotMatch(body, /border-radius: [1-9]/, selector);
  }
  assert.match(css, /\.nav \.btn \{[^}]*border-radius: 0;/);
  assert.match(css, /\.nav-in \{[^}]*border: 2px solid/);
  assert.doesNotMatch(css, /\.nav[^{]*\{[^}]*border-radius: [1-9]/);
});

test('the page ends at the FAQ and the footer, with no closing banner', () => {
  assert.doesNotMatch(html, /Put the fleet to work/);
  assert.doesNotMatch(css, /\.finale/);
  assert.match(html, /<\/section>\s*<\/main>\s*<footer class="foot" data-ink>/);
});

test('the hero shows the octopus artwork and no light canvas', () => {
  assert.match(html, /<div class="hero-art">\s*<img src="\/assets\/hero-kraken\.webp" width="1672" height="941"[^>]* alt="[^"]{40,}">/);
  const art = readFileSync(join(ROOT, 'assets/hero-kraken.webp'));
  assert.equal(art.toString('ascii', 0, 4), 'RIFF');
  assert.equal(art.toString('ascii', 8, 12), 'WEBP');
  assert.ok(art.length < 800_000, `${art.length} bytes`);
  assert.doesNotMatch(html, /<canvas/);
  assert.equal(existsSync(join(ROOT, 'assets/beam.js')), false);
  assert.doesNotMatch(app, /beam/i);
});

test('the agent icons are the ones the app draws', async () => {
  const desktop = await import('../../desktop/ui/js/icons.js');
  assert.deepEqual(PROVIDER_ICONS, desktop.PROVIDER_ICONS);
  assert.equal(Object.keys(PROVIDER_ICONS).length, 8);
  const first = agentIcon('codex', 30);
  const second = agentIcon('codex', 30);
  assert.match(first, /^<svg width="30" height="30" viewBox="0 0 24 24"/);
  assert.notEqual(first.match(/id="([^"]+)"/)[1], second.match(/id="([^"]+)"/)[1]);
  assert.equal(first.match(/id="([^"]+)"/)[1], first.match(/url\(#([^)]+)\)/)[1]);
  assert.equal(agentIcon('nobody', 30), '');
});

test('the orchestration diagram goes from you to Octo to agents to subagents', () => {
  const ids = ORCH_NODES.map((node) => node.id);
  assert.equal(new Set(ids).size, ids.length);
  assert.deepEqual(ids.slice(0, 2), ['you', 'octo']);
  const tier = Object.fromEntries(ORCH_NODES.map((node) => [node.id, node.tier]));
  const below = { you: 'octo', octo: 'agent', agent: 'sub' };
  for (const edge of ORCH_EDGES) {
    assert.equal(tier[edge.to], below[tier[edge.from]], edge.id);
    assert.match(edge.d, /^M[\d.]+ [\d.]+V[\d.]+H[\d.]+V[\d.]+$/);
  }
  for (const node of ORCH_NODES.filter((entry) => entry.tier === 'agent')) assert.ok(PROVIDER_ICONS[node.id], node.id);
  for (const node of ORCH_NODES) {
    assert.ok(node.x - node.w / 2 >= 0 && node.x + node.w / 2 <= 1200, node.id);
    assert.ok(node.y + node.h / 2 <= 490, node.id);
  }
});

test('every step of the diagram lights nodes and edges that exist', () => {
  const ids = new Set(ORCH_NODES.map((node) => node.id));
  const edges = new Set(ORCH_EDGES.map((edge) => edge.id));
  assert.ok(ORCH_STEPS.length >= 5);
  for (const step of ORCH_STEPS) {
    assert.ok(step.say.length > 20, step.say);
    assert.ok(step.nodes.length > 0 && step.edges.length > 0);
    for (const id of step.nodes) assert.ok(ids.has(id), id);
    for (const id of step.edges) assert.ok(edges.has(id), id);
    for (const [id, state] of Object.entries(step.states)) {
      assert.ok(ids.has(id), id);
      assert.ok(ORCH_STATE_TEXT[state], state);
    }
  }
  const last = ORCH_STEPS.at(-1);
  assert.ok(Object.values(last.states).every((state) => state === 'done'));
  assert.equal(html.indexOf('id="flow"') < html.indexOf('id="ade"'), true);
});

test('the phone section pairs every feature with a real screen of the app', () => {
  const block = html.slice(html.indexOf('id="mobile"'), html.indexOf('id="everywhere"'));
  const feats = [...block.matchAll(/<button class="m-feat" type="button" role="tab" aria-selected="(true|false)" data-m-feat="(\d)"><i>(\d\d)<\/i><b>([^<]+)<\/b><span>([^<]+)<\/span><\/button>/g)];
  const screens = [...block.matchAll(/<img class="m-screen( is-on)?" data-m-screen="(\d)" src="\/assets\/mobile\/([a-z]+)\.webp" width="780" height="1688" alt="" loading="lazy" decoding="async">/g)];
  assert.equal(feats.length, 6);
  for (const [, , , name] of screens) {
    const file = readFileSync(join(ROOT, 'assets/mobile', `${name}.webp`));
    assert.equal(file.toString('ascii', 8, 12), 'WEBP', name);
    assert.ok(file.length > 10_000 && file.length < 200_000, name);
  }
  assert.equal(new Set(screens.map((screen) => screen[3])).size, screens.length);
  assert.equal(screens.length, feats.length);
  feats.forEach(([, selected, at, number, title, text], index) => {
    assert.equal(Number(at), index);
    assert.equal(number, String(index + 1).padStart(2, '0'));
    assert.equal(selected, String(index === 0));
    assert.ok(title.length > 10 && text.length > 40, title);
    assert.equal(Number(screens[index][2]), index);
    assert.equal(Boolean(screens[index][1]), index === 0);
  });
  assert.match(html, /<a href="#mobile">Mobile<\/a>/);
  assert.equal([...html.matchAll(/<li class="reach-card" data-reveal>/g)].length, 2);
});

test('the header is see-through and wide at the top, then a floating pink bar with a black brand corner', () => {
  assert.match(css, /\.nav\.is-stuck \.nav-in \{[^}]*max-width: 1120px;[^}]*background: var\(--header\);/);
  assert.match(css, /\.nav-in \{[^}]*max-width: 100%;[^}]*background: transparent;/);
  assert.match(css, /\.nav \{[^}]*top: 0;/);
  assert.match(css, /\.nav\.is-stuck \{ top: 14px; \}/);
  assert.equal(rootToken(css, 'header'), rootToken(css, 'kai'));
  assert.match(css, /\.nav\.is-stuck \.brand \{[^}]*background: #000;/);
  assert.match(css, /\.nav \.btn-accent:hover \{[^}]*transform: translate\(-3px, -3px\);[^}]*box-shadow:/);
});

test('on screens wider than the artwork the hero shows the whole picture instead of zooming', () => {
  assert.match(css, /@media \(min-aspect-ratio: 16 \/ 9\) \{\s*\.hero-art img \{[^}]*width: auto;[^}]*aspect-ratio: 1672 \/ 941;/);
});

test('cards glow in different colors under the pointer', () => {
  const colors = new Set([...css.matchAll(/\.glow-card[^{]*\{ --glow: var\((--kind-[a-z]+)\); \}/g)].map((match) => match[1]));
  assert.ok(colors.size >= 5, [...colors].join(' '));
  assert.match(css, /\.glow-card::before \{[^}]*radial-gradient\(420px circle at var\(--mx\) var\(--my\)/);
  assert.match(app, /querySelectorAll\('\.era, \.cell, \.reach-card, \.get-link, \.define, \.m-stage, \.faq-more'\)/);
});

test('the phone section links to the app in both stores', () => {
  const block = html.slice(html.indexOf('id="mobile"'), html.indexOf('id="everywhere"'));
  const stores = [...block.matchAll(/<a class="store store-(?:apple|play)" href="([^"]+)"><svg [^>]*aria-hidden="true"[^>]*>[\s\S]*?<\/svg><span><i>[^<]+<\/i><b>([^<]+)<\/b><\/span><\/a>/g)].map((match) => [match[2], match[1]]);
  assert.deepEqual(stores, [
    ['App Store', 'https://apps.apple.com/us/app/octo-ai-agents/id6832724791'],
    ['Google Play', 'https://play.google.com/store/apps/details?id=dev.marujo.octo'],
  ]);
  assert.ok(block.indexOf('class="m-get"') < block.indexOf('data-m-feat="0"'));
  assert.ok(block.indexOf('class="m-get"') > block.indexOf('class="m-side"'));
  assert.match(css, /\.store-apple \{ background: #fff; color: #000; \}/);
  const play = block.slice(block.indexOf('store-play'));
  for (const color of ['#00a0ff', '#00e676', '#ff3a44', '#ffd500']) assert.ok(play.includes(`fill="${color}"`), color);
});

test('the phone section advances one feature at a time as the page scrolls', () => {
  assert.deepEqual(stepAt(-0.2, 6), { index: 0, part: 0 });
  assert.deepEqual(stepAt(0, 6), { index: 0, part: 0 });
  assert.equal(stepAt(0.5, 6).index, 3);
  assert.deepEqual(stepAt(1, 6), { index: 5, part: 1 });
  assert.deepEqual(stepAt(4, 6), { index: 5, part: 1 });
  let last = -1;
  for (let step = 0; step <= 100; step += 1) {
    const { index, part } = stepAt(step / 100, 6);
    assert.ok(index === last || index === last + 1, `jump at ${step}`);
    assert.ok(part >= 0 && part <= 1);
    last = index;
  }
  assert.equal(last, 5);
  for (let index = 0; index < 6; index += 1) assert.equal(stepAt(progressFor(index, 6), 6).index, index);
  assert.match(html, /<div class="m-track" data-m-track>\s*<div class="mobile" data-reveal data-mobile>/);
  assert.match(css, /\.js \.m-track \.mobile \{ position: sticky;/);
});

test('the drawn letters of the wordmark sit on the same line as the O of the icon', () => {
  const number = (rule, property) => Number(css.match(new RegExp(`\\.${rule} \\{[^}]*${property}: (-?[\\d.]+)em;`))[1]);
  const box = number('wordmark-o', 'height');
  const ringBottom = -number('wordmark-o', 'margin-bottom') - (box * 12) / 392;
  const [top, height] = html.match(/class="wordmark-cto" viewBox="-?\d+ (-?\d+) \d+ (\d+)"/).slice(1).map(Number);
  const unit = number('wordmark-cto', 'height') / height;
  const lettersBottom = -number('wordmark-cto', 'margin-bottom') - (top + height - 100) * unit;
  assert.ok(Math.abs(ringBottom - lettersBottom) < 0.002, `${ringBottom} vs ${lettersBottom}`);
  assert.ok(Math.abs((box * 368) / 392 - 100 * unit) < 0.002);
});

test('the favicon is the Kai icon of the current site, not the O', () => {
  assert.match(html, /<link rel="icon" href="\/octo-kai-peek\.ico" sizes="48x48">\s*<link rel="icon" href="\/favicon\.svg" type="image\/svg\+xml">\s*<link rel="apple-touch-icon" href="\/octo-kai-peek-touch-icon\.png">/);
  assert.doesNotMatch(html, /rel="icon" href="\/assets\/octo-mark\.svg"/);
  for (const file of ['favicon.ico', 'favicon.svg', 'octo-kai-peek.ico', 'octo-kai-peek-touch-icon.png', 'og-octo-kai.png']) {
    assert.ok(readFileSync(join(ROOT, file)).equals(readFileSync(join(REPO, 'site', file))), file);
  }
});

test('the page runs under the content security policy the server sends', () => {
  const nginx = read('nginx.conf');
  const policy = nginx.match(/Content-Security-Policy "([^"]+)"/)[1];
  const rules = Object.fromEntries(policy.split('; ').map((rule) => [rule.split(' ')[0], rule.split(' ').slice(1)]));
  assert.deepEqual(rules['default-src'], ["'self'"]);
  assert.equal(rules['script-src'], undefined);
  assert.deepEqual(rules['frame-ancestors'], ["'self'"]);
  assert.deepEqual(rules['connect-src'], ["'self'", 'https://api.github.com']);
  assert.ok(rules['style-src'].includes("'unsafe-inline'"));
  const scripts = [...html.matchAll(/<script([^>]*)>/g)].map((match) => match[1]);
  for (const attributes of scripts) assert.ok(/ src="\/assets\/[a-z]+\.js"/.test(attributes) || attributes.includes('application/ld+json'), attributes);
  assert.doesNotMatch(html, / on[a-z]+="/);
  assert.equal(read('assets/boot.js').trim(), "document.documentElement.className = 'js';");
  assert.doesNotThrow(() => JSON.parse(html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)[1]));
});

test('the pages around the landing page come along: privacy, robots, sitemap and the social card', () => {
  const privacy = read('privacy/index.html');
  assert.match(privacy, /<link rel="stylesheet" href="\/assets\/legal\.css">/);
  assert.ok(existsSync(join(ROOT, 'assets/legal.css')));
  assert.match(html, /<a href="\/privacy\/">privacy policy<\/a>/);
  assert.match(read('robots.txt'), /Sitemap: https:\/\/octo\.marujo\.dev\/sitemap\.xml/);
  assert.match(read('sitemap.xml'), /<loc>https:\/\/octo\.marujo\.dev\/privacy\/<\/loc>/);
  assert.match(html, /<meta property="og:image" content="https:\/\/octo\.marujo\.dev\/og-octo-kai\.png">/);
  assert.match(html, /<link rel="canonical" href="https:\/\/octo\.marujo\.dev\/">/);
  assert.ok(existsSync(join(ROOT, 'docker-compose.yml')));
});
