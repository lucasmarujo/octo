import { ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { TileTone } from '../core/agents';
import { waiting } from '../core/agents';
import type { Device } from '../core/devices';
import type { DeviceLink, LinkPhase, LinkState } from '../core/link';
import type { Mood } from '../core/model';
import model from '../core/model';
import { hostOf } from '../core/pairing';
import type { AgentView } from '../core/protocol';
import { useStore } from '../core/store';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { CAPS, mono, sans, tracking } from '../theme/type';
import { Tile } from '../ui/atoms';
import { Icon } from '../ui/Icon';
import { Press } from '../ui/Press';
import { Rise } from '../ui/Rise';
import { subStyles, Top } from '../ui/Top';
import { linkWord } from './device/Header';

type Props = {
  devices: Device[];
  links: ReadonlyMap<string, DeviceLink>;
  mood: Mood;
  onOpen: (device: Device) => void;
  onMenu: (device: Device) => void;
  onPair: () => void;
  onSettings: () => void;
};

type Glyph = {
  tone: TileTone;
  text: string;
  spin: boolean;
};

type Count = {
  n: number;
  mark: string;
  color: string;
};

const STAGGER_MS = 30;
const SEPARATOR_LEFT = 62;
const MOODS: Record<Mood, Glyph> = {
  needs: { tone: 'needs', text: '‖', spin: false },
  working: { tone: 'working', text: model.SPINNER[0], spin: true },
  done: { tone: 'done', text: '✓', spin: false },
  lost: { tone: 'lost', text: '↻', spin: false },
  idle: { tone: 'idle', text: '●', spin: false },
};
const WAITING: Glyph = { tone: 'starting', text: '◌', spin: false };
const BROKEN: Glyph = { tone: 'lost', text: '!', spin: false };
const NOTES: Partial<Record<LinkPhase, string>> = {
  connecting: 'Connecting…',
  offline: "Can't reach this computer",
  unpaired: 'Not paired anymore · open it to pair again',
  replaced: 'In use on another phone',
  outdated: 'Speaks another version · update Octo',
};

export function glyphOf(state: LinkState): Glyph {
  if (state.phase === 'live' && state.overview) return MOODS[model.moodOf(state.overview.agents)];
  return state.phase === 'connecting' || state.phase === 'live' ? WAITING : BROKEN;
}

export function countsOf(t: Theme, agents: AgentView[]): Count[] {
  const count = (test: (agent: AgentView) => boolean) => agents.filter(test).length;
  return [
    { n: count(waiting), mark: '‖', color: t.waiting },
    { n: count(agent => agent.state === 'working' || agent.starting), mark: '⠹', color: t.waiting },
    { n: count(agent => agent.state === 'done'), mark: '✓', color: t.done },
    { n: count(agent => agent.state === 'lost'), mark: '↻', color: t.lost },
  ].filter(part => part.n);
}

const styles = (t: Theme) =>
  StyleSheet.create({
    view: { flex: 1 },
    content: { paddingTop: 4, paddingHorizontal: 18, gap: 12 },
    caps: { ...mono(11), ...tracking(11, CAPS), color: t.textFaint, paddingHorizontal: 4 },
    card: { borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    row: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 12, paddingHorizontal: 14 },
    rowHeld: { backgroundColor: t.surfaceRaised },
    rowAsking: { backgroundColor: mix(t.waiting, 5) },
    separator: { position: 'absolute', top: 0, left: SEPARATOR_LEFT, right: 0, height: 1, backgroundColor: t.border },
    body: { flex: 1, minWidth: 0, gap: 2 },
    line: { flexDirection: 'row', alignItems: 'baseline', gap: 8, minWidth: 0 },
    name: { ...mono(15, 700, 20), flex: 1, minWidth: 0, color: t.text },
    word: { ...mono(12), flexShrink: 0 },
    host: { ...mono(12, 400, 18), color: t.textFaint, flexShrink: 1, minWidth: 0 },
    note: { ...sans(14, 400, 18), color: t.textMuted, flexShrink: 1, minWidth: 0 },
    counts: { flexDirection: 'row', alignItems: 'center', gap: 10, flexShrink: 0 },
    count: { flexDirection: 'row', alignItems: 'center', gap: 4 },
    mark: mono(12, 700, 18),
    number: { ...mono(12, 400, 18), color: t.textMuted },
    add: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, paddingVertical: 20, paddingHorizontal: 16, borderRadius: 18, borderWidth: 1, borderStyle: 'dashed', borderColor: t.borderStrong },
    addText: { ...sans(15, 600), color: t.textMuted },
  });

