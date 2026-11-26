import { COLUMNS, MODES, MOODS, ORCH_EDGES, ORCH_NODES, ORCH_STATE_TEXT, ORCH_STEPS, OS_LABELS, SPINNER, detectOs, formatStars, progressFor, stepAt } from './session.js';
import { PROVIDER_ICONS, agentIcon } from './icons.js';
import { setupFooter } from './footer.js';

const STARS_URL = 'https://api.github.com/repos/lucasmarujo/octo';
const STARS_KEY = 'octo-stars';
const STARS_TTL = 3600000;
const APP_WIDTH = 1440;
const APP_PAGE = 'octo-page';
const APP_DEMO = 'octo-demo';
const APP_SAY_MAX = 120;

const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function kai(mood, className) {
  const node = el('span', `kai ${className}`);
  node.dataset.mood = mood;
  node.setAttribute('aria-hidden', 'true');
  node.append(el('span', 'kai-strip'));
  return node;
}

const typing = new WeakMap();

async function say(node, text) {
  const turn = {};
  typing.set(node, turn);
  const shown = el('span');
  const rest = el('span', 'say-rest', text);
  node.replaceChildren(shown, rest);
  if (reduced) {
    shown.textContent = text;
    rest.textContent = '';
    return;
  }
  for (let index = 1; index <= text.length; index += 1) {
    if (typing.get(node) !== turn) return;
    shown.textContent = text.slice(0, index);
    rest.textContent = text.slice(index);
    await sleep(16);
  }
}

function whenVisible(target, onChange, options) {
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) onChange(entry.isIntersecting, entry.target, observer);
  }, options);
  observer.observe(target);
  return observer;
}

function setupNav() {
  const nav = document.querySelector('[data-nav]');
  const update = () => nav.classList.toggle('is-stuck', window.scrollY > 24);
  update();
  window.addEventListener('scroll', update, { passive: true });
}

function setupReveal() {
  const items = document.querySelectorAll('[data-reveal]');
  const observer = new IntersectionObserver((entries) => {
    let order = 0;
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.style.setProperty('--d', String(order));
      entry.target.classList.add('in');
      observer.unobserve(entry.target);
      order += 1;
    }
  }, { rootMargin: '0px 0px -8% 0px', threshold: 0.12 });
  for (const item of items) observer.observe(item);
}

function setupSpinners() {
  let frame = 0;
  setInterval(() => {
    frame = (frame + 1) % SPINNER.length;
    for (const node of document.querySelectorAll('[data-spin]')) node.textContent = SPINNER[frame];
  }, 90);
}

function setupApp() {
  const fit = document.querySelector('[data-app-fit]');
  const frame = document.querySelector('[data-app-frame]');
  const perchKai = document.querySelector('[data-perch-kai]');
  const perchSay = document.querySelector('[data-perch-say]');
  let visible = false;

  const tell = () => {
    if (frame.contentWindow) frame.contentWindow.postMessage({ source: APP_PAGE, visible }, window.location.origin);
  };

  const resize = () => fit.style.setProperty('--s', String(fit.clientWidth / APP_WIDTH));
  resize();
  new ResizeObserver(resize).observe(fit);

  window.addEventListener('message', (event) => {
    if (event.origin !== window.location.origin || event.source !== frame.contentWindow) return;
    const line = event.data;
    if (!line || line.source !== APP_DEMO) return;
    if (line.ready) tell();
    if (typeof line.say !== 'string' || line.say.length > APP_SAY_MAX || !MOODS.includes(line.mood)) return;
    perchKai.dataset.mood = line.mood;
    say(perchSay, line.say);
  });
  whenVisible(fit, (state) => {
    visible = state;
    tell();
  }, { threshold: 0.15 });
}

