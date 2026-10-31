import { $, h } from './dom.js';
import { LANGUAGES, language, setLanguage } from './i18n.js';

const key = (text) => h('span', { class: 'help-key' }, text);

export const HELP = {
  en: {
    title: 'How to use Octo',
    lead: 'Run many AI agents across your projects from one window. Closing Octo stops no agent: they keep working and Octo reconnects when you come back.',
    language: 'Language',
    steps: [
      ['Pick the folder', () => ['Click ', key('where'), ' or press ', key('ctrl L'), '. Each session remembers its own folder.']],
      ['Pick who runs it', () => ['Any installed agent or a terminal. ', key('tab'), ' cycles through them.']],
      ['Write the task', () => ['Press ', key('enter'), ' and the agent starts already working on it.']],
      ['Answer when it needs you', () => ['Octo chimes and ', key('alt N'), ' opens the agent waiting the longest.']],
    ],
    groups: [
      ['Launching', [['enter', 'Launch the agent'], ['shift enter', 'New line in the task'], ['ctrl ↑', 'Tasks you already sent'], ['tab', 'Change who runs it'], ['ctrl L', 'Change the folder'], ['ctrl T', 'One worktree per agent'], ['/', 'Commands and skills'], ['!command', 'Run it in a new terminal']]],
      ['Agents', [['alt N', 'Open who needs you'], ['ctrl ]', 'Back to the overview'], ['alt 1…9', 'Jump to an open tab'], ['ctrl E', 'Rename the agent'], ['delete', 'Stop and remove it'], ['ctrl R', 'Recover interrupted'], ['ctrl P', 'Pair your phone'], ['ctrl click', 'Open a link in the terminal']]],
      ['Sessions & app', [['ctrl N', 'New session'], ['alt ↑↓', 'Switch session'], ['F1', 'This screen'], ['ctrl Q', 'Quit, agents keep going']]],
    ],
    states: 'States',
    legend: [
      ['⠹', 't-yellow', 'Working'],
      ['‖', 't-yellow strong', 'Waiting for your answer', 't-yellow'],
      ['✓', 't-green strong', 'Done, not seen yet'],
      ['●', 't-faint', 'Idle'],
      ['$', 't-text strong', 'Terminal'],
      ['↻', 't-red', () => ['Interrupted — ', key('ctrl R'), ' recovers']],
      ['✗', 't-faint', 'Ended'],
    ],
  },
  'pt-BR': {
    title: 'Como usar o Octo',
    lead: 'Rode vários agentes de IA nos seus projetos a partir de uma janela. Fechar o Octo não para nenhum agente: eles continuam trabalhando e o Octo se reconecta quando você volta.',
    language: 'Idioma',
    steps: [
      ['Escolha a pasta', () => ['Clique em ', key('onde'), ' ou aperte ', key('ctrl L'), '. Cada sessão lembra a própria pasta.']],
      ['Escolha quem roda', () => ['Qualquer agente instalado ou um terminal. ', key('tab'), ' alterna entre eles.']],
      ['Escreva a tarefa', () => ['Aperte ', key('enter'), ' e o agente já começa trabalhando nela.']],
      ['Responda quando ele precisar', () => ['O Octo toca um aviso e ', key('alt N'), ' abre o agente que espera há mais tempo.']],
    ],
    groups: [
      ['Para lançar', [['enter', 'Lança o agente'], ['shift enter', 'Nova linha na tarefa'], ['ctrl ↑', 'Tarefas que você já enviou'], ['tab', 'Troca quem roda'], ['ctrl L', 'Troca a pasta'], ['ctrl T', 'Um worktree por agente'], ['/', 'Comandos e skills'], ['!comando', 'Roda num terminal novo']]],
      ['Agentes', [['alt N', 'Abre quem precisa de você'], ['ctrl ]', 'Volta para a visão geral'], ['alt 1…9', 'Pula para uma aba aberta'], ['ctrl E', 'Renomeia o agente'], ['delete', 'Para e remove o agente'], ['ctrl R', 'Recupera os interrompidos'], ['ctrl P', 'Pareia o celular'], ['ctrl clique', 'Abre um link no terminal']]],
      ['Sessões e app', [['ctrl N', 'Nova sessão'], ['alt ↑↓', 'Troca de sessão'], ['F1', 'Esta tela'], ['ctrl Q', 'Sai, os agentes continuam']]],
    ],
    states: 'Estados',
    legend: [
      ['⠹', 't-yellow', 'Trabalhando'],
      ['‖', 't-yellow strong', 'Esperando sua resposta', 't-yellow'],
      ['✓', 't-green strong', 'Terminou, ainda não visto'],
      ['●', 't-faint', 'Ocioso'],
      ['$', 't-text strong', 'Terminal'],
      ['↻', 't-red', () => ['Interrompido — ', key('ctrl R'), ' recupera']],
      ['✗', 't-faint', 'Encerrado'],
    ],
  },
};

const text = (value) => (typeof value === 'function' ? value() : value);

function shortcut([keys, meaning]) {
  return h('div', { class: 'help-row' }, h('span', { class: 'help-row-key' }, h('span', { class: 'kbd-chip' }, keys)), h('span', { class: 'help-row-text' }, meaning));
}

function legendRow([mark, tone, meaning, textTone]) {
  return h('div', { class: 'help-row' }, h('span', { class: `help-mark ${tone}` }, mark), h('span', { class: `help-row-text${textTone ? ` ${textTone}` : ''}` }, text(meaning)));
}

export function renderHelp() {
  const code = language();
  const copy = HELP[code];
  $('help-view').replaceChildren(
    h(
      'header',
      { class: 'help-head' },
      h('div', { class: 'help-intro' }, h('h1', { class: 'help-title' }, copy.title), h('p', { class: 'help-lead' }, copy.lead)),
      h(
        'div',
        { class: 'help-language' },
        h('span', { class: 'caps' }, copy.language),
        h(
          'div',
          { class: 'segmented', role: 'group', 'aria-label': copy.language },
          ...LANGUAGES.map((entry) =>
            h('button', { type: 'button', 'aria-pressed': String(entry.code === code), onclick: () => setLanguage(entry.code, 'help') }, entry.name),
          ),
        ),
      ),
    ),
    h(
      'div',
      { class: 'help-steps' },
      ...copy.steps.map(([title, body], index) =>
        h(
          'div',
          { class: `help-step${index === 0 ? ' first' : ''}` },
          h('span', { class: 'help-step-number' }, String(index + 1).padStart(2, '0')),
          h('span', { class: 'help-step-title' }, title),
          h('span', { class: 'help-step-text' }, text(body)),
        ),
      ),
    ),
    h(
      'div',
      { class: 'help-reference' },
      ...copy.groups.map(([title, rows]) => h('div', { class: 'help-group' }, h('span', { class: 'caps help-group-title' }, title), ...rows.map(shortcut))),
      h('div', { class: 'help-group help-states' }, h('span', { class: 'caps help-group-title' }, copy.states), ...copy.legend.map(legendRow)),
    ),
  );
}
