import { StyleSheet, Text, View } from 'react-native';

import { title } from '../core/agents';
import { useSecond } from '../core/clock';
import { changeDevice, findDevice } from '../core/devices';
import model from '../core/model';
import type { AgentKind, UsageEntry } from '../core/protocol';
import { useStore } from '../core/store';
import type { DeviceScope } from '../screens/device/context';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono, sans } from '../theme/type';
import { kindColor } from '../ui/atoms';
import { Icon } from '../ui/Icon';
import { Group, Option } from '../ui/Option';
import { Press } from '../ui/Press';
import { ProviderMark } from '../ui/ProviderMark';
import { RingArt } from '../ui/RingArt';
import { closeSheet, openSheet, SheetScroll } from '../ui/Sheet';

type Line = {
  text: string;
  tone: 'muted' | 'red' | 'faint';
};

const RING = 36;
const RING_STROKE = 5;
const RING_MARK = 12;
const OPTION_ICON = 18;

export function usageLine(entry: UsageEntry, now: number): Line {
  if (entry.windows.length) {
    const top = entry.windows[0];
    if (top.used >= model.USAGE_ALERT && top.resetsAt) return { text: `${Math.round(top.used)}% · resets ${model.duration(top.resetsAt - now)}`, tone: 'red' };
    return { text: `${Math.round(top.used)}% of ${top.label}`, tone: 'muted' };
  }
  if (entry.credits !== null) return { text: `$${entry.credits.toFixed(2)} in funds`, tone: 'muted' };
  return { text: entry.problem || 'no plan limits', tone: 'faint' };
}

const styles = (t: Theme) =>
  StyleSheet.create({
    tiles: { flexDirection: 'row', flexWrap: 'wrap', gap: 10 },
    tile: { flexGrow: 1, flexShrink: 1, flexBasis: '45%', minWidth: 0, gap: 10, padding: 14, borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    tileOn: { backgroundColor: mix(t.accent, 8, t.surface), borderWidth: 1.5, borderColor: t.accent, padding: 13.5 },
    top: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
    check: { width: 22, height: 22, borderRadius: 11, backgroundColor: t.accent, alignItems: 'center', justifyContent: 'center' },
    name: { ...sans(17, 700), color: t.text },
    line: { ...mono(11), color: t.textMuted },
    red: { color: t.lost },
    faint: { color: t.textFaint },
    mark: mono(15),
  });

function WhoBody({ scope }: { scope: DeviceScope }) {
  const { link, workspace } = scope;
  const t = useTheme();
  const s = useStyles(styles);
  const { overview, usage } = useStore(link.store);
  useSecond();
  const kinds = overview ? overview.kinds : [];
  const byKind = new Map((usage || []).map(entry => [entry.kind, entry]));
  const who = workspace.currentWho();
  const now = link.now();
  const choose = (kind: AgentKind) => {
    const device = findDevice(scope.device.id);
    if (device) changeDevice(device.id, { prefs: { ...device.prefs, who: kind } });
    closeSheet();
  };
  const metered = kinds.filter(kind => byKind.has(kind));
  const plain = kinds.filter(kind => !byKind.has(kind));
  return (
    <SheetScroll>
      {metered.length ? (
        <View style={s.tiles}>
          {metered.map(kind => {
            const entry = byKind.get(kind)!;
            const line = usageLine(entry, now);
            const on = kind === who;
            return (
              <Press key={kind} style={[s.tile, on && s.tileOn]} selected={on} onPress={() => choose(kind)}>
                <View style={s.top}>
                  <RingArt entry={entry} size={RING} stroke={RING_STROKE} markSize={RING_MARK} />
                  {on ? (
                    <View style={s.check}>
                      <Icon name="check" size={12} color={t.accentInk} stroke={3.2} />
                    </View>
                  ) : null}
                </View>
                <View>
                  <Text style={s.name}>{title(kind)}</Text>
                  <Text style={[s.line, line.tone === 'red' && s.red, line.tone === 'faint' && s.faint]} numberOfLines={1}>
                    {line.text}
                  </Text>
                </View>
              </Press>
            );
          })}
        </View>
      ) : null}
      <Group>
        {plain.map(kind => (
          <Option
            key={kind}
            icon={() => <ProviderMark kind={kind} size={OPTION_ICON} style={[s.mark, { color: kindColor(t, kind) }]} />}
            iconFill={mix(kindColor(t, kind), 14)}
            text={title(kind)}
            detail={kind === 'terminal' ? 'a shell in the folder' : null}
            tone={kind === who ? 'current' : undefined}
            trail={kind === who ? <Icon name="check" size={16} color={t.link} /> : undefined}
            onPress={() => choose(kind)}
          />
        ))}
      </Group>
    </SheetScroll>
  );
}

export function openWho(scope: DeviceScope): void {
  const usage = scope.link.store.get().usage;
  openSheet({ title: 'Who runs it', sub: usage && usage.length ? 'Rings show how much of each plan is used' : '', content: <WhoBody scope={scope} /> });
}
