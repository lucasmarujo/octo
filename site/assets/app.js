import { HINT, SPINNER, durationShort, glyph, slugify } from './tui.js';

const REDUCED_MOTION = matchMedia('(prefers-reduced-motion: reduce)').matches;
const TICK_MS = 100;
const KEY_MS = 1_100;
const BELL_MS = 1_800;
const TOUR_TASK = 'fix the flaky login test';
const SCRAMBLE_GLYPHS = '0123456789abcdef#%&*+=';
const CHAOS_SCATTER = [[0.02, 0.06, -7], [0.62, 0, 5], [0.3, 0.34, -3], [0.76, 0.4, 8], [0.05, 0.66, 4], [0.47, 0.7, -6], [0.84, 0.8, -9], [0.2, 0.16, 3]];

let frame = 0;
const tickers = new Set();
const tracked = [];
const scrollers = [];
const layouts = [];
const viewport = { w: innerWidth, h: innerHeight, doc: 1 };
let framePending = false;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const clamp = (value, min = 0, max = 1) => Math.min(Math.max(value, min), max);
const lerp = (a, b, t) => a + (b - a) * t;
const easeOut = (t) => 1 - (1 - t) ** 3;
const easeInOut = (t) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);
const span = (p, start, length) => clamp((p - start) / length);

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

function setClass(node, className) {
  if (node.className !== className) node.className = className;
}

function whenVisible(node, onChange, rootMargin = '120px 0px') {
  if (!('IntersectionObserver' in window)) {
    onChange(true);
    return;
  }
  new IntersectionObserver(([entry]) => onChange(entry.isIntersecting), { rootMargin }).observe(node);
}

function track(node) {
  const item = { node, top: 0, height: 0 };
  tracked.push(item);
  return item;
}

function measure() {
  viewport.w = document.documentElement.clientWidth;
  viewport.h = innerHeight;
  viewport.doc = Math.max(1, document.documentElement.scrollHeight - innerHeight);
  for (const item of tracked) {
    const rect = item.node.getBoundingClientRect();
    item.top = rect.top + scrollY;
    item.height = rect.height;
  }
  layouts.forEach((layout) => layout());
  requestFrame();
}

function enter(item, y, start = 1, end = 0.3) {
  return clamp((y + viewport.h * start - item.top) / (viewport.h * (start - end)));
}

function pinned(item, y, offset) {
  return clamp((y + offset - item.top) / Math.max(1, item.height - viewport.h + offset));
}

function requestFrame() {
  if (framePending) return;
  framePending = true;
  requestAnimationFrame(() => {
    framePending = false;
    const y = scrollY;
    scrollers.forEach((scroller) => scroller(y));
  });
}

function startScroll() {
  addEventListener('scroll', requestFrame, { passive: true });
  addEventListener('resize', measure);
  addEventListener('load', measure);
  document.fonts.ready.then(measure);
  if ('ResizeObserver' in window) new ResizeObserver(() => measure()).observe(document.body);
  measure();
}

function startClock() {
  setInterval(() => {
    if (document.hidden) return;
    if (!REDUCED_MOTION) frame += 1;
    for (const node of document.querySelectorAll('[data-spin]')) setText(node, SPINNER[frame % SPINNER.length]);
    tickers.forEach((tick) => tick(Date.now()));
  }, TICK_MS);
}

function syncChildren(parent, items, key, create, update) {
  const existing = new Map([...parent.children].map((child) => [child.dataset.key, child]));
  items.forEach((item, index) => {
    const id = key(item);
    let node = existing.get(id);
    if (node) {
      existing.delete(id);
    } else {
      node = create(item);
      node.dataset.key = id;
    }
    update(node, item);
    if (parent.children[index] !== node) parent.insertBefore(node, parent.children[index] || null);
  });
  existing.forEach((node) => node.remove());
}

function flash(row) {
  if (!row || REDUCED_MOTION) return;
  row.animate([{ backgroundColor: 'rgb(255 95 135 / 0.22)' }, { backgroundColor: 'rgb(255 95 135 / 0)' }], { duration: 1_800, easing: 'cubic-bezier(0.16, 1, 0.3, 1)' });
}

