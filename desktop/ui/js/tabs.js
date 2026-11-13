import { rename } from './agent.js';
import { $, h, svg } from './dom.js';
import { closeMenu, menuShown, openMenu } from './dialogs.js';
import { initTabDrag } from './drag.js';
import { OVERVIEW_TAB, glyph, moveTab, tabRange } from './format.js';
import { t } from './i18n.js';
import { ICON_CHEVRON_DOWN, ICON_OVERVIEW, ICON_PIN } from './icons.js';
import { agents, findAgent, pinTab, saveTabs, state } from './state.js';

let actions = null;
let rowPainters = [];

export function tabShown(id) {
  return id === OVERVIEW_TAB ? state.active === null : state.active === id;
}

function pageBadge(id) {
  const page = actions.pages[id];
  return page.badge ? page.badge() : 0;
}

function closable(id) {
  return id !== OVERVIEW_TAB || state.tabs.length > 1;
}

function tabInfo(id) {
  if (id === OVERVIEW_TAB) return { name: t('Overview'), mark: svg(ICON_OVERVIEW), labelClass: 'tab-label' };
  if (Object.hasOwn(actions.pages, id)) {
    const page = actions.pages[id];
    return { name: page.label, mark: svg(page.icon), labelClass: 'tab-label', badge: pageBadge(id) };
  }
  const agent = findAgent(id);
  if (!agent) return null;
  const mark = glyph(agent, state.frame);
  return { name: agent.name, mark: h('span', { class: `tab-glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text), labelClass: 'label-mono' };
}

function setTabOpen(id, open) {
  if (!open) {
    actions.close(id);
    return;
  }
  if (state.tabs.includes(id)) return;
  state.tabs.push(id);
  saveTabs();
  renderTabs();
}

function setTabPinned(id, on) {
  if (on && !state.tabs.includes(id)) state.tabs.push(id);
  pinTab(id, on);
  renderTabs();
}

function shiftTab(id, step) {
  state.tabs = moveTab(state.tabs, state.pinned, id, state.tabs.indexOf(id) + step);
  saveTabs();
  renderTabs();
}

function closer(id, name) {
  return h(
    'span',
    {
      class: 'tab-close',
      role: 'button',
      'aria-label': t('Close {name}', { name }),
      onclick: (event) => {
        event.stopPropagation();
        actions.close(id);
      },
    },
    '×',
  );
}

function unpinner(id, name) {
  return h(
    'span',
    {
      class: 'tab-pin',
      role: 'button',
      'aria-label': t('Unpin {name}', { name }),
      title: t('Unpin {name}', { name }),
      onclick: (event) => {
        event.stopPropagation();
        setTabPinned(id, false);
      },
    },
    svg(ICON_PIN),
  );
}

function tabMenu(event, id) {
  event.preventDefault();
  const item = (text, act) =>
    h(
      'button',
      {
        class: 'menu-item',
        type: 'button',
        role: 'menuitem',
        onclick: () => {
          closeMenu();
          act();
        },
      },
      h('span', null, text),
    );
  const at = state.tabs.indexOf(id);
  const { low, high } = tabRange(state.tabs, state.pinned, id);
  const pinned = state.pinned.has(id);
  const items = [
    item(pinned ? t('Unpin tab') : t('Pin tab'), () => setTabPinned(id, !pinned)),
    at > low ? item(t('Move left'), () => shiftTab(id, -1)) : null,
    at < high ? item(t('Move right'), () => shiftTab(id, 1)) : null,
    closable(id) ? item(t('Close'), () => actions.close(id)) : null,
  ];
  openMenu(event.currentTarget, items.filter(Boolean), false);
}

function tabButton(id) {
  const info = tabInfo(id);
  if (!info) return null;
  const pinned = state.pinned.has(id);
  const classes = ['tab', tabShown(id) ? 'active' : '', pinned ? 'pinned' : '', state.movingTab === id ? 'moving' : ''].filter(Boolean).join(' ');
  return h(
    'div',
    {
      class: classes,
      role: 'button',
      tabindex: '0',
      dataset: { tab: String(id) },
      onclick: () => actions.open(id),
      ondblclick: findAgent(id) ? () => rename(findAgent(id)) : null,
      oncontextmenu: (event) => tabMenu(event, id),
      onkeydown: (event) => {
        if (event.key !== 'Enter' && event.key !== ' ') return;
        event.preventDefault();
        actions.open(id);
      },
    },
    info.mark,
    h('span', { class: info.labelClass }, info.name),
    info.badge ? h('span', { class: 'tab-badge' }, String(info.badge)) : null,
    pinned ? unpinner(id, info.name) : closable(id) ? closer(id, info.name) : null,
  );
}

function tabRow(id) {
  const info = tabInfo(id);
  const check = h('span', { class: 'tab-row-check' });
  const main = h(
    'button',
    { class: 'menu-item tab-row-main', type: 'button', role: 'menuitemcheckbox', onclick: () => setTabOpen(id, !state.tabs.includes(id)) },
    check,
    info.mark,
    h('span', { class: 'tab-row-name' }, info.name),
  );
  const pin = h('button', { class: 'icon-button tab-row-pin', type: 'button', onclick: () => setTabPinned(id, !state.pinned.has(id)) }, svg(ICON_PIN));
  rowPainters.push(() => {
    const open = state.tabs.includes(id);
    const pinned = state.pinned.has(id);
    const text = pinned ? t('Unpin {name}', { name: info.name }) : t('Pin {name}', { name: info.name });
    check.textContent = open ? '✓' : '';
    main.setAttribute('aria-checked', String(open));
    main.disabled = open && !closable(id);
    pin.setAttribute('aria-pressed', String(pinned));
    pin.setAttribute('aria-label', text);
    pin.title = text;
  });
  return h('div', { class: 'tab-row' }, main, pin);
}

function paintRows() {
  for (const paint of rowPainters) paint();
}

function openTabsMenu() {
  rowPainters = [];
  const ids = [OVERVIEW_TAB, ...Object.keys(actions.pages), ...agents().map((agent) => agent.id)];
  const button = $('tabs-menu');
  const list = h('div', { class: 'tab-rows' }, ids.map(tabRow));
  paintRows();
  openMenu(button, [list], false, () => {
    rowPainters = [];
    button.setAttribute('aria-expanded', 'false');
  });
  button.setAttribute('aria-expanded', 'true');
}

export function renderTabs() {
  const host = $('tabs');
  const signature = JSON.stringify([state.active, state.movingTab, [...state.pinned], state.tabs.map((id) => {
    if (id === OVERVIEW_TAB) return id;
    if (Object.hasOwn(actions.pages, id)) return [id, pageBadge(id)];
    const agent = findAgent(id);
    return agent ? [id, agent.name, agent.state, agent.kind, agent.starting] : null;
  })]);
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  for (const page of Object.keys(actions.pages)) $(`open-${page}`).setAttribute('aria-current', String(state.active === page));
  host.replaceChildren(...state.tabs.map(tabButton).filter(Boolean));
  paintRows();
}

export function initTabs(tabActions) {
  actions = tabActions;
  const button = $('tabs-menu');
  button.replaceChildren(svg(ICON_CHEVRON_DOWN));
  button.addEventListener('click', (event) => {
    event.stopPropagation();
    if (!menuShown()) openTabsMenu();
    else closeMenu();
  });
  initTabDrag(renderTabs);
}
