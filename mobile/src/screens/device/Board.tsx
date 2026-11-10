import React, { useEffect, useRef, useState } from 'react';
import type { LayoutChangeEvent, NativeScrollEvent, NativeSyntheticEvent } from 'react-native';
import { ScrollView, StyleSheet, Text, useWindowDimensions, View } from 'react-native';

import type { TileTone } from '../../core/agents';
import { alive, doing, elapsed, label, status, testMark, waiting } from '../../core/agents';
import { useSecond } from '../../core/clock';
import model from '../../core/model';
import type { AgentView } from '../../core/protocol';
import { mix } from '../../theme/color';
import { BASE_MS, EASE_OUT, lasting } from '../../theme/motion';
import { useStyles } from '../../theme/styles';
import { useReduced, useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans } from '../../theme/type';
import { Glyph, kindColor, Swatch, toneColor } from '../../ui/atoms';
import { Flash } from '../../ui/Flash';
import { Press } from '../../ui/Press';
import { Rise } from '../../ui/Rise';
import { useTween } from '../../ui/tween';
import { useDevice } from './context';

type Props = {
  agents: AgentView[];
  now: number;
  entered: number;
};

type Slot = {
  x: number;
  width: number;
};

const GUTTER = 30;
const GAP = 10;
const COLUMN_MAX = 420;
const STAGGER_MS = 30;
const INDENT = 24;
const NO_LIGATURES = ['no-common-ligatures', 'no-contextual'] as const;

function glyphColor(t: Theme, tile: TileTone): string {
  if (tile === 'working' || tile === 'needs') return t.waiting;
  if (tile === 'done') return t.done;
  if (tile === 'lost') return t.lost;
  if (tile === 'terminal') return t.text;
  return t.textFaint;
}

const styles = (t: Theme) =>
  StyleSheet.create({
    tabs: { flexGrow: 0, borderBottomWidth: 1, borderColor: t.border },
    tabsContent: { gap: 18, paddingHorizontal: 22, paddingBottom: 12 },
    tab: { flexDirection: 'row', alignItems: 'center', gap: 6 },
    tabText: { ...sans(15, 600), color: t.textMuted },
    tabOn: { ...sans(15, 700), color: t.text },
    tabAsking: { color: t.waiting },
    tabCount: { ...mono(11), color: t.textFaint },
    tabCountAsking: { ...mono(11, 700), color: t.waitingInk },
    askingBadge: { paddingVertical: 1, paddingHorizontal: 6, borderRadius: 999, backgroundColor: t.waiting },
    indicator: { position: 'absolute', bottom: 0, left: 0, height: 2, borderRadius: 2, backgroundColor: t.text },
    columnsContent: { gap: GAP, paddingTop: 14, paddingHorizontal: GUTTER },
    column: { gap: GAP },
    empty: { paddingVertical: 28, paddingHorizontal: 16, borderRadius: 18, borderWidth: 1, borderStyle: 'dashed', borderColor: t.borderStrong },
    emptyText: { ...sans(14), color: t.textFaint, textAlign: 'center' },
    card: { gap: 10, padding: 14, borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    cardWaiting: { backgroundColor: mix(t.waiting, 8, t.surface), borderColor: mix(t.waiting, 35) },
    cardStarting: { backgroundColor: 'transparent', borderStyle: 'dashed', borderColor: t.borderStrong },
    top: { flexDirection: 'row', alignItems: 'center', gap: 10 },
    glyph: { ...mono(15, 700), width: 14 },
    name: { ...mono(15, 700, 20), flex: 1, minWidth: 0, color: t.text },
    time: { ...mono(12), color: t.textFaint },
    meta: { flexDirection: 'row', alignItems: 'center', gap: 6, paddingLeft: INDENT },
    metaText: { ...mono(12), color: t.textMuted },
    text: { ...sans(15, 400, 20), paddingLeft: INDENT, fontVariant: [...NO_LIGATURES] },
    textMono: { ...mono(15, 400, 20), paddingLeft: INDENT },
    strong: { ...sans(15, 700, 20), color: t.waiting },
    soft: { color: t.textMuted },
    subs: { gap: 2, paddingLeft: INDENT },
    sub: { ...mono(12), color: t.textFaint },
    foot: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingTop: 10, paddingLeft: INDENT, borderTopWidth: 1, borderColor: t.border, minWidth: 0 },
    footText: mono(12),
    branch: { ...mono(12), color: t.link, flexShrink: 1, minWidth: 0 },
    grow: { flex: 1 },
    mini: { paddingVertical: 4, paddingHorizontal: 10, borderRadius: 8, backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.border },
    miniText: { ...sans(13, 700), color: t.text },
  });