function ringBell(bell) {
  bell.classList.remove('is-ringing');
  void bell.offsetWidth;
  bell.classList.add('is-ringing');
  setTimeout(() => bell.classList.remove('is-ringing'), BELL_MS);
}

function showKey(keycap, key) {
  setText(keycap, key);
  keycap.classList.add('is-shown');
  setTimeout(() => keycap.classList.remove('is-shown'), KEY_MS);
}

function agentRow() {
  const row = el('div', 'r');
  row.append(el('span', 'c-num'), el('span', 'c-sel'), el('span', 'c-g'), el('span', 'c-name'), el('span', 'c-t1'), el('span', 'c-t2'), el('span', 'c-kind'), el('span', 'c-act'));
  return row;
}

function splitWords(node) {
  let index = 0;
  const walk = (parent) => {
    for (const child of [...parent.childNodes]) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        walk(child);
        continue;
      }
      if (child.nodeType !== Node.TEXT_NODE) continue;
      const fragment = document.createDocumentFragment();
      for (const part of child.textContent.split(/(\s+)/)) {
        if (!part) continue;
        if (/^\s+$/.test(part)) {
          fragment.append(part);
          continue;
        }
        const inner = el('span', '', part);
        inner.style.setProperty('--d', `${index * 0.06}s`);
        index += 1;
        const outer = el('span', 'w');
        outer.append(inner);
        fragment.append(outer);
      }
      child.replaceWith(fragment);
    }
  };
  walk(node);
  node.classList.add('is-split');
}

function scramble(node, delay) {
  const text = node.textContent;
  const duration = 1_000;
  let start = 0;
  const step = (time) => {
    start ||= time + delay;
    const progress = (time - start) / duration;
    if (progress < 0) {
      requestAnimationFrame(step);
      return;
    }
    let out = '';
    [...text].forEach((char, i) => {
      const settle = 0.3 + (i / text.length) * 0.6;
      out += char === ' ' || char === '.' || progress >= settle ? char : SCRAMBLE_GLYPHS[Math.floor(Math.random() * SCRAMBLE_GLYPHS.length)];
    });
    node.textContent = out;
    if (progress < 1) requestAnimationFrame(step);
    else node.textContent = text;
  };
  requestAnimationFrame(step);
}

function mountReveal() {
  const splits = document.querySelectorAll('[data-split]');
  if (!REDUCED_MOTION) splits.forEach(splitWords);
  const items = document.querySelectorAll('[data-reveal], [data-split]');
  const show = (node) => {
    node.classList.add('in');
    if (!REDUCED_MOTION) node.querySelectorAll('[data-scramble]').forEach((value, i) => scramble(value, i * 140));
  };
  const loaders = document.querySelectorAll('[data-split="load"]');
  requestAnimationFrame(() => requestAnimationFrame(() => loaders.forEach(show)));
  if (!('IntersectionObserver' in window)) {
    items.forEach(show);
    return;
  }
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      show(entry.target);
      observer.unobserve(entry.target);
    }
  }, { rootMargin: '0px 0px -10% 0px' });
  items.forEach((item) => {
    if (item.dataset.split !== 'load') observer.observe(item);
  });
}

function mountHeader(top) {
  const bar = top.querySelector('[data-progress]');
  const runner = top.querySelector('[data-runner]');
  let idle = 0;
  let runnerWidth = 0;
  layouts.push(() => {
    runnerWidth = runner.offsetWidth;
  });
  scrollers.push((y) => {
    top.classList.toggle('is-scrolled', y > 8);
    const progress = clamp(y / viewport.doc);
    bar.style.transform = `scaleX(${progress})`;
    runner.style.transform = `translate3d(${progress * (viewport.w - runnerWidth)}px, 0, 0)`;
    if (REDUCED_MOTION) return;
    runner.classList.add('is-walking');
    clearTimeout(idle);
    idle = setTimeout(() => runner.classList.remove('is-walking'), 160);
  });
}

