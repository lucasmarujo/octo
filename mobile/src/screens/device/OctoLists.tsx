import React, { useCallback, useEffect, useMemo } from 'react';
import { FlatList, StyleSheet, Text, View } from 'react-native';

import { useSecond } from '../../core/clock';
import type { DecisionItem, ProjectRow, Segment } from '../../core/octo';
import { decisionItems, mergeActivity, MODE_NAMES, projectRows, sameRow, saysText, segments } from '../../core/octo';
import type { ActivityEntry, AgentView, OrchestratorView } from '../../core/protocol';
import { useStore } from '../../core/store';
import { openOctoProject } from '../../sheets/OctoProject';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans } from '../../theme/type';
import { Spin, Swatch } from '../../ui/atoms';
import { Icon } from '../../ui/Icon';
import { Press } from '../../ui/Press';
import { useDevice } from './context';
import { DecidingRow, DecisionCard, Head, HandledRow } from './OctoCard';

const MINUTE = 60;
const END_WITHIN = 0.6;
const NO_LIGATURES = ['no-common-ligatures', 'no-contextual'] as const;

const styles = (t: Theme) =>
  StyleSheet.create({
    list: { flex: 1, minHeight: 0 },
    content: { gap: 10, paddingTop: 4, paddingHorizontal: 18, paddingBottom: 24 },
    log: { paddingTop: 4, paddingHorizontal: 18, paddingBottom: 24 },
    empty: { alignItems: 'center', gap: 6, paddingVertical: 36, paddingHorizontal: 24 },
    emptyTitle: { ...sans(18, 800, 22), color: t.text, textAlign: 'center' },
    emptyText: { ...sans(15, 400, 21), color: t.textMuted, textAlign: 'center' },
    foot: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, paddingVertical: 16 },
    footText: { ...mono(12), color: t.textFaint },
    footMark: { ...mono(12, 700), color: t.accent },
    project: { gap: 8, paddingVertical: 14, paddingHorizontal: 14, borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    projectWaiting: { backgroundColor: mix(t.waiting, 6, t.surface), borderColor: mix(t.waiting, 32) },
    projectTop: { flexDirection: 'row', alignItems: 'center', gap: 8 },
    projectName: { ...mono(15, 700, 20), color: t.text, flex: 1, minWidth: 0 },
    mode: { flexDirection: 'row', alignItems: 'center', gap: 4, height: 28, paddingHorizontal: 10, borderRadius: 999, backgroundColor: t.surfaceRaised, flexShrink: 0 },
    modeBypass: { backgroundColor: mix(t.waiting, 16) },
    modeText: { ...sans(13, 700), color: t.text },
    modeFollows: { ...sans(13, 600), color: t.textMuted },
    fleet: { flexDirection: 'row', alignItems: 'center', gap: 10 },
    bar: { flex: 1, flexDirection: 'row', height: 4, borderRadius: 2, overflow: 'hidden', backgroundColor: t.track, gap: 2 },
    counts: { ...mono(12, 400, 16), color: t.textMuted, flexShrink: 0 },
    needs: { ...mono(12, 700, 16), color: t.waiting },
    says: { ...sans(14, 400, 19), color: t.textMuted, fontVariant: [...NO_LIGATURES] },
    saysNeeds: { color: t.waiting },
    saysDeciding: { color: t.accent },
    saysNone: { color: t.textFaint },
  });

type Sheet = ReturnType<typeof styles>;

function Empty({ title, text }: { title: string; text: string }) {
  const s = useStyles(styles);
  return (
    <View style={s.empty}>
      <Text style={s.emptyTitle}>{title}</Text>
      <Text style={s.emptyText}>{text}</Text>
    </View>
  );
}

function decisionRow({ item }: { item: DecisionItem }) {
  if (item.type === 'head') return <Head text={item.text} count={item.count} />;
  if (item.type === 'needs') return <DecisionCard decision={item.decision} />;
  if (item.type === 'deciding') return <DecidingRow decision={item.decision} />;
  if (item.type === 'handled') return <HandledRow entry={item.entry} />;
  return <Empty title="Nothing needs you" text="Octo is handling the rest. A question it leaves for you lands here." />;
}

export const OctoDecisions = React.memo(function OctoDecisions({ view }: { view: OrchestratorView }) {
  const { link } = useDevice();
  const s = useStyles(styles);
  const minute = Math.floor(useSecond() / MINUTE);
  const items = useMemo(() => decisionItems(view, link.now()), [view.decisions, view.handled, minute, link]);
  return <FlatList style={s.list} contentContainerStyle={s.content} data={items} keyExtractor={item => item.key} renderItem={decisionRow} showsVerticalScrollIndicator={false} accessibilityLabel="decisions" />;
});

