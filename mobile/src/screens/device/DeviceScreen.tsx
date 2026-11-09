import { useEffect, useMemo, useRef } from 'react';
import { BackHandler, ScrollView, StyleSheet, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { Device } from '../../core/devices';
import { changeDevice, findDevice } from '../../core/devices';
import { buzz } from '../../core/haptics';
import type { DeviceLink } from '../../core/link';
import { watching } from '../../core/nav';
import type { Layout } from '../../core/settings';
import { changeSettings, settings } from '../../core/settings';
import { useStore } from '../../core/store';
import { Workspace } from '../../core/workspace';
import { openAgentMenu } from '../../sheets/AgentMenu';
import { openAppearance } from '../../sheets/Appearance';
import { openDiff } from '../../sheets/Diff';
import { openPrompt } from '../../sheets/Prompt';
import { openUsage } from '../../sheets/Usage';
import { openWhere } from '../../sheets/Where';
import { openWho } from '../../sheets/Who';
import { useKeyboard } from '../../ui/keyboard';
import { Rise } from '../../ui/Rise';
import { toast } from '../../ui/Toast';
import type { PairKind } from '../Pair';
import { Pair } from '../Pair';
import { AgentList, Skeleton } from './AgentList';
import { Lost, Needs, Offline } from './Alerts';
import { Approval, Keys } from './Approval';
import { Board } from './Board';
import type { ComposerHandle } from './Composer';
import { Composer } from './Composer';
import type { DeviceScope } from './context';
import { DeviceContext } from './context';
import { Header } from './Header';
import { Rings } from './Rings';
import { Sessions } from './Sessions';
import { Chips, Terminal } from './Terminal';
import { Viewbar } from './Viewbar';

type Props = {
  device: Device;
  link: DeviceLink;
  agent?: number;
  at?: number;
  onBack: () => void;
  onMenu: () => void;
  onPaired: (device: Device) => void;
};

const DOCK_ROOM = 160;
const VIEW_FROM = 8;
const OFFLINE_OPACITY = 0.4;
const STOPPED: Partial<Record<string, PairKind>> = { unpaired: 'unpaired', replaced: 'replaced', outdated: 'outdated' };
const NO_PREFS = { who: null, worktree: false, bypass: false };

const styles = StyleSheet.create({
  view: { flex: 1 },
  main: { flex: 1, minHeight: 0 },
  dimmed: { opacity: OFFLINE_OPACITY },
});

export function DeviceScreen({ device, link, agent: wanted, at, onBack, onMenu, onPaired }: Props) {
  const insets = useSafeAreaInsets();
  const keyboard = useKeyboard();
  const composer = useRef<ComposerHandle>(null);
  const scopeRef = useRef<DeviceScope | null>(null);

  const workspace = useMemo(
    () =>
      new Workspace(link, {
        toast: (text, tone, ask) => toast(text, tone, ask),
        buzz,
        diff: message => {
          if (scopeRef.current) openDiff(scopeRef.current, message);
        },
        prefs: () => findDevice(device.id)?.prefs || NO_PREFS,
        reduced: () => settings.get().reduced,
      }),
    [link, device.id],
  );
  useEffect(() => () => workspace.dispose(), [workspace]);

  const scope = useMemo<DeviceScope>(() => ({ device, link, workspace }), [device, link, workspace]);
  scopeRef.current = scope;

  const state = useStore(workspace.store);
  const { phase, overview, usage } = useStore(link.store);
  const layout = useStore(settings).layout;

  useEffect(() => {
    if (wanted !== undefined) workspace.openAgent(wanted);
  }, [workspace, wanted, at]);

  useEffect(() => {
    watching.set({ device: device.id, agent: state.view === 'agent' ? state.agent : null });
  }, [device.id, state.view, state.agent]);
  useEffect(() => () => watching.set(null), []);

  useEffect(() => {
    const back = BackHandler.addEventListener('hardwareBackPress', () => {
      if (workspace.store.get().view === 'agent') workspace.leaveAgent();
      else onBack();
      return true;
    });
    return () => back.remove();
  }, [workspace, onBack]);

  const stopped = STOPPED[phase];
  if (stopped) return <Pair kind={stopped} replace={device.id} onBack={onBack} onPaired={onPaired} onRetry={() => link.retryNow()} />;

  const setLayout = (mode: Layout) => {
    if (mode === layout) return;
    changeSettings({ layout: mode });
    workspace.replay();
  };
  const setPrefs = (part: Partial<Device['prefs']>) => {
    const current = findDevice(device.id);
    if (current) changeDevice(device.id, { prefs: { ...current.prefs, ...part } });
  };
  const toggleWorktree = () => {
    const on = !device.prefs.worktree;
    setPrefs({ worktree: on });
    toast(on ? 'New agents get their own git worktree' : 'New agents run directly in the folder', 'info', { mark: { text: '⎇', tile: 'idle' } });
  };
  const toggleBypass = () => {
    const on = !device.prefs.bypass;
    setPrefs({ bypass: on });
    if (on) toast('Bypass on: new agents skip every permission check', 'warn', { mark: { text: '⚡', tile: 'working' } });
    else toast('New agents ask before running tools', 'info');
  };
  const newSession = () => openPrompt('New session', 'name', '', name => workspace.newSession(name));
  const openSettings = () => openAppearance({ paired: { name: overview ? overview.host : device.name, open: onMenu }, onLayout: () => workspace.replay() });

  const list = workspace.inSession();
  const now = link.now();
  const agent = state.view === 'agent' ? workspace.findAgent(state.agent) : null;
  const dimmed = phase === 'offline' ? styles.dimmed : null;

  return (
    <DeviceContext.Provider value={scope}>
      <View style={[styles.view, { paddingBottom: keyboard }]}>
        <Header onBack={onBack} onSettings={openSettings} onMenu={() => openAgentMenu(scope)} />
        {state.view === 'agent' ? (
          <Rise key="agent" replay={state.agent || 0} from={VIEW_FROM} shrink={1} style={styles.main}>
            <Chips agent={agent} />
            <Terminal />
            {state.approval ? <Approval asking={state.approval} agent={agent} /> : <Keys />}
          </Rise>
        ) : (
          <Rise key="overview" from={VIEW_FROM} shrink={1} style={styles.main}>
            <ScrollView contentContainerStyle={{ paddingBottom: (keyboard ? 0 : insets.bottom) + DOCK_ROOM }} keyboardShouldPersistTaps="handled" showsVerticalScrollIndicator={false}>
              {overview ? (
                <>
                  <Sessions overview={overview} filter={state.filter} onPick={name => workspace.pickSession(name)} onNew={newSession} />
                  {phase === 'offline' ? <Offline onRetry={() => link.retryNow()} /> : null}
                  <Lost agents={list} now={now} onRecover={() => workspace.send({ type: 'recover', session: state.filter || '' })} />
                  <View style={dimmed}>
                    <Needs agents={list} previews={state.previews} now={now} />
                    <Rings entries={usage || []} now={now} onOpen={() => openUsage(link)} />
                    <Viewbar layout={layout} agents={list} onLayout={setLayout} />
                    {layout === 'board' && list.length ? (
                      <Board agents={list} now={now} entered={state.entered} />
                    ) : (
                      <AgentList agents={list} now={now} entered={state.entered} session={state.filter || ''} onStarter={text => composer.current?.fill(text)} />
                    )}
                  </View>
                </>
              ) : (
                <Skeleton />
              )}
            </ScrollView>
          </Rise>
        )}
        <Composer ref={composer} onWho={() => openWho(scope)} onWhere={() => openWhere(scope)} onWorktree={toggleWorktree} onBypass={toggleBypass} />
      </View>
    </DeviceContext.Provider>
  );
}
