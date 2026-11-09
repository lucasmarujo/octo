import React, { useEffect, useMemo, useRef, useState } from 'react';
import { FlatList, ScrollView, StyleSheet, Text, View } from 'react-native';

import { buzz } from '../core/haptics';
import type { DiffFile, DiffLine } from '../core/model';
import model from '../core/model';
import type { DiffMsg } from '../core/protocol';
import * as Clipboard from 'expo-clipboard';
import type { DeviceScope } from '../screens/device/context';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import type { Theme } from '../theme/tokens';
import { mono } from '../theme/type';
import { Round } from '../ui/atoms';
import { Button } from '../ui/Button';
import { Note } from '../ui/Option';
import { closeSheet, openSheet, SheetFoot } from '../ui/Sheet';
import { toast } from '../ui/Toast';

const DIFF_LINES = 4000;
const CONFIRM_MS = 3000;
const FIRST_FILES = 4;
const COPY_SIZE = 50;
const COPY_RADIUS = 15;

type Shown = {
  files: DiffFile[];
  cut: boolean;
};

export function budgeted(files: DiffFile[], budget: number): Shown {
  const shown: DiffFile[] = [];
  let left = budget;
  for (const file of files) {
    if (left <= 0) break;
    const hunks = [];
    for (const hunk of file.hunks) {
      if (left <= 0) break;
      const lines = hunk.lines.slice(0, left);
      left -= lines.length;
      hunks.push({ ...hunk, lines });
    }
    shown.push({ ...file, hunks });
  }
  return { files: shown, cut: left <= 0 };
}

const styles = (t: Theme) =>
  StyleSheet.create({
    list: { flexShrink: 1, flexGrow: 1, minHeight: 0 },
    content: { gap: 14 },
    head: { flexDirection: 'row', alignItems: 'flex-end', justifyContent: 'space-between', gap: 10, paddingHorizontal: 4 },
    branch: { ...mono(12), color: t.link, flexShrink: 1 },
    stats: { flexDirection: 'row', gap: 8, alignItems: 'baseline' },
    stat: { ...mono(12), color: t.textMuted },
    bold: mono(12, 700),
    bar: { flexDirection: 'row', height: 6, borderRadius: 6, overflow: 'hidden', marginHorizontal: 4, backgroundColor: t.track },
    file: { borderRadius: 16, backgroundColor: t.termBg, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    fileHead: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 10, paddingHorizontal: 12, borderBottomWidth: 1, borderColor: t.border, backgroundColor: t.surface },
    fileName: { ...mono(13, 700), flex: 1, minWidth: 0, color: t.text },
    fileStat: mono(13),
    hunk: { paddingVertical: 6, paddingHorizontal: 12, backgroundColor: mix(t.link, 8) },
    hunkText: { ...mono(11), color: t.link },
    line: { flexDirection: 'row', paddingHorizontal: 12, minWidth: '100%' },
    lineText: { ...mono(12, 400, 19), color: t.textMuted },
    number: { ...mono(11, 400, 19), width: 34, flexShrink: 0, color: t.textFaint },
    add: { backgroundColor: mix(t.done, 12) },
    addText: { color: t.done },
    del: { backgroundColor: mix(t.lost, 12) },
    delText: { color: t.lost },
    actions: { flexDirection: 'row', gap: 8 },
    merge: { flex: 1, height: 50 },
    green: { color: t.done },
    red: { color: t.lost },
  });

function Line({ line }: { line: DiffLine }) {
  const s = useStyles(styles);
  const mark = line.kind === 'ctx' ? String(line.number) : line.kind === 'add' ? '+' : '−';
  return (
    <View style={[s.line, line.kind === 'add' && s.add, line.kind === 'del' && s.del]}>
      <Text style={s.number}>{mark}</Text>
      <Text style={[s.lineText, line.kind === 'add' && s.addText, line.kind === 'del' && s.delText]}>{line.text}</Text>
    </View>
  );
}