function setupFleet() {
  const fleet = document.querySelector('[data-fleet]');
  const kinds = Object.keys(PROVIDER_ICONS);
  const states = ['working', 'working', 'done', 'working', 'waiting', 'working', 'done', 'working'];
  const cells = kinds.map((kind, index) => {
    const cell = el('span');
    cell.title = kind;
    cell.innerHTML = agentIcon(kind, 26);
    const glyph = el('b');
    cell.append(glyph);
    fleet.append(cell);
    return { cell, glyph, state: states[index] };
  });
  const paint = () => {
    for (const item of cells) {
      const column = COLUMNS.find((entry) => entry.state === item.state);
      item.glyph.style.setProperty('--tone', column.tone);
      item.glyph.textContent = column.glyph;
      if (item.state === 'working') item.glyph.setAttribute('data-spin', '');
      else item.glyph.removeAttribute('data-spin');
      item.cell.classList.toggle('is-waiting', item.state === 'waiting');
    }
  };
  paint();
  if (reduced) return;
  const next = { working: 'done', done: 'working', waiting: 'working' };
  setInterval(() => {
    const item = cells[Math.floor(Math.random() * cells.length)];
    const waiting = cells.some((entry) => entry.state === 'waiting');
    item.state = item.state === 'working' && !waiting && Math.random() < 0.4 ? 'waiting' : next[item.state];
    paint();
  }, 1300);
}

function flowNode(node) {
  const left = node.x - node.w / 2;
  const top = node.y - node.h / 2;
  const middle = node.h / 2;
  let inner = '';
  if (node.tier === 'you') inner = `<text class="o-label" x="${node.w / 2}" y="${middle + 5}" text-anchor="middle">${node.label}</text>`;
  if (node.tier === 'octo') inner = `<svg x="16" y="12" width="56" height="56" viewBox="0 0 48 48"><image href="/assets/kai/idle.svg" width="192" height="48"/></svg><text class="o-label" x="86" y="${middle - 2}">${node.label}</text><text class="o-note" x="86" y="${middle + 18}">${node.note}</text>`;
  if (node.tier === 'agent') inner = `${agentIcon(node.id, 30).replace('<svg ', `<svg x="18" y="${middle - 15}" `)}<text class="o-label" x="62" y="${middle - 3}">${node.label}</text><text class="o-state" x="62" y="${middle + 17}" data-flow-state></text>`;
  if (node.tier === 'sub') inner = `<text class="o-label" x="${node.w / 2}" y="${middle - 2}" text-anchor="middle">${node.label}</text><text class="o-state" x="${node.w / 2}" y="${middle + 15}" text-anchor="middle" data-flow-state></text>`;
  return `<g class="o-node o-${node.tier}" data-flow-node="${node.id}" transform="translate(${left} ${top})"><rect width="${node.w}" height="${node.h}"/>${inner}</g>`;
}

