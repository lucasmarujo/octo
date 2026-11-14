import { $, h, keep, place, storage, svg } from './dom.js';
import { askModelsOf } from './composer.js';
import { ask, closeMenu, confirm, menuShown, openMenu } from './dialogs.js';
import { OCTO_TAB, SPINNER, duration, kindColor, label, plural } from './format.js';
import { hubText, t } from './i18n.js';
import { ICON_ACTIVITY, ICON_CHAT, ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_DECISIONS, ICON_OCTO, ICON_PROJECTS, ICON_SEARCH } from './icons.js';
import { slideThumb } from './motion.js';
import {
  CHAT_CARDS,
  MODES,
  MODE_NAMES,
  MODE_TEXTS,
  MODE_WORDS,
  OFF_VIEW,
  RAIL_HANDLED,
  RAIL_NEEDS,
  activityPage,
  activityRequest,
  activitySignature,
  answerMessage,
  byWord,
  canSaveInstructions,
  chatKeys,
  chatMessage,
  chatSignature,
  clearChatMessage,
  deciding,
  decisionMeta,
  decisionProjects,
  decisionSignature,
  detailSignature,
  enabledMessage,
  engineInstalled,
  engineKinds,
  engineMessage,
  engineModelChoices,
  engineModelName,
  filterProjects,
  fleetCounts,
  handledByOcto,
  installedEngines,
  instructionTabs,
  instructionsFile,
  instructionsRequest,
  instructionsTooLong,
  mergeActivity,
  modeMessage,
  modelMessage,
  navCounts,
  needsBypassConfirm,
  needsYou,
  octoView,
  outcomeMark,
  pickDecision,
  projectFilterCounts,
  projectModeMessage,
  projectRows,
  saveInstructionsMessage,
  secondaryChoices,
  shortAge,
  shownDecisions,
  startChoice,
  suggestionAnswer,
  suggestionText,
  turnOnMessage,
} from './orchestrator.js';
import { segmented, toggle } from './settings.js';
import { agents, findAgent, modelsOf, now, state } from './state.js';
import { toast } from './toast.js';
import { send } from './transport.js';

const VIEW_KEY = 'octo.orchestratorView';
const GLOBAL_FILE = 'OCTO.md';
const PAUSED_MARK = '‖';
const INPUT_MAX_HEIGHT = 160;
const NAV = [
  ['chat', t('Chat'), ICON_CHAT],
  ['decisions', t('Decisions'), ICON_DECISIONS],
  ['activity', t('Activity'), ICON_ACTIVITY],
  ['projects', t('Projects'), ICON_PROJECTS],
];
const TITLES = { chat: t('Octo'), decisions: t('Decisions'), activity: t('Activity'), projects: t('Projects'), settings: t('Orchestrator settings') };
const MODE_OPTIONS = MODES.map((mode) => [mode, MODE_NAMES[mode]]);
const SAYS_TONES = { needs: 't-yellow', deciding: 't-working', handled: 't-muted', none: 't-faint' };

const nodes = new Map();
const details = new Map();
const lists = { chat: new Map(), cards: new Map(), needs: new Map(), deciding: new Map(), handled: new Map(), projects: new Map(), activity: new Map() };
const choice = { primary: null, mode: null };
const queue = { filter: 'needs', project: null, selected: null };
const table = { query: '', filter: 'all' };
const files = { tab: null, saved: new Map(), drafts: new Map(), asked: new Set() };
const log = { older: [], next: null, loading: false };

let actions = { openAgent: () => {}, openPage: () => {} };
let shownView = octoView(storage.get(VIEW_KEY));
let entered = null;
let seen = null;
let version = 0;

const view = () => state.orchestrator || OFF_VIEW;
const typing = (node) => node.contains(document.activeElement) && document.activeElement.matches('textarea, input[type="text"]');
const caps = (text, tone) => h('span', { class: `caps${tone ? ` ${tone}` : ''}` }, text);
const waited = (at) => h('span', { dataset: { since: String(at) } }, duration(now() - at));
const age = (at, tone) => h('span', { class: `octo-age${tone ? ` ${tone}` : ''}`, dataset: { ago: String(at) } }, shortAge(now() - at));
const spinner = () => h('span', { class: 'glyph t-working', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]);
const kindTag = (kind) => h('span', { class: 'octo-kind', style: `color:${kindColor(kind)}` }, label(kind));
const needsText = (count) => (count === 1 ? t('{count} needs you', { count }) : t('{count} need you', { count }));

function block(key, signature, build) {
  const hit = nodes.get(key);
  if (hit && typing(hit.node)) return hit.node;
  return keep(nodes, key, signature, build);
}

function keyed(cache, entries, key, signature, build) {
  const keys = new Set();
  const built = entries.map((entry) => {
    keys.add(key(entry));
    return keep(cache, key(entry), signature(entry), () => build(entry));
  });
  for (const stale of [...cache.keys()]) if (!keys.has(stale)) cache.delete(stale);
  return built;
}

function menuItem(text, checked, act) {
  return h(
    'button',
    {
      class: `menu-item${checked ? ' active' : ''}`,
      type: 'button',
      role: 'menuitem',
      onclick: () => {
        closeMenu();
        act();
      },
    },
    h('span', null, text),
    checked ? h('span', { class: 'check' }, '✓') : null,
  );
}

function toggleMenu(event, items) {
  event.stopPropagation();
  if (menuShown()) closeMenu();
  else openMenu(event.currentTarget, items(), false);
}

function confirmBypass(project) {
  return confirm({
    title: project ? t('Run {project} in Bypass?', { project }) : t('Switch Octo to Bypass?'),
    text: t('Octo will answer every question and allow everything on its own, with no block list and no limits. Nothing waits for you.'),
    action: t('Use Bypass'),
    danger: true,
  });
}

function redraw(...keys) {
  for (const key of keys) nodes.delete(key);
  renderOcto();
}

async function pickMode(mode) {
  if (needsBypassConfirm(mode, view().mode) && !(await confirmBypass(null))) {
    redraw('mode', 'modes');
    return;
  }
  send(modeMessage(mode));
}

async function pickProjectMode(row, mode) {
  if (needsBypassConfirm(mode || view().mode, row.mode) && !(await confirmBypass(row.project))) return;
  send(projectModeMessage(row.project, mode));
}

async function turnOn() {
  const current = view();
  const primary = startChoice(current, choice.primary);
  const mode = choice.mode || current.mode;
  if (!primary) return;
  if (mode === 'bypass' && !(await confirmBypass(null))) return;
  send(turnOnMessage(primary, mode));
}

