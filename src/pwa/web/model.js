(function (root) {
  'use strict';

  const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  const USAGE_WARN = 70;
  const USAGE_ALERT = 90;
  const THEMES = ['dark', 'gray', 'light', 'system'];
  const THEME_INK = { dark: '#0e0e13', gray: '#282828', light: '#ffffff' };
  const COLUMNS = [
    { key: 'needs', title: 'Needs you', empty: 'No agent is waiting for you.' },
    { key: 'working', title: 'Working', empty: 'Nothing is running.' },
    { key: 'done', title: 'Done', empty: 'Finished agents land here.' },
    { key: 'idle', title: 'Idle', empty: 'No idle agents.' },
  ];
  const MARKS = { claude: '✻', codex: '>_', terminal: '$', pi: 'π' };
  const CHOICE = /^(?:[❯›>▶●○*]\s*)?([1-9])[.)]\s+(\S.*)$/;
  const HINT = /\s*\((?:esc|tab|shift\+tab|enter|ctrl\+[a-z])[^)]*\)\s*$/i;
  const BORDER = /^[\s│┃|╭╮╰╯─━┌┐└┘]+$/;
  const DANGER = /^(no|reject|deny|cancel|decline)\b/i;
  const CHOICE_WINDOW = 18;
  const CHOICE_GAP = 3;
  const CONTEXT_LINES = 3;
  const QUESTION_GAP = 4;

  function duration(ms) {
    const secs = Math.floor(Math.max(0, ms) / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h${String(mins % 60).padStart(2, '0')}`;
    return `${Math.floor(hours / 24)}d`;
  }

  function column(agent) {
    if (agent.starting) return 'working';
    if (agent.state === 'paused') return 'needs';
    if (agent.state === 'working') return 'working';
    if (agent.state === 'done') return 'done';
    return 'idle';
  }

  function moodOf(list) {
    const live = list.filter((agent) => !agent.starting);
    if (live.some((agent) => agent.state === 'paused')) return 'needs';
    if (list.some((agent) => agent.state === 'working' || agent.starting)) return 'working';
    if (live.some((agent) => agent.state === 'done')) return 'done';
    if (live.some((agent) => agent.state === 'lost')) return 'lost';
    return 'idle';
  }

  function usageTone(used) {
    if (used >= USAGE_ALERT) return 'red';
    if (used >= USAGE_WARN) return 'yellow';
    return 'green';
  }

  function arc(used, radius) {
    const length = 2 * Math.PI * radius;
    const filled = (length * Math.min(100, Math.max(0, used))) / 100;
    return `${filled.toFixed(1)} ${length.toFixed(1)}`;
  }

  function resolveTheme(choice, prefersLight) {
    const theme = THEMES.includes(choice) ? choice : 'dark';
    if (theme === 'system') return prefersLight ? 'light' : 'dark';
    return theme;
  }

  function kindMark(kind) {
    return MARKS[kind] || String(kind || '?').charAt(0).toUpperCase();
  }

  function shortPath(path) {
    const parts = String(path).replace(/\\/g, '/').split('/').filter(Boolean);
    return parts.length > 2 ? `…/${parts.slice(-2).join('/')}` : String(path);
  }

  function crumbs(path) {
    return String(path).replace(/\\/g, '/').split('/').filter(Boolean);
  }

  function runsText(runs) {
    return (runs || []).map((run) => run[0]).join('');
  }

  function unbox(line) {
    return String(line).replace(/^[\s│┃|]+/, '').replace(/[\s│┃|]+$/, '');
  }

  function choices(lines) {
    const tail = lines.slice(-CHOICE_WINDOW).map(unbox);
    const found = [];
    tail.forEach((line, index) => {
      const match = CHOICE.exec(line);
      if (match) found.push({ index, key: match[1], label: match[2] });
    });
    if (!found.length) return null;
    const group = [found[found.length - 1]];
    for (let i = found.length - 2; i >= 0; i -= 1) {
      const next = group[0];
      if (Number(found[i].key) !== Number(next.key) - 1 || next.index - found[i].index > CHOICE_GAP) break;
      group.unshift(found[i]);
    }
    if (group[0].key !== '1' || group.length < 2) return null;
    const options = group.map((option, position) => {
      const until = position + 1 < group.length ? group[position + 1].index : option.index + 1;
      const more = tail.slice(option.index + 1, until).filter((line) => line.trim() && !BORDER.test(line));
      const label = [option.label, ...more].join(' ').replace(HINT, '').trim();
      return { key: option.key, label, danger: DANGER.test(label) };
    });
    const above = [];
    for (let i = group[0].index - 1; i >= 0 && above.length < QUESTION_GAP; i -= 1) {
      if (tail[i].trim() && !BORDER.test(tail[i])) above.push(i);
    }
    const asked = above.find((index) => tail[index].trim().endsWith('?'));
    const at = asked === undefined ? above[0] : asked;
    if (at === undefined) return { question: '', context: [], options };
    const between = above.filter((index) => index > at).reverse();
    const before = [];
    for (let i = at - 1; i >= 0 && before.length < CONTEXT_LINES; i -= 1) {
      if (tail[i].trim() && !BORDER.test(tail[i])) before.unshift(i);
    }
    const context = (between.length ? between : before).slice(-CONTEXT_LINES).map((index) => tail[index].trim());
    return { question: tail[at].trim(), context, options };
  }

  function diffFiles(text) {
    const files = [];
    let file = null;
    let hunk = null;
    let number = 0;
    for (const line of String(text || '').split('\n')) {
      if (line.startsWith('diff ')) {
        const name = line.split(' b/').pop();
        file = { name, insertions: 0, deletions: 0, hunks: [] };
        files.push(file);
        hunk = null;
        continue;
      }
      if (!file || line.startsWith('+++') || line.startsWith('---') || line.startsWith('index ')) continue;
      if (line.startsWith('@@')) {
        const start = /\+(\d+)/.exec(line);
        number = start ? Number(start[1]) : 0;
        const title = line.replace(/^@@[^@]*@@\s*/, '');
        hunk = { title, lines: [] };
        file.hunks.push(hunk);
        continue;
      }
      if (!hunk) continue;
      if (line.startsWith('+')) {
        file.insertions += 1;
        hunk.lines.push({ kind: 'add', number, text: line.slice(1) });
        number += 1;
      } else if (line.startsWith('-')) {
        file.deletions += 1;
        hunk.lines.push({ kind: 'del', number: null, text: line.slice(1) });
      } else if (line.length) {
        hunk.lines.push({ kind: 'ctx', number, text: line.slice(1) });
        number += 1;
      }
    }
    return files;
  }

  function diffTotals(files) {
    return files.reduce(
      (sum, file) => ({ files: sum.files + 1, insertions: sum.insertions + file.insertions, deletions: sum.deletions + file.deletions }),
      { files: 0, insertions: 0, deletions: 0 },
    );
  }

  function shortModel(id) {
    if (!id) return '';
    const name = id.split('/').pop().replace(/\[.*\]$/, '').replace(/-\d{8}$/, '');
    const claude = name.match(/claude-[\d.-]*([a-z]+)/);
    return claude ? claude[1] : name.split('-').slice(0, 2).join('-');
  }

  const api = {
    SPINNER,
    USAGE_WARN,
    USAGE_ALERT,
    THEMES,
    THEME_INK,
    COLUMNS,
    duration,
    column,
    moodOf,
    usageTone,
    arc,
    resolveTheme,
    kindMark,
    shortPath,
    crumbs,
    runsText,
    choices,
    diffFiles,
    diffTotals,
    shortModel,
  };
  root.OrkaiModel = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
