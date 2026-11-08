(function () {
  'use strict';

  const Wire = window.OrkaiWire;
  const M = window.OrkaiModel;
  const SVG_NS = 'http://www.w3.org/2000/svg';
  const SECRET_KEY = 'orkai.secret';
  const WHO_KEY = 'orkai.who';
  const WORKTREE_KEY = 'orkai.worktree';
  const BYPASS_KEY = 'orkai.bypass';
  const RELOAD_KEY = 'orkai.reloaded';
  const THEME_KEY = 'orkai.theme';
  const LAYOUT_KEY = 'orkai.layout';
  const HAPTICS_KEY = 'orkai.haptics';
  const MOTION_KEY = 'orkai.motion';
  const RETRY = [400, 1000, 2000, 4000, 8000];
  const RETRY_MAX = 10000;
  const TERM_FONT = 12;
  const TERM_MIN = 6;
  const TERM_MAX = 15;
  const LINE = 1.2;
  const PAD = 6;
  const DIFF_LINES = 4000;
  const CONFIRM_MS = 3000;
  const INPUT_MAX = 132;
  const PEEK_ROWS = 18;
  const SEARCH_DELAY = 250;
  const PENDING_MS = 6000;
  const FLASH_MS = 700;
  const SHEET_MS = 320;
  const DRAG_CLOSE = 90;
  const SWIPE_AWAY = -30;
  const RECENT_MAX = 6;
  const RING_OUTER = 25;
  const RING_INNER = 18.5;
  const TOAST_MS = 3500;
  const TOAST_ERROR_MS = 6000;
  const DEFAULT = -1;
  const TRUECOLOR = 256;
  const BOLD = 1;
  const DIM = 2;
  const ITALIC = 4;
  const UNDERLINE = 8;
  const INVERSE = 16;
  const TERM_BG = 'var(--term-bg)';
  const TERM_FG = 'var(--term-fg)';
  const LABELS = { antigravity: 'agy' };
  const NAMES = { claude: 'Claude', codex: 'Codex', opencode: 'OpenCode', pi: 'Pi', kilo: 'Kilo', terminal: 'Terminal', antigravity: 'Antigravity', grok: 'Grok', qwen: 'Qwen', kimi: 'Kimi' };
  const KEY_GROUPS = [
    { keys: [['esc', '\x1b'], ['tab', '\t'], ['⇧tab', '\x1b[Z']] },
    { keys: [['↑', '\x1b[A'], ['↓', '\x1b[B'], ['←', '\x1b[D'], ['→', '\x1b[C'], ['⏎', '\r']] },
    { keys: [['1', '1'], ['2', '2'], ['3', '3'], ['y', 'y'], ['n', 'n']] },
    { keys: [['^C', '\x03']], tone: 'warn' },
  ];
  const STARTERS = ['Summarize what changed in this repo this week', 'Find the failing tests and fix them'];
  const ICON = {
    chevron: '<path d="M6 9l6 6l6 -6"/>',
    list: '<path d="M9 6l11 0"/><path d="M9 12l11 0"/><path d="M9 18l11 0"/><path d="M5 6l0 .01"/><path d="M5 12l0 .01"/><path d="M5 18l0 .01"/>',
    board: '<path d="M4 4l6 0"/><path d="M14 4l6 0"/><path d="M4 10a2 2 0 0 1 2 -2h2a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-2a2 2 0 0 1 -2 -2z"/><path d="M14 10a2 2 0 0 1 2 -2h2a2 2 0 0 1 2 2v2a2 2 0 0 1 -2 2h-2a2 2 0 0 1 -2 -2z"/>',
    folder: '<path d="M5 4h4l3 3h7a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-11a2 2 0 0 1 2 -2"/>',
    up: '<path d="M18 18h-6a3 3 0 0 1 -3 -3v-10l-4 4m8 0l-4 -4"/>',
    branch: '<path d="M5 18a2 2 0 1 0 4 0a2 2 0 1 0 -4 0"/><path d="M5 6a2 2 0 1 0 4 0a2 2 0 1 0 -4 0"/><path d="M15 6a2 2 0 1 0 4 0a2 2 0 1 0 -4 0"/><path d="M7 8l0 8"/><path d="M9 18h6a2 2 0 0 0 2 -2v-5"/><path d="M14 14l3 -3l3 3"/>',
    bolt: '<path d="M13 3l0 7l6 0l-8 11l0 -7l-6 0l8 -11"/>',
    plus: '<path d="M12 5l0 14"/><path d="M5 12l14 0"/>',
    pencil: '<path d="M4 20h4l10.5 -10.5a2.828 2.828 0 1 0 -4 -4l-10.5 10.5v4"/><path d="M13.5 6.5l4 4"/>',
    diff: '<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21h-10a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2h7l5 5v11a2 2 0 0 1 -2 2z"/><path d="M12 10l0 4"/><path d="M10 12l4 0"/><path d="M10 17l4 0"/>',
    merge: '<path d="M5 18a2 2 0 1 0 4 0a2 2 0 1 0 -4 0"/><path d="M5 6a2 2 0 1 0 4 0a2 2 0 1 0 -4 0"/><path d="M15 12a2 2 0 1 0 4 0a2 2 0 1 0 -4 0"/><path d="M7 8l0 8"/><path d="M7 8a4 4 0 0 0 4 4h4"/>',
    recover: '<path d="M20 11a8.1 8.1 0 0 0 -15.5 -2m-.5 -4v4h4"/><path d="M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4"/>',
    trash: '<path d="M4 7l16 0"/><path d="M10 11l0 6"/><path d="M14 11l0 6"/><path d="M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2 -2l1 -12"/><path d="M9 7v-3a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v3"/>',
    search: '<path d="M3 10a7 7 0 1 0 14 0a7 7 0 1 0 -14 0"/><path d="M21 21l-6 -6"/>',
    copy: '<path d="M7 9.667a2.667 2.667 0 0 1 2.667 -2.667h8.666a2.667 2.667 0 0 1 2.667 2.667v8.666a2.667 2.667 0 0 1 -2.667 2.667h-8.666a2.667 2.667 0 0 1 -2.667 -2.667z"/><path d="M4.012 16.737a2.005 2.005 0 0 1 -1.012 -1.737v-10c0 -1.1 .9 -2 2 -2h10c.75 0 1.158 .385 1.5 1"/>',
    check: '<path d="M5 12l5 5l10 -10"/>',
    offline: '<path d="M12 18l.01 0"/><path d="M9.172 15.172a4 4 0 0 1 5.656 0"/><path d="M6.343 12.343a7.963 7.963 0 0 1 3.864 -2.14m4.163 .155a7.965 7.965 0 0 1 3.287 2"/><path d="M3.515 9.515a12 12 0 0 1 3.544 -2.455m3.101 -.92a12 12 0 0 1 10.325 3.374"/><path d="M3 3l18 18"/>',
  };
  const PAIR = {
    new: { title: 'Pair this phone', text: 'Watch and steer your agents from anywhere. Encrypted end to end.', steps: true, form: true, retry: false, mood: 'idle' },
    unpaired: { badge: 'unpaired', title: 'This Octo forgot this phone', text: 'This phone is not paired with this Octo anymore. Scan the new code or paste its link to pair it again.', steps: true, form: true, retry: true, mood: 'lost' },
    replaced: { badge: 'another tab', title: 'Octo is open in another tab', text: 'This Octo is open in another tab of this browser. Tap use it here to move it to this tab.', steps: false, form: false, retry: true, again: 'Use it here', mood: 'idle' },
    outdated: { badge: 'update needed', title: 'Octo speaks another version', text: 'This Octo speaks another version of the app. Update Octo on the computer.', steps: true, form: true, retry: true, mood: 'lost' },
    unsupported: { badge: 'unsupported browser', title: "This browser can't pair", text: 'This browser cannot run the Octo encryption. Open the link in Safari or Chrome.', steps: true, form: false, retry: false, mood: 'lost' },
  };

  const $ = (id) => document.getElementById(id);
  const el = {
    back: $('back'), mascot: $('mascot'), host: $('host'), where: $('where'), settings: $('settings'), menu: $('menu'), link: $('link'),
    themeColor: $('theme-color'),
    overview: $('overview'), sessions: $('sessions'), offline: $('offline'), lost: $('lost'), needs: $('needs'), usage: $('usage'),
    modes: $('modes'), counts: $('counts'), list: $('list'), board: $('board'), tabs: $('tabs'), columns: $('columns'),
    agent: $('agent'), context: $('context'), screen: $('screen'), rows: $('rows'), cursor: $('cursor'), live: $('live'),
    approval: $('approval'), keys: $('keys'),
    pair: $('pair'), pairMascot: $('pair-mascot'), pairBadge: $('pair-badge'), pairTitle: $('pair-title'), pairText: $('pair-text'),
    pairSteps: $('pair-steps'), pairPaste: $('pair-paste'), pairForm: $('pair-form'), pairLink: $('pair-link'), retry: $('retry'),
    composer: $('composer'), form: $('form'), input: $('input'), picks: $('picks'), send: $('send'), sendLabel: $('send-label'),
    sheet: $('sheet'), sheetCard: $('sheet-card'), sheetGrab: $('sheet-grab'), sheetLead: $('sheet-lead'), sheetTitle: $('sheet-title'),
    sheetSub: $('sheet-sub'), sheetClose: $('sheet-close'), sheetBody: $('sheet-body'),
    toast: $('toast'),
  };

  const state = {
    secret: null,
    session: null,
    overview: null,
    usage: null,
    skew: 0,
    view: 'overview',
    link: 'connecting',
    filter: null,
    agent: null,
    screen: null,
    wanted: null,
    fullHeight: 0,
    width: 0,
    ratio: 0.6,
    font: TERM_FONT,
    shift: 0,
    who: null,
    where: null,
    worktree: false,
    bypass: false,
    theme: 'dark',
    layout: 'list',
    haptics: true,
    reduced: false,
    attempt: 0,
    retryTimer: 0,
    resizeTimer: 0,
    toastTimer: 0,
    sheetTimer: 0,
    searchTimer: 0,
    frame: 0,
    folders: null,
    folderSeq: 0,
    scroll: 0,
    scrollTimer: 0,
    touchY: null,
    previews: new Map(),
    peekKey: '',
    pending: null,
    approvalSig: '',
    answeredSig: '',
    flash: new Map(),
    enter: true,
    rings: new Map(),
    columns: new Map(),
    column: 0,
  };

  const storage = {
    get(key) {
      try {
        return window.localStorage.getItem(key);
      } catch (err) {
        return null;
      }
    },
    set(key, value) {
      try {
        if (value === null) window.localStorage.removeItem(key);
        else window.localStorage.setItem(key, value);
      } catch (err) {
        return;
      }
    },
  };

  function h(tag, props, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props || {})) {
      if (value === undefined || value === null || value === false) continue;
      if (key === 'class') node.className = value;
      else if (key === 'text') node.textContent = value;
      else if (key === 'dataset') Object.assign(node.dataset, value);
      else if (key === 'css') for (const [name, css] of Object.entries(value)) node.style.setProperty(name, css);
      else if (key.startsWith('on')) node.addEventListener(key.slice(2), value);
      else node.setAttribute(key, value === true ? '' : value);
    }
    for (const child of children.flat()) {
      if (child === null || child === undefined || child === false) continue;
      node.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
    return node;
  }

  function put(node, ...children) {
    node.replaceChildren(...children.flat().filter((child) => child !== null && child !== undefined && child !== false));
  }

  function icon(name) {
    const node = document.createElementNS(SVG_NS, 'svg');
    node.setAttribute('viewBox', '0 0 24 24');
    node.setAttribute('aria-hidden', 'true');
    node.innerHTML = ICON[name];
    return node;
  }

  function circle(radius, cls) {
    const node = document.createElementNS(SVG_NS, 'circle');
    node.setAttribute('cx', '28');
    node.setAttribute('cy', '28');
    node.setAttribute('r', String(radius));
    node.setAttribute('class', cls);
    return node;
  }

  const label = (kind) => LABELS[kind] || kind;
  const title = (kind) => NAMES[kind] || kind;
  const now = () => Date.now() - state.skew;
  const duration = M.duration;
  const moodOf = M.moodOf;

  function send(message) {
    if (state.session) state.session.send(message);
  }

  function buzz(pattern) {
    if (state.haptics && navigator.vibrate) navigator.vibrate(pattern);
  }

  function kai(mood) {
    return h('span', { class: 'kai', dataset: { mood }, 'aria-hidden': 'true' }, h('span', { class: 'kai-strip' }));
  }

  function toneIcon(tone) {
    return {
      warn: { text: '‖', cls: 'needs' },
      success: { text: '✓', cls: 'done' },
      error: { text: '!', cls: 'lost-fill' },
      info: { text: 'i', cls: 'idle' },
    }[tone] || { text: 'i', cls: 'idle' };
  }

  function hideToast() {
    if (el.toast.hidden) return;
    el.toast.classList.add('leaving');
    clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(() => {
      el.toast.hidden = true;
      el.toast.classList.remove('leaving');
    }, 180);
  }

  function toast(text, tone, options = {}) {
    const kind = tone || 'info';
    const mark = options.icon || toneIcon(kind);
    const ms = kind === 'error' ? TOAST_ERROR_MS : TOAST_MS;
    const action = options.action
      ? h('button', { class: 'toast-action press', type: 'button', onclick: () => {
        hideToast();
        options.action.run();
      } }, options.action.label)
      : null;
    put(el.toast, 
      h('div', { class: 'toast-body' },
        h('span', { class: `tile ${mark.cls}` }, mark.text),
        h('div', { class: 'toast-text' }, options.title ? h('b', { text: options.title }) : null, h('span', { text })),
        action),
      h('div', { class: 'toast-time' }),
    );
    el.toast.dataset.tone = kind;
    el.toast.style.setProperty('--toast-ms', `${ms}ms`);
    el.toast.classList.remove('leaving');
    el.toast.hidden = false;
    el.toast.style.animation = 'none';
    void el.toast.offsetWidth;
    el.toast.style.animation = '';
    clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(hideToast, ms);
  }

  function agents() {
    return state.overview ? state.overview.agents : [];
  }

  function findAgent(id) {
    return agents().find((agent) => agent.id === id) || null;
  }

  function inSession() {
    return agents().filter((agent) => agent.session === state.filter);
  }

  function alive(agent) {
    return Boolean(agent) && agent.state !== 'exited' && agent.state !== 'lost';
  }

  function waiting(agent) {
    return agent.state === 'paused' && !agent.starting;
  }

  function status(agent) {
    if (agent.starting) return { text: '◌', tile: 'starting', pill: 'idle', word: 'starting' };
    if (agent.state === 'lost') return { text: '↻', tile: 'lost', pill: 'lost', word: 'interrupted' };
    if (agent.state === 'exited') return { text: '✗', tile: 'idle', pill: 'idle', word: 'exited' };
    if (agent.state === 'paused') return { text: '‖', tile: 'needs', pill: 'needs', word: 'needs you' };
    if (agent.kind === 'terminal') return { text: '$', tile: 'terminal', pill: 'idle', word: agent.state === 'working' ? 'running' : 'open' };
    if (agent.state === 'working') return { text: M.SPINNER[state.frame % M.SPINNER.length], tile: 'working', pill: 'working', word: 'working', spin: true };
    if (agent.state === 'done') return { text: '✓', tile: 'done', pill: 'done', word: 'done' };
    return { text: '●', tile: 'idle', pill: 'idle', word: 'idle' };
  }

  function elapsed(agent) {
    if (agent.startedAt === null || !alive(agent)) return null;
    const at = now();
    if (agent.kind !== 'terminal') {
      if (agent.state === 'working') return duration(at - (agent.turnStarted || agent.stateSince));
      if (agent.state === 'paused') return duration(at - agent.stateSince);
      if (agent.lastTurn !== null) return duration(at - agent.stateSince);
    }
    return duration(at - agent.startedAt);
  }

  function doing(agent) {
    if (agent.starting) return { text: 'Starting…', tone: 't-faint' };
    if (agent.state === 'lost') return { text: 'Interrupted · recover it above', tone: 't-red' };
    if (agent.state === 'exited') return { text: agent.exit || 'exited', tone: 't-faint' };
    if (agent.state === 'paused') return { text: 'Waiting for your answer', tone: 't-yellow' };
    return { text: agent.activity || agent.task || '', tone: agent.state === 'working' ? 't-text' : 't-muted' };
  }

  function flashOf(agent) {
    const mark = state.flash.get(agent.id);
    return mark && Date.now() - mark.at < FLASH_MS ? `flash ${mark.tone}` : '';
  }

  function glyphNode(agent, cls) {
    const mark = status(agent);
    return h('span', { class: `${cls} ${mark.tile}`, dataset: mark.spin ? { spin: '1' } : null, text: mark.text });
  }

  function renderSessions() {
    const pills = state.overview.sessions.map((session) => {
      const members = agents().filter((agent) => agent.session === session.name);
      const lost = members.some((agent) => agent.state === 'lost');
      const asking = members.some(waiting);
      const on = session.name === state.filter;
      return h(
        'button',
        { class: `session press${on ? ' on' : ''}`, type: 'button', onclick: () => pickSession(session.name) },
        !on && (lost || asking) ? h('span', { class: `flag${lost ? '' : ' needs'}` }) : null,
        session.name,
        h('span', { class: 'count', text: String(session.agents) }),
      );
    });
    put(el.sessions, ...pills, h('button', { class: 'session add press', type: 'button', 'aria-label': 'new session', onclick: newSession }, icon('plus')));
  }

  function renderOffline() {
    const show = state.link === 'offline' && Boolean(state.overview);
    el.offline.hidden = !show;
    if (!show) return;
    put(el.offline, 
      h('div', { class: 'offline-card' },
        icon('offline'),
        h('div', { class: 'text' }, h('strong', { text: "Can't reach the computer" }), h('span', { text: 'Showing the last state it sent' })),
        h('button', { class: 'press', type: 'button', onclick: retryNow }, 'Retry')),
    );
  }

  function renderLost(list) {
    const lost = list.filter((agent) => agent.state === 'lost');
    el.lost.hidden = lost.length === 0;
    if (!lost.length) return;
    const at = now();
    put(el.lost, 
      h('div', { class: 'alert-card lost' },
        h('div', { class: 'recover-head' },
          h('span', { class: 'tile big lost-fill', text: '↻' }),
          h('div', {}, h('strong', { text: lost.length === 1 ? '1 agent was interrupted' : `${lost.length} agents were interrupted` }), h('span', { text: 'The computer stopped them while they ran' }))),
        h('div', { class: 'lost-list' }, lost.map((agent) => h('div', { class: 'lost-row' },
          h('b', { text: agent.name }),
          h('span', { class: `k-${agent.kind}`, text: label(agent.kind) }),
          h('span', { class: 'grow' }),
          agent.stateSince ? h('span', { class: 'ago', text: `stopped ${duration(at - agent.stateSince)} ago` }) : null))),
        h('button', { class: 'danger-fill press', type: 'button', onclick: () => send({ type: 'recover', session: state.filter }) },
          lost.length === 1 ? 'Recover it where it stopped' : 'Recover all where they stopped')),
    );
  }

  function renderNeeds(list) {
    const asking = list.filter(waiting);
    put(el.needs, ...asking.map((agent) => {
      const preview = state.previews.get(agent.id);
      const first = preview ? preview.options[0] : null;
      const question = (preview && preview.question) || agent.activity || 'is waiting for your answer';
      const actions = first
        ? [
          h('button', { class: 'answer press', type: 'button', onclick: () => answerFromOverview(agent, first) }, first.label),
          h('button', { class: 'answer-soft press', type: 'button', onclick: () => openAgent(agent.id) }, 'Open agent'),
        ]
        : [h('button', { class: 'answer press', type: 'button', onclick: () => openAgent(agent.id) }, 'Open agent')];
      return h('div', { class: `alert-card needs-you ${flashOf(agent)}` },
        h('div', { class: 'eyebrow' }, h('span', { class: 'dot' }), `NEEDS YOU · ${elapsed(agent) || ''}`),
        h('div', {},
          h('div', { class: 'alert-name', text: agent.name }),
          h('div', { class: 'alert-text' }, h('span', { class: `k-${agent.kind} mono`, text: `${label(agent.kind)} ` }), question)),
        h('div', { class: 'alert-actions' }, actions));
    }));
    requestPeek(asking);
  }

  function requestPeek(asking) {
    const key = asking.map((agent) => `${agent.id}:${agent.stateSince}`).join(',');
    if (key === state.peekKey) return;
    state.peekKey = key;
    for (const id of [...state.previews.keys()]) if (!asking.some((agent) => agent.id === id)) state.previews.delete(id);
    if (asking.length) send({ type: 'peek', agents: asking.map((agent) => agent.id), rows: PEEK_ROWS });
  }

  function ringArt(entry) {
    const box = h('div', { class: 'ring-art' });
    const art = document.createElementNS(SVG_NS, 'svg');
    art.setAttribute('viewBox', '0 0 56 56');
    art.setAttribute('aria-hidden', 'true');
    const fills = [];
    if (entry.windows.length) {
      entry.windows.slice(0, 2).forEach((_, index) => {
        const radius = index ? RING_INNER : RING_OUTER;
        const fill = circle(radius, 'fill');
        fill.style.strokeDasharray = M.arc(0, radius);
        art.append(circle(radius, 'track'), fill);
        fills.push({ node: fill, radius });
      });
    } else if (entry.credits !== null) {
      art.append(circle(RING_OUTER, 'funds'));
    } else {
      art.append(circle(RING_OUTER, 'dashed'));
    }
    box.append(art, h('span', { class: `ring-mark k-${entry.kind}`, text: entry.windows.length || entry.credits === null ? M.kindMark(entry.kind) : `$${Math.round(entry.credits)}` }));
    return { box, fills };
  }

  function paintRing(ring, entry) {
    ring.fills.forEach((fill, index) => {
      const used = entry.windows[index] ? entry.windows[index].used : 0;
      fill.node.style.stroke = `var(--color-${{ green: 'done', yellow: 'waiting', red: 'lost' }[M.usageTone(used)]})`;
      fill.node.style.strokeDasharray = M.arc(used, fill.radius);
    });
  }

  function ringValue(entry) {
    if (entry.windows.length) {
      const top = entry.windows[0];
      if (top.used >= M.USAGE_ALERT && top.resetsAt) return { text: `${Math.round(top.used)}% · ${duration(top.resetsAt - now())}`, tone: 't-red' };
      return { text: entry.windows.map((window) => Math.round(window.used)).slice(0, 2).join(' · ') + '%', tone: '' };
    }
    if (entry.credits !== null) return { text: 'funds', tone: '' };
    return { text: entry.problem || 'no limits', tone: 't-faint' };
  }

  function renderUsage() {
    const entries = state.usage || [];
    if (!entries.length || state.view !== 'overview') {
      if (!entries.length) put(el.usage);
      return;
    }
    const nodes = entries.map((entry) => {
      const shape = `${entry.kind}:${entry.windows.length}:${entry.credits === null}`;
      let ring = state.rings.get(entry.kind);
      if (!ring || ring.shape !== shape) {
        ring = { shape, ...ringArt(entry) };
        ring.node = h('div', { class: 'ring' }, ring.box, h('span', { class: 'ring-name', text: label(entry.kind) }), h('span', { class: 'ring-value' }));
        state.rings.set(entry.kind, ring);
      }
      const value = ringValue(entry);
      const valueNode = ring.node.querySelector('.ring-value');
      valueNode.textContent = value.text;
      valueNode.className = `ring-value ${value.tone}`;
      ring.node.classList.toggle('alert', entry.windows.some((window) => window.used >= M.USAGE_ALERT));
      requestAnimationFrame(() => paintRing(ring, entry));
      return ring.node;
    });
    let row = el.usage.querySelector('.rings-row');
    if (!row) {
      row = h('div', { class: 'rings-row' });
      put(el.usage, h('button', { class: 'rings-head', type: 'button', onclick: openUsage }, h('span', { text: 'PLAN USAGE' }), h('span', { text: 'details ›' })), row);
      row.addEventListener('click', openUsage);
    }
    put(row, ...nodes);
  }

  function renderModes() {
    if (el.modes.childElementCount) {
      for (const button of el.modes.querySelectorAll('button')) button.classList.toggle('on', button.dataset.mode === state.layout);
    } else {
      const button = (mode, text) => h('button', { class: `press${mode === state.layout ? ' on' : ''}`, type: 'button', role: 'tab', dataset: { mode }, onclick: () => setLayout(mode) }, icon(mode), text);
      put(el.modes, h('span', { class: 'thumb' }), button('list', 'List'), button('board', 'Board'));
    }
    requestAnimationFrame(() => {
      const on = el.modes.querySelector('button.on');
      const thumb = el.modes.querySelector('.thumb');
      if (!on || !thumb) return;
      thumb.style.width = `${on.offsetWidth}px`;
      thumb.style.transform = `translateX(${on.offsetLeft - 3}px)`;
    });
  }

  function renderCounts(list) {
    const count = (test) => list.filter(test).length;
    const parts = [
      { n: count((agent) => agent.state === 'working' || agent.starting), mark: '⠹', tone: 't-yellow' },
      { n: count(waiting), mark: '‖', tone: 't-yellow' },
      { n: count((agent) => agent.state === 'done'), mark: '✓', tone: 't-green' },
      { n: count((agent) => agent.state === 'lost'), mark: '↻', tone: 't-red' },
    ].filter((part) => part.n);
    put(el.counts, ...parts.map((part) => h('span', {}, h('b', { class: part.tone, text: part.mark }), String(part.n))));
  }

  function rowAgent(agent, index) {
    const mark = status(agent);
    const what = doing(agent);
    const time = elapsed(agent);
    const classes = ['row-agent', 'press', waiting(agent) ? 'waiting' : '', alive(agent) ? '' : 'faded', state.enter ? 'rise' : '', flashOf(agent)];
    return h(
      'button',
      { class: classes.filter(Boolean).join(' '), type: 'button', css: state.enter ? { 'animation-delay': `${index * 30}ms` } : null, onclick: () => openAgent(agent.id) },
      glyphNode(agent, 'tile'),
      h('span', { class: 'row-body' },
        h('span', { class: 'row-line' }, h('span', { class: 'row-name', text: agent.name }), h('span', { class: `row-time ${mark.pill === 'needs' ? 't-yellow' : ''}`, text: time || '' })),
        h('span', { class: 'row-line' },
          h('span', { class: `row-kind k-${agent.kind}`, text: label(agent.kind) }),
          h('span', { class: `row-doing ${what.tone}${agent.kind === 'terminal' ? ' mono' : ''}`, text: what.text }))),
    );
  }

  function emptyState() {
    return h('div', { class: 'empty rise' },
      kai('idle'),
      h('strong', { text: 'A quiet session' }),
      h('p', { text: `Nothing runs in ${state.filter} yet. Describe a task below and pick who does it.` }),
      h('div', { class: 'starters' },
        h('div', { class: 'label-caps', text: 'Try' }),
        STARTERS.map((text) => h('button', { class: 'starter press', type: 'button', onclick: () => fillTask(text) }, h('span', { text }), h('i', { text: '›' })))));
  }

  function renderList(list) {
    if (!list.length) {
      put(el.list, emptyState());
      return;
    }
    const groups = new Map();
    for (const agent of list) {
      if (!groups.has(agent.project)) groups.set(agent.project, []);
      groups.get(agent.project).push(agent);
    }
    let index = 0;
    const blocks = [];
    for (const [project, members] of groups) {
      blocks.push(h('div', { class: 'group' },
        h('div', { class: 'group-head' }, h('span', { class: 'swatch', css: { background: members[0].color } }), project, h('span', { class: 'count', text: String(members.length) })),
        h('div', { class: 'rows-card' }, members.map((agent) => rowAgent(agent, index++)))));
    }
    put(el.list, ...blocks);
  }

  function boardCard(agent, index) {
    const mark = status(agent);
    const time = elapsed(agent);
    const what = doing(agent);
    const text = waiting(agent)
      ? h('div', { class: 'card-text' }, h('span', { class: 'strong', text: 'Waiting for your answer' }), h('span', { class: 'soft', text: agent.activity }))
      : h('div', { class: `card-text ${what.tone}${agent.kind === 'terminal' ? ' mono' : ''}`, text: what.text });
    const subs = (agent.subagents || []).map((sub) => h('div', { text: `↳ ${sub.role} ${duration(now() - sub.startedAt)}${sub.activity ? ` · ${sub.activity}` : ''}` }));
    const changes = agent.changes && agent.changes.files ? agent.changes : null;
    const test = testMark(agent.test);
    const settled = agent.state === 'done' || agent.state === 'idle' || !alive(agent);
    const foot = agent.branch || changes || test
      ? h('div', { class: 'card-foot' },
        agent.branch ? h('span', { class: 'branch', text: `⎇ ${agent.branch}` }) : null,
        changes ? h('span', { class: 't-green', text: `+${changes.insertions}` }) : null,
        changes ? h('span', { class: 't-red', text: `−${changes.deletions}` }) : null,
        h('span', { class: 'grow' }),
        test ? h('span', { class: test.tone, text: test.text }) : null,
        changes && settled && agent.branch ? h('button', { class: 'mini press', type: 'button', onclick: (event) => {
          event.stopPropagation();
          send({ type: 'diff', agent: agent.id });
        } }, 'Diff') : null)
      : null;
    const classes = ['board-card', 'press', waiting(agent) ? 'waiting' : '', agent.starting ? 'starting' : '', state.enter ? 'rise' : '', flashOf(agent)];
    return h(
      'button',
      { class: classes.filter(Boolean).join(' '), type: 'button', css: state.enter ? { 'animation-delay': `${index * 30}ms` } : null, onclick: () => openAgent(agent.id) },
      h('div', { class: 'card-top' }, h('span', { class: `glyph t-${{ working: 'yellow', needs: 'yellow', done: 'green', lost: 'red', terminal: 'text' }[mark.tile] || 'faint'}`, dataset: mark.spin ? { spin: '1' } : null, text: mark.text }), h('span', { class: 'name', text: agent.name }), h('span', { class: 'time', text: time || '' })),
      h('div', { class: 'card-meta' }, h('span', { class: 'swatch', css: { background: agent.color } }), `${agent.project} ·`, h('span', { class: `k-${agent.kind}`, text: label(agent.kind) })),
      text,
      subs.length ? h('div', { class: 'card-subs' }, subs) : null,
      foot,
    );
  }

  function testMark(test) {
    return {
      passed: { text: '✓ tests', tone: 't-green' },
      failed: { text: '✗ tests', tone: 't-red' },
      running: { text: '⠋ tests', tone: 't-yellow' },
      untrusted: { text: '! tests', tone: 't-yellow' },
    }[test] || null;
  }

  function renderBoard(list) {
    const counts = M.COLUMNS.map((def) => list.filter((agent) => M.column(agent) === def.key));
    put(el.tabs, 
      ...M.COLUMNS.map((def, index) => h(
        'button',
        { class: `press${index === state.column ? ' on' : ''}${def.key === 'needs' && counts[index].length ? ' asking' : ''}`, type: 'button', onclick: () => scrollToColumn(index, true) },
        def.title,
        h('span', { class: 'count', text: String(counts[index].length) }),
      )),
      h('span', { class: 'indicator' }),
    );
    let index = 0;
    M.COLUMNS.forEach((def, position) => {
      let section = state.columns.get(def.key);
      if (!section) {
        section = h('section', { class: 'column', dataset: { column: def.key } });
        state.columns.set(def.key, section);
        el.columns.append(section);
      }
      const members = counts[position];
      put(section, ...(members.length ? members.map((agent) => boardCard(agent, index++)) : [h('div', { class: 'column-empty', text: def.empty })]));
    });
    requestAnimationFrame(placeIndicator);
  }

  function columnWidth() {
    const first = el.columns.firstElementChild;
    return first ? first.getBoundingClientRect().width + 10 : el.columns.clientWidth;
  }

  function scrollToColumn(index, smooth) {
    state.column = index;
    el.columns.scrollTo({ left: index * columnWidth(), behavior: smooth && !state.reduced ? 'smooth' : 'auto' });
    placeIndicator();
  }

  function placeIndicator() {
    const tabs = [...el.tabs.querySelectorAll('button')];
    tabs.forEach((tab, index) => tab.classList.toggle('on', index === state.column));
    const on = tabs[state.column];
    const line = el.tabs.querySelector('.indicator');
    if (!on || !line) return;
    line.style.width = `${on.offsetWidth}px`;
    line.style.transform = `translateX(${on.offsetLeft}px)`;
  }

  function onColumnsScroll() {
    const index = Math.round(el.columns.scrollLeft / columnWidth());
    if (index === state.column) return;
    state.column = Math.max(0, Math.min(M.COLUMNS.length - 1, index));
    placeIndicator();
  }

  function renderAgents() {
    if (!state.overview) return;
    const list = inSession();
    el.list.hidden = state.layout !== 'list';
    el.board.hidden = state.layout !== 'board';
    if (state.layout === 'board' && list.length) renderBoard(list);
    else if (state.layout === 'board') el.board.hidden = true;
    if (state.layout === 'list' || !list.length) {
      el.list.hidden = false;
      renderList(list);
    }
  }

  function renderSkeleton() {
    put(el.sessions, );
    put(el.needs, );
    put(el.counts, );
    el.lost.hidden = true;
    el.list.hidden = false;
    el.board.hidden = true;
    put(el.list, h('div', { class: 'skeleton' }, h('i'), h('i'), h('i')));
  }

  function renderOverview() {
    if (!state.overview) {
      renderSkeleton();
      renderHeader();
      return;
    }
    const list = inSession();
    renderSessions();
    renderOffline();
    renderLost(list);
    renderNeeds(list);
    renderUsage();
    renderModes();
    renderCounts(list);
    renderAgents();
    state.enter = false;
    el.mascot.dataset.mood = moodOf(agents());
    const asking = agents().filter(waiting).length;
    document.title = asking ? `(${asking}) Octo` : 'Octo';
    renderHeader();
  }

  function linkWord() {
    return {
      live: { text: 'live', tone: 't-green' },
      connecting: { text: 'connecting…', tone: 't-yellow' },
      offline: { text: 'offline · retrying', tone: 't-red' },
      unpaired: { text: 'not paired', tone: 't-red' },
      replaced: { text: 'in another tab', tone: 't-red' },
    }[state.link] || { text: state.link, tone: 't-faint' };
  }

  function renderHeader() {
    if (state.view === 'agent') {
      const agent = findAgent(state.agent);
      el.host.textContent = agent ? agent.name : `#${state.agent}`;
      if (!agent) {
        put(el.where, );
        return;
      }
      const mark = status(agent);
      const time = elapsed(agent);
      const model = M.shortModel(agent.model);
      put(el.where, 
        h('span', { class: `pill ${mark.pill}` }, h('span', { dataset: mark.spin ? { spin: '1' } : null, text: mark.text }), `${mark.word}${time ? ` ${time}` : ''}`),
        h('span', { class: `k-${agent.kind}`, text: model ? `${label(agent.kind)} · ${model}` : label(agent.kind) }),
      );
      el.mascot.dataset.mood = moodOf([agent]);
      return;
    }
    el.host.textContent = state.overview ? state.overview.host : 'Octo';
    const word = linkWord();
    put(el.where, 
      h('span', { class: word.tone, text: word.text }),
      state.overview ? h('span', { text: `· ${M.shortPath(currentWhere())}` }) : null,
    );
  }

  function renderContext(agent) {
    if (!agent) {
      put(el.context, );
      return;
    }
    const changes = agent.changes && agent.changes.files ? agent.changes : null;
    const test = testMark(agent.test);
    const subs = (agent.subagents || []).length;
    put(el.context, 
      agent.branch ? h('span', { class: 'chip branch', text: `⎇ ${agent.branch}` }) : null,
      changes ? h('span', { class: 'chip' }, h('span', { class: 't-green', text: `+${changes.insertions}` }), h('span', { class: 't-red', text: `−${changes.deletions}` })) : null,
      test ? h('span', { class: `chip ${test.tone}`, text: test.text }) : null,
      subs ? h('span', { class: 'chip', text: `↳ ${subs} subagent${subs === 1 ? '' : 's'}` }) : null,
    );
  }

  function installed() {
    return state.overview ? state.overview.kinds : [];
  }

  function currentWho() {
    const kinds = installed();
    if (state.who && kinds.includes(state.who)) return state.who;
    return kinds.includes('claude') ? 'claude' : kinds[0] || 'terminal';
  }

  function currentWhere() {
    if (state.where) return state.where;
    const overview = state.overview;
    if (!overview) return '~';
    return overview.dirs[state.filter] || overview.whereDir || '~';
  }

  function composing() {
    return state.view === 'overview' && (document.activeElement === el.input || el.input.value.trim() !== '');
  }

  function renderPicks() {
    const open = composing();
    el.form.classList.toggle('open', open);
    if (state.view !== 'overview' || !state.overview) {
      put(el.picks, );
      el.sendLabel.textContent = '';
      return;
    }
    const who = currentWho();
    const picks = [
      h('button', { class: `pick pick-who k-${who} bg-${who} press`, type: 'button', onclick: pickWho }, `${M.kindMark(who)} ${label(who)}`, icon('chevron')),
      h('button', { class: 'pick press', type: 'button', onclick: pickFolder }, icon('folder'), M.shortPath(currentWhere())),
    ];
    if (who !== 'terminal') {
      picks.push(h('button', { class: `toggle press${state.worktree ? ' on' : ''}`, type: 'button', 'aria-label': 'own worktree', 'aria-pressed': String(state.worktree), onclick: toggleWorktree }, icon('branch'), h('span', { class: 'label', text: 'Worktree' })));
      picks.push(h('button', { class: `toggle bypass press${state.bypass ? ' on' : ''}`, type: 'button', 'aria-label': 'bypass permissions', 'aria-pressed': String(state.bypass), onclick: toggleBypass }, icon('bolt'), h('span', { class: 'label', text: 'Bypass' })));
    }
    put(el.picks, ...picks);
    el.sendLabel.textContent = open ? `Launch ${label(who)}` : '';
  }

  function updateComposer() {
    if (state.view === 'agent') {
      const agent = findAgent(state.agent);
      el.input.placeholder = alive(agent) ? `Message ${agent.name}…` : 'this agent is not running';
    } else {
      const who = currentWho();
      el.input.placeholder = who === 'terminal' ? 'Open a terminal, or type a command…' : `Ask ${label(who)} to…`;
    }
    renderPicks();
  }

  function show(view) {
    state.view = view;
    document.body.dataset.view = view;
    el.overview.hidden = view !== 'overview';
    el.agent.hidden = view !== 'agent';
    el.pair.hidden = view !== 'pair';
    el.composer.hidden = view === 'pair';
    el.back.hidden = view !== 'agent';
    el.menu.hidden = view !== 'agent';
    el.settings.hidden = view !== 'overview';
    updateComposer();
    renderHeader();
  }

  function showPair(kind) {
    const preset = PAIR[kind] || PAIR.new;
    el.pairTitle.textContent = preset.title;
    el.pairText.textContent = preset.text;
    el.pairBadge.hidden = !preset.badge;
    el.pairBadge.textContent = preset.badge || '';
    el.pair.classList.toggle('error', Boolean(preset.badge));
    el.pairSteps.hidden = !preset.steps;
    el.pairForm.hidden = !preset.form;
    el.pairPaste.hidden = !preset.form;
    el.retry.hidden = !preset.retry;
    el.retry.textContent = preset.again || 'Try again';
    const art = el.pairMascot.querySelector('.kai');
    if (art) art.dataset.mood = preset.mood;
    show('pair');
  }

  function pickSession(name) {
    if (name === state.filter) return;
    state.filter = name;
    state.where = null;
    state.enter = true;
    renderOverview();
    updateComposer();
  }

  function setLayout(mode) {
    if (mode === state.layout) return;
    state.layout = mode;
    storage.set(LAYOUT_KEY, mode === 'list' ? null : mode);
    state.enter = true;
    renderModes();
    renderAgents();
    state.enter = false;
    if (mode === 'board') {
      const list = inSession();
      const first = M.COLUMNS.findIndex((def) => list.some((agent) => M.column(agent) === def.key));
      requestAnimationFrame(() => scrollToColumn(Math.max(0, first), false));
    }
  }

  function openSheet(options, build) {
    clearTimeout(state.sheetTimer);
    el.sheetTitle.textContent = options.title;
    el.sheetSub.textContent = options.sub || '';
    put(el.sheetLead, ...(options.lead ? [options.lead] : []));
    el.sheet.classList.toggle('tall', Boolean(options.tall));
    el.sheet.classList.toggle('agent-head', Boolean(options.agentHead));
    put(el.sheetBody, );
    el.sheetCard.style.transform = '';
    state.folders = null;
    build(el.sheetBody);
    if (el.sheet.hidden) {
      el.sheet.hidden = false;
      requestAnimationFrame(() => requestAnimationFrame(() => el.sheet.classList.add('open')));
    } else {
      el.sheet.classList.add('open');
    }
  }

  function closeSheet() {
    if (el.sheet.hidden) return;
    el.sheet.classList.remove('open');
    el.sheetCard.style.transform = '';
    state.folders = null;
    clearTimeout(state.sheetTimer);
    state.sheetTimer = setTimeout(() => {
      el.sheet.hidden = true;
      put(el.sheetBody, );
    }, state.reduced ? 120 : SHEET_MS);
  }

  function option(spec) {
    return h(
      'button',
      { class: `option press${spec.tone ? ` ${spec.tone}` : ''}`, type: 'button', onclick: spec.onclick },
      spec.icon ? h('span', { class: `icon ${spec.iconTone || ''}` }, spec.icon) : null,
      h('span', { class: 'label' }, h('span', { text: spec.text }), spec.detail ? h('small', { text: spec.detail }) : null),
      spec.trail || null,
      spec.chevron ? h('span', { class: 'chev', text: '›' }) : null,
    );
  }

  function armedOption(spec) {
    const fill = h('span', { class: 'fill' });
    const countdown = h('span', { class: 'countdown' });
    let timer = 0;
    const node = option({
      ...spec,
      tone: spec.tone || 'danger',
      onclick: () => {
        if (node.classList.contains('armed')) {
          clearTimeout(timer);
          spec.action();
          closeSheet();
          return;
        }
        node.classList.add('armed');
        node.querySelector('.label span').textContent = spec.armed;
        const art = document.createElementNS(SVG_NS, 'svg');
        art.setAttribute('viewBox', '0 0 28 28');
        art.innerHTML = '<circle class="track" cx="14" cy="14" r="11"/><circle class="run" cx="14" cy="14" r="11"/>';
        put(countdown, art);
        buzz([20, 40, 20]);
        timer = setTimeout(() => {
          node.classList.remove('armed');
          node.querySelector('.label span').textContent = spec.text;
        }, CONFIRM_MS);
      },
    });
    node.prepend(fill);
    node.append(countdown);
    return node;
  }

  function group(...items) {
    const nodes = items.flat().filter(Boolean);
    return nodes.length ? h('div', { class: 'group-card' }, nodes) : null;
  }

  function promptSheet(heading, placeholder, value, done) {
    openSheet({ title: heading }, (body) => {
      const input = h('input', { type: 'text', placeholder, autocapitalize: 'off', autocorrect: 'off', spellcheck: 'false', enterkeyhint: 'done' });
      input.value = value || '';
      const go = () => {
        const text = input.value.trim();
        if (!text) return;
        done(text);
        closeSheet();
      };
      input.addEventListener('keydown', (event) => {
        if (event.key === 'Enter') {
          event.preventDefault();
          go();
        }
      });
      body.append(h('div', { class: 'field' }, input, h('button', { class: 'primary press', type: 'button', onclick: go }, 'OK')));
      setTimeout(() => input.focus(), 80);
    });
  }

  function newSession() {
    promptSheet('New session', 'name', '', (name) => {
      send({ type: 'session', name });
      state.filter = name;
      state.where = null;
      state.enter = true;
    });
  }

  function usageLine(entry) {
    if (entry.windows.length) {
      const top = entry.windows[0];
      if (top.used >= M.USAGE_ALERT && top.resetsAt) return { text: `${Math.round(top.used)}% · resets ${duration(top.resetsAt - now())}`, tone: 't-red' };
      return { text: `${Math.round(top.used)}% of ${top.label}`, tone: '' };
    }
    if (entry.credits !== null) return { text: `$${entry.credits.toFixed(2)} in funds`, tone: '' };
    return { text: entry.problem || 'no plan limits', tone: 't-faint' };
  }

  function pickWho() {
    const kinds = installed();
    const usage = new Map((state.usage || []).map((entry) => [entry.kind, entry]));
    const who = currentWho();
    const choose = (kind) => {
      state.who = kind;
      storage.set(WHO_KEY, kind);
      closeSheet();
      updateComposer();
    };
    openSheet({ title: 'Who runs it', sub: usage.size ? 'Rings show how much of each plan is used' : '' }, (body) => {
      const tiles = kinds.filter((kind) => usage.has(kind)).map((kind) => {
        const entry = usage.get(kind);
        const ring = ringArt(entry);
        requestAnimationFrame(() => paintRing(ring, entry));
        const line = usageLine(entry);
        return h('button', { class: `agent-tile press${kind === who ? ' on' : ''}`, type: 'button', onclick: () => choose(kind) },
          h('span', { class: 'tile-top' }, ring.box, kind === who ? h('span', { class: 'check' }, icon('check')) : null),
          h('span', {}, h('b', { text: title(kind) }), h('small', { class: line.tone, text: line.text })));
      });
      const rows = kinds.filter((kind) => !usage.has(kind)).map((kind) => option({
        icon: h('span', { class: `k-${kind} mono`, text: M.kindMark(kind) }),
        iconTone: `bg-${kind}`,
        text: title(kind),
        detail: kind === 'terminal' ? 'a shell in the folder' : null,
        tone: kind === who ? 'current' : '',
        trail: kind === who ? h('span', { class: 'trail t-link' }, icon('check')) : null,
        onclick: () => choose(kind),
      }));
      if (tiles.length) body.append(h('div', { class: 'tiles' }, tiles));
      const rest = group(rows);
      if (rest) body.append(rest);
    });
  }

  function toggleWorktree() {
    state.worktree = !state.worktree;
    storage.set(WORKTREE_KEY, state.worktree ? '1' : '0');
    toast(state.worktree ? 'New agents get their own git worktree' : 'New agents run directly in the folder', 'info', { icon: { text: '⎇', cls: 'idle' } });
    renderPicks();
  }

  function toggleBypass() {
    state.bypass = !state.bypass;
    storage.set(BYPASS_KEY, state.bypass ? '1' : '0');
    if (state.bypass) toast('Bypass on: new agents skip every permission check', 'warn', { icon: { text: '⚡', cls: 'working' } });
    else toast('New agents ask before running tools', 'info');
    renderPicks();
  }

  function pathAt(path, index) {
    const parts = M.crumbs(path);
    const windows = /^[A-Za-z]:/.test(parts[0] || '');
    const sep = windows ? '\\' : '/';
    const joined = parts.slice(0, index + 1).join(sep);
    if (windows) return index === 0 ? `${joined}\\` : joined;
    return `/${joined}`;
  }

  function pickFolder() {
    openSheet({ title: 'Where', tall: true }, (body) => {
      let current = currentWhere();
      let query = '';
      const crumbs = h('div', { class: 'crumbs' });
      const list = h('div', { class: 'group-card' });
      const use = h('button', { class: 'primary use-folder press', type: 'button', onclick: () => {
        state.where = current;
        closeSheet();
        updateComposer();
        renderHeader();
      } });
      const input = h('input', { type: 'search', placeholder: 'Search folders', autocapitalize: 'off', autocorrect: 'off', spellcheck: 'false', enterkeyhint: 'search' });
      const recent = [...new Map(agents().slice().reverse().map((agent) => [agent.cwd, agent])).values()].slice(0, RECENT_MAX);
      const paint = () => {
        const parts = M.crumbs(current);
        put(crumbs, ...parts.flatMap((part, index) => [
          index ? h('span', { text: '›' }) : null,
          index === parts.length - 1 ? h('span', { class: 'here', text: part }) : h('button', { type: 'button', onclick: () => browse(pathAt(current, index)) }, part),
        ]));
        put(use, 'Use ', h('span', { text: M.shortPath(current) }));
      };
      const browse = (target) => {
        query = '';
        input.value = '';
        current = target;
        paint();
        state.folderSeq += 1;
        send({ type: 'folders', path: target, seq: state.folderSeq });
      };
      const folderRow = (dir, detail) => {
        const row = option({ icon: icon('folder'), text: dir.name, detail, trail: dir.git ? h('span', { class: 'git', text: 'git' }) : null, chevron: true, onclick: () => browse(dir.path) });
        row.classList.add('folder');
        return row;
      };
      state.folders = (message) => {
        if (message.seq && message.seq !== state.folderSeq) return;
        if (message.type === 'folderSearch') {
          if (!query) return;
          put(list, ...(message.hits.length
            ? message.hits.map((hit) => folderRow(hit, hit.rel))
            : [h('p', { class: 'note', text: 'No folder matches that' })]));
          return;
        }
        current = message.path;
        paint();
        const items = [];
        if (message.parent) items.push(option({ icon: icon('up'), text: `Up to ${M.crumbs(message.parent).pop() || message.parent}`, onclick: () => browse(message.parent) }));
        for (const dir of message.dirs) items.push(folderRow(dir));
        put(list, ...(items.length ? items : [h('p', { class: 'note', text: 'No folders inside' })]));
      };
      input.addEventListener('input', () => {
        query = input.value.trim();
        clearTimeout(state.searchTimer);
        if (!query) {
          browse(current);
          return;
        }
        state.searchTimer = setTimeout(() => {
          state.folderSeq += 1;
          send({ type: 'folderSearch', root: current, query, seq: state.folderSeq });
        }, SEARCH_DELAY);
      });
      body.append(h('label', { class: 'search' }, icon('search'), input), crumbs);
      if (recent.length) {
        body.append(h('div', { class: 'label-caps', text: 'Recent' }), h('div', { class: 'recent' }, recent.map((agent) => h('button', { class: 'press', type: 'button', onclick: () => browse(agent.cwd) },
          h('span', { class: 'swatch', css: { background: agent.color } }),
          h('b', { text: M.crumbs(agent.cwd).pop() || agent.cwd }),
          h('small', { text: M.shortPath(agent.cwd) })))));
      }
      body.append(list, h('div', { class: 'sheet-foot' }, use));
      browse(current);
    });
  }

  function openUsage() {
    const entries = state.usage || [];
    openSheet({ title: 'Plan usage' }, (body) => {
      const minis = [];
      for (const entry of entries) {
        if (!entry.windows.length) {
          minis.push(h('div', { class: 'usage-mini' },
            h('b', { class: `k-${entry.kind}`, text: title(entry.kind) }),
            h('strong', { text: entry.credits !== null ? `$${entry.credits.toFixed(2)}` : '∞' }),
            h('small', { text: entry.credits !== null ? 'funds left' : entry.problem || 'no plan limits' })));
          continue;
        }
        const ring = ringArt(entry);
        requestAnimationFrame(() => paintRing(ring, entry));
        const alert = entry.windows.some((window) => window.used >= M.USAGE_ALERT);
        if (entry.windows.length === 1) {
          put(ring.box.querySelector('.ring-mark'), h('span', { class: 'big-pct', text: `${Math.round(entry.windows[0].used)}%` }), h('span', { class: `k-${entry.kind}`, text: `${M.kindMark(entry.kind)} ${entry.windows[0].label}` }));
        }
        const lines = entry.windows.map((window) => h('div', { class: 'window-line' },
          h('div', { class: 'top-line' },
            h('i', { css: { background: `var(--color-${{ green: 'done', yellow: 'waiting', red: 'lost' }[M.usageTone(window.used)]})` } }),
            h('span', { text: window.label }),
            h('b', { text: `${Math.round(window.used)}%` })),
          window.resetsAt ? h('small', { text: window.resetsAt > now() ? `resets in ${duration(window.resetsAt - now())}` : 'resets now' }) : null));
        body.append(h('div', { class: `usage-card${alert ? ' alert' : ''}` }, ring.box, h('div', { class: 'detail' }, h('b', { class: `k-${entry.kind}`, text: title(entry.kind) }), lines)));
      }
      if (minis.length) body.append(h('div', { class: 'usage-pair' }, minis));
      if (!entries.length) body.append(h('p', { class: 'note', text: 'No plan usage yet' }));
    });
  }

  function openAppearance() {
    openSheet({ title: 'Appearance', sub: 'Same themes as Octo on the computer. Terminals follow.' }, (body) => {
      const themes = h('div', { class: 'themes' });
      const paintThemes = () => put(themes, ...M.THEMES.map((theme) => h(
        'button',
        { class: `theme-card press${theme === state.theme ? ' on' : ''}`, type: 'button', onclick: () => {
          setTheme(theme);
          paintThemes();
        } },
        h('span', { class: `theme-art ${theme}` }, theme === 'system' ? null : [h('i', { class: 'title' }), h('i', { class: 'alert' }), h('i', { class: 'row' }), h('i', { class: 'row' }), h('i', { class: 'fab' })]),
        theme.charAt(0).toUpperCase() + theme.slice(1),
      )));
      paintThemes();
      const seg = h('div', { class: 'mini-seg' });
      const paintSeg = () => put(seg, ...['list', 'board'].map((mode) => h('button', { class: mode === state.layout ? 'on' : '', type: 'button', onclick: () => {
        setLayout(mode);
        paintSeg();
      } }, mode === 'list' ? 'List' : 'Board')));
      paintSeg();
      const toggle = (on, flip) => {
        const node = h('button', { class: `switch${on() ? ' on' : ''}`, type: 'button', role: 'switch', 'aria-checked': String(on()), onclick: () => {
          flip();
          node.classList.toggle('on', on());
          node.setAttribute('aria-checked', String(on()));
        } });
        return node;
      };
      body.append(
        themes,
        h('div', { class: 'group-card' },
          h('div', { class: 'setting' }, h('span', { class: 'label', text: 'Open agents as' }), seg),
          h('div', { class: 'setting' }, h('span', { class: 'label' }, 'Haptics', h('small', { text: 'Buzz when an agent needs you or finishes' })), toggle(() => state.haptics, () => {
            state.haptics = !state.haptics;
            storage.set(HAPTICS_KEY, state.haptics ? null : '0');
            buzz(15);
          })),
          h('div', { class: 'setting' }, h('span', { class: 'label', text: 'Reduce motion' }), toggle(() => state.reduced, () => {
            state.reduced = !state.reduced;
            storage.set(MOTION_KEY, state.reduced ? 'reduce' : null);
            applyMotion();
          })),
          h('div', { class: 'setting' }, h('span', { class: 'label', text: 'Paired with' }), h('span', { class: 'value', text: state.overview ? state.overview.host : '—' }))),
      );
    });
  }

  function agentMenu() {
    const agent = findAgent(state.agent);
    if (!agent) return;
    const mark = status(agent);
    const time = elapsed(agent);
    const changes = agent.changes && agent.changes.files ? agent.changes : null;
    openSheet({ title: agent.name, sub: `${label(agent.kind)} · ${mark.word}${time ? ` ${time}` : ''}`, lead: kai(moodOf([agent])), agentHead: true }, (body) => {
      const actions = group(
        option({ icon: icon('pencil'), text: 'Rename', chevron: true, onclick: () => promptSheet('Rename', 'new name', agent.name, (name) => send({ type: 'rename', agent: agent.id, name })) }),
        agent.branch ? option({
          icon: icon('diff'),
          iconTone: 'link',
          text: 'View changes',
          trail: changes ? h('span', { class: 'trail' }, h('span', { class: 't-green', text: `+${changes.insertions} ` }), h('span', { class: 't-red', text: `−${changes.deletions}` })) : null,
          chevron: true,
          onclick: () => {
            closeSheet();
            send({ type: 'diff', agent: agent.id });
          },
        }) : null,
        agent.branch ? armedOption({ icon: icon('merge'), iconTone: 'accent', tone: 'confirm', text: `Merge ${agent.branch}`, armed: 'Tap again to merge', action: () => send({ type: 'merge', agent: agent.id }) }) : null,
        agent.state === 'lost' ? option({ icon: icon('recover'), iconTone: 'green', text: 'Recover', detail: 'start it again where it stopped', onclick: () => {
          closeSheet();
          send({ type: 'recover', session: agent.session });
        } }) : null,
      );
      const danger = alive(agent)
        ? armedOption({ icon: h('span', { class: 'stop-square' }), iconTone: 'red', text: 'Stop agent', armed: 'Tap again to stop', action: () => send({ type: 'stop', agent: agent.id }) })
        : armedOption({ icon: icon('trash'), iconTone: 'red', text: `Remove ${agent.name}`, armed: 'Tap again to remove', action: () => {
          send({ type: 'remove', agent: agent.id });
          back();
        } });
      body.append(...[actions, group(danger)].filter(Boolean));
    });
  }

  function showDiff(message) {
    const files = M.diffFiles(message.text);
    const totals = M.diffTotals(files);
    openSheet({ title: 'Changes', sub: '', tall: true }, (body) => {
      body.append(h('div', { class: 'diff-head' },
        h('span', { class: 'branch', text: `⎇ ${message.branch}` }),
        h('span', { class: 'stats' },
          `${totals.files} file${totals.files === 1 ? '' : 's'}`,
          h('b', { class: 't-green', text: `+${totals.insertions}` }),
          h('b', { class: 't-red', text: `−${totals.deletions}` }))));
      if (totals.insertions + totals.deletions) {
        body.append(h('div', { class: 'change-bar' }, h('span', { class: 'add', css: { flex: String(totals.insertions) } }), h('span', { class: 'del', css: { flex: String(totals.deletions) } })));
      }
      if (!files.length) {
        body.append(h('p', { class: 'note', text: 'No changes yet' }));
        return;
      }
      let budget = DIFF_LINES;
      for (const file of files) {
        if (budget <= 0) break;
        const section = h('div', { class: 'diff-file' },
          h('div', { class: 'diff-file-head' }, h('b', { text: file.name }), h('span', { class: 't-green', text: `+${file.insertions}` }), h('span', { class: 't-red', text: `−${file.deletions}` })));
        for (const hunk of file.hunks) {
          if (budget <= 0) break;
          const lines = hunk.lines.slice(0, budget);
          budget -= lines.length;
          section.append(
            h('div', { class: 'diff-hunk', text: `@@ ${hunk.title}` }),
            h('div', { class: 'diff-lines' }, lines.map((line) => h('div', { class: `diff-line ${line.kind}` },
              h('span', { class: 'num', text: line.kind === 'ctx' ? String(line.number) : line.kind === 'add' ? '+' : '−' }),
              line.text))));
        }
        body.append(section);
      }
      if (message.truncated || budget <= 0) body.append(h('p', { class: 'note', text: 'The diff was cut; see the rest on the computer' }));
      const agent = findAgent(message.agent);
      let armed = false;
      const merge = h('button', { class: 'primary press', type: 'button', onclick: () => {
        if (armed) {
          send({ type: 'merge', agent: message.agent });
          closeSheet();
          return;
        }
        armed = true;
        merge.textContent = 'Tap again to merge';
        buzz([20, 40, 20]);
        setTimeout(() => {
          armed = false;
          merge.textContent = `Merge ${message.branch}`;
        }, CONFIRM_MS);
      } }, `Merge ${message.branch}`);
      const copy = h('button', { class: 'round press', type: 'button', 'aria-label': 'copy the diff', onclick: () => {
        if (!navigator.clipboard) return;
        navigator.clipboard.writeText(message.text).then(() => toast('Diff copied', 'success'), () => toast('Could not copy the diff', 'error'));
      } }, icon('copy'));
      body.append(h('div', { class: 'sheet-foot diff-actions' }, agent && agent.branch ? merge : null, copy));
    });
  }

  function measure() {
    const probe = h('span', { text: 'M'.repeat(50) });
    probe.style.font = `100px ${getComputedStyle(el.screen).fontFamily}`;
    probe.style.position = 'absolute';
    probe.style.visibility = 'hidden';
    probe.style.whiteSpace = 'pre';
    document.body.append(probe);
    const width = probe.getBoundingClientRect().width;
    probe.remove();
    if (width > 0) state.ratio = width / 5000;
  }

  function phoneSize() {
    const width = el.screen.clientWidth - PAD * 2;
    const height = el.screen.clientHeight - PAD;
    if (width !== state.width) {
      state.width = width;
      state.fullHeight = 0;
    }
    state.fullHeight = Math.max(state.fullHeight, height);
    const cols = Math.floor(width / (TERM_FONT * state.ratio));
    const rows = Math.floor(state.fullHeight / (TERM_FONT * LINE));
    return { rows: Math.max(8, rows), cols: Math.max(20, cols) };
  }

  function fitFont() {
    if (!state.screen) return;
    const width = el.screen.clientWidth - PAD * 2;
    const fit = width / (state.screen.cols * state.ratio);
    const font = Math.max(TERM_MIN, Math.min(TERM_MAX, Math.floor(fit * 4) / 4));
    if (font !== state.font) {
      state.font = font;
      el.screen.style.fontSize = `${font}px`;
    }
  }

  function anchor() {
    const shift = Math.max(0, Math.round(el.rows.offsetHeight + PAD - el.screen.clientHeight));
    state.shift = shift;
    el.rows.style.transform = shift ? `translateY(${-shift}px)` : '';
  }

  function placeCursor() {
    const screen = state.screen;
    anchor();
    if (!screen || !screen.cursor || screen.scroll) {
      el.cursor.hidden = true;
      return;
    }
    const [row, col] = screen.cursor;
    el.cursor.hidden = false;
    el.cursor.style.transform = `translate(${PAD + col * state.font * state.ratio}px, ${PAD + row * state.font * LINE - state.shift}px)`;
  }

  function palette(index) {
    if (index < 16) return `var(--ansi-${index})`;
    if (index < 232) {
      const value = index - 16;
      const level = (step) => (step === 0 ? 0 : 55 + step * 40);
      return `rgb(${level(Math.floor(value / 36))},${level(Math.floor(value / 6) % 6)},${level(value % 6)})`;
    }
    const gray = 8 + (index - 232) * 10;
    return `rgb(${gray},${gray},${gray})`;
  }

  function color(value) {
    if (value >= TRUECOLOR) {
      const rgb = value - TRUECOLOR;
      return `rgb(${(rgb >> 16) & 255},${(rgb >> 8) & 255},${rgb & 255})`;
    }
    return palette(value);
  }

  function run([text, fg, bg, flags]) {
    if (fg === DEFAULT && bg === DEFAULT && !flags) return document.createTextNode(text);
    const node = document.createElement('span');
    node.textContent = text;
    let front = fg === DEFAULT ? null : color(fg);
    let back = bg === DEFAULT ? null : color(bg);
    if (flags & INVERSE) [front, back] = [back || TERM_BG, front || TERM_FG];
    if (front) node.style.color = front;
    if (back) node.style.backgroundColor = back;
    if (flags & BOLD) node.style.fontWeight = '700';
    if (flags & DIM) node.style.opacity = '0.55';
    if (flags & ITALIC) node.style.fontStyle = 'italic';
    if (flags & UNDERLINE) node.style.textDecoration = 'underline';
    return node;
  }

  function screenLines() {
    return [...el.rows.children].map((row) => row.textContent);
  }

  function renderApproval() {
    const agent = findAgent(state.agent);
    const asked = agent && waiting(agent) && state.screen && !state.screen.scroll ? M.choices(screenLines()) : null;
    const sig = asked ? JSON.stringify(asked) : '';
    if (!asked || sig === state.answeredSig) {
      if (!el.approval.hidden) {
        el.approval.hidden = true;
        el.keys.hidden = false;
        requestAnimationFrame(placeCursor);
      }
      state.approvalSig = '';
      if (!asked) state.answeredSig = '';
      return;
    }
    if (sig === state.approvalSig) return;
    state.approvalSig = sig;
    const [first, ...rest] = asked.options;
    el.approval.classList.remove('answered');
    put(el.approval, 
      h('div', { class: 'eyebrow' }, h('span', { class: 'dot' }), 'NEEDS YOU'),
      h('div', { class: 'approval-question', text: asked.question || agent.activity || 'Pick an answer' }),
      asked.context.length ? h('pre', { class: 'approval-context', text: asked.context.join('\n') }) : null,
      h('div', { class: 'approval-options' },
        h('button', { class: 'first press', type: 'button', onclick: () => answer(first.key, sig) }, first.label),
        rest.length ? h('div', { class: 'rest' }, rest.map((choice) => h('button', { class: `press${choice.danger ? ' danger' : ''}`, type: 'button', onclick: () => answer(choice.key, sig) }, choice.label))) : null),
      h('div', { class: 'approval-hint', text: `or type below to tell ${label(agent.kind)} what to do instead` }),
    );
    el.approval.hidden = false;
    el.keys.hidden = true;
    requestAnimationFrame(placeCursor);
  }

  function answer(key, sig) {
    send({ type: 'keys', data: key });
    buzz(12);
    state.answeredSig = sig;
    el.approval.classList.add('answered');
    setTimeout(() => {
      if (state.answeredSig === sig) {
        el.approval.hidden = true;
        el.keys.hidden = false;
        requestAnimationFrame(placeCursor);
      }
    }, state.reduced ? 0 : 240);
  }

  function answerFromOverview(agent, choice) {
    state.pending = { agent: agent.id, key: choice.key, label: choice.label, at: Date.now() };
    openAgent(agent.id);
  }

  function settlePending() {
    const pending = state.pending;
    if (!pending || pending.agent !== state.agent) return;
    state.pending = null;
    if (Date.now() - pending.at > PENDING_MS) return;
    const asked = M.choices(screenLines());
    const same = asked && asked.options.find((choice) => choice.key === pending.key && choice.label === pending.label);
    if (!same) {
      toast('The question changed. Answer it on the screen.', 'warn');
      return;
    }
    answer(pending.key, JSON.stringify(asked));
  }

  function applyScreen(message) {
    if (message.agent !== state.agent) return;
    if (message.full || !state.screen || el.rows.children.length !== message.rows) {
      put(el.rows, ...Array.from({ length: message.rows }, () => h('div', { class: 'row' })));
    }
    state.screen = { rows: message.rows, cols: message.cols, cursor: message.cursor, scroll: message.scroll };
    for (const [index, runs] of message.lines) {
      const row = el.rows.children[index];
      if (row) put(row, ...runs.map(run));
    }
    fitFont();
    placeCursor();
    el.live.hidden = message.scroll === 0;
    settlePending();
    renderApproval();
  }

  function openAgent(id, again) {
    if (!again) {
      if (state.view !== 'agent') window.history.pushState({ agent: id }, '');
      state.agent = id;
      state.screen = null;
      state.approvalSig = '';
      state.answeredSig = '';
      put(el.rows, );
      el.cursor.hidden = true;
      el.live.hidden = true;
      el.approval.hidden = true;
      el.keys.hidden = false;
      show('agent');
      renderContext(findAgent(id));
    }
    state.wanted = phoneSize();
    send({ type: 'open', agent: id, rows: state.wanted.rows, cols: state.wanted.cols });
  }

  function leaveAgent() {
    if (state.view !== 'agent') return;
    send({ type: 'close' });
    state.agent = null;
    state.screen = null;
    put(el.rows, );
    show('overview');
    state.enter = true;
    renderOverview();
  }

  function back() {
    if (window.history.state && window.history.state.agent !== undefined) window.history.back();
    else leaveAgent();
  }

  function queueScroll(lines) {
    state.scroll += lines;
    if (state.scrollTimer) return;
    state.scrollTimer = setTimeout(() => {
      const total = state.scroll;
      state.scroll = 0;
      state.scrollTimer = 0;
      if (total) send({ type: 'scroll', lines: total });
    }, 40);
  }

  function grow() {
    el.input.style.height = 'auto';
    const height = el.input.scrollHeight;
    el.input.style.height = `${Math.min(height, INPUT_MAX)}px`;
    el.input.style.overflowY = height > INPUT_MAX ? 'auto' : 'hidden';
    renderPicks();
  }

  function fillTask(text) {
    el.input.value = text;
    grow();
    el.input.focus();
  }

  function submit(event) {
    event.preventDefault();
    const text = el.input.value.trim();
    if (state.view === 'agent') {
      if (text) send({ type: 'send', text: el.input.value });
      else send({ type: 'keys', data: '\r' });
    } else {
      const who = currentWho();
      if (!text && who !== 'terminal') {
        toast('Write the task first', 'info', { icon: { text: '✎', cls: 'idle' } });
        return;
      }
      const ai = who !== 'terminal';
      send({ type: 'start', kind: who, session: state.filter, cwd: currentWhere(), task: text, worktree: state.worktree && ai, bypass: state.bypass && ai });
      el.input.blur();
    }
    el.input.value = '';
    grow();
  }

  function attention(previous, next) {
    if (!previous) return;
    const before = new Map(previous.agents.map((agent) => [agent.id, agent.state]));
    for (const agent of next.agents) {
      const was = before.get(agent.id);
      if (!was || was === agent.state) continue;
      if (agent.state === 'paused') state.flash.set(agent.id, { tone: 'needs', at: Date.now() });
      if (agent.state === 'done') state.flash.set(agent.id, { tone: 'done', at: Date.now() });
      if (state.view === 'agent' && state.agent === agent.id) continue;
      if (agent.state === 'paused') {
        toast('is waiting for you', 'warn', { title: agent.name, action: { label: 'Open', run: () => openAgent(agent.id) } });
      } else if (agent.state === 'done' && was === 'working' && agent.kind !== 'terminal') {
        toast('finished', 'success', { title: agent.name, action: { label: 'View', run: () => openAgent(agent.id) } });
      } else {
        continue;
      }
      buzz(agent.state === 'paused' ? [30, 60, 30] : 20);
    }
  }

  function onMessage(message) {
    switch (message.type) {
      case 'welcome':
        state.attempt = 0;
        state.peekKey = '';
        setLink('live');
        try {
          window.sessionStorage.removeItem(RELOAD_KEY);
        } catch (err) {
          state.attempt = 0;
        }
        if (state.view === 'pair') show('overview');
        if (state.view === 'agent' && state.agent !== null) openAgent(state.agent, true);
        break;
      case 'state': {
        const previous = state.overview;
        state.skew = Date.now() - message.now;
        state.overview = message;
        attention(previous, message);
        if (!message.sessions.some((session) => session.name === state.filter)) {
          state.filter = message.session;
          state.enter = true;
        }
        if (state.view === 'overview') renderOverview();
        else {
          renderHeader();
          renderContext(findAgent(state.agent));
          renderApproval();
        }
        updateComposer();
        break;
      }
      case 'usage':
        state.usage = message.entries;
        renderUsage();
        break;
      case 'screen':
        applyScreen(message);
        break;
      case 'peek':
        for (const preview of message.previews) {
          const asked = M.choices(preview.lines.map(M.runsText));
          if (asked) state.previews.set(preview.agent, asked);
          else state.previews.delete(preview.agent);
        }
        if (state.view === 'overview' && state.overview) renderNeeds(inSession());
        break;
      case 'notice':
        toast(message.text, message.tone);
        break;
      case 'folders':
      case 'folderSearch':
        if (state.folders) state.folders(message);
        break;
      case 'diff':
        showDiff(message);
        break;
      default:
        break;
    }
  }

  function setLink(link) {
    state.link = link;
    el.link.dataset.state = link;
    el.link.setAttribute('aria-label', link);
    document.body.dataset.link = link;
    renderHeader();
    if (state.view === 'overview') renderOffline();
  }

  function reloadOnce() {
    let reloaded = '1';
    try {
      reloaded = window.sessionStorage.getItem(RELOAD_KEY);
      window.sessionStorage.setItem(RELOAD_KEY, '1');
    } catch (err) {
      reloaded = '1';
    }
    if (reloaded) {
      showPair('outdated');
      return;
    }
    window.location.reload();
  }

  function onClose(code) {
    if (code === Wire.UNPAIRED) {
      setLink('unpaired');
      showPair('unpaired');
      return;
    }
    if (code === Wire.OUTDATED) {
      setLink('offline');
      reloadOnce();
      return;
    }
    if (code === Wire.REPLACED) {
      setLink('replaced');
      showPair('replaced');
      return;
    }
    setLink('offline');
    const delay = RETRY[state.attempt] || RETRY_MAX;
    state.attempt += 1;
    clearTimeout(state.retryTimer);
    state.retryTimer = setTimeout(connect, delay);
  }

  function connect() {
    clearTimeout(state.retryTimer);
    if (state.session || !state.secret) return;
    setLink('connecting');
    const scheme = window.location.protocol === 'https:' ? 'wss' : 'ws';
    const session = new Wire.Session(`${scheme}://${window.location.host}/ws`, state.secret, {
      message: (message) => {
        if (state.session === session) onMessage(message);
      },
      close: (code) => {
        if (state.session !== session) return;
        state.session = null;
        onClose(code);
      },
      error: (err) => console.warn('orkai pwa:', err),
    });
    state.session = session;
  }

  function reconnectNow() {
    if (document.hidden || state.session || !state.secret || ['unpaired', 'replaced'].includes(state.link)) return;
    state.attempt = 0;
    connect();
  }

  function retryNow() {
    state.attempt = 0;
    connect();
  }

  function readLink() {
    const value = Wire.linkKey(window.location.hash);
    if (!value) return null;
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
    const secret = Wire.secretFrom(value);
    if (secret) storage.set(SECRET_KEY, value);
    return secret;
  }

  function pairWith(text) {
    const value = Wire.linkKey(text);
    const secret = Wire.secretFrom(value);
    if (!secret) {
      toast('Copy it again from Octo, with the part after #k=', 'error', { title: 'This is not the whole pairing link' });
      return;
    }
    storage.set(SECRET_KEY, value);
    state.secret = secret;
    el.pairLink.value = '';
    el.pairLink.blur();
    show('overview');
    renderOverview();
    state.attempt = 0;
    connect();
  }

  function pairWithLink(event) {
    event.preventDefault();
    pairWith(el.pairLink.value);
  }

  function pasteLink() {
    if (!navigator.clipboard || !navigator.clipboard.readText) {
      el.pairLink.focus();
      return;
    }
    navigator.clipboard.readText().then(
      (text) => pairWith(text),
      () => {
        el.pairLink.focus();
        toast('Paste the link in the field below', 'info');
      },
    );
  }

  function applyTheme() {
    const light = Boolean(window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches);
    const theme = M.resolveTheme(state.theme, light);
    document.documentElement.dataset.theme = theme;
    el.themeColor.setAttribute('content', M.THEME_INK[theme]);
  }

  function setTheme(theme) {
    state.theme = theme;
    storage.set(THEME_KEY, theme === 'dark' ? null : theme);
    applyTheme();
  }

  function applyMotion() {
    if (state.reduced) document.documentElement.dataset.motion = 'reduce';
    else delete document.documentElement.dataset.motion;
  }

  function onVisible() {
    if (!document.hidden && state.view === 'agent' && state.wanted) {
      state.wanted = phoneSize();
      send({ type: 'size', rows: state.wanted.rows, cols: state.wanted.cols });
    }
    reconnectNow();
  }

  function onResize() {
    if (state.view === 'overview' && state.layout === 'board') placeIndicator();
    if (state.view !== 'agent') return;
    const size = phoneSize();
    if (state.wanted && (size.rows !== state.wanted.rows || size.cols !== state.wanted.cols)) {
      state.wanted = size;
      send({ type: 'size', rows: size.rows, cols: size.cols });
    }
    fitFont();
    placeCursor();
  }

  function dragToClose(handle) {
    let start = null;
    handle.addEventListener('pointerdown', (event) => {
      if (event.target.closest('button')) return;
      start = event.clientY;
      el.sheet.classList.add('dragging');
      handle.setPointerCapture(event.pointerId);
    });
    handle.addEventListener('pointermove', (event) => {
      if (start === null) return;
      el.sheetCard.style.transform = `translateY(${Math.max(0, event.clientY - start)}px)`;
    });
    const end = (event) => {
      if (start === null) return;
      const moved = event.clientY - start;
      start = null;
      el.sheet.classList.remove('dragging');
      if (moved > DRAG_CLOSE) closeSheet();
      else el.sheetCard.style.transform = '';
    };
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
  }

  function bind() {
    el.back.addEventListener('click', back);
    el.menu.addEventListener('click', agentMenu);
    el.settings.addEventListener('click', openAppearance);
    el.live.addEventListener('click', () => send({ type: 'scroll', lines: 0 }));
    el.retry.addEventListener('click', () => {
      show('overview');
      renderOverview();
      state.attempt = 0;
      connect();
    });
    el.pairForm.addEventListener('submit', pairWithLink);
    el.pairPaste.addEventListener('click', pasteLink);
    el.form.addEventListener('submit', submit);
    el.input.addEventListener('input', grow);
    el.input.addEventListener('focus', renderPicks);
    el.input.addEventListener('blur', () => setTimeout(renderPicks, 120));
    el.input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) submit(event);
    });
    el.sheetClose.addEventListener('click', closeSheet);
    el.sheet.addEventListener('click', (event) => {
      if (event.target === el.sheet) closeSheet();
    });
    dragToClose(el.sheetGrab);
    dragToClose(el.sheetCard.querySelector('.sheet-head'));
    let toastY = null;
    el.toast.addEventListener('pointerdown', (event) => {
      toastY = event.clientY;
    });
    el.toast.addEventListener('pointerup', (event) => {
      if (toastY !== null && event.clientY - toastY < SWIPE_AWAY) hideToast();
      toastY = null;
    });
    el.columns.addEventListener('scroll', onColumnsScroll, { passive: true });
    for (const spec of KEY_GROUPS) {
      el.keys.append(h('div', { class: `key-group${spec.tone ? ` ${spec.tone}` : ''}` }, spec.keys.map(([text, data]) => {
        const key = h('button', { class: 'key', type: 'button', text, onclick: () => send({ type: 'keys', data }) });
        key.addEventListener('pointerdown', (event) => event.preventDefault());
        return key;
      })));
    }
    el.screen.addEventListener('touchstart', (event) => {
      state.touchY = event.touches[0].clientY;
    }, { passive: true });
    el.screen.addEventListener('touchmove', (event) => {
      if (state.touchY === null) return;
      const line = state.font * LINE;
      const delta = (event.touches[0].clientY - state.touchY) / line;
      if (Math.abs(delta) < 1) return;
      const lines = Math.trunc(delta);
      state.touchY += lines * line;
      queueScroll(lines);
    }, { passive: true });
    el.screen.addEventListener('touchend', () => {
      state.touchY = null;
    });
    el.screen.addEventListener('wheel', (event) => {
      if (Math.abs(event.deltaY) < Math.abs(event.deltaX)) return;
      event.preventDefault();
      queueScroll(Math.round(-event.deltaY / (state.font * LINE)) || (event.deltaY < 0 ? 1 : -1));
    }, { passive: false });
    if (window.ResizeObserver) new ResizeObserver(() => placeCursor()).observe(el.screen);
    window.addEventListener('popstate', () => {
      if (state.view === 'agent') leaveAgent();
    });
    window.addEventListener('resize', () => {
      clearTimeout(state.resizeTimer);
      state.resizeTimer = setTimeout(onResize, 150);
    });
    if (window.matchMedia) window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', applyTheme);
    document.addEventListener('visibilitychange', onVisible);
    window.addEventListener('online', reconnectNow);
    setInterval(() => {
      state.frame += 1;
      for (const node of document.querySelectorAll('[data-spin]')) node.textContent = M.SPINNER[state.frame % M.SPINNER.length];
    }, 100);
    setInterval(() => {
      if (document.hidden || !state.overview) return;
      if (state.view === 'overview') {
        const list = inSession();
        renderAgents();
        renderNeeds(list);
        if (!el.lost.hidden) renderLost(list);
      }
      renderHeader();
    }, 1000);
  }

  function boot() {
    state.theme = storage.get(THEME_KEY) || 'dark';
    state.layout = storage.get(LAYOUT_KEY) === 'board' ? 'board' : 'list';
    state.haptics = storage.get(HAPTICS_KEY) !== '0';
    state.reduced = storage.get(MOTION_KEY) === 'reduce';
    applyTheme();
    applyMotion();
    el.pairMascot.append(kai('idle'));
    el.pairMascot.firstChild.classList.add('mascot');
    state.who = storage.get(WHO_KEY);
    state.worktree = storage.get(WORKTREE_KEY) === '1';
    state.bypass = storage.get(BYPASS_KEY) === '1';
    bind();
    measure();
    if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(() => {});
    if (!Wire.supported) {
      setLink('unpaired');
      showPair('unsupported');
      return;
    }
    state.secret = readLink() || Wire.secretFrom(storage.get(SECRET_KEY));
    if (!state.secret) {
      setLink('unpaired');
      showPair('new');
      return;
    }
    show('overview');
    renderOverview();
    connect();
  }

  boot();
})();
