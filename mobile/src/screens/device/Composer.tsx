import React, { useCallback, useImperativeHandle, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { alive, label } from '../../core/agents';
import type { Attachment } from '../../core/attach';
import { attachCapable, chipNote, fileLabel, progress, sendLabel, uploading } from '../../core/attach';
import model from '../../core/model';
import { useSelect, useStore } from '../../core/store';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans } from '../../theme/type';
import { kindColor } from '../../ui/atoms';
import { Icon } from '../../ui/Icon';
import { useKeyboard } from '../../ui/keyboard';
import { Press } from '../../ui/Press';
import { ProviderMark } from '../../ui/ProviderMark';
import { useDevice } from './context';

export type ComposerHandle = {
  fill(text: string): void;
};

type Props = {
  onWho: () => void;
  onWhere: () => void;
  onWorktree: () => void;
  onBypass: () => void;
  onAttach: () => void;
};

const INPUT_MAX = 132;
const PICK_ICON = 14;
const NO_FILES: Attachment[] = [];
const CHIP_MAX = 240;
const OCTO_ASK = 'Ask Octo, or tell it what to do…';
const OCTO_BUSY = 'Octo is thinking…';

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
    attachOn: { backgroundColor: mix(t.accent, 14) },
    off: { opacity: 0.4 },
    files: { flexGrow: 0, marginRight: -12, marginLeft: -14 },
    filesContent: { gap: 6, paddingRight: 12, paddingLeft: 14 },
    chip: { flexDirection: 'row', alignItems: 'center', gap: 8, maxWidth: CHIP_MAX, paddingVertical: 6, paddingRight: 4, paddingLeft: 10, borderRadius: 12, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    chipFailed: { borderColor: mix(t.lost, 45), backgroundColor: mix(t.lost, 8, t.surface) },
    chipBody: { flexShrink: 1, minWidth: 0 },
    chipName: { ...mono(12, 700, 16), color: t.text },
    chipNote: { ...mono(11, 400, 14), color: t.textFaint },
    chipProblem: { color: t.lost },
    chipDone: { color: t.done },
    chipBar: { position: 'absolute', left: 0, bottom: 0, height: 2, backgroundColor: t.accent },
    chipRemove: { width: 26, height: 26, borderRadius: 13, alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    send: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, height: 38, minWidth: 38, paddingHorizontal: 10, borderRadius: 999, backgroundColor: t.accent, boxShadow: [{ offsetX: 0, offsetY: 6, blurRadius: 18, color: mix(t.accent, 40) }] },
    sendInline: { height: 36, minWidth: 36 },
    sendLabel: { ...sans(15, 700), color: t.accentInk, paddingLeft: 6 },
  });

const Chip = React.memo(function Chip({ file, onRemove }: { file: Attachment; onRemove: (key: number) => void }) {
  const t = useTheme();
  const s = useStyles(styles);
  const failed = file.phase === 'failed';
  const share = file.phase === 'sending' || file.phase === 'sent' ? progress(file) : 0;
  return (
    <View style={[s.chip, failed && s.chipFailed]} accessibilityLabel={`${file.name}, ${chipNote(file)}`}>
      <View style={s.chipBody}>
        <Text style={s.chipName} numberOfLines={1}>
          {fileLabel(file.name)}
        </Text>
        <Text style={[s.chipNote, failed && s.chipProblem, file.phase === 'done' && s.chipDone]} numberOfLines={failed ? 2 : 1}>
          {chipNote(file)}
        </Text>
      </View>
      <Press style={s.chipRemove} label={`remove ${file.name}`} onPress={() => onRemove(file.key)}>
        <Icon name="close" size={12} color={t.textMuted} stroke={2.4} />
      </Press>
      {share ? <View style={[s.chipBar, { width: `${Math.round(share * 100)}%` }]} /> : null}
    </View>
  );
});

