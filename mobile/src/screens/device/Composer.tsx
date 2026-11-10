import React, { useImperativeHandle, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { alive, label } from '../../core/agents';
import model from '../../core/model';
import { useStore } from '../../core/store';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans } from '../../theme/type';
import { kindColor } from '../../ui/atoms';
import { Icon } from '../../ui/Icon';
import { useKeyboard } from '../../ui/keyboard';
import { Press } from '../../ui/Press';
import { useDevice } from './context';

export type ComposerHandle = {
  fill(text: string): void;
};

type Props = {
  onWho: () => void;
  onWhere: () => void;
  onWorktree: () => void;
  onBypass: () => void;
};

const INPUT_MAX = 132;

const styles = (t: Theme) =>
  StyleSheet.create({
    dock: { flexShrink: 0, paddingTop: 10, paddingHorizontal: 10 },
    floating: { position: 'absolute', left: 0, right: 0, bottom: 0, paddingTop: 28, paddingHorizontal: 12, backgroundImage: `linear-gradient(180deg, ${mix(t.bg, 0)} 0%, ${mix(t.bg, 92)} 30%, ${t.bg} 60%)` },
    composer: { gap: 10, paddingTop: 12, paddingRight: 12, paddingBottom: 10, paddingLeft: 14, borderRadius: 24, backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.borderStrong, boxShadow: [t.shadow] },
    focused: { borderColor: mix(t.accent, 55, t.borderStrong) },
    inline: { flexDirection: 'row', alignItems: 'flex-end', gap: 8, paddingTop: 6, paddingRight: 6, paddingBottom: 6, paddingLeft: 16, borderRadius: 22, boxShadow: [] },
    input: { ...sans(17, 400, 22), color: t.text, padding: 0, minHeight: 24, maxHeight: INPUT_MAX, textAlignVertical: 'top' },
    inputInline: { ...sans(16, 400, 22), flex: 1, paddingVertical: 7 },
    bar: { flexDirection: 'row', alignItems: 'center', gap: 6, minWidth: 0 },
    picks: { flex: 1, minWidth: 0 },
    picksContent: { alignItems: 'center', gap: 6 },
    pick: { flexDirection: 'row', alignItems: 'center', gap: 5, height: 30, paddingHorizontal: 10, borderRadius: 999, backgroundColor: t.surface },
    pickText: { ...mono(12), color: t.textMuted },
    whoText: mono(12, 700),
    toggle: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 6, height: 30, minWidth: 30, paddingHorizontal: 8, borderRadius: 999, backgroundColor: t.surface },
    toggleText: sans(13, 700),
    worktreeOn: { backgroundColor: mix(t.link, 16) },
    bypassOn: { backgroundColor: mix(t.waiting, 16) },
    send: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, height: 38, minWidth: 38, paddingHorizontal: 10, borderRadius: 999, backgroundColor: t.accent, boxShadow: [{ offsetX: 0, offsetY: 6, blurRadius: 18, color: 'rgba(232, 94, 182, 0.4)' }] },
    sendInline: { height: 36, minWidth: 36 },
    sendLabel: { ...sans(15, 700), color: t.accentInk, paddingLeft: 6 },
  });

export const Composer = React.forwardRef<ComposerHandle, Props>(function Composer({ onWho, onWhere, onWorktree, onBypass }, ref) {
  const { device, link, workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const insets = useSafeAreaInsets();
  const keyboard = useKeyboard();
  const bottom = keyboard ? 0 : insets.bottom;
  const state = useStore(workspace.store);
  const overview = useStore(link.store).overview;
  const [text, setText] = useState('');
  const [focused, setFocused] = useState(false);
  const input = useRef<React.ComponentRef<typeof TextInput>>(null);

  useImperativeHandle(ref, () => ({
    fill(value: string) {
      setText(value);
      input.current?.focus();
    },
  }));

  const inAgent = state.view === 'agent';
  const who = workspace.currentWho();
  const agent = inAgent ? workspace.findAgent(state.agent) : null;
  const open = !inAgent && (focused || text.trim() !== '');
  const placeholder = inAgent ? (agent && alive(agent) ? `Message ${agent.name}…` : 'this agent is not running') : who === 'terminal' ? 'Open a terminal, or type a command…' : `Ask ${label(who)} to…`;

  const submit = () => {
    if (!workspace.submit(text)) return;
    if (!inAgent) input.current?.blur();
    setText('');
  };

  const field = (
    <TextInput
      ref={input}
      value={text}
      onChangeText={setText}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      onSubmitEditing={submit}
      submitBehavior="submit"
      multiline
      placeholder={placeholder}
      placeholderTextColor={t.textFaint}
      selectionColor={t.accent}
      autoCapitalize="none"
      autoCorrect={false}
      spellCheck={false}
      returnKeyType="send"
      accessibilityLabel="message"
      style={[s.input, inAgent && s.inputInline]}
    />
  );
  const send = (
    <Press style={[s.send, inAgent && s.sendInline]} label="send" onPress={submit}>
      {open ? <Text style={s.sendLabel}>{`Launch ${label(who)}`}</Text> : null}
      <Icon name="send" size={18} color={t.accentInk} stroke={2.6} />
    </Press>
  );

  if (inAgent) {
    return (
      <View style={[s.dock, { paddingBottom: bottom + 10 }]}>
        <View style={[s.composer, s.inline, focused && s.focused]}>
          {field}
          {send}
        </View>
      </View>
    );
  }

  const prefs = device.prefs;
  const tint = kindColor(t, who);
  return (
    <View style={[s.dock, s.floating, { paddingBottom: bottom + 12 }]} pointerEvents="box-none">
      <View style={[s.composer, focused && s.focused]}>
        {field}
        <View style={s.bar}>
          {overview ? (
            <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.picks} contentContainerStyle={s.picksContent} keyboardShouldPersistTaps="handled">
              <Press style={[s.pick, { backgroundColor: mix(tint, 14) }]} onPress={onWho}>
                <Text style={[s.whoText, { color: tint }]}>{`${model.kindMark(who)} ${label(who)}`}</Text>
                <Icon name="chevron" size={12} color={tint} stroke={2.4} />
              </Press>
              <Press style={s.pick} onPress={onWhere}>
                <Icon name="folder" size={12} color={t.textMuted} stroke={2.4} />
                <Text style={s.pickText}>{model.shortPath(workspace.currentWhere())}</Text>
              </Press>
              {who !== 'terminal' ? (
                <>
                  <Press style={[s.toggle, prefs.worktree && s.worktreeOn]} label="own worktree" role="switch" selected={prefs.worktree} onPress={onWorktree}>
                    <Icon name="branch" size={14} color={prefs.worktree ? t.link : t.textFaint} />
                    {open ? <Text style={[s.toggleText, { color: prefs.worktree ? t.link : t.textFaint }]}>Worktree</Text> : null}
                  </Press>
                  <Press style={[s.toggle, prefs.bypass && s.bypassOn]} label="bypass permissions" role="switch" selected={prefs.bypass} onPress={onBypass}>
                    <Icon name="bolt" size={14} color={prefs.bypass ? t.waiting : t.textFaint} />
                    {open ? <Text style={[s.toggleText, { color: prefs.bypass ? t.waiting : t.textFaint }]}>Bypass</Text> : null}
                  </Press>
                </>
              ) : null}
            </ScrollView>
          ) : (
            <View style={s.picks} />
          )}
          {send}
        </View>
      </View>
    </View>
  );
});
