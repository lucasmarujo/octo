import { closeDialog, openDialog } from './dialogs.js';
import { $, h, storage, svg } from './dom.js';
import { t } from './i18n.js';
import {
  ICON_ALERT, ICON_BRANCH, ICON_CHEVRON_RIGHT, ICON_CLOCK, ICON_CODE, ICON_DESKTOP, ICON_DOCUMENT,
  ICON_DOWNLOAD, ICON_DRIVE, ICON_EYE, ICON_FOLDER, ICON_FOLDER_UP, ICON_HOME, ICON_SEARCH,
} from './icons.js';
import { state, worktreeOn } from './state.js';
import { send } from './transport.js';

const MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const RECENT_KEY = 'octo.recentFolders';
const HIDDEN_KEY = 'octo.hiddenFolders';
const RECENT_FOLDERS = 8;
const SEARCH_DELAY_MS = 160;
const MIN_DEEP_QUERY = 2;
const ENOUGH_WHOLE = 3;
const PATH_LIKE = /^(~|\/|\\|[A-Za-z]:[\\/])/;
const PLACE_ICONS = { home: ICON_HOME, desktop: ICON_DESKTOP, documents: ICON_DOCUMENT, downloads: ICON_DOWNLOAD, code: ICON_CODE, volume: ICON_DRIVE };

let current = null;
let requests = 0;

const trimSep = (path) => (path.length > 1 ? path.replace(/[\\/]+$/, '') : path);
const baseName = (path) => trimSep(path).split(/[\\/]/).at(-1) || path;
const samePath = (a, b) => Boolean(a && b) && trimSep(a).toLowerCase() === trimSep(b).toLowerCase();
const isBoundary = (text, at) => at === 0 || /[-_. /]/.test(text[at - 1]) || (/[a-z]/.test(text[at - 1]) && /[A-Z]/.test(text[at]));

export function fuzzy(query, text) {
  const q = query.toLowerCase();
  const lower = text.toLowerCase();
  if (!q || q.length > lower.length || lower.length !== text.length) return null;
  const extra = lower.length - q.length;
  const run = (at) => Array.from({ length: q.length }, (_, i) => at + i);
  if (lower === q) return { score: 1000, marks: run(0), whole: true };
  if (lower.startsWith(q)) return { score: 800 - extra, marks: run(0), whole: true };
  const found = [];
  for (let at = lower.indexOf(q); at >= 0; at = lower.indexOf(q, at + 1)) found.push(at);
  const word = found.find((at) => isBoundary(text, at));
  if (word !== undefined) return { score: 650 - word - Math.floor(extra / 2), marks: run(word), whole: true };
  if (found.length) return { score: 500 - found[0] - Math.floor(extra / 2), marks: run(found[0]), whole: true };
  let first = -1;
  for (let at = 0; at < lower.length; at++) if (lower[at] === q[0] && isBoundary(text, at)) { first = at; break; }
  if (first < 0) return null;
  const marks = [first];
  let gaps = 0;
  let starts = 1;
  for (const wanted of q.slice(1)) {
    const at = lower.indexOf(wanted, marks.at(-1) + 1);
    if (at < 0) return null;
    gaps += at - marks.at(-1) - 1;
    starts += isBoundary(text, at) ? 1 : 0;
    marks.push(at);
  }
  if (marks.at(-1) + 1 - first > q.length * 3) return null;
  return { score: 300 - gaps * 6 + starts * 25 - Math.floor(extra / 4), marks, whole: false };
}

function marked(name, marks) {
  if (!marks?.length) return name;
  const set = new Set(marks);
  const parts = [];
  let at = 0;
  while (at < name.length) {
    const on = set.has(at);
    let end = at;
    while (end < name.length && set.has(end) === on) end++;
    parts.push(on ? h('mark', null, name.slice(at, end)) : name.slice(at, end));
    at = end;
  }
  return parts;
}

