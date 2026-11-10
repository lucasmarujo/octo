import React, { useEffect, useRef } from 'react';
import { Animated, Easing, StyleSheet, Text, View } from 'react-native';

import { alive, doing, elapsed, label, status, waiting } from '../../core/agents';
import { useSecond } from '../../core/clock';
import type { AgentView } from '../../core/protocol';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useReduced, useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, TIGHT, tracking } from '../../theme/type';
import { kindColor, Swatch, Tile, toneColor } from '../../ui/atoms';
import { Flash } from '../../ui/Flash';
import { Kai } from '../../ui/Kai';
import { Press } from '../../ui/Press';
import { Rise } from '../../ui/Rise';
import { useDevice } from './context';
import { AgentTool } from './Tools';

type ListProps = {
  agents: AgentView[];
  now: number;
  entered: number;
  session: string;
  onStarter: (text: string) => void;
};

const STARTERS = ['Summarize what changed in this repo this week', 'Find the failing tests and fix them'];
const STAGGER_MS = 30;
const SEPARATOR_LEFT = 62;
const NO_LIGATURES = ['no-common-ligatures', 'no-contextual'] as const;
const SHIMMER_MS = 1400;

const styles = (t: Theme) =>
  StyleSheet.create({
    list: { gap: 16, paddingTop: 4, paddingHorizontal: 18 },
    group: { gap: 8 },
    groupHead: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingHorizontal: 4 },
    groupName: { ...sans(15, 700), color: t.text },
    count: { ...mono(12), color: t.textFaint },
    card: { borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    row: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 12, paddingHorizontal: 14 },
    rowHeld: { backgroundColor: t.surfaceRaised },
    rowWaiting: { backgroundColor: mix(t.waiting, 5) },
    separator: { position: 'absolute', top: 0, left: SEPARATOR_LEFT, right: 0, height: 1, backgroundColor: t.border },
    body: { flex: 1, minWidth: 0, gap: 2 },
    line: { flexDirection: 'row', alignItems: 'baseline', gap: 8, minWidth: 0 },
    name: { ...mono(15, 700, 20), flex: 1, minWidth: 0, color: t.text },
    time: { ...mono(12), color: t.textFaint, flexShrink: 0 },
    kind: { ...mono(12, 400, 16), flexShrink: 0 },
    doing: { ...sans(14, 400, 18), flexShrink: 1, minWidth: 0, fontVariant: [...NO_LIGATURES] },
    doingMono: { ...mono(13, 400, 18), flexShrink: 1, minWidth: 0 },
    faded: { color: t.textFaint },
    tool: { marginRight: -6 },
    empty: { alignItems: 'center', gap: 12, paddingTop: 48, paddingHorizontal: 32 },
    emptyTitle: { ...sans(26, 800, 30), ...tracking(26, TIGHT), color: t.text, textAlign: 'center' },
    emptyText: { ...sans(16, 400, 22), color: t.textMuted, textAlign: 'center' },
    starters: { gap: 8, paddingTop: 26, alignSelf: 'stretch' },
    caps: { ...mono(11), ...tracking(11, CAPS), color: t.textFaint, paddingHorizontal: 4, textTransform: 'uppercase' },
    starter: { flexDirection: 'row', alignItems: 'center', gap: 10, paddingVertical: 12, paddingHorizontal: 14, borderRadius: 14, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    starterText: { ...sans(15), flex: 1, color: t.text },
    starterChevron: { ...sans(18), color: t.textFaint },
    skeleton: { gap: 10, paddingVertical: 8, paddingHorizontal: 18 },
    bone: { height: 62, borderRadius: 18, overflow: 'hidden', backgroundColor: t.surface },
    boneTall: { height: 110 },
    shine: { position: 'absolute', top: 0, bottom: 0, left: 0, width: '400%', backgroundImage: `linear-gradient(90deg, ${t.surface} 0%, ${t.surfaceRaised} 25%, ${t.surface} 50%, ${t.surfaceRaised} 75%, ${t.surface} 100%)` },
  });