function Card({ agent, now }: { agent: AgentView; now: number }) {
  const { link, workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const mark = status(agent);
  const time = elapsed(agent, now);
  const what = doing(agent);
  const asking = waiting(agent);
  const subs = agent.subagents || [];
  const changes = agent.changes && agent.changes.files ? agent.changes : null;
  const test = testMark(agent.test);
  const settled = agent.state === 'done' || agent.state === 'idle' || !alive(agent);
  const flash = link.flashOf(agent.id);
  return (
    <Flash tone={flash}>
      <Press style={[s.card, asking && s.cardWaiting, agent.starting && s.cardStarting]} onPress={() => workspace.openAgent(agent.id)}>
        <View style={s.top}>
          <Glyph text={mark.text} spin={mark.spin} style={[s.glyph, { color: glyphColor(t, mark.tile) }]} />
          <Text style={s.name} numberOfLines={1}>
            {agent.name}
          </Text>
          <Text style={s.time}>{time || ''}</Text>
        </View>
        <View style={s.meta}>
          <Swatch color={agent.color} size={6} radius={2} />
          <Text style={s.metaText}>{`${agent.project} ·`}</Text>
          <Text style={[s.metaText, { color: kindColor(t, agent.kind) }]}>{label(agent.kind)}</Text>
        </View>
        {asking ? (
          <Text style={s.text}>
            <Text style={s.strong}>{'Waiting for your answer\n'}</Text>
            <Text style={s.soft}>{agent.activity}</Text>
          </Text>
        ) : (
          <Text style={[agent.kind === 'terminal' ? s.textMono : s.text, { color: toneColor(t, what.tone) }]}>{what.text}</Text>
        )}
        {subs.length ? (
          <View style={s.subs}>
            {subs.map((sub, index) => (
              <Text key={index} style={s.sub}>{`↳ ${sub.role} ${model.duration(now - sub.startedAt)}${sub.activity ? ` · ${sub.activity}` : ''}`}</Text>
            ))}
          </View>
        ) : null}
        {agent.branch || changes || test ? (
          <View style={s.foot}>
            {agent.branch ? (
              <Text style={s.branch} numberOfLines={1}>
                {`⎇ ${agent.branch}`}
              </Text>
            ) : null}
            {changes ? <Text style={[s.footText, { color: t.done }]}>{`+${changes.insertions}`}</Text> : null}
            {changes ? <Text style={[s.footText, { color: t.lost }]}>{`−${changes.deletions}`}</Text> : null}
            <View style={s.grow} />
            {test ? <Text style={[s.footText, { color: toneColor(t, test.tone) }]}>{test.text}</Text> : null}
            {changes && settled && agent.branch ? (
              <Press style={s.mini} onPress={() => workspace.send({ type: 'diff', agent: agent.id })}>
                <Text style={s.miniText}>Diff</Text>
              </Press>
            ) : null}
          </View>
        ) : null}
      </Press>
    </Flash>
  );
}

export function Board({ agents, now, entered }: Props) {
  const s = useStyles(styles);
  const reduced = useReduced();
  const window = useWindowDimensions();
  useSecond();
  const members = model.COLUMNS.map(def => agents.filter(agent => model.column(agent) === def.key));
  const firstFilled = Math.max(
    0,
    members.findIndex(list => list.length > 0),
  );
  const [column, setColumn] = useState(firstFilled);
  const [slots, setSlots] = useState<Record<number, Slot>>({});
  const columns = useRef<React.ComponentRef<typeof ScrollView>>(null);
  const picked = useRef<number | null>(null);
  const columnWidth = Math.min(window.width - 2 * GUTTER, COLUMN_MAX);
  const step = columnWidth + GAP;
  const slot = slots[column];
  const duration = lasting(BASE_MS, reduced);
  const x = useTween(slot ? slot.x : 0, duration, EASE_OUT);
  const width = useTween(slot ? slot.width : 0, duration, EASE_OUT);

  useEffect(() => {
    columns.current?.scrollTo({ x: firstFilled * step, animated: false });
    setColumn(firstFilled);
  }, [entered]);

  const measure = (index: number) => (event: LayoutChangeEvent) => {
    const { x: left, width: wide } = event.nativeEvent.layout;
    setSlots(current => (current[index] && current[index].x === left && current[index].width === wide ? current : { ...current, [index]: { x: left, width: wide } }));
  };
  const onScroll = (event: NativeSyntheticEvent<NativeScrollEvent>) => {
    const index = Math.max(0, Math.min(model.COLUMNS.length - 1, Math.round(event.nativeEvent.contentOffset.x / step)));
    if (picked.current !== null) {
      if (index === picked.current) picked.current = null;
      return;
    }
    if (index !== column) setColumn(index);
  };
  const pick = (index: number) => {
    picked.current = index === column ? null : index;
    setColumn(index);
    columns.current?.scrollTo({ x: index * step, animated: !reduced });
  };

  let order = 0;
  return (
    <View accessibilityLabel="board">
      <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.tabs} contentContainerStyle={s.tabsContent}>
        {model.COLUMNS.map((def, index) => {
          const asking = def.key === 'needs' && members[index].length > 0;
          return (
            <View key={def.key} onLayout={measure(index)}>
              <Press style={s.tab} role="tab" selected={index === column} onPress={() => pick(index)}>
                <Text style={[index === column ? s.tabOn : s.tabText, asking && s.tabAsking]}>{def.title}</Text>
                <View style={asking ? s.askingBadge : null}>
                  <Text style={asking ? s.tabCountAsking : s.tabCount}>{String(members[index].length)}</Text>
                </View>
              </Press>
            </View>
          );
        })}
        <View style={[s.indicator, { width, transform: [{ translateX: x }] }]} />
      </ScrollView>
      <ScrollView
        ref={columns}
        horizontal
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={s.columnsContent}
        snapToInterval={step}
        decelerationRate="fast"
        disableIntervalMomentum
        scrollEventThrottle={16}
        onScroll={onScroll}>
        {model.COLUMNS.map((def, index) => (
          <View key={def.key} style={[s.column, { width: columnWidth }]}>
            {members[index].length ? (
              members[index].map(agent => {
                const delay = order * STAGGER_MS;
                order += 1;
                return (
                  <Rise key={agent.id} replay={entered} delay={delay}>
                    <Card agent={agent} now={now} />
                  </Rise>
                );
              })
            ) : (
              <View style={s.empty}>
                <Text style={s.emptyText}>{def.empty}</Text>
              </View>
            )}
          </View>
        ))}
      </ScrollView>
    </View>
  );
}