export function folderCrumbs(path, home) {
  const sep = path.includes('\\') && !path.includes('/') ? '\\' : '/';
  const homeDir = home ? trimSep(home) : '';
  let first;
  let rest;
  if (homeDir && (samePath(path, homeDir) || path.toLowerCase().startsWith(`${homeDir}${sep}`.toLowerCase()))) [first, rest] = [{ name: '~', path: homeDir }, path.slice(homeDir.length)];
  else if (/^[A-Za-z]:/.test(path)) [first, rest] = [{ name: path.slice(0, 2), path: `${path.slice(0, 2)}${sep}` }, path.slice(2)];
  else [first, rest] = [{ name: sep, path: sep }, path];
  const crumbs = [first];
  let walked = first.path.replace(/[\\/]+$/, '');
  for (const part of rest.split(/[\\/]/).filter(Boolean)) {
    walked += sep + part;
    crumbs.push({ name: part, path: walked });
  }
  return crumbs;
}

export function splitTyped(text) {
  if (text === '~') return ['~', ''];
  const cut = Math.max(text.lastIndexOf('/'), text.lastIndexOf('\\'));
  return [text.slice(0, cut + 1), text.slice(cut + 1)];
}

export function emptyTypedNote(tail, typed) {
  if (!tail) return [t('No folders inside'), t('You can still use this folder.')];
  return [t('No folder starts with “{name}” here', { name: tail }), t('Enter goes to {path}', { path: typed })];
}

export function folderNote(repo, missing, worktree) {
  if (missing) return { tone: 'missing', icon: ICON_ALERT, text: t('This folder does not exist') };
  if (repo && worktree) return { tone: 'worktree', icon: ICON_BRANCH, text: t('{repo} is a git repository · each agent gets its own worktree', { repo: baseName(repo) }) };
  if (repo) return { tone: 'git', icon: ICON_BRANCH, text: t('{repo} is a git repository · worktree is off, agents work right in it', { repo: baseName(repo) }) };
  if (worktree) return { tone: 'blocked', icon: ICON_ALERT, text: t('Not a git repository · turn worktree off to start AI agents here') };
  return { tone: 'plain', icon: ICON_FOLDER, text: t('Not a git repository · agents work right in this folder') };
}

function recentFolders() {
  const overview = state.overview || {};
  const saved = storage.json(RECENT_KEY, []);
  const seen = [...(Array.isArray(saved) ? saved : []), overview.whereDir, ...Object.values(state.where), ...Object.values(overview.dirs || {})];
  const unique = [];
  for (const path of seen) if (typeof path === 'string' && path && path !== '~' && !unique.some((known) => samePath(known, path))) unique.push(path);
  return unique.slice(0, RECENT_FOLDERS);
}

function remember(path) {
  const saved = storage.json(RECENT_KEY, []);
  const list = [path, ...(Array.isArray(saved) ? saved : []).filter((known) => typeof known === 'string' && !samePath(known, path))];
  storage.set(RECENT_KEY, JSON.stringify(list.slice(0, RECENT_FOLDERS)));
}

function keyHints() {
  const key = (label, text) => [h('kbd', null, label), h('span', null, text)];
  return h('p', { class: 'folder-keys', id: 'folder-keys' },
    key('↑↓', t('move')), key('↵', t('open')), key('Tab', t('complete')), key(MAC ? '⌘↵' : 'Ctrl+↵', t('use')));
}