function mountHero(hero) {
  if (REDUCED_MOTION) return;
  const [first, second] = hero.querySelectorAll('.display .line');
  const copy = hero.querySelector('.hero-copy');
  const mascot = hero.querySelector('.hero-mascot');
  const stage = hero.querySelector('[data-tilt]');
  const screen = stage.querySelector('[data-screen]');
  const heroItem = track(hero);
  const stageItem = track(stage);
  scrollers.push((y) => {
    const p = clamp((y - heroItem.top) / (viewport.h * 0.9));
    first.style.translate = `${-p * 14}vw 0`;
    second.style.translate = `${p * 10}vw 0`;
    copy.style.translate = `0 ${-p * 60}px`;
    mascot.style.translate = `0 ${-p * 140}px`;
    const t = easeOut(enter(stageItem, y, 1, 0.28));
    screen.style.transform = `rotateX(${(1 - t) * 26}deg) scale(${0.9 + t * 0.1}) translate3d(0, ${(1 - t) * 40}px, 0)`;
  });
}

function mountKai(kai) {
  let cheer = 0;
  kai.addEventListener('click', () => {
    kai.classList.remove('cheer');
    void kai.offsetWidth;
    kai.classList.add('cheer');
    clearTimeout(cheer);
    cheer = setTimeout(() => kai.classList.remove('cheer'), 1300);
  });
}

function mountMarquee(rows) {
  if (REDUCED_MOTION) return;
  rows.forEach((row) => {
    const trackNode = row.querySelector('.marquee-track');
    const clone = trackNode.cloneNode(true);
    clone.setAttribute('aria-hidden', 'true');
    row.append(clone);
    const direction = Number(row.dataset.marquee);
    let width = 1;
    let visible = false;
    let running = false;
    let drift = 0;
    let last = 0;
    layouts.push(() => {
      width = trackNode.offsetWidth || 1;
    });
    const loop = (time) => {
      if (!visible) {
        running = false;
        return;
      }
      drift += Math.min(time - (last || time), 64) * 0.045;
      last = time;
      const offset = (drift + scrollY * 0.55) % width;
      const x = direction > 0 ? -offset : offset - width;
      trackNode.style.transform = `translate3d(${x}px, 0, 0)`;
      clone.style.transform = `translate3d(${x}px, 0, 0)`;
      requestAnimationFrame(loop);
    };
    whenVisible(row, (isVisible) => {
      visible = isVisible;
      if (visible && !running) {
        running = true;
        last = 0;
        requestAnimationFrame(loop);
      }
    }, '0px');
  });
}

