import { useCallback, useMemo, useState } from 'react';
import { ScrollView, StyleSheet, Text, View } from 'react-native';

import type { OctoTab } from '../../core/octo';
import { bypassAsk, chatCards, engineKinds, engineName, entryLine, gateOf, installedEngines, MODE_TEXTS, moreCards, needsCount, OCTO_TABS, startChoice, TAB_NAMES } from '../../core/octo';
import type { AgentView, Mode, OrchestratorView } from '../../core/protocol';
import { useSelect, useStore } from '../../core/store';
import { openConfirm } from '../../sheets/Confirm';
import { MODE_ITEMS, openOctoSettings, pickMode } from '../../sheets/OctoSettings';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, TIGHT, tracking } from '../../theme/type';
import { kindColor } from '../../ui/atoms';
import { Brand } from '../../ui/Brand';
import { Button } from '../../ui/Button';
import { Icon } from '../../ui/Icon';
import { Group, Note, Option } from '../../ui/Option';
import { Press } from '../../ui/Press';
import { ProviderMark } from '../../ui/ProviderMark';
import type { SegmentItem } from '../../ui/Segmented';
import { Segmented } from '../../ui/Segmented';
import { Skeleton } from './AgentList';
import { useDevice } from './context';
import { OctoChat } from './OctoChat';
import { OctoActivity, OctoDecisions, OctoProjects } from './OctoLists';

const MARK = 72;
const ENTRY_MARK = 30;
const PICK_ICON = 14;
const OPTION_ICON = 18;
const STALE_OPACITY = 0.4;
const NO_AGENTS: AgentView[] = [];
const AWAY_TITLE = 'Open Octo on the computer';
const AWAY_TEXT = 'Octo answers your agents from the app on the computer. Open it there and this screen follows.';

const styles = (t: Theme) =>
  StyleSheet.create({
    octo: { flex: 1, minHeight: 0 },
    body: { flex: 1, minHeight: 0 },
    stale: { opacity: STALE_OPACITY },
    strip: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 8, paddingHorizontal: 18, paddingBottom: 10 },
    engine: { flexDirection: 'row', alignItems: 'center', gap: 5, height: 34, paddingHorizontal: 10, borderRadius: 999, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, flexShrink: 1, minWidth: 0 },
    engineText: { ...mono(12, 700), flexShrink: 1 },
    tabs: { paddingHorizontal: 18, paddingBottom: 12 },
    center: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingHorizontal: 32, paddingBottom: 48 },
    title: { ...sans(26, 800, 30), ...tracking(26, TIGHT), color: t.text, textAlign: 'center' },
    text: { ...sans(16, 400, 22), color: t.textMuted, textAlign: 'center' },
    banner: { flexDirection: 'row', alignItems: 'center', gap: 12, marginHorizontal: 18, marginBottom: 12, paddingVertical: 12, paddingHorizontal: 14, borderRadius: 16, backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.borderStrong },
    bannerText: { flex: 1, minWidth: 0 },
    bannerTitle: { ...sans(15, 700), color: t.text },
    bannerSub: { ...sans(13), color: t.textMuted },
    intro: { gap: 16, paddingTop: 12, paddingHorizontal: 18, paddingBottom: 32 },
    introHead: { alignItems: 'center', gap: 12, paddingHorizontal: 14, paddingBottom: 6 },
    section: { gap: 8 },
    caps: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.textFaint, paddingHorizontal: 4 },
    mark: mono(15),
    off: { opacity: 0.4 },
    wrap: { paddingHorizontal: 18, paddingBottom: 12 },
    entry: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 12, paddingRight: 12, paddingLeft: 14, borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    entryWaiting: { backgroundColor: mix(t.waiting, 6, t.surface), borderColor: mix(t.waiting, 32) },
    entryHeld: { backgroundColor: t.surfaceRaised },
    entryBody: { flex: 1, minWidth: 0 },
    entryTitle: { ...sans(16, 700, 20), color: t.text },
    entrySub: { ...mono(12, 400, 16), color: t.textMuted },
    badge: { paddingVertical: 2, paddingHorizontal: 8, borderRadius: 999, backgroundColor: t.waiting, flexShrink: 0 },
    badgeText: { ...mono(12, 700), color: t.waitingInk },
    chevron: { ...sans(18), color: t.textFaint, flexShrink: 0 },
  });

export function OctoEntry() {
  const { workspace } = useDevice();
  const s = useStyles(styles);
  const line = useSelect(workspace.octo.view, view => entryLine(view).text);
  const needs = useSelect(workspace.octo.view, needsCount);
  return (
    <View style={s.wrap}>
      <Press style={[s.entry, needs > 0 && s.entryWaiting]} held={s.entryHeld} label={needs ? `Octo, ${needs} need you` : 'Octo'} onPress={() => workspace.openOcto()}>
        <Brand mood={needs ? 'needs' : 'idle'} size={ENTRY_MARK} />
        <View style={s.entryBody}>
          <Text style={s.entryTitle}>Octo</Text>
          <Text style={s.entrySub} numberOfLines={1}>
            {line}
          </Text>
        </View>
        {needs ? (
          <View style={s.badge}>
            <Text style={s.badgeText}>{String(needs)}</Text>
          </View>
        ) : null}
        <Text style={s.chevron}>›</Text>
      </Press>
    </View>
  );
}