function setupFlow() {
  const art = document.querySelector('[data-flow-art]');
  const line = document.querySelector('[data-flow-say]');
  const dots = document.querySelector('[data-flow-dots]');
  const edges = ORCH_EDGES.map((edge) => `<path class="o-edge" data-flow-edge="${edge.id}" d="${edge.d}"/><circle class="o-pulse" data-flow-pulse="${edge.id}" r="5" style="offset-path: path('${edge.d}')"/>`).join('');
  art.innerHTML = `<svg viewBox="0 0 1200 490" fill="none" focusable="false" aria-hidden="true">${edges}${ORCH_NODES.map(flowNode).join('')}</svg>`;
  for (const step of ORCH_STEPS) dots.append(el('li'));
  const nodes = new Map([...art.querySelectorAll('[data-flow-node]')].map((node) => [node.dataset.flowNode, node]));
  let index = 0;
  let visible = false;

  const show = (position) => {
    const step = ORCH_STEPS[position];
    line.textContent = step.say;
    [...dots.children].forEach((dot, at) => dot.classList.toggle('is-on', at === position));
    for (const edge of art.querySelectorAll('[data-flow-edge]')) edge.classList.toggle('is-on', step.edges.includes(edge.dataset.flowEdge));
    for (const pulse of art.querySelectorAll('[data-flow-pulse]')) {
      pulse.classList.toggle('is-on', !reduced && step.edges.includes(pulse.dataset.flowPulse));
      pulse.classList.toggle('is-up', step.up);
    }
    for (const [id, node] of nodes) {
      const state = step.states[id];
      const fixed = id === 'you' || id === 'octo';
      node.classList.toggle('is-on', step.nodes.includes(id));
      node.classList.toggle('is-born', fixed || Boolean(state));
      for (const name of Object.keys(ORCH_STATE_TEXT)) node.classList.toggle(`is-${name}`, state === name);
      const label = node.querySelector('[data-flow-state]');
      if (!label) continue;
      if (state === 'working') label.setAttribute('data-spin', '');
      else label.removeAttribute('data-spin');
      label.textContent = state ? ORCH_STATE_TEXT[state] : '';
    }
  };

  if (reduced) {
    show(ORCH_STEPS.length - 1);
    return;
  }
  show(0);
  whenVisible(art, (state) => { visible = state; }, { threshold: 0.3 });
  setInterval(() => {
    if (!visible) return;
    index = (index + 1) % ORCH_STEPS.length;
    show(index);
  }, 3600);
}

function setupEvents() {
  const items = [...document.querySelectorAll('[data-events] li')];
  let index = 0;
  const paint = () => items.forEach((item, position) => item.classList.toggle('is-on', position === index));
  paint();
  if (reduced) return;
  setInterval(() => {
    index = (index + 1) % items.length;
    paint();
  }, 1500);
}

function setupRace() {
  const race = document.querySelector('[data-race]');
  if (reduced) {
    race.classList.add('is-run');
    return;
  }
  let timer = 0;
  const start = () => {
    race.classList.remove('is-run');
    requestAnimationFrame(() => requestAnimationFrame(() => race.classList.add('is-run')));
  };
  whenVisible(race, (visible) => {
    clearInterval(timer);
    if (!visible) return;
    start();
    timer = setInterval(start, 7000);
  }, { threshold: 0.5 });
}

function chatItem(entry) {
  if (entry.type === 'user') return el('div', 'c-item c-user', entry.text);
  if (entry.type === 'summary') {
    const row = el('div', 'c-item c-sum');
    row.append(el('b', '', '✔'), el('span', '', entry.text));
    return row;
  }
  if (entry.type === 'octo') {
    const row = el('div', 'c-item c-octo');
    const body = el('div', 'c-body');
    body.append(el('span', '', entry.text));
    for (const action of entry.actions) body.append(el('span', 'c-act', action));
    row.append(kai('idle', 'kai-chat'), body);
    return row;
  }
  const card = el('div', 'c-item c-card');
  const head = el('div', 'c-card-head');
  head.append(el('b', '', '‖'), el('strong', '', entry.agent), el('span', '', entry.project), el('em', '', entry.wait));
  const would = el('div', 'c-would');
  would.append(el('i', '', 'Octo would answer'), el('b', '', entry.would), el('span', '', entry.reason));
  const buttons = el('div', 'c-buttons');
  for (const label of entry.buttons) buttons.append(el('span', '', label));
  card.append(head, el('div', 'c-ask', entry.ask), would, buttons);
  return card;
}

function setupChat() {
  const tabs = [...document.querySelectorAll('[data-modes] .mode')];
  const thread = document.querySelector('[data-chat-thread]');
  const status = document.querySelector('[data-chat-sub]');
  const text = document.querySelector('[data-mode-text]');
  const show = (name) => {
    const mode = MODES[name];
    for (const tab of tabs) tab.setAttribute('aria-selected', String(tab.dataset.mode === name));
    text.textContent = mode.text;
    status.textContent = mode.status;
    thread.textContent = '';
    mode.thread.forEach((entry, index) => {
      const item = chatItem(entry);
      item.style.animationDelay = `${index * 260}ms`;
      thread.append(item);
    });
  };
  for (const tab of tabs) tab.addEventListener('click', () => show(tab.dataset.mode));
  let started = false;
  whenVisible(thread, (visible, target, observer) => {
    if (!visible || started) return;
    started = true;
    observer.disconnect();
    show('moderate');
  }, { threshold: 0.3 });
}

