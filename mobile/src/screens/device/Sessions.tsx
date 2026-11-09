import { ScrollView, StyleSheet, Text, View } from 'react-native';

import { waiting } from '../../core/agents';
import type { Overview } from '../../core/protocol';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans } from '../../theme/type';
import { Icon } from '../../ui/Icon';
import { Press } from '../../ui/Press';

type Props = {
  overview: Overview;
  filter: string | null;
  onPick: (name: string) => void;
  onNew: () => void;
};

const styles = (t: Theme) =>
  StyleSheet.create({
    sessions: { flexGrow: 0, flexShrink: 0 },
    content: { gap: 8, paddingHorizontal: 18, paddingBottom: 14 },
    session: { flexDirection: 'row', alignItems: 'center', gap: 8, height: 36, paddingHorizontal: 14, borderRadius: 999, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    on: { backgroundColor: t.text, borderColor: t.text },
    name: { ...sans(15, 600), color: t.textMuted },
    nameOn: { ...sans(15, 700), color: t.bg },
    count: { ...mono(12), color: t.textFaint },
    countOn: { color: t.bg, opacity: 0.55 },
    flag: { width: 6, height: 6, borderRadius: 3, backgroundColor: t.lost },
    asking: { backgroundColor: t.waiting },
    add: { width: 36, height: 36, borderRadius: 999, alignItems: 'center', justifyContent: 'center', borderWidth: 1, borderStyle: 'dashed', borderColor: t.borderStrong },
  });

export function Sessions({ overview, filter, onPick, onNew }: Props) {
  const t = useTheme();
  const s = useStyles(styles);
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.sessions} contentContainerStyle={s.content} accessibilityLabel="sessions">
      {overview.sessions.map(session => {
        const members = overview.agents.filter(agent => agent.session === session.name);
        const lost = members.some(agent => agent.state === 'lost');
        const asking = members.some(waiting);
        const on = session.name === filter;
        return (
          <Press key={session.name} style={[s.session, on && s.on]} role="tab" selected={on} onPress={() => onPick(session.name)}>
            {!on && (lost || asking) ? <View style={[s.flag, !lost && s.asking]} /> : null}
            <Text style={on ? s.nameOn : s.name}>{session.name}</Text>
            <Text style={[s.count, on && s.countOn]}>{String(session.agents)}</Text>
          </Press>
        );
      })}
      <Press style={s.add} label="new session" onPress={onNew}>
        <Icon name="plus" size={16} color={t.textMuted} />
      </Press>
    </ScrollView>
  );
}