const File = React.memo(function File({ file }: { file: DiffFile }) {
  const s = useStyles(styles);
  return (
    <View style={s.file}>
      <View style={s.fileHead}>
        <Text style={s.fileName} numberOfLines={1}>
          {file.name}
        </Text>
        <Text style={[s.fileStat, s.green]}>{`+${file.insertions}`}</Text>
        <Text style={[s.fileStat, s.red]}>{`−${file.deletions}`}</Text>
      </View>
      {file.hunks.map((hunk, index) => (
        <View key={index}>
          <View style={s.hunk}>
            <Text style={s.hunkText} numberOfLines={1}>{`@@ ${hunk.title}`}</Text>
          </View>
          <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={{ flexDirection: 'column', minWidth: '100%' }}>
            {hunk.lines.map((line, at) => (
              <Line key={at} line={line} />
            ))}
          </ScrollView>
        </View>
      ))}
    </View>
  );
});

function DiffBody({ scope, message }: { scope: DeviceScope; message: DiffMsg }) {
  const { workspace } = scope;
  const s = useStyles(styles);
  const [armed, setArmed] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const files = useMemo(() => model.diffFiles(message.text), [message.text]);
  const totals = useMemo(() => model.diffTotals(files), [files]);
  const shown = useMemo(() => budgeted(files, DIFF_LINES), [files]);
  const agent = workspace.findAgent(message.agent);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const merge = () => {
    if (armed) {
      workspace.send({ type: 'merge', agent: message.agent });
      closeSheet();
      return;
    }
    setArmed(true);
    buzz('arm');
    timer.current = setTimeout(() => setArmed(false), CONFIRM_MS);
  };
  const copy = () => {
    Clipboard.setStringAsync(message.text).then(
      () => toast('Diff copied', 'success'),
      () => toast('Could not copy the diff', 'error'),
    );
  };

  return (
    <>
      <FlatList
        style={s.list}
        contentContainerStyle={s.content}
        data={shown.files}
        keyExtractor={file => file.name}
        renderItem={({ item }) => <File file={item} />}
        initialNumToRender={FIRST_FILES}
        showsVerticalScrollIndicator={false}
        ListHeaderComponentStyle={s.content}
        ListHeaderComponent={
          <>
            <View style={s.head}>
              <Text style={s.branch} numberOfLines={1}>{`⎇ ${message.branch}`}</Text>
              <View style={s.stats}>
                <Text style={s.stat}>{`${totals.files} file${totals.files === 1 ? '' : 's'}`}</Text>
                <Text style={[s.bold, s.green]}>{`+${totals.insertions}`}</Text>
                <Text style={[s.bold, s.red]}>{`−${totals.deletions}`}</Text>
              </View>
            </View>
            {totals.insertions + totals.deletions ? (
              <View style={s.bar}>
                <View style={{ flex: totals.insertions, backgroundColor: s.green.color }} />
                <View style={{ flex: totals.deletions, backgroundColor: s.red.color }} />
              </View>
            ) : null}
            {files.length ? null : <Note text="No changes yet" />}
          </>
        }
        ListFooterComponent={files.length && (message.truncated || shown.cut) ? <Note text="The diff was cut; see the rest on the computer" /> : undefined}
      />
      {files.length ? (
        <SheetFoot>
          <View style={s.actions}>
            {agent && agent.branch ? <Button kind="primary" flat style={s.merge} text={armed ? 'Tap again to merge' : `Merge ${message.branch}`} onPress={merge} /> : null}
            <Round icon="copy" label="copy the diff" size={COPY_SIZE} radius={COPY_RADIUS} onPress={copy} />
          </View>
        </SheetFoot>
      ) : null}
    </>
  );
}

export function openDiff(scope: DeviceScope, message: DiffMsg): void {
  openSheet({ title: 'Changes', tall: true, content: <DiffBody scope={scope} message={message} /> });
}
