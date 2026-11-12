import { StyleSheet, Text, View } from 'react-native';

import { alive, elapsed, label, status } from '../core/agents';
import model from '../core/model';
import type { AgentView } from '../core/protocol';
import type { DeviceScope } from '../screens/device/context';
import { useStyles } from '../theme/styles';
import type { Theme } from '../theme/tokens';
import { mono } from '../theme/type';
import { Icon } from '../ui/Icon';
import { Brand } from '../ui/Brand';
import { ArmedOption, Group, Option } from '../ui/Option';
import { closeSheet, openSheet, SheetScroll } from '../ui/Sheet';
import { openPrompt } from './Prompt';

const LEAD = 40;

const styles = (t: Theme) =>
  StyleSheet.create({
    trail: { flexDirection: 'row', flexShrink: 0 },
    trailText: mono(12),
    square: { width: 11, height: 11, borderRadius: 3 },
    green: { color: t.done },
    red: { color: t.lost },
  });

function MenuBody({ scope, agent }: { scope: DeviceScope; agent: AgentView }) {
  const { workspace } = scope;
  const s = useStyles(styles);
  const changes = agent.changes && agent.changes.files ? agent.changes : null;
  const branch = agent.branch;
  const act = (run: () => void) => () => {
    run();
    closeSheet();
  };
  return (
    <SheetScroll>
      <Group>
        <Option
          icon={color => <Icon name="pencil" size={16} color={color} />}
          text="Rename"
          chevron
          onPress={() => openPrompt('Rename', 'new name', agent.name, name => workspace.send({ type: 'rename', agent: agent.id, name }))}
        />
        {branch ? (
          <Option
            icon={color => <Icon name="diff" size={16} color={color} />}
            iconTone="link"
            text="View changes"
            chevron
            trail={
              changes ? (
                <View style={s.trail}>
                  <Text style={[s.trailText, s.green]}>{`+${changes.insertions} `}</Text>
                  <Text style={[s.trailText, s.red]}>{`−${changes.deletions}`}</Text>
                </View>
              ) : undefined
            }
            onPress={() => {
              closeSheet();
              workspace.send({ type: 'diff', agent: agent.id });
            }}
          />
        ) : null}
        {branch ? (
          <ArmedOption
            icon={color => <Icon name="merge" size={16} color={color} />}
            iconTone="accent"
            tone="current"
            text={`Merge ${branch}`}
            armed="Tap again to merge"
            action={act(() => workspace.send({ type: 'merge', agent: agent.id }))}
          />
        ) : null}
        {agent.state === 'lost' ? (
          <Option
            icon={color => <Icon name="recover" size={16} color={color} />}
            iconTone="green"
            text="Recover"
            detail="start it again where it stopped"
            onPress={() => {
              closeSheet();
              workspace.send({ type: 'recover', session: agent.session });
            }}
          />
        ) : null}
      </Group>
      <Group>
        {alive(agent) ? (
          <ArmedOption
            icon={color => <View style={[s.square, { backgroundColor: color }]} />}
            text="Stop agent"
            armed="Tap again to stop"
            action={act(() => workspace.send({ type: 'stop', agent: agent.id }))}
          />
        ) : (
          <ArmedOption
            icon={color => <Icon name="trash" size={16} color={color} />}
            text={`Remove ${agent.name}`}
            armed="Tap again to remove"
            action={act(() => {
              workspace.send({ type: 'remove', agent: agent.id });
              workspace.leaveAgent();
            })}
          />
        )}
      </Group>
    </SheetScroll>
  );
}

export function openAgentMenu(scope: DeviceScope): void {
  const { link, workspace } = scope;
  const agent = workspace.findAgent(workspace.store.get().agent);
  if (!agent) return;
  const mark = status(agent);
  const time = elapsed(agent, link.now());
  openSheet({
    title: agent.name,
    sub: `${label(agent.kind)} · ${mark.word}${time ? ` ${time}` : ''}`,
    lead: <Brand mood={model.moodOf([agent])} size={LEAD} />,
    agentHead: true,
    content: <MenuBody scope={scope} agent={agent} />,
  });
}