function Row({ agent, now, first }: { agent: AgentView; now: number; first: boolean }) {
  const { link, workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const mark = status(agent);
  const what = doing(agent);
  const time = elapsed(agent, now);
  const gone = !alive(agent);
  const flash = link.flashOf(agent.id);
  const terminal = agent.kind === 'terminal';
  return (
    <Flash tone={flash}>
      <Press style={[s.row, waiting(agent) && s.rowWaiting]} held={s.rowHeld} onPress={() => workspace.openAgent(agent.id)}>
        {first ? null : <View style={s.separator} />}
        <Tile tone={mark.tile} text={mark.text} spin={mark.spin} flash={flash} />
        <View style={s.body}>
          <View style={s.line}>
            <Text style={[s.name, gone && s.faded]} numberOfLines={1}>
              {agent.name}
            </Text>
            <Text style={[s.time, mark.pill === 'needs' && { color: t.waiting }]}>{time || ''}</Text>
          </View>
          <View style={s.line}>
            <Text style={[s.kind, { color: kindColor(t, agent.kind) }]}>{label(agent.kind)}</Text>
            <Text style={[terminal ? s.doingMono : s.doing, { color: toneColor(t, what.tone) }, gone && s.faded]} numberOfLines={1}>
              {what.text}
            </Text>
          </View>
        </View>
        <AgentTool agent={agent} style={s.tool} />
      </Press>
    </Flash>
  );
}

function Empty({ session, onStarter }: { session: string; onStarter: (text: string) => void }) {
  const s = useStyles(styles);
  return (
    <Rise style={s.empty}>
      <Kai mood="idle" size={96} />
      <Text style={s.emptyTitle}>A quiet session</Text>
      <Text style={s.emptyText}>{`Nothing runs in ${session} yet. Describe a task below and pick who does it.`}</Text>
      <View style={s.starters}>
        <Text style={s.caps}>Try</Text>
        {STARTERS.map(text => (
          <Press key={text} style={s.starter} onPress={() => onStarter(text)}>
            <Text style={s.starterText}>{text}</Text>
            <Text style={s.starterChevron}>›</Text>
          </Press>
        ))}
      </View>
    </Rise>
  );
}

export function AgentList({ agents, now, entered, session, onStarter }: ListProps) {
  const s = useStyles(styles);
  useSecond();
  if (!agents.length) return <View style={s.list}>{<Empty session={session} onStarter={onStarter} />}</View>;
  const groups = new Map<string, AgentView[]>();
  for (const agent of agents) {
    const members = groups.get(agent.project);
    if (members) members.push(agent);
    else groups.set(agent.project, [agent]);
  }
  let index = 0;
  return (
    <View style={s.list}>
      {[...groups].map(([project, members]) => (
        <View key={project} style={s.group}>
          <View style={s.groupHead}>
            <Swatch color={members[0].color} size={8} radius={3} />
            <Text style={s.groupName}>{project}</Text>
            <Text style={s.count}>{String(members.length)}</Text>
          </View>
          <View style={s.card}>
            {members.map((agent, position) => {
              const delay = index * STAGGER_MS;
              index += 1;
              return (
                <Rise key={agent.id} replay={entered} delay={delay}>
                  <Row agent={agent} now={now} first={position === 0} />
                </Rise>
              );
            })}
          </View>
        </View>
      ))}
    </View>
  );
}

function Bone({ tall }: { tall?: boolean }) {
  const s = useStyles(styles);
  const reduced = useReduced();
  const slide = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    if (reduced) return undefined;
    const motion = Animated.loop(Animated.timing(slide, { toValue: 1, duration: SHIMMER_MS, easing: Easing.linear, useNativeDriver: true }));
    motion.start();
    return () => motion.stop();
  }, [slide, reduced]);

  const [width, setWidth] = React.useState(0);
  const translateX = slide.interpolate({ inputRange: [0, 1], outputRange: [0, -2 * width] });
  return (
    <View style={[s.bone, tall && s.boneTall]} onLayout={event => setWidth(event.nativeEvent.layout.width)}>
      <Animated.View style={[s.shine, { transform: [{ translateX }] }]} />
    </View>
  );
}

export function Skeleton() {
  const s = useStyles(styles);
  return (
    <View style={s.skeleton}>
      <Bone tall />
      <Bone />
      <Bone />
    </View>
  );
}
