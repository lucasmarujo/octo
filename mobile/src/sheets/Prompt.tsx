import React, { useEffect, useRef, useState } from 'react';
import { StyleSheet, TextInput, View } from 'react-native';

import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { sans } from '../theme/type';
import { Button } from '../ui/Button';
import { closeSheet, openSheet } from '../ui/Sheet';

type Props = {
  placeholder: string;
  value: string;
  done: (text: string) => void;
};

const FOCUS_AFTER_MS = 80;

export const fieldStyles = (t: Theme) =>
  StyleSheet.create({
    field: { flexDirection: 'row', gap: 8 },
    input: { ...sans(16), flex: 1, minWidth: 0, height: 48, borderRadius: 14, borderWidth: 1, borderColor: t.border, backgroundColor: t.surface, paddingHorizontal: 14, paddingVertical: 0, color: t.text },
    focused: { borderColor: t.accent },
    ok: { height: 48 },
  });

function PromptBody({ placeholder, value, done }: Props) {
  const t = useTheme();
  const s = useStyles(fieldStyles);
  const [text, setText] = useState(value);
  const [focused, setFocused] = useState(false);
  const input = useRef<React.ComponentRef<typeof TextInput>>(null);

  useEffect(() => {
    const timer = setTimeout(() => input.current?.focus(), FOCUS_AFTER_MS);
    return () => clearTimeout(timer);
  }, []);

  const go = () => {
    const clean = text.trim();
    if (!clean) return;
    done(clean);
    closeSheet();
  };

  return (
    <View style={s.field}>
      <TextInput
        ref={input}
        value={text}
        onChangeText={setText}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onSubmitEditing={go}
        placeholder={placeholder}
        placeholderTextColor={t.textFaint}
        selectionColor={t.accent}
        autoCapitalize="none"
        autoCorrect={false}
        spellCheck={false}
        returnKeyType="done"
        style={[s.input, focused && s.focused]}
      />
      <Button kind="primary" text="OK" flat style={s.ok} onPress={go} />
    </View>
  );
}

export function openPrompt(heading: string, placeholder: string, value: string, done: (text: string) => void): void {
  openSheet({ title: heading, content: <PromptBody placeholder={placeholder} value={value} done={done} /> });
}