function setupMobile() {
  const track = document.querySelector('[data-m-track]');
  const root = track.querySelector('[data-mobile]');
  const tabs = [...root.querySelectorAll('[data-m-feat]')];
  const screens = [...root.querySelectorAll('[data-m-screen]')];
  const wide = window.matchMedia('(min-width: 1081px)');
  const delay = 4200;
  let index = 0;
  let visible = false;
  let held = false;
  let timer = 0;
  let pending = false;

  const byScroll = () => wide.matches && !reduced;

  const show = (position, timed) => {
    index = position;
    tabs.forEach((tab, at) => {
      tab.setAttribute('aria-selected', String(at === position));
      tab.classList.remove('is-timed');
    });
    screens.forEach((screen, at) => screen.classList.toggle('is-on', at === position));
    if (!timed) return;
    void tabs[position].offsetWidth;
    tabs[position].classList.add('is-timed');
  };

  const room = () => Math.max(1, track.offsetHeight - window.innerHeight);

  const follow = () => {
    pending = false;
    if (!byScroll()) return;
    const step = stepAt(-track.getBoundingClientRect().top / room(), tabs.length);
    if (step.index !== index || tabs[index].getAttribute('aria-selected') !== 'true') show(step.index, false);
    tabs[step.index].style.setProperty('--m-part', step.part.toFixed(3));
  };

  const request = () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(follow);
  };

  const schedule = () => {
    clearTimeout(timer);
    if (byScroll() || reduced || !visible || held) return;
    show(index, true);
    timer = setTimeout(() => {
      index = (index + 1) % tabs.length;
      schedule();
    }, delay);
  };

  root.style.setProperty('--m-ms', `${delay}ms`);
  for (const tab of tabs) {
    tab.addEventListener('click', () => {
      const target = Number(tab.dataset.mFeat);
      if (byScroll()) {
        const top = track.getBoundingClientRect().top + window.scrollY;
        window.scrollTo({ top: top + progressFor(target, tabs.length) * room(), behavior: 'smooth' });
        return;
      }
      show(target, false);
      schedule();
    });
  }
  root.addEventListener('pointerenter', () => {
    held = true;
    clearTimeout(timer);
    if (!byScroll()) show(index, false);
  });
  root.addEventListener('pointerleave', () => {
    held = false;
    schedule();
  });
  whenVisible(root, (state) => {
    visible = state;
    schedule();
  }, { threshold: 0.35 });
  window.addEventListener('scroll', request, { passive: true });
  window.addEventListener('resize', request);
  wide.addEventListener('change', () => {
    for (const tab of tabs) tab.style.removeProperty('--m-part');
    request();
    schedule();
  });
  request();
}

function setupGlow() {
  const cards = document.querySelectorAll('.era, .cell, .reach-card, .get-link, .define, .m-stage, .faq-more');
  const fine = window.matchMedia('(hover: hover) and (pointer: fine)').matches;
  for (const card of cards) {
    card.classList.add('glow-card');
    if (!fine || reduced) continue;
    card.addEventListener('pointermove', (event) => {
      const rect = card.getBoundingClientRect();
      const x = (event.clientX - rect.left) / rect.width;
      const y = (event.clientY - rect.top) / rect.height;
      card.style.setProperty('--mx', `${(x * 100).toFixed(1)}%`);
      card.style.setProperty('--my', `${(y * 100).toFixed(1)}%`);
      card.style.setProperty('--ry', `${((x - 0.5) * 5).toFixed(2)}deg`);
      card.style.setProperty('--rx', `${((0.5 - y) * 5).toFixed(2)}deg`);
    });
    card.addEventListener('pointerleave', () => {
      card.style.setProperty('--rx', '0deg');
      card.style.setProperty('--ry', '0deg');
    });
  }
}

