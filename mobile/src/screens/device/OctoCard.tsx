import React from 'react';
import type { StyleProp, TextStyle } from 'react-native';
import { StyleSheet, Text, View } from 'react-native';

import { useSecond } from '../../core/clock';
import { activityNote, answerMessage, decisionMeta, outcomeMark, shortAge, suggestionText } from '../../core/octo';
import type { ActivityEntry, Decision } from '../../core/protocol';
import { openDecision } from '../../sheets/Decision';
import { openPrompt } from '../../sheets/Prompt';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, tracking } from '../../theme/type';
import { Spin, toneColor } from '../../ui/atoms';
import { Press } from '../../ui/Press';
import { useDevice } from './context';

const NO_LIGATURES = ['no-common-ligatures', 'no-contextual'] as const;
const READING = 'Reading the question…';

export const octoStyles = (t: Theme) =>
  StyleSheet.create({
    card: { gap: 12, paddingVertical: 14, paddingHorizontal: 16, borderRadius: 20, borderWidth: 1, backgroundImage: `linear-gradient(180deg, ${mix(t.waiting, 13)}, ${mix(t.waiting, 5)})`, borderColor: mix(t.waiting, 32) },
    eyebrow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
    eyebrowDot: { width: 7, height: 7, borderRadius: 3.5, backgroundColor: t.waiting, boxShadow: [{ offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 4, color: mix(t.waiting, 20) }] },
    eyebrowText: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.waiting, flex: 1, minWidth: 0 },
    name: { ...mono(16, 700, 20), color: t.text },
    meta: { ...mono(12, 400, 16), color: t.textMuted },
    caps: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.textFaint },
    capsAccent: { color: t.accent },
    capsRed: { color: t.lost },
    question: { ...sans(16, 600, 22), color: t.text, fontVariant: [...NO_LIGATURES] },
    would: { gap: 4, paddingLeft: 12, borderLeftWidth: 2, borderColor: t.accent },
    wouldFailed: { borderColor: t.lost },
    wouldText: { ...sans(15, 600, 21), color: t.text },
    reason: { ...sans(14, 400, 19), color: t.textMuted },
    block: { gap: 4 },
    actions: { flexDirection: 'row', gap: 8 },
    action: { flex: 1, height: 42, borderRadius: 12, paddingHorizontal: 10, alignItems: 'center', justifyContent: 'center' },
    answer: { backgroundColor: t.waiting },
    solo: { flex: 0 },
    answerText: { ...sans(15, 700), color: t.waitingInk },
    soft: { backgroundColor: mix(t.waiting, 12) },
    softText: { ...sans(15, 600), color: t.waiting },
    quiet: { alignSelf: 'center', paddingVertical: 4, paddingHorizontal: 12 },
    quietText: { ...sans(14, 600), color: t.textMuted },
    head: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingTop: 6, paddingHorizontal: 4 },
    headCount: { ...mono(11), color: t.textFaint },
    line: { flexDirection: 'row', alignItems: 'flex-start', gap: 10, paddingVertical: 10, paddingHorizontal: 4, borderTopWidth: 1, borderColor: t.border },
    lineMark: { ...mono(13, 700, 18), width: 14, textAlign: 'center' },
    lineBody: { flex: 1, minWidth: 0, gap: 2 },
    lineTop: { flexDirection: 'row', alignItems: 'baseline', gap: 8 },
    lineName: { ...mono(13, 700, 18), color: t.text, flexShrink: 1, minWidth: 0 },
    lineAge: { ...mono(11, 400, 18), color: t.textFaint, marginLeft: 'auto', flexShrink: 0 },
    lineText: { ...sans(14, 400, 19), color: t.textMuted, fontVariant: [...NO_LIGATURES] },
    lineNote: { ...mono(11, 400, 15), color: t.textFaint },
    accent: { color: t.accent },
    waiting: { color: t.waiting },
  });

export function Age({ since, style }: { since: number; style: StyleProp<TextStyle> }) {
  const { link } = useDevice();
  useSecond();
  return <Text style={style}>{since ? shortAge(link.now() - since) : ''}</Text>;
}

export function Head({ text, count }: { text: string; count: number }) {
  const s = useStyles(octoStyles);
  return (
    <View style={s.head}>
      <Text style={s.caps}>{text.toUpperCase()}</Text>
      <Text style={s.headCount}>{String(count)}</Text>
    </View>
  );
}