function activityRow({ item }: { item: ActivityEntry }) {
  return <HandledRow entry={item} />;
}

export const OctoActivity = React.memo(function OctoActivity({ handled }: { handled: ActivityEntry[] }) {
  const { workspace } = useDevice();
  const s = useStyles(styles);
  const log = useStore(workspace.octo.log);
  const entries = useMemo(() => mergeActivity(handled, log.older), [handled, log.older]);

  useEffect(() => workspace.octo.openLog(), [workspace, log.asked]);

  const older = useCallback(() => workspace.octo.older(), [workspace]);
  const foot = log.loading ? (
    <View style={s.foot}>
      <Spin style={s.footMark} />
      <Text style={s.footText}>Reading the log…</Text>
    </View>
  ) : entries.length && log.next === null && log.asked ? (
    <View style={s.foot}>
      <Text style={s.footText}>That is the whole log</Text>
    </View>
  ) : null;

  return (
    <FlatList
      style={s.list}
      contentContainerStyle={s.log}
      data={entries}
      keyExtractor={entry => String(entry.seq)}
      renderItem={activityRow}
      onEndReached={older}
      onEndReachedThreshold={END_WITHIN}
      ListEmptyComponent={log.loading ? undefined : <Empty title="Nothing handled yet" text="Every question Octo answers, denies or leaves for you is listed here." />}
      ListFooterComponent={foot || undefined}
      showsVerticalScrollIndicator={false}
      accessibilityLabel="activity"
    />
  );
});

function Bar({ parts, s }: { parts: Segment[]; s: Sheet }) {
  const t = useTheme();
  const colors = { working: t.accent, needs: t.waiting, done: t.done, idle: t.borderStrong };
  return (
    <View style={s.bar}>
      {parts.map(part => (
        <View key={part.column} style={{ flex: part.share, backgroundColor: colors[part.column] }} />
      ))}
    </View>
  );
}

const Project = React.memo(function Project({ row }: { row: ProjectRow }) {
  const scope = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const bypass = row.mode === 'bypass';
  return (
    <View style={[s.project, row.needs > 0 && s.projectWaiting]}>
      <View style={s.projectTop}>
        <Swatch color={row.color} size={8} radius={3} />
        <Text style={s.projectName} numberOfLines={1}>
          {row.project}
        </Text>
        <Press style={[s.mode, bypass && s.modeBypass]} label={`mode of ${row.project}, ${MODE_NAMES[row.mode]}`} onPress={() => openOctoProject(scope, row.project)}>
          <Text style={[row.override ? s.modeText : s.modeFollows, bypass && { color: t.waiting }]}>{MODE_NAMES[row.mode]}</Text>
          <Icon name="chevron" size={12} color={bypass ? t.waiting : t.textFaint} stroke={2.4} />
        </Press>
      </View>
      <View style={s.fleet}>
        <Bar parts={segments(row.counts)} s={s} />
        <Text style={s.counts}>
          {`${row.counts.total} ${row.counts.total === 1 ? 'agent' : 'agents'}`}
          {row.needs ? <Text style={s.needs}>{` ‖${row.needs}`}</Text> : null}
          {` · ${row.handled} handled`}
        </Text>
      </View>
      <Text style={[s.says, row.says.kind === 'needs' && s.saysNeeds, row.says.kind === 'deciding' && s.saysDeciding, row.says.kind === 'none' && s.saysNone]} numberOfLines={2}>
        {saysText(row.says)}
      </Text>
    </View>
  );
}, (before, after) => sameRow(before.row, after.row));

function projectRow({ item }: { item: ProjectRow }) {
  return <Project row={item} />;
}

export const OctoProjects = React.memo(function OctoProjects({ agents, view }: { agents: AgentView[]; view: OrchestratorView }) {
  const s = useStyles(styles);
  const rows = useMemo(() => projectRows(agents, view), [agents, view.decisions, view.handled, view.handledByProject, view.projects, view.mode]);
  return (
    <FlatList
      style={s.list}
      contentContainerStyle={s.content}
      data={rows}
      keyExtractor={row => row.project}
      renderItem={projectRow}
      ListEmptyComponent={<Empty title="No projects yet" text="Projects show up here as soon as an agent runs in one." />}
      showsVerticalScrollIndicator={false}
      accessibilityLabel="projects"
    />
  );
});