export const Composer = React.forwardRef<ComposerHandle, Props>(function Composer({ onWho, onWhere, onWorktree, onBypass, onAttach }, ref) {
  const { device, link, workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const insets = useSafeAreaInsets();
  const keyboard = useKeyboard();
  const bottom = keyboard ? 0 : insets.bottom;
  const state = useStore(workspace.store);
  const overview = useStore(link.store).overview;
  const files = useStore(workspace.uploads.store);
  const thinking = useSelect(workspace.octo.view, view => Boolean(view && view.thinking));
  const [text, setText] = useState('');
  const [focused, setFocused] = useState(false);
  const input = useRef<React.ComponentRef<typeof TextInput>>(null);

  useImperativeHandle(ref, () => ({
    fill(value: string) {
      setText(value);
      input.current?.focus();
    },
  }));

  const inAgent = state.view !== 'overview';
  const who = workspace.currentWho();
  const agent = state.view === 'agent' ? workspace.findAgent(state.agent) : null;
  const attachable = !inAgent && who !== 'terminal' && attachCapable(link.version);
  const attached = attachable ? files : NO_FILES;
  const busy = uploading(attached);
  const open = !inAgent && (focused || text.trim() !== '' || attached.length > 0);
  const asking = state.view === 'octo' ? (thinking ? OCTO_BUSY : OCTO_ASK) : agent && alive(agent) ? `Message ${agent.name}…` : 'this agent is not running';
  const placeholder = inAgent ? asking : who === 'terminal' ? 'Open a terminal, or type a command…' : `Ask ${label(who)} to…`;

  const sent = () => {
    if (!inAgent) input.current?.blur();
    setText('');
  };
  const submit = () => {
    if (busy) {
      workspace.uploads.cancel();
      return;
    }
    if (attached.length) {
      workspace.launch(text).then(
        launched => {
          if (launched) sent();
        },
        () => {},
      );
      return;
    }
    if (workspace.submit(text)) sent();
  };
  const remove = useCallback((key: number) => workspace.uploads.remove(key), [workspace]);

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
    <Press style={[s.send, inAgent && s.sendInline]} label={busy ? 'cancel sending the files' : 'send'} onPress={submit}>
      {open ? <Text style={s.sendLabel}>{sendLabel(attached) || `Launch ${label(who)}`}</Text> : null}
      <Icon name={busy ? 'close' : 'send'} size={18} color={t.accentInk} stroke={2.6} />
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
    <View style={[s.dock, s.floating, { bottom: keyboard, paddingBottom: bottom + 12 }]} pointerEvents="box-none">
      <View style={[s.composer, focused && s.focused]}>
        {field}
        {attached.length ? (
          <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.files} contentContainerStyle={s.filesContent} keyboardShouldPersistTaps="handled" accessibilityLabel="attached files">
            {attached.map(file => (
              <Chip key={file.key} file={file} onRemove={remove} />
            ))}
          </ScrollView>
        ) : null}
        <View style={s.bar}>
          {overview ? (
            <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.picks} contentContainerStyle={s.picksContent} keyboardShouldPersistTaps="handled">
              <Press style={[s.pick, { backgroundColor: mix(tint, 14) }]} onPress={onWho}>
                <ProviderMark kind={who} size={PICK_ICON} style={[s.whoText, { color: tint }]} />
                <Text style={[s.whoText, { color: tint }]}>{label(who)}</Text>
                <Icon name="chevron" size={12} color={tint} stroke={2.4} />
              </Press>
              <Press style={s.pick} onPress={onWhere}>
                <Icon name="folder" size={12} color={t.textMuted} stroke={2.4} />
                <Text style={s.pickText}>{model.shortPath(workspace.currentWhere())}</Text>
              </Press>
              {attachable ? (
                <Press style={[s.toggle, attached.length > 0 && s.attachOn, busy && s.off]} label={attached.length ? `attach files, ${attached.length} attached` : 'attach files'} disabled={busy} onPress={onAttach}>
                  <Icon name="attach" size={14} color={attached.length ? t.accent : t.textFaint} />
                  {attached.length ? <Text style={[s.toggleText, { color: t.accent }]}>{String(attached.length)}</Text> : null}
                </Press>
              ) : null}
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
