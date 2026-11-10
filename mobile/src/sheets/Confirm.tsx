import { StyleSheet, Text, View } from 'react-native';

import type { Ask } from '../core/agents';
import { useStyles } from '../theme/styles';
import type { Theme } from '../theme/tokens';
import { sans } from '../theme/type';
import { Button } from '../ui/Button';
import { closeSheet, openSheet } from '../ui/Sheet';

type Props = {
  text: string;
  action: string;
  done: () => void;
};

const styles = (t: Theme) =>
  StyleSheet.create({
    body: { gap: 18, paddingHorizontal: 4 },
    text: { ...sans(16, 400, 22), color: t.textMuted },
    buttons: { flexDirection: 'row', gap: 10 },
    button: { flex: 1, height: 50 },
  });

function ConfirmBody({ text, action, done }: Props) {
  const s = useStyles(styles);
  const go = () => {
    closeSheet();
    done();
  };
  return (
    <View style={s.body}>
      <Text style={s.text}>{text}</Text>
      <View style={s.buttons}>
        <Button kind="secondary" text="Cancel" style={s.button} onPress={closeSheet} />
        <Button kind="danger" text={action} style={s.button} onPress={go} />
      </View>
    </View>
  );
}

export function openConfirm(ask: Ask, done: () => void): void {
  openSheet({ title: ask.title, content: <ConfirmBody text={ask.text} action={ask.action} done={done} /> });
}