async function answerMyself(decision) {
  const text = await ask({ title: t('Answer {name}', { name: decision.name }), text: decision.question, action: t('Send'), placeholder: t('Type your answer…') });
  if (text) send(answerMessage(decision.id, 'text', { text }));
}

function openDecisionAgent(decision) {
  actions.openAgent(findAgent(decision.agent));
}

function showDecision(id) {
  queue.filter = 'needs';
  queue.project = null;
  queue.selected = id;
  openOcto('decisions');
}

function selectDecision(id) {
  queue.selected = id;
  renderOcto();
}

function navRow([name, text, icon], active, counts) {
  const tail = {
    decisions: counts.needs ? h('span', { class: 'octo-badge' }, String(counts.needs)) : null,
    activity: counts.mode ? h('span', { class: 'session-count' }, String(counts.handled)) : null,
    projects: counts.projects ? h('span', { class: 'session-count' }, String(counts.projects)) : null,
  }[name];
  return h('button', { class: 'session-row orchestrator-row', type: 'button', 'aria-current': String(active && shownView === name), onclick: () => openOcto(name) }, svg(icon), h('span', { class: 'session-name' }, text), tail);
}

export function renderOctoNav() {
  const host = $('octo-nav');
  const active = state.active === OCTO_TAB;
  if (!active) entered = null;
  const counts = navCounts(view(), agents());
  const signature = JSON.stringify([active, shownView, counts.mode, counts.needs, counts.handled, counts.projects]);
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  const mode = $('octo-mode');
  mode.textContent = counts.mode ? MODE_WORDS[counts.mode] : t('off');
  mode.classList.toggle('off', !counts.mode);
  host.replaceChildren(...NAV.map((entry) => navRow(entry, active, counts)));
}

export function openOcto(name) {
  shownView = octoView(name);
  storage.set(VIEW_KEY, shownView);
  if (state.active === OCTO_TAB) renderOcto();
  else actions.openPage(OCTO_TAB);
  renderOctoNav();
}

function subtitle(current, counts) {
  const needs = counts.needs ? h('span', { class: 'octo-subtitle-needs' }, needsText(counts.needs)) : null;
  const crowd = { agents: plural(counts.agents, t('agent'), t('agents')), projects: plural(counts.projects, t('project'), t('projects')) };
  if (!current.enabled) return [t('The orchestrator is off')];
  if (shownView === 'chat') return [t('Running {agents} across {projects}', crowd), needs ? ' · ' : null, needs];
  if (shownView === 'decisions') return [t('Every question from every agent, in one queue'), needs ? ' · ' : null, needs];
  if (shownView === 'activity') return [t('What Octo answered, allowed, denied or left for you')];
  if (shownView === 'projects') return [`${crowd.projects} · ${crowd.agents} · ${t('each project can run in its own mode')}`];
  return [t('Which agent thinks for Octo, how far it can go, and what it must follow')];
}

function engineChip(current) {
  return h(
    'button',
    { class: 'octo-engine', type: 'button', title: t('Orchestrator settings'), onclick: () => openOcto('settings') },
    h('span', { class: 'octo-engine-word' }, t('engine')),
    current.primary ? h('span', { class: 'octo-engine-kind', style: `color:${kindColor(current.primary)}` }, label(current.primary)) : h('span', { class: 'octo-engine-kind t-red' }, t('none')),
    current.secondary ? h('span', { class: 'octo-engine-word' }, t('then {engine}', { engine: label(current.secondary) })) : null,
    svg(ICON_CHEVRON_DOWN),
  );
}

function modeSwitch(current) {
  const group = segmented(MODE_OPTIONS, current.mode, pickMode, t('Mode'));
  group.classList.add('octo-mode');
  return group;
}

function head(current) {
  const counts = navCounts(current, agents());
  const title = current.enabled ? TITLES[shownView] : TITLES.chat;
  const host = block('head', '', () => h('header', { class: 'octo-head' }));
  const headline = block('headline', JSON.stringify([shownView, current.enabled, counts]), () =>
    h('div', { class: 'octo-headline' }, h('h1', { class: 'octo-title' }, title), h('p', { class: 'octo-subtitle' }, subtitle(current, counts))),
  );
  if (!current.enabled) {
    place(host, [headline]);
    return host;
  }
  const engine = block('engine', JSON.stringify([current.primary, current.secondary]), () => engineChip(current));
  const mode = block('mode', current.mode, () => modeSwitch(current));
  place(host, [headline, engine, mode]);
  return host;
}

function problemStrip(current) {
  const strip = block('problem', '', () => h('div', { class: 'octo-problem', role: 'status' }));
  const text = current.enabled && current.problem ? hubText(current.problem) : '';
  strip.hidden = !text;
  if (strip.textContent !== text) strip.textContent = text;
  return strip;
}

function engineChoice(kind, current, primary) {
  const installed = engineInstalled(current, kind);
  return h(
    'button',
    {
      class: 'octo-choice',
      type: 'button',
      disabled: !installed,
      'aria-pressed': String(kind === primary),
      onclick: () => {
        choice.primary = kind;
        renderOcto();
      },
    },
    h('span', { class: 'octo-choice-kind', style: `color:${kindColor(kind)}` }, label(kind)),
    h('span', { class: 'octo-choice-note' }, installed ? t('installed') : t('not installed')),
  );
}

function intro(current) {
  const kinds = engineKinds(current);
  const primary = startChoice(current, choice.primary);
  const mode = choice.mode || current.mode;
  const pickIntroMode = (picked) => {
    choice.mode = picked;
    renderOcto();
  };
  return block('intro', JSON.stringify([current.engines, primary, mode]), () =>
    h(
      'div',
      { class: 'octo-intro' },
      h(
        'div',
        { class: 'octo-intro-card' },
        h('span', { class: 'octo-ring large' }, svg(ICON_OCTO)),
        h('h2', { class: 'octo-intro-title' }, t('Let Octo answer your agents')),
        h('p', { class: 'octo-intro-text' }, t('Octo watches every agent. When one stops to ask something, Octo reads the question, thinks with an agent you already have and answers it, or leaves it for you with what it would say. You can also ask it about everything that is running.')),
        h('div', { class: 'octo-intro-step' }, caps(t('Engine')), h('div', { class: 'octo-choices' }, kinds.map((kind) => engineChoice(kind, current, primary)))),
        installedEngines(current).length ? null : h('p', { class: 'octo-intro-note' }, t('Octo thinks with {engines}. Install one of them and it can be turned on.', { engines: kinds.map(label).join(' / ') })),
        h('div', { class: 'octo-intro-step' }, caps(t('Mode')), segmented(MODE_OPTIONS, mode, pickIntroMode, t('Mode')), h('p', { class: 'octo-intro-mode' }, MODE_TEXTS[mode])),
        h('button', { class: 'button primary octo-intro-start', type: 'button', disabled: !primary, onclick: turnOn }, t('Turn on Octo')),
      ),
    ),
  );
}

