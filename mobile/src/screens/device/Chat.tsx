import React, { useCallback, useEffect, useRef, useState } from 'react';
import type { NativeScrollEvent, NativeSyntheticEvent, TextStyle } from 'react-native';
import { FlatList, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import { alive, elapsed, testMark } from '../../core/agents';
import type { Block, Step, Table } from '../../core/chat';
import { columnWidths, lineKind, pieces, running, spans, stepNote, stepTitle, unjoined } from '../../core/chat';
import { useSecond } from '../../core/clock';
import type { AgentView } from '../../core/protocol';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, tracking } from '../../theme/type';
import { Spin } from '../../ui/atoms';
import { Icon } from '../../ui/Icon';
import { Press } from '../../ui/Press';
import { useDevice } from './context';

const STICK_WITHIN = 80;
const ARMED_MS = 3000;
const DONE_MARK = '✓';
const FAILED_MARK = '!';
const QUIET_MARK = '·';
const CHEVRON = 14;
const CLOSED_TURN = '-90deg';
const OPEN_TURN = '0deg';
const WORKING_MARK = '✳';

export const chatStyles = (t: Theme) =>
  StyleSheet.create({
    chat: { flex: 1, minHeight: 0 },
    content: { flexGrow: 1, justifyContent: 'flex-end', gap: 14, paddingTop: 4, paddingHorizontal: 16, paddingBottom: 14 },
    user: { alignSelf: 'flex-end', maxWidth: '82%', paddingVertical: 10, paddingHorizontal: 14, backgroundColor: t.surfaceRaised, borderTopLeftRadius: 18, borderTopRightRadius: 18, borderBottomLeftRadius: 18, borderBottomRightRadius: 4 },
    userText: { ...sans(15, 400, 21), color: t.text },
    said: { gap: 10 },
    text: { ...sans(15, 400, 22), color: t.text },
    strong: sans(15, 700, 22),
    inlineCode: { ...mono(13, 400, 22), color: t.link },
    code: { paddingVertical: 10, paddingHorizontal: 12, borderRadius: 12, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    codeText: { ...mono(11, 400, 16), color: t.textMuted },
    table: { flexGrow: 0, overflow: 'hidden', borderRadius: 12, borderWidth: 1, borderColor: t.border, backgroundColor: t.surface },
    tableRow: { flexDirection: 'row' },
    tableHead: { backgroundColor: t.surfaceRaised },
    tableCell: { paddingVertical: 8, paddingHorizontal: 10 },
    tableDivided: { borderLeftWidth: 1, borderColor: t.border },
    tableText: { ...sans(13, 400, 18), color: t.text },
    tableHeadText: { ...sans(13, 700, 18), color: t.text },
    card: { borderRadius: 14, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    live: { backgroundColor: mix(t.accent, 5), borderColor: mix(t.accent, 40) },
    divided: { borderTopWidth: 1, borderColor: t.border },
    row: { flexDirection: 'row', alignItems: 'center', gap: 10, paddingVertical: 10, paddingHorizontal: 12 },
    mark: { width: 22, height: 22, borderRadius: 11, alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    markDone: { backgroundColor: mix(t.done, 14) },
    markFailed: { backgroundColor: mix(t.lost, 14) },
    markLive: { backgroundColor: mix(t.accent, 16) },
    markQuiet: { backgroundColor: t.surfaceRaised },
    markText: mono(11, 700, 14),
    title: { ...sans(14, 600, 18), color: t.text, flex: 1, minWidth: 0 },
    note: { ...mono(11, 400, 14), color: t.textFaint, flexShrink: 0 },
    chevron: { flexShrink: 0 },
    green: { color: t.done },
    red: { color: t.lost },
    accent: { color: t.accent },
    faint: { color: t.textFaint },
    body: { borderTopWidth: 1, borderColor: t.border, paddingVertical: 8 },
    detail: { ...mono(11, 400, 16), color: t.textMuted, paddingHorizontal: 12, paddingBottom: 6 },
    line: { ...mono(11, 400, 17), color: t.textMuted, paddingHorizontal: 12 },
    added: { color: t.done, backgroundColor: mix(t.done, 10) },
    removed: { color: t.lost, backgroundColor: mix(t.lost, 12) },
    command: { ...mono(11, 400, 16), color: t.textMuted, paddingHorizontal: 12, paddingBottom: 12 },
    status: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 12 },
    statusText: { ...mono(12, 400, 16), color: t.accent, flex: 1, minWidth: 0 },
    stop: { height: 28, paddingHorizontal: 12, borderRadius: 999, borderWidth: 1, borderColor: mix(t.lost, 45), alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    stopText: { ...sans(13, 600, 16), color: t.lost },
    done: { gap: 10, padding: 12, borderRadius: 14, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    doneHead: { ...mono(11, 700, 14), ...tracking(11, CAPS), color: t.done },
    doneRow: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 12 },
    doneLabel: { ...mono(12, 700, 16), color: t.text, flex: 1, minWidth: 0 },
    doneValue: { ...mono(12, 400, 16), flexShrink: 0 },
    actions: { flexDirection: 'row', gap: 8 },
    action: { flex: 1, height: 42, borderRadius: 12, alignItems: 'center', justifyContent: 'center', backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.border },
    actionText: { ...sans(14, 600, 18), color: t.text },
    primary: { backgroundColor: t.accent, borderColor: t.accent },
    primaryText: { ...sans(14, 700, 18), color: t.accentInk },
  });

type Sheet = ReturnType<typeof chatStyles>;

export function useFollow<T>() {
  const list = useRef<FlatList<T>>(null);
  const stuck = useRef(true);
  const height = useRef(0);
  const follow = useCallback((_width?: number, tall?: number) => {
    if (typeof tall === 'number') height.current = tall;
    if (stuck.current && height.current > 0) list.current?.scrollToOffset({ offset: height.current, animated: false });
  }, []);
  const scrolled = useCallback((event: NativeSyntheticEvent<NativeScrollEvent>) => {
    const { contentOffset, contentSize, layoutMeasurement } = event.nativeEvent;
    stuck.current = contentSize.height - contentOffset.y - layoutMeasurement.height < STICK_WITHIN;
  }, []);
  return { list, follow, scrolled };
}

function Spans({ text, style, s }: { text: string; style: TextStyle; s: Sheet }) {
  return (
    <Text style={style} selectable>
      {spans(text).map((span, at) => (
        <Text key={at} style={span.code ? s.inlineCode : span.bold ? s.strong : null}>
          {span.text}
        </Text>
      ))}
    </Text>
  );
}

function Grid({ table, s }: { table: Table; s: Sheet }) {
  const widths = columnWidths(table);
  const row = (cells: string[], head: boolean, index: number) => (
    <View key={index} style={[s.tableRow, index > 0 && s.divided, head && s.tableHead]}>
      {cells.map((cell, column) => (
        <View key={column} style={[s.tableCell, column > 0 && s.tableDivided, { width: widths[column] }]}>
          <Spans text={cell} style={head ? s.tableHeadText : s.tableText} s={s} />
        </View>
      ))}
    </View>
  );
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.table} accessibilityLabel="table">
      <View>{[table.head, ...table.rows].map((cells, index) => row(cells, index === 0, index))}</View>
    </ScrollView>
  );
}

export function Said({ text, s }: { text: string; s: Sheet }) {
  return (
    <View style={s.said}>
      {pieces(text).map((piece, index) => {
        if (piece.kind === 'table') return <Grid key={index} table={piece.table} s={s} />;
        if (piece.kind === 'code') {
          return (
            <View key={index} style={s.code}>
              <Text style={s.codeText} selectable>
                {piece.text}
              </Text>
            </View>
          );
        }
        return <Spans key={index} text={piece.text} style={s.text} s={s} />;
      })}
    </View>
  );
}

function Lines({ lines, s }: { lines: string[]; s: Sheet }) {
  return (
    <>
      {lines.map((line, index) => {
        const kind = lineKind(line);
        return (
          <Text key={index} style={[s.line, kind === 'add' && s.added, kind === 'del' && s.removed]} numberOfLines={1}>
            {line || ' '}
          </Text>
        );
      })}
    </>
  );
}

function StepRow({ step, first, working, s }: { step: Step; first: boolean; working: boolean; s: Sheet }) {
  const t = useTheme();
  const [open, setOpen] = useState(false);
  const note = stepNote(step);
  const live = !step.outcome && working;
  const failed = Boolean(step.outcome && step.outcome.failed);
  const expandable = step.body.length > 0 || Boolean(step.outcome && step.outcome.body.length);
  const changed = note.added + note.removed > 0;
  const result = step.outcome && !changed ? step.outcome.body : [];
  return (
    <View style={first ? null : s.divided}>
      <Pressable style={s.row} onPress={() => setOpen(!open)} disabled={!expandable} accessibilityRole="button" accessibilityState={{ expanded: open }} accessibilityLabel={stepTitle(step)}>
        <View style={[s.mark, live ? s.markLive : failed ? s.markFailed : step.outcome ? s.markDone : s.markQuiet]}>
          {live ? <Spin style={[s.markText, s.accent]} /> : <Text style={[s.markText, failed ? s.red : step.outcome ? s.green : s.faint]}>{failed ? FAILED_MARK : step.outcome ? DONE_MARK : QUIET_MARK}</Text>}
        </View>
        <Text style={s.title} numberOfLines={1}>
          {unjoined(stepTitle(step))}
        </Text>
        {changed ? (
          <Text style={s.note}>
            <Text style={s.green}>{`+${note.added}`}</Text>
            <Text style={s.red}>{` −${note.removed}`}</Text>
          </Text>
        ) : note.text ? (
          <Text style={[s.note, failed && s.red]}>{note.text}</Text>
        ) : null}
        {expandable ? (
          <View style={[s.chevron, { transform: [{ rotate: open ? OPEN_TURN : CLOSED_TURN }] }]}>
            <Icon name="chevron" size={CHEVRON} color={t.textFaint} />
          </View>
        ) : null}
      </Pressable>
      {open ? (
        <View style={s.body}>
          {step.detail ? (
            <Text style={s.detail} numberOfLines={2}>
              {step.detail}
            </Text>
          ) : null}
          <Lines lines={step.body} s={s} />
          <Lines lines={result} s={s} />
        </View>
      ) : null}
    </View>
  );
}

function Steps({ steps, working, s }: { steps: Step[]; working: boolean; s: Sheet }) {
  return (
    <View style={s.card}>
      {steps.map((step, index) => (
        <StepRow key={step.id} step={step} first={index === 0} working={working} s={s} />
      ))}
    </View>
  );
}

function Live({ step, s }: { step: Step; s: Sheet }) {
  const command = step.body.find(line => line.trim() !== '');
  return (
    <View style={[s.card, s.live]}>
      <View style={s.row}>
        <View style={[s.mark, s.markLive]}>
          <Spin style={[s.markText, s.accent]} />
        </View>
        <Text style={s.title} numberOfLines={1}>
          {unjoined(stepTitle(step))}
        </Text>
      </View>
      {command ? (
        <Text style={s.command} numberOfLines={2}>
          {command}
        </Text>
      ) : null}
    </View>
  );
}

function Working({ agent, s }: { agent: AgentView; s: Sheet }) {
  const { link, workspace } = useDevice();
  useSecond();
  const time = elapsed(agent, link.now());
  return (
    <View style={s.status}>
      <Text style={s.statusText} numberOfLines={1}>
        {`${WORKING_MARK} ${agent.activity || 'Working'}… `}
        <Text style={s.faint}>{time || ''}</Text>
      </Text>
      <Press style={s.stop} label="interrupt the agent" onPress={() => workspace.interrupt()}>
        <Text style={s.stopText}>Stop</Text>
      </Press>
    </View>
  );
}

function Done({ agent, s }: { agent: AgentView; s: Sheet }) {
  const { link, workspace } = useDevice();
  const [armed, setArmed] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );
  const changes = agent.changes && agent.changes.files ? agent.changes : null;
  const test = testMark(agent.test);
  const time = elapsed(agent, link.now());
  const merge = () => {
    if (armed) {
      setArmed(false);
      workspace.send({ type: 'merge', agent: agent.id });
      return;
    }
    setArmed(true);
    timer.current = setTimeout(() => setArmed(false), ARMED_MS);
  };
  return (
    <View style={s.done}>
      <Text style={s.doneHead}>{`${DONE_MARK} DONE${time ? ` · ${time}` : ''}`}</Text>
      {changes ? (
        <View style={s.doneRow}>
          <Text style={s.doneLabel} numberOfLines={1}>{`${changes.files} file${changes.files === 1 ? '' : 's'} changed`}</Text>
          <Text style={s.doneValue}>
            <Text style={s.green}>{`+${changes.insertions}`}</Text>
            <Text style={s.red}>{` −${changes.deletions}`}</Text>
          </Text>
        </View>
      ) : null}
      {test ? (
        <View style={s.doneRow}>
          <Text style={s.doneLabel}>tests</Text>
          <Text style={[s.doneValue, test.tone === 'red' ? s.red : test.tone === 'green' ? s.green : s.faint]}>{test.text}</Text>
        </View>
      ) : null}
      {agent.branch ? (
        <View style={s.actions}>
          <Press style={s.action} onPress={() => workspace.send({ type: 'diff', agent: agent.id })}>
            <Text style={s.actionText}>View diff</Text>
          </Press>
          <Press style={[s.action, s.primary]} onPress={merge}>
            <Text style={s.primaryText}>{armed ? 'Tap again to merge' : 'Merge'}</Text>
          </Press>
        </View>
      ) : null}
    </View>
  );
}