function Away() {
  const s = useStyles(styles);
  return (
    <View style={s.center}>
      <Brand mood="idle" size={MARK} />
      <Text style={s.title}>{AWAY_TITLE}</Text>
      <Text style={s.text}>{AWAY_TEXT}</Text>
    </View>
  );
}

function Stale() {
  const t = useTheme();
  const s = useStyles(styles);
  return (
    <View style={s.banner}>
      <Icon name="offline" size={20} color={t.textMuted} />
      <View style={s.bannerText}>
        <Text style={s.bannerTitle}>{AWAY_TITLE}</Text>
        <Text style={s.bannerSub}>Showing what it last sent</Text>
      </View>
    </View>
  );
}

function Intro({ view }: { view: OrchestratorView }) {
  const { workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const [picked, setPicked] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>(view.mode);
  const installed = installedEngines(view);
  const primary = startChoice(view, picked);
  const turnOn = () => {
    if (!primary) return;
    if (mode === 'bypass') openConfirm(bypassAsk(null), () => workspace.octo.turnOn(primary, mode));
    else workspace.octo.turnOn(primary, mode);
  };
  return (
    <ScrollView contentContainerStyle={s.intro} showsVerticalScrollIndicator={false}>
      <View style={s.introHead}>
        <Brand mood="idle" size={MARK} />
        <Text style={s.title}>Let Octo answer your agents</Text>
        <Text style={s.text}>When an agent stops to ask something, Octo reads the question, thinks with an agent you already have and answers it, or leaves it for you with what it would say.</Text>
      </View>
      <View style={s.section}>
        <Text style={s.caps}>ENGINE</Text>
        <Group>
          {engineKinds(view).map(kind => {
            const ready = installed.includes(kind);
            return (
              <Option
                key={kind}
                icon={() => <ProviderMark kind={kind} size={OPTION_ICON} style={[s.mark, { color: kindColor(t, kind) }]} />}
                iconFill={mix(kindColor(t, kind), 14)}
                text={engineName(kind)}
                detail={ready ? 'on the plan you already have' : 'not installed on the computer'}
                tone={kind === primary ? 'current' : undefined}
                trail={kind === primary ? <Icon name="check" size={16} color={t.link} /> : undefined}
                onPress={() => (ready ? setPicked(kind) : undefined)}
              />
            );
          })}
        </Group>
        {installed.length ? null : <Note text="Octo thinks with Claude or Codex. Install one of them on the computer and it can be turned on." />}
      </View>
      <View style={s.section}>
        <Text style={s.caps}>MODE</Text>
        <Segmented items={MODE_ITEMS} value={mode} label="mode" stretch onPick={setMode} />
        <Note text={MODE_TEXTS[mode]} />
      </View>
      <Button kind="primary" big text="Turn on Octo" style={primary ? null : s.off} onPress={turnOn} />
    </ScrollView>
  );
}

function Strip({ view }: { view: OrchestratorView }) {
  const scope = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const tint = view.primary ? kindColor(t, view.primary) : t.textMuted;
  return (
    <View style={s.strip}>
      <Press style={s.engine} label="engine and settings of Octo" onPress={() => openOctoSettings(scope)}>
        {view.primary ? <ProviderMark kind={view.primary} size={PICK_ICON} style={[s.engineText, { color: tint }]} /> : null}
        <Text style={[s.engineText, { color: tint }]} numberOfLines={1}>
          {view.primary || 'no engine'}
        </Text>
        <Icon name="chevron" size={12} color={tint} stroke={2.4} />
      </Press>
      <Segmented items={MODE_ITEMS} value={view.mode} label="mode" onPick={mode => pickMode(scope, view, mode)} />
    </View>
  );
}

export function Octo() {
  const { link, workspace } = useDevice();
  const s = useStyles(styles);
  const tab = useSelect(workspace.store, state => state.tab);
  const view = useStore(workspace.octo.view);
  const agents = useSelect(link.store, state => (state.overview ? state.overview.agents : NO_AGENTS));
  const needs = needsCount(view);
  const tabs = useMemo<SegmentItem<OctoTab>[]>(() => OCTO_TABS.map(value => ({ value, text: TAB_NAMES[value], count: value === 'decisions' ? needs : 0 })), [needs]);
  const cards = useMemo(() => (view ? chatCards(view) : []), [view && view.decisions]);
  const showDecisions = useCallback(() => workspace.showTab('decisions'), [workspace]);

  if (!view) return <Skeleton />;
  const gate = gateOf(view);
  if (gate === 'offline') return <Away />;
  if (gate === 'intro') return <Intro view={view} />;
  const stale = gate === 'stale';
  return (
    <View style={s.octo}>
      {stale ? <Stale /> : null}
      <View style={[s.body, stale && s.stale]} pointerEvents={stale ? 'none' : 'auto'}>
        <Strip view={view} />
        <View style={s.tabs}>
          <Segmented items={tabs} value={tab} label="Octo" stretch onPick={value => workspace.showTab(value)} />
        </View>
        {tab === 'chat' ? <OctoChat chat={view.chat} cards={cards} more={moreCards(view)} thinking={view.thinking} problem={view.problem} onMore={showDecisions} /> : null}
        {tab === 'decisions' ? <OctoDecisions view={view} /> : null}
        {tab === 'activity' ? <OctoActivity handled={view.handled} /> : null}
        {tab === 'projects' ? <OctoProjects agents={agents} view={view} /> : null}
      </View>
    </View>
  );
}