function chatEntry(entry) {
  if (entry.role === 'you') return h('div', { class: 'octo-msg you' }, h('div', { class: 'octo-bubble' }, entry.text));
  return h(
    'div',
    { class: `octo-msg${entry.failed ? ' failed' : ''}` },
    h('span', { class: 'octo-ring' }, svg(ICON_OCTO)),
    h(
      'div',
      { class: 'octo-msg-body' },
      h('div', { class: 'octo-msg-text' }, entry.text),
      entry.actions.length ? h('div', { class: 'octo-msg-actions' }, entry.actions.map((line) => h('span', { class: 'octo-msg-action' }, line))) : null,
    ),
  );
}

function thinkingLine() {
  return h('div', { class: 'octo-msg thinking' }, h('span', { class: 'octo-ring' }, svg(ICON_OCTO)), h('div', { class: 'octo-msg-body' }, h('div', { class: 'octo-thinking' }, spinner(), t('Octo is thinking…'))));
}

function chatEmpty() {
  return h('div', { class: 'octo-msg' }, h('span', { class: 'octo-ring' }, svg(ICON_OCTO)), h('div', { class: 'octo-msg-body' }, h('div', { class: 'octo-msg-text t-muted' }, t('Ask me what is going on, or tell me what to do across your projects.'))));
}

function wouldAnswer(decision) {
  if (decision.suggestion) {
    return h(
      'div',
      { class: 'octo-would' },
      caps(t('Octo would answer'), 't-working'),
      h('div', { class: 'octo-would-text' }, suggestionText(decision.suggestion)),
      decision.suggestion.reason ? h('div', { class: 'octo-would-reason' }, decision.suggestion.reason) : null,
    );
  }
  return decision.problem ? h('div', { class: 'octo-would failed' }, caps(t('Octo could not judge this'), 't-red'), h('div', { class: 'octo-would-reason' }, hubText(decision.problem))) : null;
}

function decisionCard(decision) {
  return h(
    'div',
    { class: 'octo-card' },
    h(
      'div',
      { class: 'octo-card-head' },
      h('span', { class: 'octo-card-mark' }, PAUSED_MARK),
      h('span', { class: 'octo-agent' }, decision.name),
      h('span', { class: 'octo-project' }, decision.project),
      kindTag(decision.kind),
      h('span', { class: 'grow' }),
      h('span', { class: 'octo-card-wait' }, `${t('waiting')} `, waited(decision.since)),
    ),
    h('div', { class: 'octo-asks' }, caps(t('It asks')), h('div', { class: 'octo-asks-text' }, decision.question)),
    wouldAnswer(decision),
    h(
      'div',
      { class: 'octo-card-actions' },
      decision.suggestion ? h('button', { class: 'button waiting', type: 'button', onclick: () => send(answerMessage(decision.id, 'suggestion')) }, t("Send Octo's answer")) : null,
      h('button', { class: 'button', type: 'button', onclick: () => answerMyself(decision) }, t('Answer myself')),
      h('button', { class: 'button', type: 'button', onclick: () => send(answerMessage(decision.id, 'allow')) }, t('Allow it')),
      h('span', { class: 'grow' }),
      h('button', { class: 'octo-quiet', type: 'button', onclick: () => openDecisionAgent(decision) }, t('Open agent')),
    ),
  );
}

function handledSummary(count) {
  return h(
    'button',
    { class: 'octo-summary', type: 'button', onclick: () => openOcto('activity') },
    h('span', { class: 'octo-summary-mark' }, '✓'),
    h('span', { class: 'octo-summary-text' }, plural(count, t('decision handled today'), t('decisions handled today'))),
    h('span', { class: 'grow' }),
    svg(ICON_CHEVRON_RIGHT),
  );
}

function moreNeeds(count) {
  return h('button', { class: 'link-button octo-more', type: 'button', onclick: () => openOcto('decisions') }, t('{count} more in Decisions', { count }));
}

function paintThread(host, current) {
  if (host.dataset.signature === String(version)) return;
  host.dataset.signature = String(version);
  const keys = chatKeys(current.chat);
  const entries = keyed(lists.chat, current.chat.map((entry, index) => ({ entry, key: keys[index] })), (item) => item.key, (item) => chatSignature(item.entry), (item) => chatEntry(item.entry));
  const waiting = needsYou(current);
  const cards = keyed(lists.cards, waiting.slice(0, CHAT_CARDS), (decision) => decision.id, decisionSignature, decisionCard);
  const more = waiting.length > CHAT_CARDS ? keep(nodes, 'chat:more', String(waiting.length), () => moreNeeds(waiting.length - CHAT_CARDS)) : null;
  const handled = current.handledToday ? keep(nodes, 'chat:handled', String(current.handledToday), () => handledSummary(current.handledToday)) : null;
  const under = keep(nodes, 'chat:under', '', () => h('div', { class: 'octo-under' }));
  place(under, [...cards, more, handled].filter(Boolean));
  const empty = entries.length || current.thinking ? null : keep(nodes, 'chat:empty', '', chatEmpty);
  const thinking = current.thinking ? keep(nodes, 'chat:thinking', '', thinkingLine) : null;
  place(host, [empty, ...entries, under.childElementCount ? under : null, thinking].filter(Boolean));
  host.dataset.tail = JSON.stringify([keys[keys.length - 1], current.thinking, cards.length]);
}

function growInput(input) {
  input.style.height = 'auto';
  input.style.height = `${Math.min(input.scrollHeight, INPUT_MAX_HEIGHT)}px`;
  input.style.overflowY = input.scrollHeight > INPUT_MAX_HEIGHT ? 'auto' : 'hidden';
}

function submitChat(event) {
  event.preventDefault();
  const input = event.currentTarget.querySelector('.octo-input');
  const message = chatMessage(view(), input.value);
  if (!message) return;
  send(message);
  input.value = '';
  growInput(input);
}

function onChatKey(event) {
  if (event.key !== 'Enter' || event.shiftKey) return;
  event.preventDefault();
  const input = event.currentTarget;
  if (!event.altKey) {
    input.form.requestSubmit();
    return;
  }
  input.setRangeText('\n', input.selectionStart, input.selectionEnd, 'end');
  growInput(input);
}

