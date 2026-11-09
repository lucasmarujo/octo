import { useEffect, useState } from 'react';
import { BackHandler, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import Svg, { Path, Rect } from 'react-native-svg';

import type { ScanFailed, Scanned } from '../../modules/octo-scanner';
import { ScannerView } from '../../modules/octo-scanner';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { sans, TIGHT, tracking } from '../theme/type';
import { Round } from '../ui/atoms';

type Props = {
  onScanned: (text: string) => void;
  onFailed: (reason: ScanFailed['nativeEvent']['reason']) => void;
  onClose: () => void;
};

const WINDOW_MARGIN = 44;
const WINDOW_MAX = 300;
const WINDOW_HEIGHT_SHARE = 0.5;
const WINDOW_TOP_SHARE = 0.4;
const WINDOW_RADIUS = 28;
const EDGE = 3;
const SCRIM = 'rgba(5, 5, 8, 0.62)';

export function scanWindow(width: number, height: number): { left: number; top: number; side: number } {
  const side = Math.min(width - 2 * WINDOW_MARGIN, WINDOW_MAX, height * WINDOW_HEIGHT_SHARE);
  return { left: (width - side) / 2, top: (height - side) * WINDOW_TOP_SHARE, side };
}

function roundedHole(left: number, top: number, side: number, radius: number): string {
  const right = left + side;
  const bottom = top + side;
  return [
    `M${left + radius} ${top}`,
    `H${right - radius}`,
    `A${radius} ${radius} 0 0 1 ${right} ${top + radius}`,
    `V${bottom - radius}`,
    `A${radius} ${radius} 0 0 1 ${right - radius} ${bottom}`,
    `H${left + radius}`,
    `A${radius} ${radius} 0 0 1 ${left} ${bottom - radius}`,
    `V${top + radius}`,
    `A${radius} ${radius} 0 0 1 ${left + radius} ${top}`,
    'Z',
  ].join(' ');
}

const styles = (t: Theme) =>
  StyleSheet.create({
    view: { ...StyleSheet.absoluteFill, backgroundColor: t.bg },
    camera: StyleSheet.absoluteFill,
    close: { position: 'absolute', left: 18 },
    words: { position: 'absolute', left: 32, right: 32, alignItems: 'center', gap: 6 },
    title: { ...sans(24, 800, 28), ...tracking(24, TIGHT), color: '#ececf2', textAlign: 'center' },
    hint: { ...sans(16, 400, 22), color: '#9a9aa8', textAlign: 'center' },
  });

export function Scan({ onScanned, onFailed, onClose }: Props) {
  const t = useTheme();
  const s = useStyles(styles);
  const insets = useSafeAreaInsets();
  const window = useWindowDimensions();
  const [size, setSize] = useState({ width: window.width, height: window.height });

  useEffect(() => {
    const back = BackHandler.addEventListener('hardwareBackPress', () => {
      onClose();
      return true;
    });
    return () => back.remove();
  }, [onClose]);

  const hole = scanWindow(size.width, size.height);
  const scrim = `M0 0 H${size.width} V${size.height} H0 Z ${roundedHole(hole.left, hole.top, hole.side, WINDOW_RADIUS)}`;

  return (
    <View style={s.view} onLayout={event => setSize({ width: event.nativeEvent.layout.width, height: event.nativeEvent.layout.height })}>
      <ScannerView style={s.camera} onScanned={(event: Scanned) => onScanned(event.nativeEvent.text)} onFailed={(event: ScanFailed) => onFailed(event.nativeEvent.reason)} />
      <Svg width={size.width} height={size.height} style={StyleSheet.absoluteFill} pointerEvents="none">
        <Path d={scrim} fill={SCRIM} fillRule="evenodd" />
        <Rect x={hole.left} y={hole.top} width={hole.side} height={hole.side} rx={WINDOW_RADIUS} ry={WINDOW_RADIUS} fill="none" stroke={t.accent} strokeWidth={EDGE} />
      </Svg>
      <View style={[s.close, { top: insets.top + 8 }]}>
        <Round icon="close" label="close the camera" onPress={onClose} />
      </View>
      <View style={[s.words, { bottom: insets.bottom + 40 }]} pointerEvents="none">
        <Text style={s.title}>Scan the code</Text>
        <Text style={s.hint}>Point the camera at the code Octo shows on your computer</Text>
      </View>
    </View>
  );
}