function mountChaos(section) {
  if (REDUCED_MOTION) return;
  section.classList.add('is-live');
  const field = section.querySelector('[data-chaos-field]');
  const wins = [...section.querySelectorAll('.win')];
  const rows = [...section.querySelectorAll('.chaos-rows [data-row]')];
  const question = section.querySelector('.chaos-q');
  const answer = section.querySelector('.chaos-a');
  const before = section.querySelector('.chaos-before');
  const after = section.querySelector('.chaos-after');
  const item = track(section);
  let poses = [];
  layouts.push(() => {
    const box = field.getBoundingClientRect();
    poses = wins.map((win, i) => {
      const row = rows[i].getBoundingClientRect();
      const width = win.offsetWidth;
      const height = win.offsetHeight;
      const scale = clamp((row.height * 1.4) / height, 0.18, 1);
      const [fx, fy, rotate] = CHAOS_SCATTER[i];
      return {
        from: [fx * (box.width - width), fy * (box.height - height), rotate],
        to: [row.left - box.left, row.top - box.top + row.height / 2 - (height * scale) / 2, 0],
        scale,
      };
    });
  });
  const topOffset = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--top-h')) || 64;
  let offset = 64;
  layouts.push(() => {
    offset = topOffset();
  });
  scrollers.push((y) => {
    if (!poses.length) return;
    const p = pinned(item, y, offset);
    wins.forEach((win, i) => {
      const pose = poses[i];
      const appear = easeOut(span(p, 0.01 + i * 0.03, 0.1));
      const converge = span(p, 0.46 + i * 0.03, 0.2);
      const e = easeInOut(converge);
      const sway = (1 - e) * (p * 90 - 30) * (i % 2 ? 1 : -1);
      const x = lerp(pose.from[0], pose.to[0], e);
      const yy = lerp(pose.from[1] + sway, pose.to[1], e);
      const rotate = lerp(pose.from[2], 0, e);
      const scale = lerp(0.86 + 0.14 * appear, pose.scale, e);
      win.style.transform = `translate3d(${x}px, ${yy}px, 0) rotate(${rotate}deg) scale(${scale})`;
      win.style.opacity = String(appear * (1 - span(converge, 0.62, 0.38)));
      win.classList.toggle('is-ringing', converge === 0 && p > 0.2 + (i % 3) * 0.04 && (win.classList.contains('is-ask') || win.classList.contains('is-done')));
      rows[i].style.opacity = String(span(converge, 0.72, 0.28));
    });
    section.style.setProperty('--built', String(span(p, 0.48, 0.14)));
    const swap = span(p, 0.7, 0.08);
    question.style.opacity = String(1 - swap);
    question.style.translate = `0 ${-swap * 24}px`;
    answer.style.opacity = String(span(p, 0.75, 0.08));
    answer.style.translate = `0 ${(1 - span(p, 0.75, 0.08)) * 24}px`;
    before.style.opacity = String(1 - span(p, 0.6, 0.08));
    after.style.opacity = String(span(p, 0.78, 0.08));
  });
}