function chatComposer() {
  const input = h('textarea', { class: 'task octo-input', rows: '1', placeholder: t('Ask Octo, or tell it what to do across your projects…'), spellcheck: 'false', onkeydown: onChatKey, oninput: (event) => growInput(event.currentTarget) });
  return h(
    'form',
    { class: 'composer octo-composer', autocomplete: 'off', onsubmit: submitChat },
    h('div', { class: 'composer-line' }, h('span', { class: 'caret' }), input),
    h(
      'div',
      { class: 'composer-options' },
      h('button', { class: 'octo-quiet', type: 'button', onclick: () => send(clearChatMessage()) }, t('Clear chat')),
      h('div', { class: 'grow' }),
      h('span', { class: 'hint' }, 'alt ↵ ', t('new line')),
      h('button', { class: 'button primary', type: 'submit' }, h('span', null, t('Send')), ' ', h('span', { class: 'mono' }, '↵')),
    ),
  );
}

function paintComposer(form, current) {
  form.querySelector('button[type="submit"]').disabled = current.thinking;
  form.setAttribute('aria-busy', String(current.thinking));
}

function railNeed(decision) {
  return h(
    'button',
    { class: 'octo-need', type: 'button', onclick: () => showDecision(decision.id) },
    h('span', { class: 'octo-need-head' }, h('span', { class: 'octo-agent' }, decision.name), age(decision.since, 't-yellow')),
    h('span', { class: 'octo-need-question' }, decision.question),
    h('span', { class: 'octo-need-meta' }, [decision.project, decision.tag].filter(Boolean).join(' · ')),
  );
}

function railNeeds(waiting) {
  return h(
    'section',
    { class: 'octo-rail-block' },
    h('div', { class: 'octo-rail-head' }, h('span', { class: 'octo-rail-mark' }, PAUSED_MARK), h('span', { class: 'octo-rail-title' }, t('Needs you')), h('span', { class: 'octo-rail-count' }, String(waiting.length))),
    waiting.length ? waiting.slice(0, RAIL_NEEDS).map(railNeed) : h('span', { class: 'octo-rail-note' }, t('Nothing is waiting for you.')),
    waiting.length > RAIL_NEEDS ? moreNeeds(waiting.length - RAIL_NEEDS) : null,
  );
}

function railHandledRow(entry) {
  const mark = outcomeMark(entry);
  return h(
    'div',
    { class: 'octo-done', title: entry.question },
    h('span', { class: `octo-done-mark ${mark.tone}` }, mark.text),
    h('span', { class: 'octo-done-text' }, entry.summary, ' ', h('span', { class: 'octo-done-name' }, entry.name)),
    age(entry.at),
  );
}

function railHandled(current) {
  const recent = handledByOcto(current).slice(0, RAIL_HANDLED);
  return h(
    'section',
    { class: 'octo-rail-block' },
    h('div', { class: 'octo-rail-head' }, h('span', { class: 'octo-rail-title' }, t('Handled by Octo')), h('span', { class: 'octo-rail-count' }, t('{count} today', { count: current.handledToday }))),
    recent.length ? h('div', { class: 'octo-done-list' }, recent.map(railHandledRow)) : h('span', { class: 'octo-rail-note' }, t('Nothing handled yet.')),
    h('button', { class: 'link-button octo-more', type: 'button', onclick: () => openOcto('activity') }, t('See all activity')),
  );
}

function stateBar(counts) {
  return h(
    'div',
    { class: 'octo-bar' },
    counts.working ? h('span', { class: 'working', style: `flex-grow:${counts.working}` }) : null,
    counts.needs ? h('span', { class: 'needs', style: `flex-grow:${counts.needs}` }) : null,
    counts.done ? h('span', { class: 'done', style: `flex-grow:${counts.done}` }) : null,
    counts.idle || !counts.total ? h('span', { class: 'idle', style: `flex-grow:${counts.idle || 1}` }) : null,
  );
}

function railFleet(counts) {
  return h(
    'section',
    { class: 'octo-fleet' },
    h('div', { class: 'octo-rail-head' }, h('span', { class: 'octo-rail-title' }, t('Fleet')), h('span', { class: 'octo-rail-count' }, plural(counts.total, t('agent'), t('agents')))),
    stateBar(counts),
    h(
      'div',
      { class: 'octo-fleet-legend' },
      h('span', { class: 't-working' }, t('{count} working', { count: counts.working })),
      h('span', { class: 't-yellow' }, t('{count} waiting', { count: counts.needs })),
      h('span', { class: 't-green' }, t('{count} done', { count: counts.done })),
      h('span', { class: 't-faint' }, t('{count} idle', { count: counts.idle })),
    ),
  );
}

function paintRail(host, current) {
  const waiting = needsYou(current);
  const counts = fleetCounts(agents());
  const needsSignature = JSON.stringify([waiting.length, waiting.slice(0, RAIL_NEEDS).map(decisionSignature)]);
  const handledSignature = JSON.stringify([current.handledToday, current.handled.slice(0, RAIL_HANDLED).map(activitySignature)]);
  place(host, [
    keep(nodes, 'rail:needs', needsSignature, () => railNeeds(waiting)),
    keep(nodes, 'rail:handled', handledSignature, () => railHandled(current)),
    keep(nodes, 'rail:fleet', JSON.stringify(counts), () => railFleet(counts)),
  ]);
}

function chatBody(current) {
  const body = block('body:chat', '', () => h('div', { class: 'octo-body octo-chat' }, h('div', { class: 'octo-chat-main' }, h('div', { class: 'octo-thread selectable' }), chatComposer()), h('aside', { class: 'octo-rail' })));
  paintThread(body.querySelector('.octo-thread'), current);
  paintComposer(body.querySelector('.octo-composer'), current);
  paintRail(body.querySelector('.octo-rail'), current);
  return body;
}

function chip(text, count, pressed, tone, act) {
  return h('button', { class: 'octo-chip', type: 'button', 'aria-pressed': String(pressed), onclick: act }, h('span', null, text), h('span', { class: `octo-chip-count${tone ? ` ${tone}` : ''}` }, String(count)));
}

function pickQueueFilter(filter) {
  queue.filter = filter;
  renderOcto();
}

function queueProjectMenu(current) {
  const pick = (project) => () => {
    queue.project = project;
    renderOcto();
  };
  return [menuItem(t('All projects'), queue.project === null, pick(null)), ...decisionProjects(current).map((project) => menuItem(project, queue.project === project, pick(project)))];
}

function queueBar(current) {
  return h(
    'div',
    { class: 'octo-toolbar' },
    chip(t('Needs you'), needsYou(current).length, queue.filter === 'needs', 't-yellow', () => pickQueueFilter('needs')),
    chip(t('Octo is deciding'), deciding(current).length, queue.filter === 'deciding', '', () => pickQueueFilter('deciding')),
    chip(t('Handled'), current.handledToday, queue.filter === 'handled', '', () => pickQueueFilter('handled')),
    h('span', { class: 'grow' }),
    h('button', { class: `filter${queue.project ? ' active' : ''}`, type: 'button', 'aria-haspopup': 'menu', onclick: (event) => toggleMenu(event, () => queueProjectMenu(view())) }, h('span', null, queue.project || t('All projects')), svg(ICON_CHEVRON_DOWN)),
  );
}

