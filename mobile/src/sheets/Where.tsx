import React, { useEffect, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';

import model from '../core/model';
import type { Dir, FolderHit } from '../core/protocol';
import type { DeviceScope } from '../screens/device/context';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { CAPS, mono, sans, tracking } from '../theme/type';
import { Swatch } from '../ui/atoms';
import { Button } from '../ui/Button';
import { Icon } from '../ui/Icon';
import { Group, Note, Option } from '../ui/Option';
import { Press } from '../ui/Press';
import { closeSheet, openSheet, SheetFoot, SheetScroll } from '../ui/Sheet';

type Listing = { kind: 'folders'; parent: string | null; dirs: Dir[] } | { kind: 'search'; hits: FolderHit[] };

const SEARCH_DELAY = 250;
const RECENT_MAX = 6;
const WINDOWS_DRIVE = /^[A-Za-z]:/;

export function pathAt(path: string, index: number): string {
  const parts = model.crumbs(path);
  const windows = WINDOWS_DRIVE.test(parts[0] || '');
  const joined = parts.slice(0, index + 1).join(windows ? '\\' : '/');
  if (windows) return index === 0 ? `${joined}\\` : joined;
  return `/${joined}`;
}

const styles = (t: Theme) =>
  StyleSheet.create({
    search: { flexDirection: 'row', alignItems: 'center', gap: 10, paddingHorizontal: 14, height: 46, borderRadius: 14, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    input: { ...sans(16), flex: 1, minWidth: 0, padding: 0, color: t.text },
    crumbs: { flexGrow: 0 },
    crumbsContent: { alignItems: 'center', gap: 6, paddingHorizontal: 4 },
    crumb: { ...mono(12), color: t.textFaint },
    here: { paddingVertical: 2, paddingHorizontal: 8, borderRadius: 999, backgroundColor: mix(t.accent, 14) },
    hereText: { ...mono(12, 700), color: t.accent },
    caps: { ...mono(11), ...tracking(11, CAPS), color: t.textFaint, paddingHorizontal: 4, textTransform: 'uppercase' },
    recent: { flexGrow: 0 },
    recentContent: { gap: 8 },
    recentCard: { gap: 6, padding: 12, borderRadius: 16, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    recentName: { ...mono(13, 700), color: t.text },
    recentPath: { ...mono(11), color: t.textFaint },
    git: { paddingVertical: 1, paddingHorizontal: 6, borderRadius: 6, backgroundColor: t.surfaceRaised },
    gitText: { ...mono(10), color: t.textMuted },
    use: { alignSelf: 'stretch', height: 52, borderRadius: 16 },
    useText: { ...sans(16, 700), color: t.accentInk },
    usePath: { ...mono(14, 700), color: t.accentInk },
  });

function WhereBody({ scope }: { scope: DeviceScope }) {
  const { workspace } = scope;
  const t = useTheme();
  const s = useStyles(styles);
  const [current, setCurrent] = useState(() => workspace.currentWhere());
  const [query, setQuery] = useState('');
  const [listing, setListing] = useState<Listing | null>(null);
  const [width, setWidth] = useState(0);
  const asked = useRef('');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const stop = workspace.watchFolders(news => {
      if (news.type === 'folderSearch') {
        if (asked.current) setListing({ kind: 'search', hits: news.hits });
        return;
      }
      setCurrent(news.path);
      setListing({ kind: 'folders', parent: news.parent, dirs: news.dirs });
    });
    workspace.browse(workspace.currentWhere());
    return () => {
      stop();
      if (timer.current) clearTimeout(timer.current);
    };
  }, [workspace]);

  const browse = (target: string) => {
    asked.current = '';
    setQuery('');
    setCurrent(target);
    workspace.browse(target);
  };
  const type = (text: string) => {
    setQuery(text);
    const clean = text.trim();
    asked.current = clean;
    if (timer.current) clearTimeout(timer.current);
    if (!clean) {
      workspace.browse(current);
      return;
    }
    timer.current = setTimeout(() => workspace.search(current, clean), SEARCH_DELAY);
  };
  const use = () => {
    workspace.setWhere(current);
    closeSheet();
  };

  const parts = model.crumbs(current);
  const recent = [...new Map([...workspace.agents()].reverse().map(agent => [agent.cwd, agent])).values()].slice(0, RECENT_MAX);
  const folderRow = (dir: Dir, detail?: string) => (
    <Option
      key={dir.path}
      icon={color => <Icon name="folder" size={16} color={color} />}
      iconTone="bare"
      text={dir.name}
      detail={detail}
      monoLabel
      chevron
      trail={
        dir.git ? (
          <View style={s.git}>
            <Text style={s.gitText}>git</Text>
          </View>
        ) : undefined
      }
      onPress={() => browse(dir.path)}
    />
  );

  return (
    <>
      <SheetScroll>
        <View style={s.search}>
          <Icon name="search" size={16} color={t.textFaint} />
          <TextInput
            value={query}
            onChangeText={type}
            placeholder="Search folders"
            placeholderTextColor={t.textFaint}
            selectionColor={t.accent}
            autoCapitalize="none"
            autoCorrect={false}
            spellCheck={false}
            returnKeyType="search"
            style={s.input}
          />
        </View>
        <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.crumbs} contentContainerStyle={s.crumbsContent}>
          {parts.map((part, index) => (
            <React.Fragment key={`${index}:${part}`}>
              {index ? <Text style={s.crumb}>›</Text> : null}
              {index === parts.length - 1 ? (
                <View style={s.here}>
                  <Text style={s.hereText}>{part}</Text>
                </View>
              ) : (
                <Text style={s.crumb} onPress={() => browse(pathAt(current, index))}>
                  {part}
                </Text>
              )}
            </React.Fragment>
          ))}
        </ScrollView>
        {recent.length ? (
          <>
            <Text style={s.caps}>Recent</Text>
            <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.recent} contentContainerStyle={s.recentContent} onLayout={event => setWidth(event.nativeEvent.layout.width)}>
              {recent.map(agent => (
                <Press key={agent.cwd} style={[s.recentCard, { width: width * 0.46 }]} onPress={() => browse(agent.cwd)}>
                  <Swatch color={agent.color} size={8} radius={3} />
                  <Text style={s.recentName} numberOfLines={1}>
                    {model.crumbs(agent.cwd).pop() || agent.cwd}
                  </Text>
                  <Text style={s.recentPath} numberOfLines={1}>
                    {model.shortPath(agent.cwd)}
                  </Text>
                </Press>
              ))}
            </ScrollView>
          </>
        ) : null}
        {listing && listing.kind === 'search' ? (
          listing.hits.length ? (
            <Group>{listing.hits.map(hit => folderRow(hit, hit.rel))}</Group>
          ) : (
            <Note text="No folder matches that" />
          )
        ) : null}
        {listing && listing.kind === 'folders' ? (
          listing.parent || listing.dirs.length ? (
            <Group>
              {listing.parent ? (
                <Option
                  key="up"
                  icon={color => <Icon name="up" size={16} color={color} />}
                  text={`Up to ${model.crumbs(listing.parent).pop() || listing.parent}`}
                  onPress={() => browse(listing.parent as string)}
                />
              ) : null}
              {listing.dirs.map(dir => folderRow(dir))}
            </Group>
          ) : (
            <Note text="No folders inside" />
          )
        ) : null}
      </SheetScroll>
      <SheetFoot>
        <Button kind="primary" style={s.use} onPress={use}>
          <Text style={s.useText}>
            {'Use '}
            <Text style={s.usePath}>{model.shortPath(current)}</Text>
          </Text>
        </Button>
      </SheetFoot>
    </>
  );
}

export function openWhere(scope: DeviceScope): void {
  openSheet({ title: 'Where', tall: true, content: <WhereBody scope={scope} /> });
}
