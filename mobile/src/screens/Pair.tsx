import React, { useEffect, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { Device } from '../core/devices';
import { pairDevice, repairDevice } from '../core/devices';
import type { LinkPhase } from '../core/link';
import type { Mood } from '../core/model';
import { parsePairingLink } from '../core/pairing';
import Scanner from '../../modules/octo-scanner';
import { fieldStyles } from '../sheets/Prompt';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono, sans, TIGHT, tracking } from '../theme/type';
import { Dot } from '../ui/atoms';
import { Button } from '../ui/Button';
import { Kai } from '../ui/Kai';
import { useKeyboard } from '../ui/keyboard';
import { Rise } from '../ui/Rise';
import { Scan } from './Scan';
import { toast } from '../ui/Toast';
import { subStyles, Top } from '../ui/Top';

export type PairKind = 'new' | 'unpaired' | 'replaced' | 'outdated';

type Preset = {
  badge?: string;
  title: string;
  text: string;
  steps: boolean;
  form: boolean;
  again?: string;
  retry: boolean;
  mood: Mood;
  phase: LinkPhase;
  word: string;
};

type Props = {
  kind: PairKind;
  replace?: string;
  onBack?: () => void;
  onPaired: (device: Device) => void;
  onRetry?: () => void;
};

const PRESETS: Record<PairKind, Preset> = {
  new: { title: 'Pair this phone', text: 'Watch and steer your agents from anywhere. Encrypted end to end.', steps: true, form: true, retry: false, mood: 'idle', phase: 'unpaired', word: 'not paired' },
  unpaired: {
    badge: 'unpaired',
    title: 'This Octo forgot this phone',
    text: 'This phone is not paired with this Octo anymore. Scan the new code or paste its link to pair it again.',
    steps: true,
    form: true,
    retry: true,
    mood: 'lost',
    phase: 'unpaired',
    word: 'not paired',
  },
  replaced: {
    badge: 'another phone',
    title: 'Octo is open on another phone',
    text: 'This pairing is in use on another phone. Tap use it here to move it to this one.',
    steps: false,
    form: false,
    retry: true,
    again: 'Use it here',
    mood: 'idle',
    phase: 'replaced',
    word: 'on another phone',
  },
  outdated: {
    badge: 'update needed',
    title: 'Octo speaks another version',
    text: 'This Octo speaks another version of the app. Update Octo on the computer and this app.',
    steps: true,
    form: true,
    retry: true,
    mood: 'lost',
    phase: 'outdated',
    word: 'update needed',
  },
};
const MASCOT = 128;
const LOCAL_PAIRING = __DEV__ || process.env.EXPO_PUBLIC_LOCAL_PAIRING === '1';
const GLOW = 320;