function decisionRow(decision) {
  return h(
    'button',
    { class: `octo-decision${decision.id === queue.selected ? ' selected' : ''}`, type: 'button', onclick: () => selectDecision(decision.id) },
    h('span', { class: 'octo-decision-mark' }, PAUSED_MARK),
    h(
      'span',
      { class: 'octo-decision-body' },
      h('span', { class: 'octo-decision-line' }, h('span', { class: 'octo-agent' }, decision.name), h('span', { class: 'octo-project' }, decision.project), kindTag(decision.kind)),
      h('span', { class: 'octo-decision-question' }, decision.question),
    ),
    decision.tag ? h('span', { class: 'octo-tag' }, decision.tag) : null,
    age(decision.since, 't-yellow'),
  );
}

function decidingRow(decision) {
  return h(
    'button',
    { class: `octo-line${decision.id === queue.selected ? ' selected' : ''}`, type: 'button', onclick: () => selectDecision(decision.id) },
    h('span', { class: 'octo-line-mark' }, spinner()),
    h('span', { class: 'octo-line-name t-text' }, decision.name),
    h('span', { class: 'octo-line-text' }, decision.question || t('Reading the question…')),
    age(decision.since),
  );
}

function handledLine(entry, wide) {
  const mark = outcomeMark(entry);
  return h(
    'div',
    { class: 'octo-line handled', title: entry.question },
    h('span', { class: `octo-line-mark ${mark.tone}` }, mark.text),
    h('span', { class: 'octo-line-name' }, entry.name),
    wide ? h('span', { class: 'octo-line-project' }, entry.project) : null,
    h('span', { class: 'octo-line-text' }, entry.summary),
    wide ? h('span', { class: 'octo-line-by' }, MODE_WORDS[entry.mode]) : null,
    h('span', { class: 'octo-line-by' }, byWord(entry)),
    age(entry.at),
  );
}

function queueEmpty() {
  return h('div', { class: 'octo-empty' }, h('span', { class: 'octo-empty-title' }, t('Nothing needs you')), h('span', null, t('Octo is handling the rest. A question it leaves for you lands here.')));
}

function sectionLabel(text, link) {
  return h('div', { class: 'octo-section-label' }, caps(text), link ? h('button', { class: 'link-button', type: 'button', onclick: () => openOcto('activity') }, t('Open activity')) : null);
}

function paintQueue(host, current, shown) {
  const overview = queue.filter === 'needs';
  const bar = keep(nodes, 'queue:bar', JSON.stringify([queue.filter, queue.project, needsYou(current).length, deciding(current).length, current.handledToday]), () => queueBar(current));
  const selected = (decision) => `${decisionSignature(decision)}:${decision.id === queue.selected}`;
  const needs = keyed(lists.needs, shown.needs, (decision) => decision.id, selected, decisionRow);
  const judging = keyed(lists.deciding, shown.deciding, (decision) => decision.id, selected, decidingRow);
  const handled = keyed(lists.handled, shown.handled, (entry) => entry.seq, activitySignature, (entry) => handledLine(entry, false));
  const group = (key, rows, tight) => {
    const node = keep(nodes, key, '', () => h('div', { class: `octo-group${tight ? ' tight' : ''}` }));
    place(node, rows);
    return node;
  };
  const empty = overview && !needs.length ? keep(nodes, 'queue:empty', '', queueEmpty) : null;
  const nothing = !overview && !judging.length && !handled.length ? keep(nodes, 'queue:nothing', '', () => h('div', { class: 'octo-empty' }, h('span', null, t('Nothing here right now.')))) : null;
  place(
    host,
    [
      bar,
      empty,
      nothing,
      needs.length ? group('queue:needs', needs, false) : null,
      judging.length ? group('queue:deciding', [keep(nodes, 'queue:deciding-label', '', () => sectionLabel(t('Octo is deciding'), false)), ...judging], false) : null,
      handled.length ? group('queue:handled', [keep(nodes, 'queue:handled-label', String(overview), () => sectionLabel(overview ? t('Handled in the last 10 minutes') : t('Handled'), true)), ...handled], true) : null,
    ].filter(Boolean),
  );
}

function detail(decision) {
  const suggestion = decision.suggestion;
  const judged = decision.status === 'needsYou';
  const meta = decisionMeta(decision, findAgent(decision.agent));
  const answer = h('div', { class: 'octo-answer selectable', hidden: !suggestion }, suggestionText(suggestion));
  const area = h('textarea', { class: 'octo-answer octo-answer-edit', rows: '4', spellcheck: 'false', placeholder: t('Type your answer…'), hidden: Boolean(suggestion) });
  area.value = suggestion && suggestion.text ? suggestion.text : '';
  const rule = h('input', { class: 'field octo-rule', type: 'text', spellcheck: 'false', disabled: true, placeholder: t('One line Octo will follow next time') });
  const check = h('input', { class: 'octo-check', type: 'checkbox' });
  const edit = h('button', { class: 'button', type: 'button' }, t('Edit'));
  const remember = () => (check.checked ? rule.value : '');
  check.addEventListener('change', () => {
    rule.disabled = !check.checked;
    if (check.checked) rule.focus();
  });
  edit.addEventListener('click', () => {
    const editing = area.hidden;
    area.hidden = !editing;
    answer.hidden = editing;
    edit.textContent = editing ? t('Undo edit') : t('Edit');
    if (editing) area.focus();
  });
  const submit = () => {
    const message = suggestionAnswer(decision, area.hidden ? null : area.value, remember());
    if (message) send(message);
    else toast(t('Write your answer first.'), 'warn');
  };
  return h(
    'div',
    { class: 'octo-detail-body' },
    h(
      'div',
      { class: 'octo-detail-head' },
      h('div', { class: 'octo-detail-title' }, judged ? h('span', { class: 'octo-detail-mark' }, PAUSED_MARK) : spinner(), h('span', { class: 'octo-detail-name' }, decision.name), h('button', { class: 'link-button', type: 'button', onclick: () => openDecisionAgent(decision) }, t('Open agent'))),
      h('div', { class: 'octo-detail-meta' }, `${meta.join(' · ')} · ${t('waiting')} `, waited(decision.since)),
    ),
    h(
      'div',
      { class: 'octo-detail-part' },
      caps(t('What led here')),
      decision.tool ? h('div', { class: 'octo-tool selectable' }, h('span', { class: 'octo-tool-name' }, decision.tool.name), decision.tool.detail) : null,
      decision.context.length ? h('pre', { class: 'octo-context selectable' }, decision.context.join('\n')) : h('span', { class: 'octo-rail-note' }, t('The screen of the agent was empty.')),
    ),
    h('div', { class: 'octo-detail-part' }, caps(t('It asks')), h('div', { class: 'octo-detail-question selectable' }, decision.question)),
    h(
      'div',
      { class: 'octo-detail-part' },
      caps(judged ? t('Octo would answer') : t('Octo is deciding'), 't-working'),
      answer,
      area,
      suggestion && suggestion.keys ? h('div', { class: 'octo-detail-keys' }, t('presses {keys}', { keys: suggestion.keys.join(' ') })) : null,
      suggestion && suggestion.reason ? h('div', { class: 'octo-would-reason' }, suggestion.reason) : null,
      decision.problem ? h('div', { class: 'octo-detail-problem' }, t('Octo could not judge this: {problem}', { problem: hubText(decision.problem) })) : null,
    ),
    h('span', { class: 'grow' }),
    h('label', { class: 'octo-remember' }, check, h('span', null, t('Remember this for'), ' ', h('span', { class: 'octo-remember-project' }, decision.project))),
    rule,
    h(
      'div',
      { class: 'octo-detail-actions' },
      h('button', { class: 'button waiting', type: 'button', onclick: submit }, suggestion ? t("Send Octo's answer") : t('Send my answer')),
      suggestion ? edit : null,
      h('button', { class: 'button', type: 'button', onclick: () => send(answerMessage(decision.id, 'allow', { remember: remember() })) }, t('Allow it')),
      h('button', { class: 'button', type: 'button', onclick: () => send(answerMessage(decision.id, 'dismiss')) }, t('Dismiss')),
    ),
  );
}

