(function () {
  'use strict';

  const Wire = window.OrkaiWire;
  const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  const SECRET_KEY = 'orkai.secret';
  const WHO_KEY = 'orkai.who';
  const WORKTREE_KEY = 'orkai.worktree';
  const BYPASS_KEY = 'orkai.bypass';
  const RELOAD_KEY = 'orkai.reloaded';
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
  const DEFAULT = -1;
  const TRUECOLOR = 256;
  const BOLD = 1;
  const DIM = 2;
  const ITALIC = 4;
  const UNDERLINE = 8;
  const INVERSE = 16;
  const INK = '#121218';
  const TEXT = '#dedee4';
  const LABELS = { antigravity: 'agy' };
  const BASE16 = [
    '#2c2c3a', '#f04e5c', '#50d782', '#f0d65a', '#6e96ff', '#ec80dc', '#5ad2d2', '#dedee4',
    '#6e6e7a', '#ff7d9d', '#96e2aa', '#f0de96', '#89bafa', '#e8aaec', '#8ce2d6', '#ffffff',
  ];
  const KEYS = [
    ['esc', '\x1b'], ['tab', '\t'], ['⇧tab', '\x1b[Z'], ['↑', '\x1b[A'], ['↓', '\x1b[B'], ['←', '\x1b[D'],
    ['→', '\x1b[C'], ['⏎', '\r'], ['1', '1'], ['2', '2'], ['3', '3'], ['y', 'y'], ['n', 'n'], ['^c', '\x03', 'warn'],
  ];

  const $ = (id) => document.getElementById(id);
  const el = {
    back: $('back'), mascot: $('mascot'), host: $('host'), where: $('where'), menu: $('menu'), link: $('link'),
    overview: $('overview'), sessions: $('sessions'), stats: $('stats'), usage: $('usage'), lost: $('lost'),
    list: $('list'), agent: $('agent'), screen: $('screen'), rows: $('rows'), cursor: $('cursor'), live: $('live'),
    keys: $('keys'), pair: $('pair'), pairMascot: $('pair-mascot'), pairText: $('pair-text'), retry: $('retry'),
    composer: $('composer'), picks: $('picks'), form: $('form'), input: $('input'), send: $('send'),
    sheet: $('sheet'), sheetTitle: $('sheet-title'), sheetBody: $('sheet-body'), sheetClose: $('sheet-close'),
    toast: $('toast'),
  };

  const state = {
    secret: null,
    session: null,
    overview: null,
    usage: null,
    skew: 0,
    view: 'overview',
    filter: null,
    agent: null,
    screen: null,
    wanted: null,
    fullHeight: 0,
    width: 0,
    ratio: 0.6,
    font: TERM_FONT,
    who: null,
    where: null,
    worktree: false,
    bypass: false,
    attempt: 0,
    retryTimer: 0,
    resizeTimer: 0,
    toastTimer: 0,
    frame: 0,
    folders: null,
    scroll: 0,
    scrollTimer: 0,
    touchY: null,
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
      else if (key.startsWith('on')) node.addEventListener(key.slice(2), value);
      else node.setAttribute(key, value === true ? '' : value);
    }
    for (const child of children.flat()) {
      if (child === null || child === undefined || child === false) continue;
      node.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
    return node;
  }

  const label = (kind) => LABELS[kind] || kind;
  const now = () => Date.now() - state.skew;

  function duration(ms) {
    const secs = Math.floor(Math.max(0, ms) / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h${String(mins % 60).padStart(2, '0')}`;
    return `${Math.floor(hours / 24)}d`;
  }

  function send(message) {
    if (state.session) state.session.send(message);
  }

  function toast(text, tone) {
    el.toast.textContent = text;
    el.toast.dataset.tone = tone || 'info';
    el.toast.hidden = false;
    clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(() => {
      el.toast.hidden = true;
    }, tone === 'error' ? 6000 : 3500);
  }

  function setLink(link) {
    el.link.dataset.state = link;
    el.link.setAttribute('aria-label', link);
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

  function glyph(agent) {
    if (agent.starting) return { text: '◌', tone: 't-dim' };
    if (agent.state === 'lost') return { text: '↻', tone: 't-red' };
    if (agent.state === 'exited') return { text: '✗', tone: 't-faint' };
    if (agent.kind === 'terminal') return { text: '$', tone: 't-text' };
    if (agent.state === 'working') return { text: SPINNER[state.frame % SPINNER.length], tone: 't-yellow', spin: true };
    if (agent.state === 'paused') return { text: '‖', tone: 't-yellow' };
    if (agent.state === 'done') return { text: '✔', tone: 't-green' };
    return { text: '●', tone: 't-dim' };
  }

  function nameTone(agent) {
    if (!alive(agent)) return 't-faint';
    if (agent.kind === 'terminal') return 't-text';
    return { done: 't-green', working: 't-text', paused: 't-yellow' }[agent.state] || 't-muted';
  }

  function elapsed(agent) {
    if (agent.startedAt === null || !alive(agent)) return null;
    const at = now();
    if (agent.kind !== 'terminal') {
      if (agent.state === 'working') return { text: duration(at - (agent.turnStarted || agent.stateSince)), tone: 't-yellow' };
      if (agent.state === 'paused') return { text: duration(at - agent.stateSince), tone: 't-yellow' };
      if (agent.lastTurn !== null) return { text: duration(at - agent.stateSince), tone: 't-dim' };
    }
    return { text: duration(at - agent.startedAt), tone: 't-dim' };
  }

  function activity(agent) {
    if (agent.starting) return { text: 'starting…', tone: 't-dim' };
    if (agent.state === 'lost') return { text: 'interrupted · recover it from the banner', tone: 't-red' };
    if (agent.state === 'exited') return { text: agent.exit || 'exited', tone: 't-faint' };
    if (agent.state === 'paused') return { text: `waiting for you · ${agent.activity}`, tone: 't-yellow' };
    return { text: agent.activity, tone: agent.state === 'working' ? 't-text' : 't-muted' };
  }

  function renderSessions() {
    const chips = state.overview.sessions.map((session) => {
      const on = session.name === state.filter;
      return h(
        'button',
        { class: `chip${on ? ' on' : ''}`, type: 'button', onclick: () => pickSession(session.name) },
        on ? `› ${session.name}` : session.name,
        h('span', { class: 'count', text: String(session.agents) }),
      );
    });
    el.sessions.replaceChildren(...chips, h('button', { class: 'chip add', type: 'button', onclick: newSession }, '+ session'));
  }

  function stat(kind, value) {
    return h('div', { class: `stat ${kind}` }, h('b', { text: String(value) }), h('span', { text: kind }));
  }

  function renderStats(list) {
    const ai = list.filter((agent) => agent.kind !== 'terminal' && agent.state !== 'exited');
    const count = (wanted) => ai.filter((agent) => agent.state === wanted).length;
    el.stats.replaceChildren(stat('agents', ai.length), stat('working', count('working')), stat('paused', count('paused')), stat('done', count('done')));
  }

  function renderUsage() {
    const entries = state.usage || [];
    if (!entries.length) {
      el.usage.replaceChildren();
      return;
    }
    const rows = [h('h2', {}, 'usage')];
    for (const entry of entries) {
      const kind = h('span', { class: `k-${entry.kind}`, text: label(entry.kind) });
      if (!entry.windows.length && entry.credits === null) {
        rows.push(h('div', { class: 'meter' }, kind, h('span', { class: 'note', text: entry.problem || 'no plan limits' })));
        continue;
      }
      entry.windows.forEach((window, index) => {
        const used = Math.round(window.used);
        const tone = window.used >= 90 ? 'red' : window.used >= 70 ? 'yellow' : 'green';
        const fill = h('i');
        fill.style.width = `${Math.min(100, Math.max(0, used))}%`;
        fill.style.background = `var(--${tone})`;
        const left = window.resetsAt ? window.resetsAt - now() : null;
        rows.push(
          h(
            'div',
            { class: 'meter' },
            index === 0 ? kind : h('span'),
            h('span', { class: 't-dim', text: window.label }),
            h('span', { class: 'bar' }, fill),
            h('span', { class: `pct t-${tone}`, text: `${used}%` }),
            left === null ? null : h('span', { class: 'resets', text: left > 0 ? `resets ${duration(left)}` : 'resets now' }),
          ),
        );
      });
      if (entry.credits !== null) {
        rows.push(
          h(
            'div',
            { class: 'meter' },
            entry.windows.length ? h('span') : kind,
            h('span', { class: 't-dim', text: 'funds' }),
            h('span'),
            h('span', { class: 'pct t-green', text: `$${entry.credits.toFixed(2)}` }),
          ),
        );
      }
    }
    el.usage.replaceChildren(...rows);
  }

  function agentRow(agent) {
    const mark = glyph(agent);
    const time = elapsed(agent);
    const doing = activity(agent);
    return h(
      'button',
      { class: `agent${agent.state === 'paused' ? ' needs' : ''}`, type: 'button', onclick: () => openAgent(agent.id) },
      h('span', { class: `glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : null, text: mark.text }),
      h('span', { class: `name ${nameTone(agent)}`, text: agent.name }),
      h('span', { class: `time ${time ? time.tone : ''}`, text: time ? time.text : '' }),
      h('span', { class: `kind k-${agent.kind}`, text: label(agent.kind) }),
      h('span', { class: `activity ${doing.tone}` }, agent.branch ? h('span', { class: 'branch', text: `⎇ ${agent.branch}  ` }) : null, doing.text),
    );
  }

  function renderList(list) {
    if (!list.length) {
      el.list.replaceChildren(h('p', { class: 'empty' }, 'no agents in this session yet', h('small', { text: 'write a task below and press ↵ to start one' })));
      return;
    }
    const groups = new Map();
    for (const agent of list) {
      if (!groups.has(agent.project)) groups.set(agent.project, []);
      groups.get(agent.project).push(agent);
    }
    const blocks = [];
    for (const [project, members] of groups) {
      const title = h('span', { text: project });
      title.style.color = members[0].color;
      blocks.push(h('div', { class: 'group' }, h('h2', {}, title, h('span', { class: 'arrow', text: '›' }), h('span', { class: 'count', text: String(members.length) }))));
      blocks.push(...members.map(agentRow));
    }
    el.list.replaceChildren(...blocks);
  }

  function renderOverview() {
    if (!state.overview) return;
    const list = inSession();
    renderSessions();
    renderStats(list);
    renderUsage();
    renderList(list);
    const lost = list.filter((agent) => agent.state === 'lost').length;
    el.lost.hidden = lost === 0;
    el.lost.textContent = `↻ ${lost} interrupted · tap to recover`;
    el.mascot.classList.toggle('busy', list.some((agent) => agent.state === 'working'));
    const waiting = agents().filter((agent) => agent.state === 'paused').length;
    document.title = waiting ? `(${waiting}) Orkai` : 'Orkai';
    renderHeader();
  }

  function renderHeader() {
    if (state.view === 'agent') {
      const agent = findAgent(state.agent);
      el.host.textContent = agent ? agent.name : `#${state.agent}`;
      if (agent) {
        const time = elapsed(agent);
        const parts = [label(agent.kind), agent.starting ? 'starting' : agent.state];
        if (time) parts.push(time.text);
        if (agent.branch) parts.push(`⎇ ${agent.branch}`);
        el.where.textContent = parts.join(' · ');
      }
      return;
    }
    el.host.textContent = state.overview ? state.overview.host : 'orkai';
    el.where.textContent = state.overview ? `${state.filter} · ${currentWhere()}` : '';
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

  function shortPath(path) {
    const parts = path.replace(/\\/g, '/').split('/').filter(Boolean);
    return parts.length > 2 ? `…/${parts.slice(-2).join('/')}` : path;
  }

  function renderPicks() {
    if (state.view !== 'overview' || !state.overview) {
      el.picks.replaceChildren();
      return;
    }
    const who = currentWho();
    const picks = [
      h('button', { class: `chip k-${who}`, type: 'button', onclick: pickWho }, `${label(who)} ▾`),
      h('button', { class: 'chip', type: 'button', onclick: pickFolder }, `${shortPath(currentWhere())} ▾`),
    ];
    if (who !== 'terminal') {
      picks.push(h('button', { class: `chip toggle${state.worktree ? ' on' : ''}`, type: 'button', onclick: toggleWorktree }, '⎇ worktree'));
      picks.push(h('button', { class: `chip toggle bypass${state.bypass ? ' on' : ''}`, type: 'button', onclick: toggleBypass }, `bypass ${state.bypass ? 'on' : 'off'}`));
    }
    el.picks.replaceChildren(...picks);
  }

  function updateComposer() {
    if (state.view === 'agent') {
      const agent = findAgent(state.agent);
      el.input.placeholder = alive(agent) ? `message ${agent.name}` : 'this agent is not running';
    } else {
      el.input.placeholder = `what should ${label(currentWho())} do?`;
    }
    renderPicks();
  }

  function show(view) {
    state.view = view;
    el.overview.hidden = view !== 'overview';
    el.agent.hidden = view !== 'agent';
    el.pair.hidden = view !== 'pair';
    el.composer.hidden = view === 'pair';
    el.back.hidden = view !== 'agent';
    el.menu.hidden = view !== 'agent';
    updateComposer();
    renderHeader();
  }

  function showPair(text, retry) {
    el.pairText.textContent = text;
    el.retry.hidden = !retry;
    show('pair');
  }

  function pickSession(name) {
    state.filter = name;
    state.where = null;
    renderOverview();
    renderPicks();
  }

  function openSheet(title, build) {
    el.sheetTitle.textContent = title;
    el.sheetBody.replaceChildren();
    state.folders = null;
    build(el.sheetBody);
    el.sheet.hidden = false;
  }

  function closeSheet() {
    el.sheet.hidden = true;
    el.sheetBody.replaceChildren();
    state.folders = null;
  }

  function option(text, detail, onclick, tone) {
    return h('button', { class: `option${tone ? ` ${tone}` : ''}`, type: 'button', onclick }, h('span', { text }), detail ? h('small', { text: detail }) : null);
  }

  function confirmOption(text, armed, action) {
    const button = option(text, null, () => {
      if (button.dataset.armed) {
        action();
        closeSheet();
        return;
      }
      button.dataset.armed = '1';
      button.firstChild.textContent = armed;
      setTimeout(() => {
        delete button.dataset.armed;
        button.firstChild.textContent = text;
      }, CONFIRM_MS);
    }, 'danger');
    return button;
  }

  function promptSheet(title, placeholder, value, done) {
    openSheet(title, (body) => {
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
      body.append(h('div', { class: 'sheet-row' }, input, h('button', { class: 'primary', type: 'button', onclick: go }, 'ok')));
      setTimeout(() => input.focus(), 50);
    });
  }

  function newSession() {
    promptSheet('new session', 'name', '', (name) => {
      send({ type: 'session', name });
      state.filter = name;
      state.where = null;
    });
  }

  function pickWho() {
    openSheet('who runs it', (body) => {
      for (const kind of installed()) {
        const choice = option(label(kind), kind === 'terminal' ? 'a shell in the folder' : null, () => {
          state.who = kind;
          storage.set(WHO_KEY, kind);
          closeSheet();
          updateComposer();
        }, kind === currentWho() ? 'current' : null);
        choice.firstChild.className = `k-${kind}`;
        body.append(choice);
      }
    });
  }

  function toggleWorktree() {
    state.worktree = !state.worktree;
    storage.set(WORKTREE_KEY, state.worktree ? '1' : '0');
    toast(state.worktree ? 'new ai agents start in their own git worktree' : 'new agents run directly in the folder', 'info');
    renderPicks();
  }

  function toggleBypass() {
    state.bypass = !state.bypass;
    storage.set(BYPASS_KEY, state.bypass ? '1' : '0');
    if (state.bypass) toast('new ai agents start with every permission check bypassed', 'warn');
    else toast('new ai agents ask before running tools', 'info');
    renderPicks();
  }

  function pickFolder() {
    openSheet('where', (body) => {
      const path = h('div', { class: 'path' });
      const list = h('div');
      let current = currentWhere();
      const use = () => {
        state.where = current;
        closeSheet();
        updateComposer();
        renderHeader();
      };
      body.append(path, h('div', { class: 'sheet-row' }, h('button', { class: 'primary', type: 'button', onclick: use }, 'use this folder')), list);
      const recent = [...new Set(agents().slice().reverse().map((agent) => agent.cwd))].slice(0, 6);
      const browse = (target) => {
        path.textContent = `${target} …`;
        send({ type: 'folders', path: target });
      };
      state.folders = (message) => {
        current = message.path;
        path.textContent = message.path;
        const items = [];
        if (message.parent) items.push(option('..', 'up one folder', () => browse(message.parent)));
        for (const dir of message.dirs) items.push(option(`${dir.name}/`, null, () => browse(dir.path)));
        if (!message.dirs.length) items.push(h('p', { class: 'path', text: 'no folders inside' }));
        if (recent.length) {
          items.push(h('div', { class: 'path', text: 'recent' }));
          for (const dir of recent) {
            items.push(option(shortPath(dir), dir, () => {
              current = dir;
              use();
            }));
          }
        }
        list.replaceChildren(...items);
      };
      browse(current);
    });
  }

  function agentMenu() {
    const agent = findAgent(state.agent);
    if (!agent) return;
    openSheet(agent.name, (body) => {
      body.append(option('rename', agent.name, () => promptSheet('rename', 'new name', agent.name, (name) => send({ type: 'rename', agent: agent.id, name }))));
      if (agent.branch) {
        body.append(option('changes', `diff of ${agent.branch}`, () => {
          closeSheet();
          send({ type: 'diff', agent: agent.id });
        }));
        body.append(confirmOption(`merge ${agent.branch}`, 'tap again to merge', () => send({ type: 'merge', agent: agent.id })));
      }
      if (agent.state === 'lost') {
        body.append(option('recover', 'start it again where it stopped', () => {
          closeSheet();
          send({ type: 'recover', session: agent.session });
        }));
      }
      if (alive(agent)) {
        body.append(confirmOption(`stop ${agent.name}`, 'tap again to stop it', () => send({ type: 'stop', agent: agent.id })));
      } else {
        body.append(confirmOption(`remove ${agent.name}`, 'tap again to remove it', () => {
          send({ type: 'remove', agent: agent.id });
          back();
        }));
      }
    });
  }

  function showDiff(message) {
    openSheet(`changes · ${message.branch}`, (body) => {
      if (!message.text) {
        body.append(h('p', { class: 'path', text: 'no changes yet' }));
        return;
      }
      const lines = message.text.split('\n');
      const shown = lines.slice(0, DIFF_LINES).map((line) => {
        let tone = null;
        if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ')) tone = 'head';
        else if (line.startsWith('+')) tone = 'add';
        else if (line.startsWith('-')) tone = 'del';
        else if (line.startsWith('@@')) tone = 'hunk';
        return h('span', { class: tone, text: `${line}\n` });
      });
      body.append(h('pre', { class: 'diff' }, shown));
      if (message.truncated || lines.length > DIFF_LINES) body.append(h('p', { class: 'path', text: 'the diff was cut; see the rest on the computer' }));
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

  function placeCursor() {
    const screen = state.screen;
    if (!screen || !screen.cursor || screen.scroll) {
      el.cursor.hidden = true;
      return;
    }
    const [row, col] = screen.cursor;
    el.cursor.hidden = false;
    el.cursor.style.transform = `translate(${PAD + col * state.font * state.ratio}px, ${PAD + row * state.font * LINE}px)`;
  }

  function palette(index) {
    if (index < 16) return BASE16[index];
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
    if (flags & INVERSE) [front, back] = [back || INK, front || TEXT];
    if (front) node.style.color = front;
    if (back) node.style.backgroundColor = back;
    if (flags & BOLD) node.style.fontWeight = '700';
    if (flags & DIM) node.style.opacity = '0.55';
    if (flags & ITALIC) node.style.fontStyle = 'italic';
    if (flags & UNDERLINE) node.style.textDecoration = 'underline';
    return node;
  }

  function applyScreen(message) {
    if (message.agent !== state.agent) return;
    if (message.full || !state.screen || el.rows.children.length !== message.rows) {
      el.rows.replaceChildren(...Array.from({ length: message.rows }, () => h('div', { class: 'row' })));
    }
    state.screen = { rows: message.rows, cols: message.cols, cursor: message.cursor, scroll: message.scroll };
    for (const [index, runs] of message.lines) {
      const row = el.rows.children[index];
      if (row) row.replaceChildren(...runs.map(run));
    }
    fitFont();
    placeCursor();
    el.live.hidden = message.scroll === 0;
  }

  function openAgent(id, again) {
    if (!again) {
      if (state.view !== 'agent') window.history.pushState({ agent: id }, '');
      state.agent = id;
      state.screen = null;
      el.rows.replaceChildren();
      el.cursor.hidden = true;
      el.live.hidden = true;
      show('agent');
    }
    state.wanted = phoneSize();
    send({ type: 'open', agent: id, rows: state.wanted.rows, cols: state.wanted.cols });
  }

  function leaveAgent() {
    if (state.view !== 'agent') return;
    send({ type: 'close' });
    state.agent = null;
    state.screen = null;
    el.rows.replaceChildren();
    show('overview');
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
    const height = el.input.scrollHeight + el.input.offsetHeight - el.input.clientHeight;
    el.input.style.height = `${Math.min(height, INPUT_MAX)}px`;
    el.input.style.overflowY = height > INPUT_MAX ? 'auto' : 'hidden';
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
        toast('write the task first', 'warn');
        return;
      }
      const ai = who !== 'terminal';
      send({ type: 'start', kind: who, session: state.filter, cwd: currentWhere(), task: text, worktree: state.worktree && ai, bypass: state.bypass && ai });
    }
    el.input.value = '';
    grow();
  }

  function attention(previous, next) {
    if (!previous) return;
    const before = new Map(previous.agents.map((agent) => [agent.id, agent.state]));
    for (const agent of next.agents) {
      const was = before.get(agent.id);
      if (!was || was === agent.state || (state.view === 'agent' && state.agent === agent.id)) continue;
      if (agent.state === 'paused') toast(`${agent.name} is waiting for you`, 'warn');
      else if (agent.state === 'done' && was === 'working' && agent.kind !== 'terminal') toast(`${agent.name} finished`, 'success');
      else continue;
      if (navigator.vibrate) navigator.vibrate(agent.state === 'paused' ? [30, 60, 30] : 20);
    }
  }

  function onMessage(message) {
    switch (message.type) {
      case 'welcome':
        state.attempt = 0;
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
        if (!message.sessions.some((session) => session.name === state.filter)) state.filter = message.session;
        renderOverview();
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
      case 'notice':
        toast(message.text, message.tone);
        break;
      case 'folders':
        if (state.folders) state.folders(message);
        break;
      case 'diff':
        showDiff(message);
        break;
      default:
        break;
    }
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
      showPair('this orkai speaks another version of the app. update orkai on the computer.', true);
      return;
    }
    window.location.reload();
  }

  function onClose(code) {
    if (code === Wire.UNPAIRED) {
      setLink('unpaired');
      showPair('this phone is not paired with this orkai anymore. scan the new code to pair it again.', true);
      return;
    }
    if (code === Wire.OUTDATED) {
      setLink('offline');
      reloadOnce();
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
    if (document.hidden || state.session || !state.secret || el.link.dataset.state === 'unpaired') return;
    state.attempt = 0;
    connect();
  }

  function readLink() {
    const params = new URLSearchParams(window.location.hash.slice(1));
    const value = params.get('k');
    if (!value) return null;
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
    const secret = Wire.secretFrom(value);
    if (secret) storage.set(SECRET_KEY, value);
    return secret;
  }

  function onResize() {
    if (state.view !== 'agent') return;
    const size = phoneSize();
    if (state.wanted && (size.rows !== state.wanted.rows || size.cols !== state.wanted.cols)) {
      state.wanted = size;
      send({ type: 'size', rows: size.rows, cols: size.cols });
    }
    fitFont();
    placeCursor();
  }

  function bind() {
    el.back.addEventListener('click', back);
    el.menu.addEventListener('click', agentMenu);
    el.lost.addEventListener('click', () => send({ type: 'recover', session: state.filter }));
    el.live.addEventListener('click', () => send({ type: 'scroll', lines: 0 }));
    el.retry.addEventListener('click', () => {
      show('overview');
      state.attempt = 0;
      connect();
    });
    el.form.addEventListener('submit', submit);
    el.input.addEventListener('input', grow);
    el.input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) submit(event);
    });
    el.sheetClose.addEventListener('click', closeSheet);
    el.sheet.addEventListener('click', (event) => {
      if (event.target === el.sheet) closeSheet();
    });
    for (const [text, data, tone] of KEYS) {
      const key = h('button', { class: `key${tone ? ` ${tone}` : ''}`, type: 'button', text, onclick: () => send({ type: 'keys', data }) });
      key.addEventListener('pointerdown', (event) => event.preventDefault());
      el.keys.append(key);
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
    window.addEventListener('popstate', () => {
      if (state.view === 'agent') leaveAgent();
    });
    window.addEventListener('resize', () => {
      clearTimeout(state.resizeTimer);
      state.resizeTimer = setTimeout(onResize, 150);
    });
    document.addEventListener('visibilitychange', reconnectNow);
    window.addEventListener('online', reconnectNow);
    setInterval(() => {
      state.frame += 1;
      for (const node of document.querySelectorAll('[data-spin]')) node.textContent = SPINNER[state.frame % SPINNER.length];
    }, 100);
    setInterval(() => {
      if (document.hidden || !state.overview) return;
      if (state.view === 'overview') renderList(inSession());
      renderHeader();
    }, 1000);
  }

  function boot() {
    const mascot = el.mascot.cloneNode(true);
    mascot.removeAttribute('id');
    el.pairMascot.append(mascot);
    state.who = storage.get(WHO_KEY);
    state.worktree = storage.get(WORKTREE_KEY) === '1';
    state.bypass = storage.get(BYPASS_KEY) === '1';
    bind();
    measure();
    if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(() => {});
    if (!Wire.supported) {
      setLink('offline');
      showPair('this browser cannot run the orkai encryption. open the link in safari or chrome.', false);
      return;
    }
    state.secret = readLink() || Wire.secretFrom(storage.get(SECRET_KEY));
    if (!state.secret) {
      setLink('offline');
      showPair('this phone is not paired yet.', false);
      return;
    }
    show('overview');
    connect();
  }

  boot();
})();