const styles = (t: Theme) =>
  StyleSheet.create({
    view: { flex: 1 },
    pair: { flexGrow: 1, paddingTop: 8, paddingHorizontal: 20, gap: 22 },
    glow: { position: 'absolute', top: -20, alignSelf: 'center', width: GLOW, height: GLOW, borderRadius: GLOW / 2 },
    glowIdle: { backgroundImage: `radial-gradient(circle, ${mix(t.accent, 26)} 0%, ${mix(t.accent, 8)} 45%, ${mix(t.accent, 0)} 70%)` },
    glowError: { backgroundImage: `radial-gradient(circle, ${mix(t.lost, 22)} 0%, ${mix(t.lost, 6)} 45%, ${mix(t.lost, 0)} 70%)` },
    hero: { alignItems: 'center', gap: 12, paddingTop: 20 },
    badge: { paddingVertical: 5, paddingHorizontal: 12, borderRadius: 999, backgroundColor: mix(t.lost, 12) },
    badgeText: { ...mono(12), color: t.lost },
    title: { ...sans(32, 800, 36), ...tracking(32, TIGHT), color: t.text, textAlign: 'center' },
    text: { ...sans(17, 400, 24), color: t.textMuted, textAlign: 'center', maxWidth: 320 },
    steps: { borderRadius: 20, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    step: { flexDirection: 'row', alignItems: 'center', gap: 14, paddingVertical: 14, paddingHorizontal: 16 },
    divided: { borderTopWidth: 1, borderColor: t.border },
    number: { width: 34, height: 34, borderRadius: 11, backgroundColor: mix(t.accent, 14), alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    numberText: { ...mono(14, 700), color: t.accent },
    stepText: { ...sans(16, 400, 21), flex: 1, color: t.text },
    strong: sans(16, 700, 21),
    kbd: { paddingHorizontal: 7, paddingTop: 1, paddingBottom: 0, borderRadius: 6, borderWidth: 1, borderBottomWidth: 2, borderColor: t.borderStrong, transform: [{ translateY: 3 }] },
    kbdText: { ...mono(12, 400, 16), color: t.textMuted },
    actions: { gap: 10, marginTop: 'auto' },
    pairButton: { height: 48 },
    note: { ...mono(12), color: t.textFaint, textAlign: 'center' },
  });

export function Pair({ kind, replace, onBack, onPaired, onRetry }: Props) {
  const t = useTheme();
  const s = useStyles(styles);
  const field = useStyles(fieldStyles);
  const sub = useStyles(subStyles);
  const insets = useSafeAreaInsets();
  const keyboard = useKeyboard();
  const [link, setLink] = useState('');
  const [focused, setFocused] = useState(false);
  const [scanning, setScanning] = useState(false);
  const scroll = useRef<React.ComponentRef<typeof ScrollView>>(null);
  const preset = PRESETS[kind];

  useEffect(() => {
    if (keyboard) scroll.current?.scrollToEnd({ animated: false });
  }, [keyboard]);

  const pairWith = (text: string): boolean => {
    const pairing = parsePairingLink(text, LOCAL_PAIRING);
    if (!pairing) return false;
    setLink('');
    setScanning(false);
    onPaired((replace ? repairDevice(replace, pairing) : pairDevice(pairing)).device);
    return true;
  };
  const pairTyped = () => {
    if (!pairWith(link)) toast('Copy it again from Octo, with the part after #k=', 'error', { title: 'This is not the whole pairing link' });
  };
  const pairScanned = (text: string) => {
    if (!pairWith(text)) toast('Scan the code Octo shows when you click Pair phone', 'error', { title: 'This is not an Octo pairing code' });
  };
  const cameraOff = () => {
    setScanning(false);
    toast('Allow the camera for Octo in the settings, or type the link below', 'error', { title: 'The camera is off for Octo' });
  };
  const scan = () => {
    Scanner.requestAccess().then(
      granted => (granted ? setScanning(true) : cameraOff()),
      () => cameraOff(),
    );
  };

  if (scanning) {
    return (
      <Scan
        onScanned={pairScanned}
        onClose={() => setScanning(false)}
        onFailed={reason => {
          if (reason === 'denied') cameraOff();
          else {
            setScanning(false);
            toast('Type the link in the field below', 'error', { title: 'The camera could not open' });
          }
        }}
      />
    );
  }

  return (
    <View style={[s.view, { paddingBottom: keyboard }]}>
      <Top title="Octo" mood={preset.mood} onBack={onBack} backLabel="back to your computers">
        <Dot phase={preset.phase} />
        <Text style={[sub.text, { color: t.lost }]}>{preset.word}</Text>
      </Top>
      <ScrollView ref={scroll} contentContainerStyle={[s.pair, { paddingBottom: (keyboard ? 0 : insets.bottom) + 16 }]} keyboardShouldPersistTaps="handled" showsVerticalScrollIndicator={false}>
        <View style={[s.glow, preset.badge ? s.glowError : s.glowIdle]} pointerEvents="none" />
        <Rise replay={kind} from={8} shrink={1} style={s.hero}>
          <Kai mood={preset.mood} size={MASCOT} />
          {preset.badge ? (
            <View style={s.badge}>
              <Text style={s.badgeText}>{preset.badge}</Text>
            </View>
          ) : null}
          <Text style={s.title}>{preset.title}</Text>
          <Text style={s.text}>{preset.text}</Text>
        </Rise>
        {preset.steps ? (
          <View style={s.steps}>
            <View style={s.step}>
              <View style={s.number}>
                <Text style={s.numberText}>1</Text>
              </View>
              <Text style={s.stepText}>
                {'Open '}
                <Text style={s.strong}>Octo</Text>
                {' on your computer, or the orkai CLI'}
              </Text>
            </View>
            <View style={[s.step, s.divided]}>
              <View style={s.number}>
                <Text style={s.numberText}>2</Text>
              </View>
              <Text style={s.stepText}>
                {'Click '}
                <Text style={s.strong}>Pair phone</Text>
                {' or press '}
                <View style={s.kbd}>
                  <Text style={s.kbdText}>ctrl+p</Text>
                </View>
              </Text>
            </View>
            <View style={[s.step, s.divided]}>
              <View style={s.number}>
                <Text style={s.numberText}>3</Text>
              </View>
              <Text style={s.stepText}>Scan the code with the camera, or paste its link here</Text>
            </View>
          </View>
        ) : null}
        <View style={s.actions}>
          {preset.form ? (
            <>
              <Button kind="primary" big text="Scan the code" onPress={scan} />
              <View style={field.field}>
                <TextInput
                  value={link}
                  onChangeText={setLink}
                  onFocus={() => setFocused(true)}
                  onBlur={() => setFocused(false)}
                  onSubmitEditing={pairTyped}
                  placeholder="or type the link from Octo"
                  placeholderTextColor={t.textFaint}
                  selectionColor={t.accent}
                  autoCapitalize="none"
                  autoCorrect={false}
                  spellCheck={false}
                  keyboardType="url"
                  returnKeyType="go"
                  accessibilityLabel="pairing link"
                  style={[field.input, focused && field.focused]}
                />
                <Button kind="ghost" text="Pair" style={s.pairButton} onPress={pairTyped} />
              </View>
            </>
          ) : null}
          {preset.retry && onRetry ? <Button kind="secondary" big text={preset.again || 'Try again'} onPress={onRetry} /> : null}
          <Text style={s.note}>the key never leaves this phone</Text>
        </View>
      </ScrollView>
    </View>
  );
}
