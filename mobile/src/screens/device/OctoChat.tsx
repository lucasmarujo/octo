import React, { useCallback, useMemo } from 'react';
import { FlatList, StyleSheet, Text, View } from 'react-native';

import type { ChatRow } from '../../core/octo';
import { chatRows } from '../../core/octo';
import type { Decision, OctoChatEntry } from '../../core/protocol';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import type { Theme } from '../../theme/tokens';
import { mono, sans, TIGHT, tracking } from '../../theme/type';
import { Spin } from '../../ui/atoms';
import { Brand } from '../../ui/Brand';
import { Press } from '../../ui/Press';
import { chatStyles, Said, useFollow } from './Chat';
import { DecisionCard } from './OctoCard';

type Props = {
  chat: OctoChatEntry[];
  cards: Decision[];
  more: number;
  thinking: boolean;
  problem: string | null;
  onMore: () => void;
};

const MARK = 72;
const ACTION_MARK = '↳';

const styles = (t: Theme) =>
  StyleSheet.create({
    said: { gap: 8 },
    failed: { paddingVertical: 10, paddingHorizontal: 12, borderRadius: 14, backgroundColor: mix(t.lost, 8), borderWidth: 1, borderColor: mix(t.lost, 35) },
    action: { ...mono(11, 400, 16), color: t.textFaint },
    foot: { gap: 12, paddingTop: 14 },
    problem: { ...sans(14, 400, 19), color: t.lost, paddingVertical: 10, paddingHorizontal: 12, borderRadius: 14, backgroundColor: mix(t.lost, 8), borderWidth: 1, borderColor: mix(t.lost, 35) },
    thinking: { flexDirection: 'row', alignItems: 'center', gap: 8 },
    thinkingMark: { ...mono(12, 700, 16), color: t.accent },
    thinkingText: { ...mono(12, 400, 16), color: t.accent },
    more: { alignSelf: 'center', paddingVertical: 6, paddingHorizontal: 12 },
    moreText: { ...sans(14, 600), color: t.link },
    empty: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingHorizontal: 32, paddingBottom: 24 },
    emptyTitle: { ...sans(24, 800, 28), ...tracking(24, TIGHT), color: t.text, textAlign: 'center' },
    emptyText: { ...sans(15, 400, 21), color: t.textMuted, textAlign: 'center' },
  });

const Entry = React.memo(function Entry({ entry }: { entry: OctoChatEntry }) {
  const chat = useStyles(chatStyles);
  const s = useStyles(styles);
  if (entry.role === 'you') {
    return (
      <View style={chat.user}>
        <Text style={chat.userText} selectable>
          {entry.text}
        </Text>
      </View>
    );
  }
  return (
    <View style={[s.said, entry.failed && s.failed]}>
      <Said text={entry.text} s={chat} />
      {entry.actions.map((line, index) => (
        <Text key={index} style={s.action}>{`${ACTION_MARK} ${line}`}</Text>
      ))}
    </View>
  );
});

export const OctoChat = React.memo(function OctoChat({ chat, cards, more, thinking, problem, onMore }: Props) {
  const look = useStyles(chatStyles);
  const s = useStyles(styles);
  const { list, follow, scrolled } = useFollow<ChatRow>();
  const rows = useMemo(() => chatRows(chat), [chat]);
  const render = useCallback(({ item }: { item: ChatRow }) => <Entry entry={item.entry} />, []);

  if (!rows.length && !cards.length && !thinking && !problem) {
    return (
      <View style={s.empty}>
        <Brand mood="idle" size={MARK} />
        <Text style={s.emptyTitle}>Ask Octo</Text>
        <Text style={s.emptyText}>What is running, what is stuck, what to do next. It reads every agent and can act on them for you.</Text>
      </View>
    );
  }

  const foot =
    cards.length || thinking || problem ? (
      <View style={s.foot}>
        {problem ? <Text style={s.problem}>{problem}</Text> : null}
        {cards.map(decision => (
          <DecisionCard key={decision.id} decision={decision} />
        ))}
        {more ? (
          <Press style={s.more} onPress={onMore}>
            <Text style={s.moreText}>{`${more} more in Decisions`}</Text>
          </Press>
        ) : null}
        {thinking ? (
          <View style={s.thinking}>
            <Spin style={s.thinkingMark} />
            <Text style={s.thinkingText}>Octo is thinking…</Text>
          </View>
        ) : null}
      </View>
    ) : null;

  return (
    <FlatList
      ref={list}
      style={look.chat}
      contentContainerStyle={look.content}
      data={rows}
      keyExtractor={row => row.key}
      renderItem={render}
      ListFooterComponent={foot || undefined}
      onContentSizeChange={follow}
      onLayout={() => follow()}
      onScroll={scrolled}
      scrollEventThrottle={64}
      keyboardShouldPersistTaps="handled"
      showsVerticalScrollIndicator={false}
      accessibilityLabel="conversation with Octo"
    />
  );
});