function paintDetail(host, decision) {
  host.hidden = !decision;
  for (const id of [...details.keys()]) if (!decision || id !== decision.id) details.delete(id);
  if (!decision) {
    place(host, []);
    return;
  }
  const hit = details.get(decision.id);
  const node = hit && typing(hit.node) ? hit.node : keep(details, decision.id, detailSignature(decision, findAgent(decision.agent)), () => detail(decision));
  place(host, [node]);
}

function decisionsBody(current) {
  const body = block('body:decisions', '', () => h('div', { class: 'octo-body octo-decisions' }, h('div', { class: 'octo-queue' }), h('aside', { class: 'octo-detail', hidden: true })));
  const shown = shownDecisions(current, queue.filter, queue.project, now());
  queue.selected = pickDecision(queue.selected, shown);
  paintQueue(body.querySelector('.octo-queue'), current, shown);
  paintDetail(body.querySelector('.octo-detail'), current.decisions.find((decision) => decision.id === queue.selected) || null);
  return body;
}

function paintLog(body, current) {
  const entries = mergeActivity(current.handled, log.older);
  const rows = keyed(lists.activity, entries, (entry) => entry.seq, activitySignature, (entry) => handledLine(entry, true));
  const empty = entries.length ? null : keep(nodes, 'log:empty', '', () => h('div', { class: 'octo-empty' }, h('span', { class: 'octo-empty-title' }, t('Nothing handled yet.')), h('span', null, t('Every question Octo answers, denies or leaves for you is listed here.'))));
  const more = log.next === null ? null : keep(nodes, 'log:more', String(log.loading), () => h('button', { class: 'button octo-older', type: 'button', disabled: log.loading, onclick: loadOlder }, t('Load older')));
  place(body.querySelector('.octo-log'), [empty, ...rows, more].filter(Boolean));
}

function loadOlder() {
  if (log.loading || log.next === null) return;
  log.loading = true;
  send(activityRequest(log.next));
  renderOcto();
}

function activityBody(current) {
  const body = block('body:activity', '', () =>
    h(
      'div',
      { class: 'octo-body octo-activity' },
      h('div', { class: 'octo-line octo-log-head' }, h('span', { class: 'octo-line-mark' }), caps(t('Agent'), 'octo-line-name'), caps(t('Project'), 'octo-line-project'), caps(t('What Octo did'), 'octo-line-text'), caps(t('Mode'), 'octo-line-by'), caps(t('By'), 'octo-line-by'), caps(t('Ago'), 'octo-age')),
      h('div', { class: 'octo-log' }),
    ),
  );
  paintLog(body, current);
  return body;
}

export function showActivity(message) {
  const page = activityPage(message);
  log.older = mergeActivity(log.older, page.items);
  log.next = page.next;
  log.loading = false;
  if (state.active === OCTO_TAB && shownView === 'activity') renderOcto();
}

function projectModeMenu(row) {
  const current = view();
  return [menuItem(t('Default ({mode})', { mode: MODE_NAMES[current.mode] }), row.override === null, () => pickProjectMode(row, null)), ...MODES.map((mode) => menuItem(MODE_NAMES[mode], row.override === mode, () => pickProjectMode(row, mode)))];
}

function says(row) {
  if (row.says.kind === 'needs') return [t('Waiting on you: {question}', { question: row.says.text })];
  if (row.says.kind === 'deciding') return [spinner(), ' ', t('Deciding…')];
  return [row.says.text];
}

function projectRow(row) {
  return h(
    'div',
    { class: 'octo-project-row' },
    h('span', { class: 'octo-project-name', title: row.project }, row.project),
    h('span', { class: 'octo-project-agents' }, stateBar(row.counts), h('span', { class: 'octo-project-count' }, String(row.counts.total)), row.needs ? h('span', { class: 'octo-project-needs' }, `${PAUSED_MARK}${row.needs}`) : null),
    h('span', { class: `octo-project-says ${SAYS_TONES[row.says.kind]}`, title: row.says.text }, says(row)),
    h('span', { class: 'octo-project-handled' }, String(row.handled)),
    h('button', { class: `octo-project-mode ${row.mode}`, type: 'button', 'aria-haspopup': 'menu', onclick: (event) => toggleMenu(event, () => projectModeMenu(row)) }, h('span', null, MODE_NAMES[row.mode]), svg(ICON_CHEVRON_DOWN)),
  );
}

function pickTableFilter(filter) {
  table.filter = filter;
  renderOcto();
}

function projectChips(counts) {
  return [
    chip(t('All'), counts.all, table.filter === 'all', '', () => pickTableFilter('all')),
    chip(t('Needs you'), counts.needs, table.filter === 'needs', 't-yellow', () => pickTableFilter('needs')),
    chip(t('Bypass'), counts.bypass, table.filter === 'bypass', '', () => pickTableFilter('bypass')),
  ];
}

