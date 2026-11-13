import { $, h, storage, svg } from './dom.js';
import { closeDialog, openDialog } from './dialogs.js';
import { language, t } from './i18n.js';
import { ICON_GIFT } from './icons.js';

const SEEN_KEY = 'octo.notesSeen';

export const NOTES = [
  {
    version: '0.2.39',
    date: '2026-11-13',
    en: [
      'Octo can answer your agents for you. Turn it on in the new Octo tab: when an agent stops to ask something, Octo reads the question, thinks with an agent you already have (claude or codex) and answers it, or leaves it for you with what it would say.',
      'Three modes, for everything or per project: Observe only reports, Moderate answers what the task and your rules settle, Bypass answers everything with no limits.',
      'Chat with Octo about everything that is running, and let it message, answer, stop or start agents. Its rules live in Markdown, one file for everything and one per project.',
      'It is off until you turn it on, and then it stays light: no engine keeps running, it only starts while a question is being judged.',
      'The Octo tab comes first, so alt+1 now opens it. The next version of the phone app brings Octo and attachments on launch.',
    ],
    'pt-BR': [
      'O Octo pode responder os seus agentes por você. Ligue na nova aba Octo: quando um agente para para perguntar algo, o Octo lê a pergunta, pensa com um agente que você já tem (claude ou codex) e responde, ou deixa para você com o que ele diria.',
      'Três modos, para tudo ou por projeto: Observe só relata, Moderate responde o que a tarefa e as suas regras resolvem, Bypass responde tudo, sem limites.',
      'Converse com o Octo sobre tudo o que está rodando, e deixe que ele mande mensagens, responda, pare ou inicie agentes. As regras dele ficam em Markdown, um arquivo para tudo e um por projeto.',
      'Ele fica desligado até você ligar, e continua leve: nenhum motor fica rodando, ele só inicia enquanto uma pergunta é avaliada.',
      'A aba Octo vem primeiro, então alt+1 agora abre ela. A próxima versão do app do celular traz o Octo e anexos no lançamento.',
    ],
  },
  {
    version: '0.2.38',
    date: '2026-11-13',
    en: [
      'A running agent takes files and images like in any terminal: drop files on its terminal and their paths are typed, and paste a copied image with ctrl+v to hand it to the agent.',
    ],
    'pt-BR': [
      'Um agente rodando recebe arquivos e imagens como em qualquer terminal: solte arquivos sobre o terminal dele e os caminhos são digitados, e cole uma imagem copiada com ctrl+v para entregá-la ao agente.',
    ],
  },
  {
    version: '0.2.37',
    date: '2026-11-12',
    en: [
      'Attach files of any kind to a task: the paperclip in the task box opens the file dialog, and files dropped on the window are attached too. The agent gets the task with the path of each file, up to 20 per task.',
      'Cursor CLI is a new agent: pick cursor in the task box. Octo shows when it is working, waiting for you or done, reopens its chat after a restart and can install it for you in Settings.',
    ],
    'pt-BR': [
      'Anexe arquivos de qualquer tipo à tarefa: o clipe da caixa de tarefa abre o diálogo de arquivos, e arquivos soltos na janela também são anexados. O agente recebe a tarefa com o caminho de cada arquivo, até 20 por tarefa.',
      'O Cursor CLI é um agente novo: escolha cursor na caixa de tarefa. O Octo mostra quando ele está trabalhando, esperando você ou concluiu, reabre a conversa dele depois de um reinício e pode instalá-lo para você nas Configurações.',
    ],
  },
  {
    version: '0.2.36',
    date: '2026-11-12',
    en: [
      'A paired phone can read a Claude agent as a conversation: what you asked, what Claude said and each step with its result. It needs the next version of the Octo phone app.',
    ],
    'pt-BR': [
      'O celular pareado pode ler um agente Claude como uma conversa: o que você pediu, o que o Claude disse e cada passo com o resultado. Precisa da próxima versão do app do Octo no celular.',
    ],
  },
  {
    version: '0.2.35',
    date: '2026-11-11',
    en: [
      'Octo has a new look: black with an aqua accent and the O as its icon. The look you knew is still there as the Old theme, in Settings > Appearance.',
      'Pick your own accent color in Settings > Appearance: presets, a hue slider, RGB or a hex. Buttons, the cursor and the O follow it.',
      'A bar under the window counts your agents, the ones working and the ones waiting for you, with plan usage, the phone and the version one click away.',
      'The terminal of an agent runs edge to edge, under a strip with the agent, its model, its branch and how long it has been alive.',
      'Pairing a phone is a small dialog until there is a code to show.',
    ],
    'pt-BR': [
      'O Octo está de visual novo: preto com destaque verde-água e o O como ícone. O visual que você conhecia continua como o tema Antigo, em Configurações > Aparência.',
      'Escolha a sua cor de destaque em Configurações > Aparência: predefinidas, matiz, RGB ou hex. Botões, cursor e o O acompanham.',
      'Uma barra embaixo da janela conta os agentes, os que estão trabalhando e os que esperam por você, com o uso dos planos, o celular e a versão a um clique.',
      'O terminal do agente vai de ponta a ponta, sob uma faixa com o agente, o modelo, a branch e há quanto tempo ele está vivo.',
      'Parear o celular é um diálogo pequeno até existir um código para mostrar.',
    ],
  },
  {
    version: '0.2.34',
    date: '2026-11-10',
    en: [
      'The folder picker has a New folder button (Ctrl+Shift+N): name it and it is made inside the folder you are looking at, already picked so the agents can start there.',
    ],
    'pt-BR': [
      'O seletor de pastas ganhou o botão Nova pasta (Ctrl+Shift+N): dê um nome e ela é criada dentro da pasta que você está vendo, já selecionada para os agentes começarem nela.',
    ],
  },
  {
    version: '0.2.33',
    date: '2026-11-10',
    en: [
      'If you allow notifications in the Octo app on your phone, it tells you when an agent finishes or waits for you, even with the app closed.',
      'Qwen Code agents show the Qwen logo in the sidebar.',
    ],
    'pt-BR': [
      'Se você permitir notificações no app do Octo no celular, ele avisa quando um agente termina ou espera por você, mesmo com o app fechado.',
      'Agentes do Qwen Code mostram o logo do Qwen na barra lateral.',
    ],
  },
  {
    version: '0.2.32',
    date: '2026-11-09',
    en: [
      'With the sidebar collapsed, hover a usage ring to see the percentage of each window of that agent, right beside it.',
      'Sessions stay in the collapsed sidebar: hover one to see how many agents it has and click any of them to open it.',
    ],
    'pt-BR': [
      'Com a barra lateral recolhida, passe o mouse num anel de consumo para ver a porcentagem de cada janela daquele agente, logo ao lado.',
      'As sessões continuam na barra lateral recolhida: passe o mouse numa delas para ver quantos agentes ela tem e clique em qualquer um para abri-lo.',
    ],
  },
  {
    version: '0.2.31',
    date: '2026-11-09',
    en: [
      'On macOS the subagents of an agent show again: Octo also follows the work Claude Code runs in the background under it.',
    ],
    'pt-BR': [
      'No macOS os subagentes de um agente voltam a aparecer: o Octo também acompanha o trabalho que o Claude Code roda em segundo plano dentro dele.',
    ],
  },
  {
    version: '0.2.30',
    date: '2026-11-09',
    en: [
      'Pick the model a new agent starts with, right next to the agent: the models Claude Code and Codex offer, remembered for each agent.',
    ],
    'pt-BR': [
      'Escolha o modelo com que o agente novo começa, logo ao lado do agente: os modelos que o Claude Code e o Codex oferecem, lembrados para cada agente.',
    ],
  },
  {
    version: '0.2.29',
    date: '2026-11-09',
    en: [
      'An agent that finished its reply but still runs a shell, a monitor or subagents now stays working until they end, instead of showing as done or idle.',
    ],
    'pt-BR': [
      'Um agente que terminou a resposta mas ainda tem um shell, um monitor ou subagentes rodando agora continua trabalhando até eles acabarem, em vez de aparecer como concluído ou ocioso.',
    ],
  },
  {
    version: '0.2.28',
    date: '2026-11-09',
    en: [
      'Octo no longer keeps a copy of every version it ran: the old ones are removed when it opens, freeing hundreds of MB.',
    ],
    'pt-BR': [
      'O Octo não guarda mais uma cópia de cada versão que já rodou: as antigas são apagadas ao abrir, liberando centenas de MB.',
    ],
  },
  {
    version: '0.2.27',
    date: '2026-11-08',
    en: [
      'Octo has a new icon: smooth app corners, a bigger prompt and a bigger Kai peeking from the corner.',
    ],
    'pt-BR': [
      'O Octo ganhou ícone novo: cantos lisos de app, um prompt maior e um Kai maior espiando pelo canto.',
    ],
  },
  {
    version: '0.2.26',
    date: '2026-11-08',
    en: [
      'The phone app has a new look: cards, the plan usage as rings and a board next to the list.',
      'An agent that needs you shows its real choices, so one tap on the phone answers it.',
      'The phone app has the Dark, Gray and Light themes of Octo, with haptics and reduce motion.',
      'On the phone a folder can be searched, and the changes of a branch read file by file.',
    ],
    'pt-BR': [
      'O app do celular ganhou visual novo: cartões, o uso do plano em anéis e um quadro ao lado da lista.',
      'Um agente que precisa de você mostra as opções reais, então um toque no celular responde.',
      'O app do celular tem os temas Escuro, Cinza e Claro do Octo, com vibração e menos animação.',
      'No celular dá para buscar uma pasta e ler as mudanças de um branch arquivo por arquivo.',
    ],
  },
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