function Row({ device, link, first, onOpen, onMenu }: { device: Device; link: DeviceLink; first: boolean; onOpen: () => void; onMenu: () => void }) {
  const t = useTheme();
  const s = useStyles(styles);
  const state = useStore(link.store);
  const glyph = glyphOf(state);
  const word = linkWord(state.phase);
  const agents = state.overview ? state.overview.agents : [];
  const live = state.phase === 'live' && state.overview !== null;
  const counts = live ? countsOf(t, agents) : [];
  const note = NOTES[state.phase] || (agents.length ? `${agents.length} agent${agents.length === 1 ? '' : 's'}, all quiet` : 'No agents yet');
  return (
    <Press style={[s.row, glyph.tone === 'needs' && s.rowAsking]} held={s.rowHeld} onPress={onOpen} onLongPress={onMenu} label={`${device.name}, ${word.text}`}>
      {first ? null : <View style={s.separator} />}
      <Tile tone={glyph.tone} text={glyph.text} spin={glyph.spin} />
      <View style={s.body}>
        <View style={s.line}>
          <Text style={s.name} numberOfLines={1}>
            {device.name}
          </Text>
          <Text style={[s.word, { color: t[word.tone] }]}>{word.text}</Text>
        </View>
        <View style={s.line}>
          {counts.length ? (
            <>
              <View style={s.counts}>
                {counts.map(part => (
                  <View key={part.mark} style={s.count}>
                    <Text style={[s.mark, { color: part.color }]}>{part.mark}</Text>
                    <Text style={s.number}>{String(part.n)}</Text>
                  </View>
                ))}
              </View>
              <Text style={s.host} numberOfLines={1}>
                {hostOf(device.url)}
              </Text>
            </>
          ) : (
            <Text style={[s.note, state.phase !== 'live' && state.phase !== 'connecting' && { color: t.lost }]} numberOfLines={1}>
              {note}
            </Text>
          )}
        </View>
      </View>
    </Press>
  );
}

export function Devices({ devices, links, mood, onOpen, onMenu, onPair, onSettings }: Props) {
  const t = useTheme();
  const s = useStyles(styles);
  const sub = useStyles(subStyles);
  const insets = useSafeAreaInsets();
  return (
    <View style={s.view}>
      <Top title="Octo" mood={mood} action={{ icon: 'settings', label: 'appearance', run: onSettings }}>
        <Text style={sub.text}>{`${devices.length} computer${devices.length === 1 ? '' : 's'}`}</Text>
      </Top>
      <ScrollView contentContainerStyle={[s.content, { paddingBottom: insets.bottom + 24 }]} showsVerticalScrollIndicator={false}>
        <Text style={s.caps}>YOUR COMPUTERS</Text>
        <View style={s.card}>
          {devices.map((device, index) => {
            const link = links.get(device.id);
            return link ? (
              <Rise key={device.id} delay={index * STAGGER_MS}>
                <Row device={device} link={link} first={index === 0} onOpen={() => onOpen(device)} onMenu={() => onMenu(device)} />
              </Rise>
            ) : null;
          })}
        </View>
        <Press style={s.add} onPress={onPair}>
          <Icon name="plus" size={16} color={t.textMuted} />
          <Text style={s.addText}>Pair another computer</Text>
        </Press>
      </ScrollView>
    </View>
  );
}