function mountTour(root) {
  root.classList.add('is-live');
  const screen = root.querySelector('[data-tour-tui]');
  const part = (name) => screen.querySelector(`[data-t-${name}]`);
  const title = part('title');
  const bell = part('bell');
  const count = part('count');
  const list = part('list');
  const input = part('input');
  const worktree = part('wt');
  const hint = part('hint');
  const keycap = part('key');
  const steps = [...root.querySelectorAll('.tour-step')];
  const dots = [...root.querySelectorAll('.tour-dots li')];
  const state = { rows: [], selected: 0, input: null, worktree: false, notice: null };
  let token = 0;
  let active = -1;
  let visible = false;

  const base = (now) => [
    { id: 1, name: 'checkout-redesign', kind: 'claude', state: 'done', activity: 'Running design update and committing changes', since: now - 32_000 },
    { id: 2, name: 'payment-tests', kind: 'codex', state: 'working', activity: 'Writing integration tests for the payment flow', since: now - 64_000 },
    { id: 3, name: 'simulator-builds', kind: 'claude', state: 'working', activity: 'Building and launching both apps in simulators', since: now - 145_000 },
  ];
  const spawned = (now) => ({ id: 4, name: slugify(TOUR_TASK), kind: 'claude', state: 'working', activity: 'Reading src/auth/login.test.ts', since: now - 9_000 });
  const row = (id) => state.rows.find((r) => r.id === id);

  function describe(r, now) {
    if (r.state === 'lost') return { mark: '↻', markTone: 'red', nameTone: 'faint', time: '', timeTone: 'dim', activity: 'interrupted — ctrl+r to recover', activityTone: 'red' };
    if (r.starting) return { mark: '◌', markTone: 'dim', nameTone: 'text', time: durationShort(now - r.since), timeTone: 'yellow', activity: 'starting…', activityTone: 'dim' };
    const [mark, markTone] = glyph(r.kind, r.state, frame);
    const nameTone = { done: 'green', paused: 'yellow' }[r.state] || 'text';
    const timeTone = r.state === 'done' ? 'dim' : 'yellow';
    const activity = r.state === 'paused' ? `waiting for you · ${r.activity}` : r.activity;
    const activityTone = { done: 'muted', paused: 'yellow' }[r.state] || 'text';
    return { mark, markTone, nameTone, time: durationShort(now - r.since), timeTone, activity, activityTone };
  }

  function render(now) {
    const waiting = state.rows.filter((r) => r.state === 'paused').length;
    const done = state.rows.filter((r) => r.state === 'done').length;
    const parts = [waiting && `‖${waiting}`, done && `✔${done}`].filter(Boolean);
    setText(title, parts.length ? `${parts.join(' ')} · Octo` : 'Octo');
    setText(count, String(state.rows.length));
    setText(worktree, state.worktree ? 'on' : 'off');
    setClass(worktree, state.worktree ? 't-pink' : 't-dim');
    setText(hint, state.notice ? state.notice.text : HINT);
    setClass(hint, state.notice ? `tui-hint is-${state.notice.tone}` : 'tui-hint t-dim');
    syncChildren(list, state.rows, (r) => `t:${r.id}`, () => {
      const node = agentRow();
      node.children[5].remove();
      node.children[5].after(el('span', 'c-wt'));
      return node;
    }, (node, r) => {
      const d = describe(r, now);
      const [num, sel, mark, name, time, kind, wt, activity] = node.children;
      node.classList.toggle('is-lost', r.state === 'lost');
      setText(num, String(r.id));
      setText(sel, state.selected === r.id ? '▸' : ' ');
      setText(mark, d.mark);
      setClass(mark, `c-g t-${d.markTone}`);
      setText(name, r.name);
      setClass(name, `c-name t-${d.nameTone}`);
      setText(time, d.time);
      setClass(time, `c-t1 t-${d.timeTone}`);
      setText(kind, r.kind);
      setClass(kind, `c-kind k-${r.kind}`);
      const badge = r.wt ? `${r.wt.files}|${r.wt.plus}|${r.wt.minus}` : '';
      if (wt.dataset.badge !== badge) {
        wt.dataset.badge = badge;
        wt.replaceChildren(...(r.wt ? [el('span', 't-link', '⎇ '), el('span', 't-dim', `${r.wt.files} files`), el('span', 't-green', ` +${r.wt.plus}`), el('span', 't-red', ` −${r.wt.minus}`)] : []));
      }
      setText(activity, d.activity);
      setClass(activity, `c-act t-${d.activityTone}`);
    });
    input.replaceChildren(state.input ? el('span', '', state.input) : el('span', 't-faint', 'write a task'));
    if (state.input !== null) input.append(el('span', 'caret'));
  }

  const now = () => Date.now();
  const alive = (mine) => mine === token;

  async function wait(mine, ms) {
    await sleep(REDUCED_MOTION ? Math.min(ms, 300) : ms);
    return alive(mine);
  }

  async function type(mine, text) {
    for (let i = 1; i <= text.length; i += 1) {
      if (!alive(mine)) return false;
      state.input = text.slice(0, i);
      render(now());
      if (!REDUCED_MOTION) await sleep(45);
    }
    return alive(mine);
  }

  function focus(id) {
    state.selected = id;
    render(now());
    flash(list.querySelector(`[data-key="t:${id}"]`));
  }

  const scenes = [
    async (mine) => {
      Object.assign(state, { rows: base(now()), selected: 3, input: '', worktree: false, notice: null });
      render(now());
      if (!(await wait(mine, 500)) || !(await type(mine, TOUR_TASK)) || !(await wait(mine, 380))) return;
      showKey(keycap, 'enter');
      state.input = null;
      state.rows.push({ ...spawned(now()), since: now(), starting: true });
      focus(4);
      if (!(await wait(mine, 1_000))) return;
      row(4).starting = false;
    },
    async (mine) => {
      Object.assign(state, { rows: [...base(now()), spawned(now())], selected: 4, input: null, worktree: false, notice: null });
      render(now());
      if (!(await wait(mine, 700))) return;
      Object.assign(row(2), { state: 'paused', activity: 'Run the migrations now?', since: now() });
      state.notice = { tone: 'warn', text: 'payment-tests is waiting for you · alt+n opens it' };
      ringBell(bell);
      render(now());
      if (!(await wait(mine, 1_800))) return;
      Object.assign(row(3), { state: 'done', since: now() });
      ringBell(bell);
      render(now());
    },
    async (mine) => {
      const rows = [...base(now()), spawned(now())];
      Object.assign(rows[1], { state: 'paused', activity: 'Run the migrations now?', since: now() - 6_000 });
      Object.assign(rows[2], { state: 'done', since: now() - 3_000 });
      Object.assign(state, { rows, selected: 4, input: null, worktree: false, notice: { tone: 'warn', text: 'payment-tests is waiting for you · alt+n opens it' } });
      render(now());
      if (!(await wait(mine, 1_000))) return;
      showKey(keycap, 'alt+n');
      Object.assign(row(2), { state: 'working', activity: 'Running the migrations', since: now() });
      state.notice = null;
      focus(2);
    },
    async (mine) => {
      const rows = [...base(now()), spawned(now())];
      Object.assign(rows[1], { activity: 'Running the migrations', since: now() - 12_000 });
      Object.assign(state, { rows, selected: 2, input: null, worktree: false, notice: null });
      render(now());
      if (!(await wait(mine, 500))) return;
      showKey(keycap, 'ctrl+t');
      state.worktree = true;
      render(now());
      if (!(await wait(mine, 700))) return;
      row(4).wt = { files: 4, plus: 12, minus: 3 };
      flash(list.querySelector('[data-key="t:4"]'));
      render(now());
      if (!(await wait(mine, 600))) return;
      row(2).wt = { files: 2, plus: 31, minus: 0 };
      flash(list.querySelector('[data-key="t:2"]'));
      render(now());
      if (!(await wait(mine, 900))) return;
      await type(mine, '/diff');
    },
    async (mine) => {
      const rows = [...base(now()), spawned(now())].map((r) => ({ ...r, state: 'lost' }));
      Object.assign(state, { rows, selected: 2, input: null, worktree: false, notice: null });
      render(now());
      if (!(await wait(mine, 1_400))) return;
      showKey(keycap, 'ctrl+r');
      const restored = base(now());
      for (const r of state.rows) {
        if (!(await wait(mine, 320))) return;
        const original = restored.find((o) => o.id === r.id);
        Object.assign(r, { state: r.id === 1 ? 'done' : 'working', activity: original ? original.activity : 'Reading src/auth/login.test.ts', since: now() });
        render(now());
        flash(list.querySelector(`[data-key="t:${r.id}"]`));
      }
    },
  ];

  function activate(index) {
    if (index === active) return;
    active = index;
    token += 1;
    steps.forEach((s, i) => s.classList.toggle('is-active', i === index));
    dots.forEach((d, i) => d.classList.toggle('is-on', i <= index));
    scenes[index](token);
  }

  tickers.add((time) => {
    if (visible) render(time);
  });
  screen.classList.add('is-ready');
  whenVisible(root, (isVisible) => {
    visible = isVisible;
    if (visible && active < 0) activate(0);
  }, '0px');
  if (!('IntersectionObserver' in window)) return;
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) activate(steps.indexOf(entry.target));
    }
  }, { rootMargin: '-48% 0px -48% 0px' });
  steps.forEach((s) => observer.observe(s));
}