export function Chat({ blocks, agent }: { blocks: Block[]; agent: AgentView | null }) {
  const s = useStyles(chatStyles);
  const { list, follow, scrolled } = useFollow<Block>();
  const working = Boolean(agent && alive(agent) && (agent.starting || agent.state === 'working'));
  const current = working ? running(blocks) : null;

  const render = useCallback(
    ({ item }: { item: Block }) => {
      if (item.type === 'user') {
        return (
          <View style={s.user}>
            <Text style={s.userText} selectable>
              {item.text}
            </Text>
          </View>
        );
      }
      if (item.type === 'text') return <Said text={item.text} s={s} />;
      const steps = current ? item.steps.filter(step => step !== current) : item.steps;
      return (
        <>
          {steps.length ? <Steps steps={steps} working={working} s={s} /> : null}
          {current && item.steps.includes(current) ? <View style={steps.length ? { marginTop: 14 } : null}><Live step={current} s={s} /></View> : null}
        </>
      );
    },
    [s, current, working],
  );

  const foot = agent ? working ? <Working agent={agent} s={s} /> : agent.state === 'done' ? <Done agent={agent} s={s} /> : null : null;

  return (
    <FlatList
      ref={list}
      style={s.chat}
      contentContainerStyle={s.content}
      data={blocks}
      keyExtractor={block => block.key}
      renderItem={render}
      ListFooterComponent={foot || undefined}
      onContentSizeChange={follow}
      onLayout={() => follow()}
      onScroll={scrolled}
      scrollEventThrottle={64}
      keyboardShouldPersistTaps="handled"
      showsVerticalScrollIndicator={false}
      accessibilityLabel="agent conversation"
    />
  );
}