export function Would({ decision }: { decision: Decision }) {
  const s = useStyles(octoStyles);
  if (decision.suggestion) {
    return (
      <View style={s.would}>
        <Text style={[s.caps, s.capsAccent]}>OCTO WOULD ANSWER</Text>
        <Text style={s.wouldText}>{suggestionText(decision.suggestion)}</Text>
        {decision.suggestion.reason ? <Text style={s.reason}>{decision.suggestion.reason}</Text> : null}
      </View>
    );
  }
  if (!decision.problem) return null;
  return (
    <View style={[s.would, s.wouldFailed]}>
      <Text style={[s.caps, s.capsRed]}>OCTO COULD NOT JUDGE THIS</Text>
      <Text style={s.reason}>{decision.problem}</Text>
    </View>
  );
}

export const DecisionCard = React.memo(function DecisionCard({ decision }: { decision: Decision }) {
  const scope = useDevice();
  const { workspace } = scope;
  const s = useStyles(octoStyles);
  const answer = workspace.octo;
  const myself = () => openPrompt(`Answer ${decision.name}`, 'your answer', '', text => answer.answer(answerMessage(decision.id, 'text', { text })));
  return (
    <Press style={s.card} label={`${decision.name} asks: ${decision.question}`} onPress={() => openDecision(scope, decision.id)}>
      <View style={s.eyebrow}>
        <View style={s.eyebrowDot} />
        <Text style={s.eyebrowText} numberOfLines={1}>
          {`NEEDS YOU${decision.tag ? ` · ${decision.tag.toUpperCase()}` : ''}`}
        </Text>
        <Age since={decision.since} style={[s.lineAge, s.waiting]} />
      </View>
      <View>
        <Text style={s.name} numberOfLines={1}>
          {decision.name}
        </Text>
        <Text style={s.meta} numberOfLines={1}>
          {decisionMeta(decision)}
        </Text>
      </View>
      <View style={s.block}>
        <Text style={s.caps}>IT ASKS</Text>
        <Text style={s.question}>{decision.question || READING}</Text>
      </View>
      <Would decision={decision} />
      {decision.suggestion ? (
        <Press style={[s.action, s.answer, s.solo]} onPress={() => answer.answer(answerMessage(decision.id, 'suggestion'))}>
          <Text style={s.answerText} numberOfLines={1}>
            Send Octo's answer
          </Text>
        </Press>
      ) : null}
      <View style={s.actions}>
        <Press style={[s.action, s.soft]} onPress={myself}>
          <Text style={s.softText} numberOfLines={1}>
            Answer myself
          </Text>
        </Press>
        <Press style={[s.action, s.soft]} onPress={() => answer.answer(answerMessage(decision.id, 'allow'))}>
          <Text style={s.softText} numberOfLines={1}>
            Allow it
          </Text>
        </Press>
      </View>
      <Press style={s.quiet} onPress={() => workspace.openAgent(decision.agent)}>
        <Text style={s.quietText}>Open agent</Text>
      </Press>
    </Press>
  );
});

export const DecidingRow = React.memo(function DecidingRow({ decision }: { decision: Decision }) {
  const scope = useDevice();
  const s = useStyles(octoStyles);
  return (
    <Press style={s.line} onPress={() => openDecision(scope, decision.id)}>
      <Spin style={[s.lineMark, s.accent]} />
      <View style={s.lineBody}>
        <View style={s.lineTop}>
          <Text style={s.lineName} numberOfLines={1}>
            {decision.name}
          </Text>
          <Age since={decision.since} style={s.lineAge} />
        </View>
        <Text style={s.lineText} numberOfLines={2}>
          {decision.question || READING}
        </Text>
      </View>
    </Press>
  );
});

export const HandledRow = React.memo(function HandledRow({ entry }: { entry: ActivityEntry }) {
  const t = useTheme();
  const s = useStyles(octoStyles);
  const mark = outcomeMark(entry);
  return (
    <View style={s.line} accessibilityLabel={`${entry.name}: ${entry.summary}`}>
      <Text style={[s.lineMark, { color: toneColor(t, mark.tone) }]}>{mark.text}</Text>
      <View style={s.lineBody}>
        <View style={s.lineTop}>
          <Text style={s.lineName} numberOfLines={1}>
            {entry.name}
          </Text>
          <Age since={entry.at} style={s.lineAge} />
        </View>
        <Text style={s.lineText} numberOfLines={2}>
          {entry.summary}
        </Text>
        <Text style={s.lineNote} numberOfLines={1}>
          {activityNote(entry)}
        </Text>
      </View>
    </View>
  );
});
