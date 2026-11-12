import { useEffect, useRef, useState } from 'react';
import type { GestureResponderEvent, LayoutChangeEvent } from 'react-native';
import { Animated, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';

import { ACCENT_DEFAULT, ACCENT_PRESETS, HUE_MAX, hueOf, normalizeHex, withHue } from '../core/accent';
import { buzz } from '../core/haptics';
import type { ThemeChoice } from '../core/model';
import { THEME_CHOICES } from '../core/model';
import type { Layout } from '../core/settings';
import { changeSettings, settings } from '../core/settings';
import { useStore } from '../core/store';
import { mix } from '../theme/color';
import { BASE_MS, EASE_SPRING, lasting } from '../theme/motion';
import { useStyles } from '../theme/styles';
import { useReduced, useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { THEMES } from '../theme/tokens';
import { mono, sans } from '../theme/type';
import { Press } from '../ui/Press';
import { openSheet, SheetScroll } from '../ui/Sheet';

type Paired = {
  name: string;
  open: () => void;
};

type Props = {
  paired?: Paired;
  onLayout?: (layout: Layout) => void;
};

const OLD_THEME: ThemeChoice = 'old';
const HUES = ['#ff5a5a', '#f0d65a', '#7ce38b', '#35fddc', '#5aa9ff', '#8b7bff', '#f472b6', '#ff5a5a'] as const;
const HUE_THUMB = 22;
const HEX_LENGTH = 7;
const KNOB_TRAVEL = 18;
const LAYOUTS: { mode: Layout; text: string }[] = [
  { mode: 'list', text: 'List' },
  { mode: 'board', text: 'Board' },
];
const ART: Record<Exclude<ThemeChoice, 'system'>, { bg: string; title: string; row: string; rowEdge: string | null; alert: string; alertEdge: string }> = {
  octo: { bg: '#000000', title: '#ececf2', row: '#141718', rowEdge: null, alert: 'rgba(240, 214, 90, 0.2)', alertEdge: 'rgba(240, 214, 90, 0.4)' },
  gray: { bg: '#282828', title: '#ececec', row: '#303030', rowEdge: null, alert: 'rgba(240, 214, 90, 0.2)', alertEdge: 'rgba(240, 214, 90, 0.4)' },
  light: { bg: '#ffffff', title: '#16161d', row: '#f6f6f9', rowEdge: '#e0e0e8', alert: 'rgba(168, 134, 0, 0.12)', alertEdge: 'rgba(168, 134, 0, 0.4)' },
  old: { bg: '#0e0e13', title: '#ececf2', row: '#16161d', rowEdge: null, alert: 'rgba(240, 214, 90, 0.2)', alertEdge: 'rgba(240, 214, 90, 0.4)' },
};

const styles = (t: Theme) =>
  StyleSheet.create({
    themes: { flexGrow: 0, marginHorizontal: -14 },
    themesContent: { gap: 8, paddingHorizontal: 14 },
    theme: { width: 84, alignItems: 'center', gap: 8 },
    themeText: { ...sans(14, 600), color: t.textMuted },
    themeTextOn: { ...sans(14, 700), color: t.text },
    art: { alignSelf: 'stretch', height: 112, paddingVertical: 12, paddingHorizontal: 10, gap: 6, borderRadius: 16, borderWidth: 1, borderColor: t.borderStrong, overflow: 'hidden' },
    artOn: { borderWidth: 2, borderColor: t.accent, paddingVertical: 11, paddingHorizontal: 9, boxShadow: [{ offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 4, color: mix(t.accent, 18) }] },
    system: { backgroundImage: 'linear-gradient(135deg, #000000 50%, #ffffff 50%)' },
    artTitle: { width: 34, height: 6, borderRadius: 3 },
    artAlert: { height: 22, borderRadius: 6, borderWidth: 1 },
    artRow: { height: 14, borderRadius: 5 },
    artFab: { alignSelf: 'flex-end', width: 14, height: 14, borderRadius: 7, marginTop: 'auto' },
    accent: { paddingVertical: 13, paddingHorizontal: 14, gap: 12 },
    accentHead: { flexDirection: 'row', alignItems: 'baseline', justifyContent: 'space-between', gap: 12 },
    swatches: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
    swatch: { width: 34, height: 34, borderRadius: 17, alignItems: 'center', justifyContent: 'center', borderWidth: 2, borderColor: 'transparent' },
    swatchDot: { width: 26, height: 26, borderRadius: 13 },
    swatchDotOn: { width: 22, height: 22, borderRadius: 11 },
    wheel: { backgroundImage: `linear-gradient(135deg, ${HUES.join(', ')})` },
    custom: { gap: 12 },
    hue: { height: HUE_THUMB, justifyContent: 'center' },
    hueTrack: { height: 8, borderRadius: 4, backgroundImage: `linear-gradient(90deg, ${HUES.join(', ')})` },
    hueThumb: { position: 'absolute', top: 0, width: HUE_THUMB, height: HUE_THUMB, borderRadius: HUE_THUMB / 2, borderWidth: 3, borderColor: '#ffffff', boxShadow: [{ offsetX: 0, offsetY: 1, blurRadius: 4, color: 'rgba(0, 0, 0, 0.4)' }] },
    hexRow: { flexDirection: 'row', alignItems: 'center', gap: 10 },
    hex: { ...mono(14), flex: 1, height: 40, paddingVertical: 0, paddingHorizontal: 12, borderRadius: 10, borderWidth: 1, borderColor: t.borderStrong, backgroundColor: t.surfaceRaised, color: t.text },
    reset: { height: 40, paddingHorizontal: 14, borderRadius: 10, borderWidth: 1, borderColor: t.borderStrong, alignItems: 'center', justifyContent: 'center' },
    resetText: { ...sans(14, 600), color: t.text },
    off: { opacity: 0.4 },
    group: { borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    setting: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 13, paddingHorizontal: 14, minHeight: 52 },
    divided: { borderTopWidth: 1, borderColor: t.border },
    label: { flex: 1 },
    labelText: { ...sans(16, 600), color: t.text },
    small: { ...sans(13), color: t.textFaint },
    value: { ...mono(13), color: t.textMuted },
    chevron: { ...sans(18), color: t.textFaint },
    seg: { flexDirection: 'row', padding: 2, borderRadius: 9, backgroundColor: t.surfaceRaised },
    segButton: { paddingVertical: 4, paddingHorizontal: 10, borderRadius: 7 },
    segOn: { backgroundColor: t.text },
    segText: { ...sans(13, 600), color: t.textMuted },
    segTextOn: { ...sans(13, 700), color: t.bg },
    switch: { width: 44, height: 26, borderRadius: 999, backgroundColor: t.borderStrong, flexShrink: 0 },
    switchOn: { backgroundColor: t.done },
    knob: { position: 'absolute', top: 3, left: 3, width: 20, height: 20, borderRadius: 10, backgroundColor: '#ffffff', boxShadow: [{ offsetX: 0, offsetY: 1, blurRadius: 3, color: 'rgba(0, 0, 0, 0.3)' }] },
  });

function Switch({ on, flip, label }: { on: boolean; flip: () => void; label: string }) {
  const s = useStyles(styles);
  const reduced = useReduced();
  const slide = useRef(new Animated.Value(on ? 1 : 0)).current;

  useEffect(() => {
    Animated.timing(slide, { toValue: on ? 1 : 0, duration: lasting(BASE_MS, reduced), easing: EASE_SPRING, useNativeDriver: true }).start();
  }, [slide, on, reduced]);

  const translateX = slide.interpolate({ inputRange: [0, 1], outputRange: [0, KNOB_TRAVEL] });
  return (
    <Pressable style={[s.switch, on && s.switchOn]} onPress={flip} accessibilityRole="switch" accessibilityLabel={label} accessibilityState={{ checked: on }}>
      <Animated.View style={[s.knob, { transform: [{ translateX }] }]} />
    </Pressable>
  );
}

function ThemeCard({ theme, on }: { theme: ThemeChoice; on: boolean }) {
  const s = useStyles(styles);
  const t = useTheme();
  const art = theme === 'system' ? null : ART[theme];
  const fab = theme === OLD_THEME ? THEMES.old.accent : t.name === OLD_THEME ? THEMES.octo.accent : t.accent;
  return (
    <Press style={s.theme} role="tab" selected={on} onPress={() => changeSettings({ theme })}>
      <View style={[s.art, on && s.artOn, art ? { backgroundColor: art.bg } : s.system]}>
        {art ? (
          <>
            <View style={[s.artTitle, { backgroundColor: art.title }]} />
            <View style={[s.artAlert, { backgroundColor: art.alert, borderColor: art.alertEdge }]} />
            <View style={[s.artRow, { backgroundColor: art.row }, art.rowEdge ? { borderWidth: 1, borderColor: art.rowEdge } : null]} />
            <View style={[s.artRow, { backgroundColor: art.row }, art.rowEdge ? { borderWidth: 1, borderColor: art.rowEdge } : null]} />
            <View style={[s.artFab, { backgroundColor: fab }]} />
          </>
        ) : null}
      </View>
      <Text style={on ? s.themeTextOn : s.themeText}>{theme.charAt(0).toUpperCase() + theme.slice(1)}</Text>
    </Press>
  );
}

export function hueAt(x: number, width: number): number {
  if (width <= 0) return 0;
  return Math.round((Math.min(width, Math.max(0, x)) / width) * HUE_MAX) % HUE_MAX;
}

function Accent() {
  const s = useStyles(styles);
  const t = useTheme();
  const state = useStore(settings);
  const locked = t.name === OLD_THEME;
  const shown = locked ? t.accent : state.accent || t.accent;
  const [open, setOpen] = useState(Boolean(state.accent) && !ACCENT_PRESETS.includes(state.accent || ''));
  const [draft, setDraft] = useState<string | null>(null);
  const [width, setWidth] = useState(0);

  const pick = (hex: string) => {
    setDraft(null);
    changeSettings({ accent: hex === ACCENT_DEFAULT ? null : hex });
  };
  const preset = (hex: string) => {
    setOpen(false);
    pick(hex);
  };
  const slide = (event: GestureResponderEvent) => pick(withHue(shown, hueAt(event.nativeEvent.locationX, width)));
  const type = (text: string) => {
    setDraft(text);
    const hex = normalizeHex(text);
    if (hex && text.trim().length >= HEX_LENGTH - 1) changeSettings({ accent: hex === ACCENT_DEFAULT ? null : hex });
  };
  const measure = (event: LayoutChangeEvent) => setWidth(event.nativeEvent.layout.width);

  return (
    <View style={s.accent}>
      <View style={s.accentHead}>
        <View style={s.label}>
          <Text style={s.labelText}>Accent color</Text>
          {locked ? <Text style={s.small}>Not available in the Old theme</Text> : null}
        </View>
        <Text style={s.value}>{shown.toUpperCase()}</Text>
      </View>
      <View style={[s.swatches, locked && s.off]} pointerEvents={locked ? 'none' : 'auto'}>
        {ACCENT_PRESETS.map(hex => {
          const on = !locked && !open && (state.accent || ACCENT_DEFAULT) === hex;
          return (
            <Pressable key={hex} style={[s.swatch, on && { borderColor: hex }]} onPress={() => preset(hex)} accessibilityRole="button" accessibilityLabel={`accent ${hex}`} accessibilityState={{ selected: on }}>
              <View style={[on ? s.swatchDotOn : s.swatchDot, { backgroundColor: hex }]} />
            </Pressable>
          );
        })}
        <Pressable style={[s.swatch, open && { borderColor: shown }]} onPress={() => setOpen(!open)} accessibilityRole="button" accessibilityLabel="custom accent color" accessibilityState={{ expanded: open }}>
          <View style={[open ? s.swatchDotOn : s.swatchDot, s.wheel]} />
        </Pressable>
      </View>
      {open && !locked ? (
        <View style={s.custom}>
          <View style={s.hue} onLayout={measure} onStartShouldSetResponder={() => true} onMoveShouldSetResponder={() => true} onResponderGrant={slide} onResponderMove={slide} accessibilityRole="adjustable" accessibilityLabel="hue">
            <View style={s.hueTrack} pointerEvents="none" />
            <View pointerEvents="none" style={[s.hueThumb, { backgroundColor: shown, left: Math.min(Math.max(0, width - HUE_THUMB), Math.max(0, (hueOf(shown) / HUE_MAX) * width - HUE_THUMB / 2)) }]} />
          </View>
          <View style={s.hexRow}>
            <TextInput style={s.hex} value={draft ?? shown.toUpperCase()} onChangeText={type} onBlur={() => setDraft(null)} maxLength={HEX_LENGTH} autoCapitalize="characters" autoCorrect={false} spellCheck={false} accessibilityLabel="accent color in hex" />
            <Press style={[s.reset, !state.accent && s.off]} disabled={!state.accent} onPress={() => pick(ACCENT_DEFAULT)}>
              <Text style={s.resetText}>Reset</Text>
            </Press>
          </View>
        </View>
      ) : null}
    </View>
  );
}

function AppearanceBody({ paired, onLayout }: Props) {
  const s = useStyles(styles);
  const state = useStore(settings);
  const setLayout = (layout: Layout) => {
    if (layout === state.layout) return;
    changeSettings({ layout });
    if (onLayout) onLayout(layout);
  };
  return (
    <SheetScroll>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.themes} contentContainerStyle={s.themesContent}>
        {THEME_CHOICES.map(theme => (
          <ThemeCard key={theme} theme={theme} on={theme === state.theme} />
        ))}
      </ScrollView>
      <View style={s.group}>
        <Accent />
        <View style={[s.setting, s.divided]}>
          <View style={s.label}>
            <Text style={s.labelText}>Open agents as</Text>
          </View>
          <View style={s.seg}>
            {LAYOUTS.map(({ mode, text }) => (
              <Pressable key={mode} style={[s.segButton, mode === state.layout && s.segOn]} onPress={() => setLayout(mode)} accessibilityRole="button" accessibilityState={{ selected: mode === state.layout }}>
                <Text style={mode === state.layout ? s.segTextOn : s.segText}>{text}</Text>
              </Pressable>
            ))}
          </View>
        </View>
        <View style={[s.setting, s.divided]}>
          <View style={s.label}>
            <Text style={s.labelText}>Haptics</Text>
            <Text style={s.small}>Buzz when an agent needs you or finishes</Text>
          </View>
          <Switch
            on={state.haptics}
            label="haptics"
            flip={() => {
              changeSettings({ haptics: !state.haptics });
              buzz('tap');
            }}
          />
        </View>
        <View style={[s.setting, s.divided]}>
          <View style={s.label}>
            <Text style={s.labelText}>Reduce motion</Text>
          </View>
          <Switch on={state.reduced} label="reduce motion" flip={() => changeSettings({ reduced: !state.reduced })} />
        </View>
        {paired ? (
          <Press style={[s.setting, s.divided]} onPress={paired.open}>
            <View style={s.label}>
              <Text style={s.labelText}>Paired with</Text>
            </View>
            <Text style={s.value}>{paired.name}</Text>
            <Text style={s.chevron}>›</Text>
          </Press>
        ) : null}
      </View>
    </SheetScroll>
  );
}

export function openAppearance(props: Props = {}): void {
  openSheet({ title: 'Appearance', sub: 'Same themes as Octo on the computer. Terminals follow.', content: <AppearanceBody {...props} /> });
}