function mountPlayer(root) {
  const video = root.querySelector('[data-video]');
  root.querySelector('[data-play]').addEventListener('click', () => {
    root.classList.add('is-playing');
    video.controls = true;
    video.focus();
    video.play().catch(() => {});
  });
  if (REDUCED_MOTION) return;
  const item = track(root);
  scrollers.push((y) => {
    const p = easeOut(enter(item, y, 1, 0.32));
    root.style.clipPath = `inset(${(1 - p) * 9}% ${(1 - p) * 16}% ${(1 - p) * 9}% ${(1 - p) * 16}%)`;
    video.style.transform = `scale(${1.2 - p * 0.2})`;
  });
}

function mountArch(root) {
  const label = root.querySelector('[data-arch-state]');
  const original = label.textContent;
  let visible = false;
  whenVisible(root, (isVisible) => {
    visible = isVisible;
  });
  (async () => {
    for (;;) {
      await sleep(3_200);
      if (!visible || document.hidden) continue;
      root.classList.add('is-closed');
      setText(label, 'closed · agents keep running');
      await sleep(2_400);
      root.classList.remove('is-closed');
      setText(label, 'reconnected to all');
      await sleep(1_600);
      setText(label, original);
    }
  })();
}

function mountKeys(list) {
  const items = new Map([...list.querySelectorAll('[data-key]')].map((item) => [item.dataset.key, item]));
  const press = (name) => {
    const item = items.get(name);
    if (!item) return;
    item.classList.add('is-pressed');
    setTimeout(() => item.classList.remove('is-pressed'), 260);
  };
  addEventListener('keydown', (event) => {
    const key = event.key.toLowerCase();
    if (key === 'enter' || key === 'tab' || key === '/' || key === '!') press(key);
    else if (event.ctrlKey && ['l', 't', ']', 'r'].includes(key)) press(`ctrl+${key}`);
    else if (event.altKey && (key === 'n' || event.code === 'KeyN')) press('alt+n');
  });
  if (REDUCED_MOTION) return;
  let played = false;
  whenVisible(list, async (isVisible) => {
    if (!isVisible || played) return;
    played = true;
    await sleep(500);
    for (const name of items.keys()) {
      press(name);
      await sleep(110);
    }
  }, '0px 0px -20% 0px');
}

