import { StyleSheet, Text, View } from 'react-native';

import { elapsed, label, waiting } from '../../core/agents';
import { useSecond } from '../../core/clock';
import type { Asked } from '../../core/model';
import model from '../../core/model';
import type { AgentView } from '../../core/protocol';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, tracking } from '../../theme/type';
import { kindColor, Tile } from '../../ui/atoms';
import { Icon } from '../../ui/Icon';
import { Press } from '../../ui/Press';
import { Rise } from '../../ui/Rise';
import { useDevice } from './context';

const NO_LIGATURES = ['no-common-ligatures', 'no-contextual'] as const;

const styles = (t: Theme) =>
  StyleSheet.create({
    wrap: { gap: 10, paddingHorizontal: 18, paddingBottom: 12 },
    offline: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 12, paddingRight: 12, paddingLeft: 14, borderRadius: 16, backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.borderStrong },
    offlineText: { flex: 1, minWidth: 0 },
    offlineTitle: { ...sans(15, 700), color: t.text },
    offlineSub: { ...sans(13), color: t.textMuted },
    retry: { paddingVertical: 8, paddingHorizontal: 12, borderRadius: 10, backgroundColor: t.text },
    retryText: { ...sans(14, 700), color: t.bg },
    card: { gap: 12, paddingVertical: 14, paddingHorizontal: 16, borderRadius: 20, borderWidth: 1 },
    needs: { backgroundImage: `linear-gradient(180deg, ${mix(t.waiting, 13)}, ${mix(t.waiting, 5)})`, borderColor: mix(t.waiting, 32) },
    lost: { backgroundImage: `linear-gradient(180deg, ${mix(t.lost, 14)}, ${mix(t.lost, 4)})`, borderColor: mix(t.lost, 35) },
    eyebrow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
    eyebrowText: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.waiting },
    eyebrowDot: { width: 7, height: 7, borderRadius: 3.5, backgroundColor: t.waiting, boxShadow: [{ offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 4, color: mix(t.waiting, 20) }] },
    name: { ...mono(16, 700, 20), color: t.text },
    text: { ...sans(15, 400, 20), color: t.textMuted, fontVariant: [...NO_LIGATURES] },
    kind: { fontFamily: mono(15).fontFamily },
    actions: { flexDirection: 'row', gap: 8 },
    action: { flex: 1, height: 42, borderRadius: 12, paddingHorizontal: 10, alignItems: 'center', justifyContent: 'center' },
    answer: { backgroundColor: t.waiting },
    answerText: { ...sans(15, 700), color: t.waitingInk },
    soft: { backgroundColor: mix(t.waiting, 12) },
    softText: { ...sans(15, 600), color: t.waiting },
    recoverHead: { flexDirection: 'row', alignItems: 'center', gap: 12 },
    recoverTitle: { ...sans(18, 800, 22), color: t.text },
    recoverSub: { ...sans(14), color: t.textMuted },
    lostList: { gap: 8 },
    lostRow: { flexDirection: 'row', alignItems: 'center', gap: 10, paddingVertical: 10, paddingHorizontal: 12, borderRadius: 12, backgroundColor: mix(t.bg, 55) },
    lostName: { ...mono(14, 700), color: t.text },
    lostKind: mono(12),
    grow: { flex: 1 },
    ago: { ...mono(12), color: t.textFaint },
    danger: { borderRadius: 14, paddingVertical: 10, paddingHorizontal: 16, alignItems: 'center', justifyContent: 'center', backgroundColor: t.lost },
    dangerText: { ...sans(16, 700), color: t.lostInk },
  });

export function Offline({ onRetry }: { onRetry: () => void }) {
  const t = useTheme();
  const s = useStyles(styles);
  return (
    <View style={s.wrap}>
      <View style={s.offline}>
        <Icon name="offline" size={20} color={t.lost} />
        <View style={s.offlineText}>
          <Text style={s.offlineTitle}>Can't reach the computer</Text>
          <Text style={s.offlineSub}>Showing the last state it sent</Text>
        </View>
        <Press style={s.retry} onPress={onRetry}>
          <Text style={s.retryText}>Retry</Text>
        </Press>
      </View>
    </View>
  );
}

export function Lost({ agents, now, onRecover }: { agents: AgentView[]; now: number; onRecover: () => void }) {
  const t = useTheme();
  const s = useStyles(styles);
  useSecond();
  const lost = agents.filter(agent => agent.state === 'lost');
  if (!lost.length) return null;
  return (
    <View style={s.wrap}>
      <Rise style={[s.card, s.lost]}>
        <View style={s.recoverHead}>
          <Tile tone="lostFill" text="↻" big />
          <View style={s.grow}>
            <Text style={s.recoverTitle}>{lost.length === 1 ? '1 agent was interrupted' : `${lost.length} agents were interrupted`}</Text>
            <Text style={s.recoverSub}>The computer stopped them while they ran</Text>
          </View>
        </View>
        <View style={s.lostList}>
          {lost.map(agent => (
            <View key={agent.id} style={s.lostRow}>
              <Text style={s.lostName}>{agent.name}</Text>
              <Text style={[s.lostKind, { color: kindColor(t, agent.kind) }]}>{label(agent.kind)}</Text>
              <View style={s.grow} />
              {agent.stateSince ? <Text style={s.ago}>{`stopped ${model.duration(now - agent.stateSince)} ago`}</Text> : null}
            </View>
          ))}
        </View>
        <Press style={s.danger} onPress={onRecover}>
          <Text style={s.dangerText}>{lost.length === 1 ? 'Recover it where it stopped' : 'Recover all where they stopped'}</Text>
        </Press>
      </Rise>
    </View>
  );
}

type NeedsProps = {
  agents: AgentView[];
  previews: ReadonlyMap<number, Asked>;
  now: number;
};

export function Needs({ agents, previews, now }: NeedsProps) {
  const { workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  useSecond();
  const asking = agents.filter(waiting);
  if (!asking.length) return null;
  return (
    <View style={s.wrap}>
      {asking.map(agent => {
        const preview = previews.get(agent.id);
        const first = preview ? preview.options[0] : null;
        const question = (preview && preview.question) || agent.activity || 'is waiting for your answer';
        return (
          <Rise key={agent.id} style={[s.card, s.needs]}>
            <View style={s.eyebrow}>
              <View style={s.eyebrowDot} />
              <Text style={s.eyebrowText}>{`NEEDS YOU · ${elapsed(agent, now) || ''}`}</Text>
            </View>
            <View>
              <Text style={s.name}>{agent.name}</Text>
              <Text style={s.text}>
                <Text style={[s.kind, { color: kindColor(t, agent.kind) }]}>{`${label(agent.kind)} `}</Text>
                {question}
              </Text>
            </View>
            <View style={s.actions}>
              {first ? (
                <>
                  <Press style={[s.action, s.answer]} onPress={() => workspace.answerFromOverview(agent, first)}>
                    <Text style={s.answerText} numberOfLines={1}>
                      {first.label}
                    </Text>
                  </Press>
                  <Press style={[s.action, s.soft]} onPress={() => workspace.openAgent(agent.id)}>
                    <Text style={s.softText} numberOfLines={1}>
                      Open agent
                    </Text>
                  </Press>
                </>
              ) : (
                <Press style={[s.action, s.answer]} onPress={() => workspace.openAgent(agent.id)}>
                  <Text style={s.answerText} numberOfLines={1}>
                    Open agent
                  </Text>
                </Press>
              )}
            </View>
          </Rise>
        );
      })}
    </View>
  );
}