function setupGuides() {
  for (const guide of document.querySelectorAll('[data-guide]')) {
    const line = guide.querySelector('[data-say]');
    const text = line.textContent;
    line.setAttribute('aria-label', text);
    whenVisible(guide, (visible, target, observer) => {
      if (!visible) return;
      observer.disconnect();
      say(line, text);
    }, { threshold: 0.6 });
  }
}

function setupFaq() {
  const items = [...document.querySelectorAll('[data-faq] .faq-item')];
  const timing = { duration: 460, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' };
  const running = new WeakMap();

  const stop = (item) => {
    const animation = running.get(item);
    if (animation) animation.cancel();
    running.delete(item);
    item.classList.remove('is-opening', 'is-closing');
  };

  const open = (item) => {
    const answer = item.querySelector('.faq-a');
    stop(item);
    item.open = true;
    item.classList.add('is-opening');
    const animation = answer.animate([{ height: '0px', opacity: 0 }, { height: `${answer.scrollHeight}px`, opacity: 1 }], timing);
    running.set(item, animation);
    animation.onfinish = () => stop(item);
  };

  const close = (item) => {
    const answer = item.querySelector('.faq-a');
    const from = answer.offsetHeight;
    stop(item);
    item.classList.add('is-closing');
    const animation = answer.animate([{ height: `${from}px`, opacity: 1 }, { height: '0px', opacity: 0 }], timing);
    running.set(item, animation);
    animation.onfinish = () => {
      item.open = false;
      stop(item);
    };
  };

  if (reduced) return;
  for (const item of items) {
    item.querySelector('summary').addEventListener('click', (event) => {
      event.preventDefault();
      const closing = item.open && !item.classList.contains('is-closing');
      if (closing) {
        close(item);
        return;
      }
      for (const other of items) {
        if (other !== item && other.open && !other.classList.contains('is-closing')) close(other);
      }
      open(item);
    });
  }
}

function readStars() {
  try {
    const saved = JSON.parse(localStorage.getItem(STARS_KEY));
    if (saved && Date.now() - saved.at < STARS_TTL) return saved.count;
  } catch {
    return null;
  }
  return null;
}

async function setupStars() {
  const label = document.querySelector('[data-stars]');
  const show = (count) => {
    const text = formatStars(count);
    if (!text) return;
    label.textContent = text;
    label.parentElement.hidden = false;
  };
  const saved = readStars();
  if (saved !== null) {
    show(saved);
    return;
  }
  try {
    const response = await fetch(STARS_URL, { headers: { Accept: 'application/vnd.github+json' } });
    if (!response.ok) return;
    const count = (await response.json()).stargazers_count;
    show(count);
    localStorage.setItem(STARS_KEY, JSON.stringify({ count, at: Date.now() }));
  } catch {
    label.parentElement.hidden = true;
  }
}

function setupOs() {
  const os = detectOs(navigator.platform, navigator.userAgent);
  if (!os) return;
  for (const label of document.querySelectorAll('[data-os-label]')) label.textContent = OS_LABELS[os];
  const item = document.querySelector(`[data-get] [data-os="${os}"]`);
  item.classList.add('is-you');
  const cta = document.querySelector('[data-os-cta]');
  cta.href = item.querySelector('.get-link').href;
}

setupNav();
setupReveal();
setupSpinners();
setupApp();
setupFleet();
setupFlow();
setupEvents();
setupRace();
setupChat();
setupMobile();
setupGlow();
setupGuides();
setupFaq();
setupOs();
setupStars();
setupFooter({ reduced });