export function pickFolder(start, onPick) {
  const dialog = $('dialog');
  const pick = {
    listing: null, places: null, hidden: storage.get(HIDDEN_KEY) === 'on', active: -1, shown: [],
    typed: null, keepFilter: false, listed: 0, search: null, timer: 0, activePath: null,
  };
  current = pick;

  const filter = h('input', {
    class: 'folder-filter', id: 'folder-filter', type: 'text', role: 'combobox', spellcheck: 'false', autocomplete: 'off', autocapitalize: 'off',
    'aria-controls': 'folder-list', 'aria-expanded': 'true', 'aria-autocomplete': 'list', 'aria-describedby': 'folder-keys',
    placeholder: t('Search folders or type a path'), 'aria-label': t('Search folders or type a path'),
  });
  const searching = h('span', { class: 'folder-searching', 'aria-hidden': 'true' });
  const up = h('button', { class: 'folder-icon-button', type: 'button', title: `${t('Up one folder')} (${MAC ? '⌘↑' : 'Alt+↑'})`, 'aria-label': t('Up one folder'), onclick: () => goUp() }, svg(ICON_FOLDER_UP));
  const hiddenToggle = h('button', {
    class: 'folder-icon-button folder-hidden-toggle', type: 'button', 'aria-pressed': String(pick.hidden),
    title: `${t('Show hidden folders')} (${MAC ? '⌘⇧.' : 'Ctrl+H'})`, 'aria-label': t('Show hidden folders'), onclick: () => toggleHidden(),
  }, svg(ICON_EYE));
  const crumbs = h('nav', { class: 'folder-crumbs', id: 'folder-crumbs', 'aria-label': t('Folder path') });
  const list = h('div', { class: 'folder-list', id: 'folder-list', role: 'listbox', 'aria-label': t('Folders'), 'aria-busy': 'true' }, h('div', { class: 'folder-note' }, t('Loading…')));
  const places = h('div', { class: 'folder-places', role: 'group', 'aria-label': t('Places') });
  const status = h('p', { class: 'folder-status', id: 'folder-status', 'aria-live': 'polite' });
  const use = h('button', { class: 'button primary folder-use', type: 'button', onclick: () => choose() }, t('Use this folder'));

  const browse = (path, keepFilter = false) => {
    pick.keepFilter = keepFilter;
    if (!keepFilter) filter.value = '';
    if (pick.listing) list.setAttribute('aria-busy', 'true');
    pick.listed = ++requests;
    send({ type: 'folders', path, hidden: pick.hidden, places: !pick.places, seq: pick.listed });
  };
  const goUp = () => pick.listing?.parent && browse(pick.listing.parent);
  const target = () => pick.shown[pick.active] || null;
  const query = () => filter.value.trim();
  const pathMode = () => PATH_LIKE.test(query());

  function toggleHidden() {
    pick.hidden = !pick.hidden;
    storage.set(HIDDEN_KEY, pick.hidden ? 'on' : null);
    hiddenToggle.setAttribute('aria-pressed', String(pick.hidden));
    pick.search = null;
    if (pick.listing) browse(pick.typed ?? pick.listing.path, Boolean(query()));
  }

  function choose(item = target()) {
    const listing = pick.listing;
    if (!listing || (!item && listing.missing)) return;
    const path = item ? item.path : listing.path;
    remember(path);
    closeDialog();
    onPick(path);
  }

  function complete(item) {
    const sep = item.path.includes('\\') && !item.path.includes('/') ? '\\' : '/';
    const home = state.home ? trimSep(state.home) : '';
    const shown = home && item.path.toLowerCase().startsWith(`${home}${sep}`.toLowerCase()) ? `~${item.path.slice(home.length)}` : item.path;
    filter.value = `${trimSep(shown)}${sep}`;
    update();
  }

  function setActive(index) {
    pick.active = index;
    pick.activePath = pick.shown[index]?.path ?? null;
    let row = null;
    list.querySelectorAll('[role="option"]').forEach((option) => {
      const on = Number(option.dataset.index) === index;
      option.setAttribute('aria-selected', String(on));
      if (on) row = option;
    });
    if (row) {
      filter.setAttribute('aria-activedescendant', row.id);
      row.scrollIntoView({ block: 'nearest' });
    } else filter.removeAttribute('aria-activedescendant');
    footer();
  }

  function footer() {
    const listing = pick.listing;
    if (!listing) return;
    const item = target();
    const name = item ? item.name : baseName(listing.path);
    use.disabled = !item && listing.missing;
    use.replaceChildren(h('span', null, t('Use {name}', { name })));
    use.title = item ? item.path : listing.path;
    const summary = folderNote(item ? item.repo : listing.repo, !item && listing.missing, worktreeOn());
    status.dataset.tone = summary.tone;
    status.replaceChildren(svg(summary.icon), h('span', null, summary.text));
  }

  function row(item, index, extra = {}) {
    return h('div', {
      class: `folder-row${item.rel !== undefined ? ' hit' : ''}`, id: `folder-option-${index}`, role: 'option', 'aria-selected': 'false',
      dataset: { index: String(index) }, title: item.path,
      onclick: () => setActive(index), ondblclick: () => browse(item.path),
    },
    svg(ICON_FOLDER),
    h('span', { class: 'folder-text' },
      h('span', { class: 'folder-name' }, marked(item.name, extra.marks)),
      item.rel !== undefined ? h('span', { class: 'folder-rel' }, `‎${item.rel || '.'}‎`) : null),
    item.git ? h('span', { class: 'folder-git', title: t('Git repository') }, svg(ICON_BRANCH), 'git') : h('span'),
    h('button', { class: 'folder-open', type: 'button', tabindex: '-1', title: t('Open {name}', { name: item.name }), 'aria-label': t('Open {name}', { name: item.name }), onclick: (event) => { event.stopPropagation(); browse(item.path); } }, svg(ICON_CHEVRON_RIGHT)));
  }

  const note = (text, detail) => h('div', { class: 'folder-note' }, text, detail ? h('span', null, detail) : null);
  const heading = (text) => h('div', { class: 'folder-heading', role: 'presentation' }, text);

  function ranked(dirs, text) {
    const scored = [];
    for (const dir of dirs) {
      const match = fuzzy(text, dir.name);
      if (match) scored.push({ dir, match });
    }
    const kept = scored.filter((entry) => entry.match.whole).length >= ENOUGH_WHOLE ? scored.filter((entry) => entry.match.whole) : scored;
    return kept.sort((a, b) => b.match.score - a.match.score || a.dir.name.localeCompare(b.dir.name));
  }

  function render() {
    const listing = pick.listing;
    if (!listing) return;
    const text = query();
    const rows = [];
    const items = [];
    const add = (item, extra) => {
      rows.push(row(item, items.length, extra));
      items.push(item);
    };
    const withRepo = (dir) => ({ ...dir, repo: dir.git ? dir.path : listing.repo });
    if (!text) {
      listing.dirs.forEach((dir) => add(withRepo(dir)));
      if (!items.length) rows.push(note(listing.missing ? t('This folder does not exist') : t('No folders inside'), listing.missing ? null : t('You can still use this folder.')));
      if (listing.truncated) rows.push(note(t('Showing the first {count} folders.', { count: listing.dirs.length }), t('Type to search all of them.')));
    } else if (pathMode()) {
      const [, tail] = splitTyped(text);
      const ready = pick.typed !== null && !pick.keepFilter;
      const matches = ready ? (tail ? ranked(listing.dirs, tail).filter((entry) => entry.match.whole) : listing.dirs.map((dir) => ({ dir, match: null }))) : [];
      for (const { dir, match } of matches) add(withRepo(dir), { marks: match?.marks });
      if (ready && listing.missing) rows.push(note(t('This folder does not exist'), t('Check the path, or press Enter to try it anyway.')));
      else if (ready && !items.length) rows.push(note(...emptyTypedNote(tail, text)));
    } else {
      const local = ranked(listing.dirs, text);
      const search = pick.search;
      const deep = search && search.query === text ? search.hits.filter((hit) => hit.rel) : [];
      const tail = text.replace(/\\/g, '/').split('/').at(-1);
      if (local.length && (deep.length || search?.pending)) rows.push(heading(t('In {name}', { name: baseName(listing.path) })));
      for (const { dir, match } of local) add(withRepo(dir), { marks: match.marks });
      if (deep.length) rows.push(heading(t('In subfolders')));
      for (const hit of deep) add(hit, { marks: fuzzy(tail, hit.name)?.marks });
      const waiting = text.length >= MIN_DEEP_QUERY && (!search || search.query !== text || search.pending);
      if (waiting) rows.push(note(t('Searching subfolders…')));
      else if (!items.length) rows.push(note(t('No folder matches “{query}”', { query: text }), t('Search looks inside {name} and its subfolders. Start with / or ~ to type a path.', { name: baseName(listing.path) })));
      else if (search?.truncated) rows.push(note(t('Stopped after the first thousands of folders.'), t('Open a folder closer to the project to search deeper.')));
    }
    pick.shown = items;
    list.replaceChildren(...rows);
    list.removeAttribute('aria-busy');
    searching.classList.toggle('on', Boolean(text) && !pathMode() && text.length >= MIN_DEEP_QUERY && (!pick.search || pick.search.pending || pick.search.query !== text));
    const kept = pick.activePath ? items.findIndex((item) => item.path === pick.activePath) : -1;
    const typingName = pathMode() ? Boolean(splitTyped(text)[1]) : Boolean(text);
    setActive(kept >= 0 ? kept : typingName && items.length ? 0 : -1);
  }

  function renderChrome() {
    const listing = pick.listing;
    up.disabled = !listing.parent;
    const parts = folderCrumbs(listing.path, state.home);
    crumbs.replaceChildren(...parts.flatMap((crumb, index) => {
      const last = index === parts.length - 1;
      const button = h('button', { class: 'folder-crumb', type: 'button', title: crumb.path, 'aria-current': last ? 'location' : null, onclick: () => !last && browse(crumb.path) },
        index === 0 && crumb.name === '~' ? [svg(ICON_HOME), h('span', { class: 'sr-only' }, t('Home'))] : crumb.name);
      return index ? [h('span', { class: 'folder-sep', 'aria-hidden': 'true' }, svg(ICON_CHEVRON_RIGHT)), button] : [button];
    }));
    crumbs.scrollLeft = crumbs.scrollWidth;
    renderPlaces();
  }

  function renderPlaces() {
    const listing = pick.listing;
    const place = (label, path, icon) => h('button', {
      class: 'folder-place', type: 'button', title: path, 'aria-current': samePath(path, listing.path) ? 'true' : null, onclick: () => browse(path),
    }, svg(icon), h('span', null, label));
    const known = pick.places || [{ kind: 'home', name: '', path: state.home || '~' }];
    const label = (place) => (place.kind === 'home' ? t('Home') : place.path === '/' ? t('Computer') : place.name);
    const group = (title, entries) => (entries.length ? [title ? h('span', { class: 'folder-places-label' }, title) : null, ...entries] : []);
    const of = (...kinds) => known.filter((entry) => kinds.includes(entry.kind)).map((entry) => place(label(entry), entry.path, PLACE_ICONS[entry.kind]));
    const recent = recentFolders().filter((path) => !known.some((entry) => samePath(entry.path, path)));
    places.replaceChildren(...[
      ...group(null, of('home', 'desktop', 'documents', 'downloads')),
      ...group(t('Recent'), recent.map((path) => place(baseName(path), path, ICON_CLOCK))),
      ...group(t('Projects'), of('code')),
      ...group(t('Drives'), of('volume')),
    ].filter(Boolean));
  }

  function searchDeep() {
    clearTimeout(pick.timer);
    const text = query();
    if (!pick.listing || !text || pathMode() || text.length < MIN_DEEP_QUERY) return;
    if (pick.search && pick.search.query === text && pick.search.root === pick.listing.path) return;
    pick.search = { query: text, root: pick.listing.path, hits: [], pending: true, truncated: false, seq: ++requests };
    pick.timer = setTimeout(() => send({ type: 'folderSearch', root: pick.listing.path, query: text, hidden: pick.hidden, seq: pick.search.seq }), SEARCH_DELAY_MS);
  }

  function update() {
    const text = query();
    pick.activePath = null;
    if (pathMode()) {
      const [folder] = splitTyped(text);
      if (folder && folder !== pick.typed) {
        pick.typed = folder;
        browse(folder, true);
      }
    } else {
      if (pick.typed !== null && pick.listing) pick.typed = null;
      searchDeep();
    }
    render();
  }

  pick.onListing = (message) => {
    if (message.seq !== pick.listed) return;
    const moved = !pick.listing || !samePath(pick.listing.path, message.path);
    pick.listing = { ...message, dirs: message.dirs || [] };
    if (message.places) pick.places = message.places;
    if (!pick.keepFilter) {
      pick.typed = null;
      if (moved) filter.value = '';
    }
    pick.keepFilter = false;
    if (moved) {
      pick.active = -1;
      pick.activePath = null;
      pick.search = null;
    }
    renderChrome();
    searchDeep();
    render();
  };

  pick.onSearch = (message) => {
    const search = pick.search;
    if (!search || message.seq !== search.seq) return;
    Object.assign(search, { hits: message.hits || [], truncated: message.truncated, pending: false });
    render();
  };

  filter.addEventListener('input', update);
  filter.addEventListener('keydown', (event) => {
    const count = pick.shown.length;
    const mod = MAC ? event.metaKey : event.ctrlKey;
    const empty = !filter.value;
    const item = target();
    const key = event.key;
    let handled = true;
    if (key === 'ArrowDown') setActive(count ? Math.min(pick.active + 1, count - 1) : -1);
    else if (key === 'ArrowUp' && (MAC ? event.metaKey : event.altKey)) goUp();
    else if (key === 'ArrowUp') setActive(Math.max(pick.active - 1, -1));
    else if (key === 'PageDown' || (key === 'End' && empty)) setActive(count - 1);
    else if (key === 'PageUp' || (key === 'Home' && empty)) setActive(count ? 0 : -1);
    else if (key === 'Enter' && mod) choose();
    else if (key === 'Enter' && item) browse(item.path);
    else if (key === 'Enter' && pathMode()) browse(query());
    else if (key === 'Enter' && !query()) choose();
    else if (key === 'Tab' && !event.shiftKey && item) complete(item);
    else if (key === 'ArrowRight' && item && filter.selectionStart === filter.value.length) browse(item.path);
    else if ((key === 'Backspace' || key === 'ArrowLeft') && empty) goUp();
    else if (key === 'Escape' && !empty) { filter.value = ''; update(); }
    else if ((MAC && mod && event.shiftKey && (key === '.' || key === '>')) || (!MAC && event.ctrlKey && key.toLowerCase() === 'h')) toggleHidden();
    else if ((mod && key.toLowerCase() === 'l') || (MAC && mod && event.shiftKey && key.toLowerCase() === 'g')) {
      filter.value = `${trimSep(folderCrumbs(pick.listing?.path || '~', state.home).map((crumb) => crumb.name).join('/').replace(/^\/\//, '/'))}/`;
      update();
    } else handled = false;
    if (handled) event.preventDefault();
  });

  dialog.classList.add('folder-picker');
  openDialog(t('Where should the agents run?'), [
    h('div', { class: 'folder-bar' }, h('label', { class: 'folder-search' }, svg(ICON_SEARCH), filter, searching), keyHints()),
    h('div', { class: 'folder-nav' }, up, crumbs, hiddenToggle),
    h('div', { class: 'folder-panes' }, places, list),
  ], [
    status,
    h('button', { class: 'button', type: 'button', onclick: closeDialog }, t('Cancel')),
    use,
  ]);
  const closed = () => {
    if (dialog.open) return;
    clearTimeout(pick.timer);
    if (current === pick) current = null;
    dialog.classList.remove('folder-picker');
    dialog.removeEventListener('close', closed);
  };
  dialog.addEventListener('close', closed);
  filter.focus();
  browse(start);
}

export function showFolders(message) {
  if (current && $('folder-list')) current.onListing(message);
}

export function showFolderSearch(message) {
  if (current && $('folder-list')) current.onSearch(message);
}
