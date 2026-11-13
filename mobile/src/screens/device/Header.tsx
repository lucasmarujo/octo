import { StyleSheet, Text, View } from 'react-native';

import type { PillTone } from '../../core/agents';
import { elapsed, label, status } from '../../core/agents';
import { useSecond } from '../../core/clock';
import type { LinkPhase } from '../../core/link';
import model from '../../core/model';
import { gateOf, headLine, octoMood } from '../../core/octo';
import { useSelect, useStore } from '../../core/store';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono } from '../../theme/type';
import { Dot, Glyph, kindColor } from '../../ui/atoms';
import { subStyles, Top } from '../../ui/Top';
import { useDevice } from './context';

type Props = {
  onBack: () => void;
  onSettings: () => void;
  onMenu: () => void;
  onOcto: () => void;
};

type Word = {
  text: string;
  tone: 'done' | 'waiting' | 'lost';
};

const WORDS: Record<LinkPhase, Word> = {
  live: { text: 'live', tone: 'done' },
  connecting: { text: 'connecting…', tone: 'waiting' },
  offline: { text: 'offline · retrying', tone: 'lost' },
  unpaired: { text: 'not paired', tone: 'lost' },
  replaced: { text: 'on another phone', tone: 'lost' },
  outdated: { text: 'update needed', tone: 'lost' },
};

export function linkWord(phase: LinkPhase): Word {
  return WORDS[phase];
}

const styles = (t: Theme) => {
  const pills: Record<PillTone, { bg: string; ink: string }> = {
    working: { bg: mix(t.waiting, 14), ink: t.waiting },
    needs: { bg: t.waiting, ink: t.waitingInk },
    done: { bg: mix(t.done, 14), ink: t.done },
    lost: { bg: mix(t.lost, 14), ink: t.lost },
    idle: { bg: t.surface, ink: t.textMuted },
  };
  return {
    pills,
    sheet: StyleSheet.create({
      pill: { flexDirection: 'row', alignItems: 'center', gap: 5, paddingVertical: 1, paddingHorizontal: 8, borderRadius: 999, flexShrink: 0 },
      pillText: mono(11, 700, 16),
      path: { flexShrink: 1 },
    }),
  };
};

export function Header({ onBack, onSettings, onMenu, onOcto }: Props) {
  const { link, workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const sub = useStyles(subStyles);
  const state = useStore(workspace.store);
  const { phase, overview } = useStore(link.store);
  const octoLine = useSelect(workspace.octo.view, headLine);
  const octoFeels = useSelect(workspace.octo.view, octoMood);
  const octoReady = useSelect(workspace.octo.view, view => Boolean(view && gateOf(view) === 'ready'));
  useSecond();

  if (state.view === 'octo') {
    return (
      <Top title="Octo" mood={octoFeels} compact onBack={() => workspace.leaveOcto()} backLabel="back to the overview" action={octoReady ? { icon: 'settings', label: 'Octo settings', run: onOcto } : undefined}>
        <Text style={[sub.text, s.sheet.path]} numberOfLines={1}>
          {octoLine}
        </Text>
      </Top>
    );
  }

  if (state.view === 'agent') {
    const agent = workspace.findAgent(state.agent);
    if (!agent) {
      return <Top title={`#${state.agent}`} mood="idle" compact onBack={() => workspace.leaveAgent()} backLabel="back to the overview" action={{ icon: 'menu', label: 'agent actions', run: onMenu }} />;
    }
    const mark = status(agent);
    const time = elapsed(agent, link.now());
    const short = model.shortModel(agent.model);
    const pill = s.pills[mark.pill];
    return (
      <Top title={agent.name} mood={model.moodOf([agent])} compact onBack={() => workspace.leaveAgent()} backLabel="back to the overview" action={{ icon: 'menu', label: 'agent actions', run: onMenu }}>
        <View style={[s.sheet.pill, { backgroundColor: pill.bg }]}>
          <Glyph text={mark.text} spin={mark.spin} style={[s.sheet.pillText, { color: pill.ink }]} />
          <Text style={[s.sheet.pillText, { color: pill.ink }]}>{`${mark.word}${time ? ` ${time}` : ''}`}</Text>
        </View>
        <Text style={[sub.text, s.sheet.path, { color: kindColor(t, agent.kind) }]} numberOfLines={1}>
          {short ? `${label(agent.kind)} · ${short}` : label(agent.kind)}
        </Text>
      </Top>
    );
  }

  const word = linkWord(phase);
  return (
    <Top title={overview ? overview.host : 'Octo'} mood={overview ? model.moodOf(overview.agents) : 'idle'} onBack={onBack} backLabel="back to your computers" action={{ icon: 'settings', label: 'appearance', run: onSettings }}>
      <Dot phase={phase} />
      <Text style={[sub.text, { color: t[word.tone] }]}>{word.text}</Text>
      {overview ? (
        <Text style={[sub.text, s.sheet.path]} numberOfLines={1}>
          {`· ${model.shortPath(workspace.currentWhere())}`}
        </Text>
      ) : null}
    </Top>
  );
}
