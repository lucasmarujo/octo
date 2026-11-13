import { useEffect, useState } from 'react';
import { StyleSheet, Text, TextInput, View } from 'react-native';

import { answerMessage, decisionMeta, suggestionAnswer, suggestionText } from '../core/octo';
import type { Decision } from '../core/protocol';
import { useSelect } from '../core/store';
import type { DeviceScope } from '../screens/device/context';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { CAPS, mono, sans, tracking } from '../theme/type';
import { Button } from '../ui/Button';
import { Press } from '../ui/Press';
import { closeSheet, openSheet, SheetFoot, SheetScroll } from '../ui/Sheet';

const NO_LIGATURES = ['no-common-ligatures', 'no-contextual'] as const;
const INPUT_MAX = 140;

const styles = (t: Theme) =>
  StyleSheet.create({
    block: { gap: 6, paddingHorizontal: 4 },
    caps: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.textFaint },
    capsAccent: { color: t.accent },
    capsRed: { color: t.lost },
    context: { paddingVertical: 10, paddingHorizontal: 12, borderRadius: 12, backgroundColor: t.termBg, borderWidth: 1, borderColor: t.border },
    row: { ...mono(11, 400, 16), color: t.textMuted },
    tool: { ...mono(12, 700, 16), color: t.text },
    question: { ...sans(18, 700, 24), color: t.text, fontVariant: [...NO_LIGATURES] },
    input: { ...sans(15, 400, 21), minHeight: 48, maxHeight: INPUT_MAX, paddingVertical: 12, paddingHorizontal: 14, borderRadius: 14, borderWidth: 1, borderColor: t.border, backgroundColor: t.surface, color: t.text, textAlignVertical: 'top' },
    focused: { borderColor: t.accent },
    reason: { ...sans(14, 400, 19), color: t.textMuted },
    gone: { ...sans(15, 400, 21), color: t.textMuted, paddingHorizontal: 4, paddingBottom: 8 },
    foot: { gap: 8, paddingTop: 10 },
    actions: { flexDirection: 'row', gap: 8 },
    action: { flex: 1, height: 46 },
    single: { height: 46 },
    answer: { height: 50, borderRadius: 14, alignItems: 'center', justifyContent: 'center', backgroundColor: t.waiting },
    answerText: { ...sans(16, 700), color: t.waitingInk },
    off: { backgroundColor: mix(t.waiting, 30) },
    quiet: { alignSelf: 'center', paddingVertical: 6, paddingHorizontal: 12 },
    quietText: { ...sans(14, 600), color: t.textMuted },
  });

function Detail({ scope, decision }: { scope: DeviceScope; decision: Decision }) {
  const { workspace } = scope;
  const t = useTheme();
  const s = useStyles(styles);
  const offered = suggestionText(decision.suggestion);
  const [text, setText] = useState(offered);
  const [rule, setRule] = useState('');
  const [focused, setFocused] = useState<'answer' | 'rule' | null>(null);

  useEffect(() => setText(offered), [offered]);

  const message = suggestionAnswer(decision, text, rule);
  const done = (run: () => void) => () => {
    run();
    closeSheet();
  };
  const send = done(() => {
    if (message) workspace.octo.answer(message);
  });
  return (
    <>
      <SheetScroll>
        {decision.context.length ? (
          <View style={s.block}>
            <Text style={s.caps}>WHAT LED HERE</Text>
            <View style={s.context}>
              {decision.tool ? <Text style={s.tool} numberOfLines={3}>{`${decision.tool.name} ${decision.tool.detail}`}</Text> : null}
              {decision.context.map((row, index) => (
                <Text key={index} style={s.row} numberOfLines={1}>
                  {row || ' '}
                </Text>
              ))}
            </View>
          </View>
        ) : null}
        <View style={s.block}>
          <Text style={s.caps}>IT ASKS</Text>
          <Text style={s.question} selectable>
            {decision.question}
          </Text>
        </View>
        {decision.status === 'needsYou' ? (
          <>
            <View style={s.block}>
              <Text style={[s.caps, decision.suggestion ? s.capsAccent : decision.problem ? s.capsRed : null]}>{decision.suggestion ? 'OCTO WOULD ANSWER' : decision.problem ? 'OCTO COULD NOT JUDGE THIS' : 'YOUR ANSWER'}</Text>
              <TextInput
                value={text}
                onChangeText={setText}
                onFocus={() => setFocused('answer')}
                onBlur={() => setFocused(null)}
                multiline
                placeholder="Write the answer to send"
                placeholderTextColor={t.textFaint}
                selectionColor={t.accent}
                autoCapitalize="none"
                autoCorrect={false}
                accessibilityLabel="answer"
                style={[s.input, focused === 'answer' && s.focused]}
              />
              {decision.suggestion && decision.suggestion.reason ? <Text style={s.reason}>{decision.suggestion.reason}</Text> : null}
              {decision.problem ? <Text style={s.reason}>{decision.problem}</Text> : null}
            </View>
            <View style={s.block}>
              <Text style={s.caps}>{`REMEMBER THIS FOR ${decision.project.toUpperCase()}`}</Text>
              <TextInput
                value={rule}
                onChangeText={setRule}
                onFocus={() => setFocused('rule')}
                onBlur={() => setFocused(null)}
                placeholder="A rule Octo follows from now on (optional)"
                placeholderTextColor={t.textFaint}
                selectionColor={t.accent}
                autoCapitalize="none"
                autoCorrect={false}
                accessibilityLabel="rule to remember"
                style={[s.input, focused === 'rule' && s.focused]}
              />
            </View>
          </>
        ) : null}
      </SheetScroll>
      <SheetFoot>
        {decision.status === 'needsYou' ? (
          <View style={s.foot}>
            <Press style={[s.answer, !message && s.off]} disabled={!message} onPress={send}>
              <Text style={s.answerText}>{message && message.how === 'text' ? 'Send my answer' : "Send Octo's answer"}</Text>
            </Press>
            <View style={s.actions}>
              <Button kind="secondary" text="Allow it" style={s.action} onPress={done(() => workspace.octo.answer(answerMessage(decision.id, 'allow', { remember: rule })))} />
              <Button kind="secondary" text="Dismiss" style={s.action} onPress={done(() => workspace.octo.answer(answerMessage(decision.id, 'dismiss')))} />
            </View>
            <Press style={s.quiet} onPress={done(() => workspace.openAgent(decision.agent))}>
              <Text style={s.quietText}>Open agent</Text>
            </Press>
          </View>
        ) : (
          <View style={s.foot}>
            <Button kind="secondary" text="Open agent" style={s.single} onPress={done(() => workspace.openAgent(decision.agent))} />
          </View>
        )}
      </SheetFoot>
    </>
  );
}

function DecisionBody({ scope, id }: { scope: DeviceScope; id: number }) {
  const s = useStyles(styles);
  const decision = useSelect(scope.workspace.octo.view, view => (view && view.decisions.find(entry => entry.id === id)) || null);
  if (!decision) return <Text style={s.gone}>This question was answered. Nothing waits here anymore.</Text>;
  return <Detail scope={scope} decision={decision} />;
}

export function openDecision(scope: DeviceScope, id: number): void {
  const view = scope.workspace.octo.view.get();
  const decision = view ? view.decisions.find(entry => entry.id === id) : null;
  if (!decision) return;
  openSheet({ title: decision.name, sub: [decisionMeta(decision), decision.tag].filter(Boolean).join(' · '), agentHead: true, tall: decision.context.length > 0, content: <DecisionBody scope={scope} id={id} /> });
}