function mountFinale(section) {
  const canvas = section.querySelector('[data-pixels]');
  const context = canvas.getContext('2d');
  const mascot = section.querySelector('[data-walk]');
  const item = track(section);
  const cell = 22;
  const pink = getComputedStyle(document.documentElement).getPropertyValue('--pink').trim();
  let columns = 0;
  let rowsCount = 0;
  let noise = [];
  let lastDrawn = -1;
  let idle = 0;
  layouts.push(() => {
    const ratio = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(canvas.clientWidth * ratio);
    canvas.height = Math.round(canvas.clientHeight * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    columns = Math.ceil(canvas.clientWidth / cell);
    rowsCount = Math.ceil(canvas.clientHeight / cell);
    noise = Array.from({ length: columns * rowsCount }, (_, i) => {
      const s = Math.sin(i * 12.9898 + 78.233) * 43758.5453;
      return s - Math.floor(s);
    });
    lastDrawn = -1;
  });
  const draw = (p) => {
    if (Math.abs(p - lastDrawn) < 0.004) return;
    lastDrawn = p;
    const height = canvas.clientHeight;
    context.clearRect(0, 0, canvas.clientWidth, height);
    context.fillStyle = pink;
    for (let r = 0; r < rowsCount; r += 1) {
      for (let c = 0; c < columns; c += 1) {
        const threshold = (r / rowsCount) * 0.8 + noise[r * columns + c] * 0.35;
        if (p * 0.95 > threshold) context.fillRect(c * cell, height - (r + 1) * cell, cell, cell);
      }
    }
  };
  scrollers.push((y) => {
    if (!columns) return;
    draw(REDUCED_MOTION ? 1 : easeOut(enter(item, y, 1.15, 0.55)));
    if (REDUCED_MOTION) return;
    const walk = easeOut(enter(item, y, 0.95, 0.35));
    mascot.style.transform = `translate3d(${(1 - walk) * -45}vw, 0, 0)`;
    if (walk > 0 && walk < 1) {
      mascot.classList.add('is-walking');
      clearTimeout(idle);
      idle = setTimeout(() => mascot.classList.remove('is-walking'), 160);
    }
  });
}

mountReveal();
mountHeader(document.querySelector('[data-top]'));
mountHero(document.querySelector('.hero'));
mountKai(document.querySelector('[data-kai]'));
mountMarquee(document.querySelectorAll('[data-marquee]'));
mountChaos(document.querySelector('[data-chaos]'));
mountTour(document.querySelector('[data-tour]'));
mountPlayer(document.querySelector('[data-player]'));
mountArch(document.querySelector('[data-arch]'));
mountKeys(document.querySelector('[data-keys]'));
mountFinale(document.querySelector('[data-finale]'));
startClock();
startScroll();
