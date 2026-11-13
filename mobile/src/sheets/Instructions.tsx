import { useEffect, useState } from 'react';
import { StyleSheet, Text, TextInput, View } from 'react-native';

import { canSaveInstructions, GLOBAL_FILE, instructionsTooLong } from '../core/octo';
import { useStore } from '../core/store';
import type { DeviceScope } from '../screens/device/context';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono } from '../theme/type';
import { Spin } from '../ui/atoms';
import { Button } from '../ui/Button';
import { openSheet, SheetFoot } from '../ui/Sheet';
import { toast } from '../ui/Toast';

const TOO_LONG = 'Over 64 KB. Shorten it to save.';
const NOT_SENT = "Can't reach the computer. Nothing was saved yet.";

const styles = (t: Theme) =>
  StyleSheet.create({
    body: { flex: 1, minHeight: 0, gap: 10 },
    input: { ...mono(13, 400, 19), flex: 1, minHeight: 0, paddingVertical: 12, paddingHorizontal: 14, borderRadius: 14, borderWidth: 1, borderColor: t.border, backgroundColor: t.termBg, color: t.termFg, textAlignVertical: 'top' },
    focused: { borderColor: t.accent },
    reading: { flex: 1, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8 },
    mark: { ...mono(12, 700), color: t.accent },
    note: { ...mono(11, 400, 15), color: t.textFaint, paddingHorizontal: 4 },
    warn: { color: t.lost },
    save: { height: 50 },
    off: { opacity: 0.4 },
  });

function InstructionsBody({ scope, project }: { scope: DeviceScope; project: string | null }) {
  const { octo } = scope.workspace;
  const t = useTheme();
  const s = useStyles(styles);
  const state = useStore(octo.file);
  const file = state && state.project === project ? state.file : null;
  const saved = file ? file.text : null;
  const saving = Boolean(state && state.saving);
  const [text, setText] = useState('');
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    octo.readInstructions(project);
    return () => octo.closeInstructions();
  }, [octo, project]);
  useEffect(() => {
    if (saved !== null) setText(saved);
  }, [saved]);

  if (!file) {
    return (
      <View style={s.reading}>
        <Spin style={s.mark} />
        <Text style={s.note}>Reading the file…</Text>
      </View>
    );
  }
  const ready = canSaveInstructions(saved, text);
  const long = instructionsTooLong(text);
  return (
    <View style={s.body}>
      <TextInput
        value={text}
        onChangeText={setText}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        multiline
        placeholder="- A rule per line, in plain words"
        placeholderTextColor={t.textFaint}
        selectionColor={t.accent}
        autoCapitalize="none"
        autoCorrect={false}
        spellCheck={false}
        accessibilityLabel="instructions"
        style={[s.input, focused && s.focused]}
      />
      <Text style={[s.note, long && s.warn]} numberOfLines={1}>
        {long ? TOO_LONG : file.path}
      </Text>
      <SheetFoot>
        <Button kind="primary" text={saving ? 'Saving…' : text === saved ? 'Saved' : 'Save'} flat style={[s.save, (!ready || saving) && s.off]} onPress={() => (ready && !saving && !octo.saveInstructions(project, text) ? toast(NOT_SENT, 'warn') : undefined)} />
      </SheetFoot>
    </View>
  );
}

export function openInstructions(scope: DeviceScope, project: string | null): void {
  openSheet({ title: project || GLOBAL_FILE, sub: project ? 'Octo reads this before every decision in this project' : 'Octo reads this before every decision', agentHead: true, tall: true, content: <InstructionsBody scope={scope} project={project} /> });
}
