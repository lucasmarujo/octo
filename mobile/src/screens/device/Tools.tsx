import type { StyleProp, ViewStyle } from 'react-native';
import { StyleSheet, Text, View } from 'react-native';

import { alive, idleRunning, idleStopped, removeAllAsk, removeAsk, still, stopAllAsk, stopAsk } from '../../core/agents';
import type { AgentView } from '../../core/protocol';
import { openConfirm } from '../../sheets/Confirm';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { sans } from '../../theme/type';
import { Icon } from '../../ui/Icon';
import { Press } from '../../ui/Press';
import { useDevice } from './context';

const TOOL = 36;
const TOOL_ICON = 18;

const styles = (t: Theme) =>
  StyleSheet.create({
    tool: { width: TOOL, height: TOOL, borderRadius: 12, alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    held: { backgroundColor: t.surfaceRaised },
    bar: { flexDirection: 'row', justifyContent: 'flex-end', gap: 8 },
    button: { paddingVertical: 6, paddingHorizontal: 12, borderRadius: 10, backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.border },
    text: { ...sans(13, 700), color: t.text },
    warn: { color: t.lost },
  });

export function AgentTool({ agent, style }: { agent: AgentView; style?: StyleProp<ViewStyle> }) {
  const { workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const running = alive(agent);
  const ask = () => openConfirm(running ? stopAsk(agent) : removeAsk(agent), () => workspace.send(running ? { type: 'stop', agent: agent.id } : { type: 'remove', agent: agent.id }));
  return (
    <Press style={[s.tool, style]} held={s.held} label={`${running ? 'stop' : 'remove'} ${agent.name}`} onPress={ask}>
      <Icon name={running ? 'stop' : 'trash'} size={TOOL_ICON} color={t.textFaint} />
    </Press>
  );
}

export function IdleTools({ agents }: { agents: AgentView[] }) {
  const { workspace } = useDevice();
  const s = useStyles(styles);
  const running = idleRunning(agents);
  const stopped = idleStopped(agents);
  if (!running.length && !stopped.length) return null;
  const stopAll = () =>
    openConfirm(stopAllAsk(running), () => {
      for (const agent of still(running, idleRunning(workspace.inSession()))) workspace.send({ type: 'stop', agent: agent.id });
    });
  const removeAll = () =>
    openConfirm(removeAllAsk(stopped), () => {
      for (const agent of still(stopped, idleStopped(workspace.inSession()))) workspace.send({ type: 'remove', agent: agent.id });
    });
  return (
    <View style={s.bar}>
      {running.length ? (
        <Press style={s.button} onPress={stopAll}>
          <Text style={s.text}>Stop all</Text>
        </Press>
      ) : null}
      {stopped.length ? (
        <Press style={s.button} onPress={removeAll}>
          <Text style={[s.text, s.warn]}>Remove all</Text>
        </Press>
      ) : null}
    </View>
  );
}