function projectsFrame() {
  const search = h('input', {
    class: 'octo-search-input',
    type: 'text',
    spellcheck: 'false',
    placeholder: t('Find a project or agent'),
    'aria-label': t('Find a project or agent'),
    oninput: (event) => {
      table.query = event.currentTarget.value;
      renderOcto();
    },
  });
  search.value = table.query;
  return h(
    'div',
    { class: 'octo-body octo-projects' },
    h('div', { class: 'octo-toolbar' }, h('label', { class: 'octo-search' }, svg(ICON_SEARCH), search), h('div', { class: 'octo-chips' }), h('span', { class: 'grow' }), h('span', { class: 'octo-sorted' }, t('sorted by needs you'))),
    h('div', { class: 'octo-table-head' }, caps(t('Project'), 'octo-project-name'), caps(t('Agents'), 'octo-project-agents'), caps(t('Octo says'), 'octo-project-says'), caps(t('Handled'), 'octo-project-handled'), caps(t('Mode'), 'octo-project-mode')),
    h('div', { class: 'octo-table' }),
    h('div', { class: 'octo-table-foot' }),
  );
}

function projectsBody(current) {
  const body = block('body:projects', '', projectsFrame);
  const rows = projectRows(agents(), current);
  const shown = filterProjects(rows, table.query, table.filter);
  const counts = projectFilterCounts(rows);
  const chips = body.querySelector('.octo-chips');
  const chipSignature = JSON.stringify([table.filter, counts]);
  if (chips.dataset.signature !== chipSignature) {
    chips.dataset.signature = chipSignature;
    chips.replaceChildren(...projectChips(counts));
  }
  const empty = shown.length ? null : keep(nodes, 'projects:empty', '', () => h('div', { class: 'octo-empty' }, h('span', null, t('No project matches.'))));
  place(body.querySelector('.octo-table'), empty ? [empty] : keyed(lists.projects, shown, (row) => row.project, (row) => JSON.stringify([row, current.mode]), projectRow));
  const foot = t('{shown} of {total} projects', { shown: shown.length, total: rows.length });
  const footer = body.querySelector('.octo-table-foot');
  if (footer.textContent !== foot) footer.textContent = foot;
  return body;
}

function engineMenu(slot, current) {
  const kinds = slot === 'primary' ? installedEngines(current) : secondaryChoices(current);
  const items = kinds.map((kind) => menuItem(label(kind), current[slot] === kind, () => send(engineMessage(slot, kind))));
  return slot === 'primary' ? items : [menuItem(t('None'), current.secondary === null, () => send(engineMessage(slot, null))), ...items];
}

function modelMenu(slot, current) {
  const chosen = current[`${slot}Model`];
  return engineModelChoices(modelsOf(current[slot]), chosen).map((model) => {
    const item = menuItem(model.name, model.id === chosen, () => send(modelMessage(slot, model.id)));
    item.title = model.description || model.name;
    return item;
  });
}

function modelPick(slot, current) {
  const name = engineModelName(modelsOf(current[slot]), current[`${slot}Model`]);
  return h(
    'button',
    { class: 'octo-engine-model', type: 'button', 'aria-haspopup': 'menu', 'aria-label': `${t('Model Octo thinks with')}: ${name}`, title: t('Model Octo thinks with'), onclick: (event) => toggleMenu(event, () => modelMenu(slot, view())) },
    h('span', { class: 'octo-engine-model-name' }, name),
    svg(ICON_CHEVRON_DOWN),
  );
}

function engineCard(slot, current) {
  const kind = current[slot];
  askModelsOf(kind);
  const installed = kind !== null && engineInstalled(current, kind);
  const status = kind === null ? null : h('span', { class: `octo-engine-status ${installed ? 't-green' : 't-red'}` }, installed ? `● ${t('installed')}` : t('not installed'));
  const note = slot === 'primary' ? t('Thinks first, on the plan you already have.') : kind === null ? t('Nothing takes over when the primary fails.') : t('Takes over when the primary fails.');
  return h(
    'div',
    { class: `octo-engine-card${slot === 'primary' ? ' primary' : ''}` },
    h('div', { class: 'octo-engine-head' }, caps(slot === 'primary' ? t('Primary') : t('Secondary')), status),
    h(
      'button',
      { class: 'octo-engine-pick', type: 'button', 'aria-haspopup': 'menu', onclick: (event) => toggleMenu(event, () => engineMenu(slot, view())) },
      kind === null ? h('span', { class: 'octo-engine-name t-faint' }, t('None')) : h('span', { class: 'octo-engine-name', style: `color:${kindColor(kind)}` }, label(kind)),
      svg(ICON_CHEVRON_DOWN),
    ),
    h('span', { class: 'octo-engine-note' }, note),
    kind === null ? null : modelPick(slot, current),
  );
}

function sectionHead(title, text) {
  return h('div', { class: 'settings-heading with-text' }, h('h2', null, title), h('p', null, text));
}

function enginesSection(current) {
  return h('section', { class: 'octo-section' }, sectionHead(t('Engine'), t('Octo thinks with an agent you already have. No extra key needed.')), h('div', { class: 'octo-engines' }, engineCard('primary', current), engineCard('secondary', current)));
}

function radioCard(mode, current) {
  return h(
    'button',
    { class: 'octo-radio', type: 'button', role: 'radio', 'aria-checked': String(mode === current.mode), onclick: () => mode !== current.mode && pickMode(mode) },
    h('span', { class: 'octo-radio-dot' }),
    h('span', { class: 'octo-radio-text' }, h('span', { class: 'octo-radio-title' }, MODE_NAMES[mode]), h('span', { class: 'octo-radio-description' }, MODE_TEXTS[mode])),
  );
}

function modesSection(current) {
  return h(
    'section',
    { class: 'octo-section' },
    sectionHead(t('Default mode'), t('How far Octo goes on its own. Each project can override it.')),
    h('div', { class: 'octo-radios', role: 'radiogroup', 'aria-label': t('Default mode') }, MODES.map((mode) => radioCard(mode, current))),
  );
}

function powerSection(current) {
  return h(
    'section',
    { class: 'octo-section' },
    h(
      'div',
      { class: 'setting' },
      h('div', { class: 'setting-text' }, h('span', { class: 'setting-title' }, t('Octo orchestrator')), h('span', { class: 'setting-description' }, t('Off, Octo stops watching, answering and thinking, and costs nothing.'))),
      toggle(current.enabled, (on) => send(enabledMessage(on)), t('Octo orchestrator')),
    ),
  );
}

