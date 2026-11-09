import { StyleSheet, Text, View } from 'react-native';

import { title } from '../core/agents';
import { useSecond } from '../core/clock';
import type { DeviceLink } from '../core/link';
import model from '../core/model';
import type { UsageEntry } from '../core/protocol';
import { useStore } from '../core/store';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono, sans } from '../theme/type';
import { kindColor } from '../ui/atoms';
import { Note } from '../ui/Option';
import { RingArt, usageColor } from '../ui/RingArt';
import { openSheet, SheetScroll } from '../ui/Sheet';

const RING = 112;
const RING_STROKE = 4.5;
const RING_MARK = 18;

const styles = (t: Theme) =>
  StyleSheet.create({
    card: { flexDirection: 'row', alignItems: 'center', gap: 18, padding: 18, borderRadius: 22, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    alert: { backgroundImage: `linear-gradient(180deg, ${mix(t.lost, 10, t.surface)}, ${t.surface})`, borderColor: mix(t.lost, 35) },
    detail: { flex: 1, minWidth: 0, gap: 12 },
    name: sans(19, 800),
    window: { gap: 1 },
    top: { flexDirection: 'row', alignItems: 'center', gap: 6 },
    dot: { width: 8, height: 8, borderRadius: 4 },
    windowLabel: { ...sans(14), flex: 1, color: t.textMuted },
    percent: { ...mono(14, 700), color: t.text },
    resets: { ...mono(11), color: t.textFaint, paddingLeft: 14 },
    bigPercent: { ...sans(30, 800, 30), color: t.text },
    bigMark: mono(18, 700),
    pair: { flexDirection: 'row', gap: 10, flexWrap: 'wrap' },
    mini: { flexGrow: 1, flexShrink: 1, flexBasis: '40%', gap: 4, padding: 14, borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    miniName: sans(15, 700),
    miniValue: { ...sans(26, 800, 30), color: t.text },
    miniNote: { ...mono(11), color: t.textFaint },
  });

function Card({ entry, now }: { entry: UsageEntry; now: number }) {
  const t = useTheme();
  const s = useStyles(styles);
  const tint = kindColor(t, entry.kind);
  const alert = entry.windows.some(window => window.used >= model.USAGE_ALERT);
  const only = entry.windows.length === 1 ? entry.windows[0] : null;
  return (
    <View style={[s.card, alert && s.alert]}>
      <RingArt entry={entry} size={RING} stroke={RING_STROKE} markSize={RING_MARK}>
        {only ? (
          <>
            <Text style={s.bigPercent}>{`${Math.round(only.used)}%`}</Text>
            <Text style={[s.bigMark, { color: tint }]}>{`${model.kindMark(entry.kind)} ${only.label}`}</Text>
          </>
        ) : null}
      </RingArt>
      <View style={s.detail}>
        <Text style={[s.name, { color: tint }]}>{title(entry.kind)}</Text>
        {entry.windows.map(window => (
          <View key={window.label} style={s.window}>
            <View style={s.top}>
              <View style={[s.dot, { backgroundColor: usageColor(t, window.used) }]} />
              <Text style={s.windowLabel}>{window.label}</Text>
              <Text style={s.percent}>{`${Math.round(window.used)}%`}</Text>
            </View>
            {window.resetsAt ? <Text style={s.resets}>{window.resetsAt > now ? `resets in ${model.duration(window.resetsAt - now)}` : 'resets now'}</Text> : null}
          </View>
        ))}
      </View>
    </View>
  );
}

function UsageBody({ link }: { link: DeviceLink }) {
  const t = useTheme();
  const s = useStyles(styles);
  const entries = useStore(link.store).usage || [];
  useSecond();
  const now = link.now();
  const metered = entries.filter(entry => entry.windows.length);
  const plain = entries.filter(entry => !entry.windows.length);
  return (
    <SheetScroll>
      {metered.map(entry => (
        <Card key={entry.kind} entry={entry} now={now} />
      ))}
      {plain.length ? (
        <View style={s.pair}>
          {plain.map(entry => (
            <View key={entry.kind} style={s.mini}>
              <Text style={[s.miniName, { color: kindColor(t, entry.kind) }]}>{title(entry.kind)}</Text>
              <Text style={s.miniValue}>{entry.credits !== null ? `$${entry.credits.toFixed(2)}` : '∞'}</Text>
              <Text style={s.miniNote}>{entry.credits !== null ? 'funds left' : entry.problem || 'no plan limits'}</Text>
            </View>
          ))}
        </View>
      ) : null}
      {entries.length ? null : <Note text="No plan usage yet" />}
    </SheetScroll>
  );
}

export function openUsage(link: DeviceLink): void {
  openSheet({ title: 'Plan usage', content: <UsageBody link={link} /> });
}
