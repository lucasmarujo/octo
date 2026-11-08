import { $, h, storage, svg } from './dom.js';
import { closeDialog, openDialog } from './dialogs.js';
import { language, t } from './i18n.js';
import { ICON_GIFT } from './icons.js';

const SEEN_KEY = 'octo.notesSeen';

export const NOTES = [
  {
    version: '0.2.25',
    date: '2026-11-08',
    en: [
      'The sidebar lists every agent of a session, each with its running subagents under it.',
      'Subagents keep showing after a /clear or a /resume, and for a Claude started from a terminal.',
      'The plan usage keeps its numbers when Claude is slow to answer, instead of showing it as unavailable.',
      'The usage rings of a collapsed sidebar fill as the plan is used.',
      'On the Mac the icon has the same size as every other icon in the Dock.',
      'An Octo installed from the Microsoft Store leaves its updates to the Store.',
    ],
    'pt-BR': [
      'A barra lateral lista todos os agentes de uma sessão, cada um com seus subagentes rodando logo abaixo.',
      'Os subagentes continuam aparecendo depois de um /clear ou /resume, e também num Claude aberto pelo terminal.',
      'O uso do plano mantém os números quando o Claude demora a responder, em vez de aparecer como indisponível.',
      'Os anéis de uso da barra lateral recolhida enchem conforme o plano é usado.',
      'No Mac o ícone tem o mesmo tamanho de todos os outros ícones do Dock.',
      'O Octo instalado pela Microsoft Store deixa as atualizações com a Store.',
    ],
  },
  {
    version: '0.2.24',
    date: '2026-11-07',
    en: [
      'Octo checks the version of every code agent when it opens, tells you when a new one is out and updates it for you with one click.',
      'A collapsed sidebar still shows the plan usage, as one ring per window around the icon of each provider.',
      'The sidebar and the agent panel slide when they collapse and expand.',
      'Rename an agent with a double click on its title or on its tab.',
      "What's new now has its own button in the sidebar, and every release note is in Settings, under Updates.",
      'With the sidebar collapsed, the volume opens beside the bell instead of covering the other icons.',
      'Kai peeking from the corner is the new icon of the app.',
    ],
    'pt-BR': [
      'O Octo confere a versão de cada code agent ao abrir, avisa quando sai uma nova e atualiza para você com um clique.',
      'A barra lateral recolhida continua mostrando o uso do plano, com um anel por janela em volta do ícone de cada provedor.',
      'A barra lateral e o painel do agente deslizam ao recolher e expandir.',
      'Renomeie um agente com um clique duplo no título ou na aba dele.',
      'As novidades ganharam um botão na barra lateral, e todas as notas de versão ficam em Configurações, em Atualizações.',
      'Com a barra lateral recolhida, o volume abre ao lado do sino em vez de cobrir os outros ícones.',
      'O Kai espiando do canto é o novo ícone do app.',
    ],
  },
  {
    version: '0.2.23',
    date: '2026-11-07',
    en: [
      'Real notifications on Windows and a bouncing icon in the macOS Dock when an agent waits for you or finishes.',
      'Drag, pin and choose the tabs, with the overview among them.',
      'The sidebar and the agent panel collapse.',
      'The stop button of a card takes the click on the whole of it.',
    ],
    'pt-BR': [
      'Notificações de verdade no Windows e o ícone pulando no Dock do macOS quando um agente espera por você ou termina.',
      'Arraste, fixe e escolha as abas, com a visão geral entre elas.',
      'A barra lateral e o painel do agente recolhem.',
      'O botão de parar de um card aceita o clique nele inteiro.',
    ],
  },
  {
    version: '0.2.22',
    date: '2026-11-07',
    en: [
      'Install a missing agent from Settings.',
      'Turn the alert sound on and off from a bell in the sidebar and set how loud it plays.',
      'The sidebar shows the provider and the model of each agent.',
      'Rename a session with a double click on its title.',
      'Resize the panel of an agent.',
      'A gray theme next to the dark and the light ones.',
    ],
    'pt-BR': [
      'Instale um agente que falta pelas Configurações.',
      'Ligue e desligue o som dos alertas por um sino na barra lateral e escolha o volume.',
      'A barra lateral mostra o provedor e o modelo de cada agente.',
      'Renomeie uma sessão com um clique duplo no título.',
      'Redimensione o painel de um agente.',
      'Um tema cinza ao lado do escuro e do claro.',
    ],
  },
  {
    version: '0.2.21',
    date: '2026-11-06',
    en: ['Drag an agent from its line in the sidebar to another session.', 'Windows picks up the new icon of the app right away.'],
    'pt-BR': ['Arraste um agente pela linha dele na barra lateral para outra sessão.', 'O Windows passa a mostrar o novo ícone do app na hora.'],
  },
];

export function noteItems(note, code = language()) {
  return note[code] || note.en;
}

export function notesUnseen(seen) {
  return seen !== NOTES[0].version;
}

export function noteBlock(note, current) {
  return h(
    'section',
    { class: `note${current ? ' current' : ''}` },
    h('div', { class: 'note-head' }, h('span', { class: 'note-version' }, `Octo ${note.version}`), current ? h('span', { class: 'note-tag' }, t('this version')) : null, h('span', { class: 'note-date' }, note.date)),
    h('ul', { class: 'notes-list' }, noteItems(note).map((item) => h('li', null, item))),
  );
}

function paintButton() {
  const button = $('whats-new');
  button.classList.toggle('unseen', notesUnseen(storage.get(SEEN_KEY)));
}

function showNotes(seeAll) {
  const [latest] = NOTES;
  storage.set(SEEN_KEY, latest.version);
  paintButton();
  const all = () => {
    closeDialog();
    seeAll();
  };
  openDialog(t("What's new in Octo {version}", { version: latest.version }), h('ul', { class: 'notes-list' }, noteItems(latest).map((item) => h('li', null, item))), [
    h('button', { class: 'button', type: 'button', onclick: all }, t('All release notes')),
    h('button', { class: 'button primary', type: 'button', onclick: closeDialog }, t('Close')),
  ]);
}

export function initNotes(seeAll) {
  const button = $('whats-new');
  button.replaceChildren(svg(ICON_GIFT));
  button.addEventListener('click', () => showNotes(seeAll));
  paintButton();
}