function askInstructions(project) {
  if (files.asked.has(project)) return;
  files.asked.add(project);
  send(instructionsRequest(project));
}

function paintInstructionsFoot(panel) {
  const area = panel.querySelector('.octo-editor');
  const saved = files.saved.get(files.tab);
  const text = saved ? saved.path : t('Reading the file…');
  const path = panel.querySelector('.octo-editor-path');
  if (path.textContent !== text) path.textContent = text;
  panel.querySelector('.octo-editor-limit').hidden = !instructionsTooLong(area.value);
  panel.querySelector('.octo-editor-save').disabled = !canSaveInstructions(saved ? saved.text : null, area.value);
}

function showInstructionsTab(panel) {
  const area = panel.querySelector('.octo-editor');
  const saved = files.saved.get(files.tab);
  const draft = files.drafts.get(files.tab);
  area.value = draft === undefined ? (saved ? saved.text : '') : draft;
  area.disabled = !saved;
  area.dataset.shown = JSON.stringify([files.tab, Boolean(saved)]);
  paintInstructionsFoot(panel);
}

function pickInstructionsTab(project) {
  files.tab = project;
  askInstructions(project);
  renderOcto();
}

function saveInstructions(panel) {
  const area = panel.querySelector('.octo-editor');
  const saved = files.saved.get(files.tab);
  if (!canSaveInstructions(saved ? saved.text : null, area.value)) return;
  send(saveInstructionsMessage(files.tab, area.value));
  files.saved.set(files.tab, { text: area.value, path: saved.path });
  files.drafts.delete(files.tab);
  paintInstructionsFoot(panel);
}

function instructionsFrame() {
  const panel = h('aside', { class: 'octo-instructions' });
  const area = h('textarea', {
    class: 'octo-editor',
    spellcheck: 'false',
    'aria-label': t('Instructions'),
    oninput: () => {
      const saved = files.saved.get(files.tab);
      if (saved && area.value !== saved.text) files.drafts.set(files.tab, area.value);
      else files.drafts.delete(files.tab);
      paintInstructionsFoot(panel);
    },
  });
  panel.append(
    h('div', { class: 'settings-heading with-text' }, h('h2', null, t('Instructions')), h('p', null, t('A Markdown file Octo reads before every decision.'))),
    h('div', { class: 'octo-file-tabs', role: 'tablist' }),
    area,
    h('span', { class: 'octo-editor-limit', hidden: true }, t('Too long to save: the file holds 64 KiB at most.')),
    h('div', { class: 'octo-editor-foot' }, h('span', { class: 'octo-editor-path selectable' }), h('button', { class: 'button primary octo-editor-save', type: 'button', disabled: true, onclick: () => saveInstructions(panel) }, t('Save'))),
  );
  return panel;
}

function paintInstructions(panel, current) {
  const tabs = instructionTabs(agents(), current);
  if (!tabs.includes(files.tab)) files.tab = null;
  askInstructions(files.tab);
  const host = panel.querySelector('.octo-file-tabs');
  const signature = JSON.stringify([tabs, files.tab]);
  if (host.dataset.signature !== signature) {
    host.dataset.signature = signature;
    host.replaceChildren(...tabs.map((project) => h('button', { class: 'octo-file-tab', type: 'button', role: 'tab', 'aria-selected': String(project === files.tab), onclick: () => pickInstructionsTab(project) }, project === null ? GLOBAL_FILE : project)));
  }
  const area = panel.querySelector('.octo-editor');
  if (area.dataset.shown !== JSON.stringify([files.tab, files.saved.has(files.tab)])) showInstructionsTab(panel);
}

export function showInstructions(message) {
  const file = instructionsFile(message);
  files.saved.set(file.project, { text: file.text, path: file.path });
  const panel = nodes.get('settings:instructions');
  if (!panel || file.project !== files.tab) return;
  if (files.drafts.has(file.project)) paintInstructionsFoot(panel.node);
  else showInstructionsTab(panel.node);
}

function settingsBody(current) {
  const body = block('body:settings', '', () =>
    h('div', { class: 'octo-body octo-settings' }, h('div', { class: 'octo-settings-main' }, h('div', { class: 'octo-slot' }), h('div', { class: 'octo-slot' }), h('div', { class: 'octo-slot' })), keep(nodes, 'settings:instructions', '', instructionsFrame)),
  );
  const [engines, modes, power] = body.querySelector('.octo-settings-main').children;
  place(engines, [block('engines', JSON.stringify([current.primary, current.secondary, current.primaryModel, current.secondaryModel, current.engines, modelsOf(current.primary), modelsOf(current.secondary)]), () => enginesSection(current))]);
  place(modes, [block('modes', current.mode, () => modesSection(current))]);
  place(power, [block('power', String(current.enabled), () => powerSection(current))]);
  paintInstructions(body.querySelector('.octo-instructions'), current);
  return body;
}

const BODIES = { chat: chatBody, decisions: decisionsBody, activity: activityBody, projects: projectsBody, settings: settingsBody };

function enter(current) {
  if (!current.enabled) entered = null;
  if (!current.enabled || entered === shownView) return;
  entered = shownView;
  if (shownView === 'activity') {
    log.older = [];
    log.next = null;
    log.loading = true;
    send(activityRequest(null));
  }
  if (shownView === 'settings') files.asked.clear();
}

export function renderOcto() {
  const host = $('octo-view');
  const current = view();
  if (seen !== current) {
    seen = current;
    version += 1;
  }
  enter(current);
  const body = current.enabled ? BODIES[shownView](current) : intro(current);
  const swapped = host.lastElementChild !== body;
  place(host, [head(current), problemStrip(current), body]);
  for (const group of host.querySelectorAll('.segmented:not(.sliding)')) slideThumb(group);
  scrollToEnd(host, swapped);
}

function scrollToEnd(host, swapped) {
  const thread = host.querySelector('.octo-thread');
  if (thread && (swapped || thread.dataset.shown !== thread.dataset.tail)) {
    thread.dataset.shown = thread.dataset.tail;
    thread.scrollTop = thread.scrollHeight;
  }
  const context = host.querySelector('.octo-context:not([data-shown])');
  if (context) {
    context.dataset.shown = '1';
    context.scrollTop = context.scrollHeight;
  }
}

export function tickOcto() {
  const time = now();
  const host = $('octo-view');
  for (const node of host.querySelectorAll('[data-since]')) node.textContent = duration(time - Number(node.dataset.since));
  for (const node of host.querySelectorAll('[data-ago]')) node.textContent = shortAge(time - Number(node.dataset.ago));
}

export function initOcto(handlers) {
  actions = handlers;
  $('open-octo').addEventListener('click', () => openOcto('settings'));
}
