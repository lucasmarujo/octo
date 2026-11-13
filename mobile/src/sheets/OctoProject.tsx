import { bypassAsk, MODE_NAMES, MODES, projectBypassConfirm } from '../core/octo';
import type { Mode } from '../core/protocol';
import { useStore } from '../core/store';
import type { DeviceScope } from '../screens/device/context';
import { useTheme } from '../theme/ThemeContext';
import { Icon } from '../ui/Icon';
import { Group, Option } from '../ui/Option';
import { closeSheet, openSheet, SheetScroll } from '../ui/Sheet';
import { openConfirm } from './Confirm';
import { openInstructions } from './Instructions';

const DETAILS: Record<Mode, string> = {
  observe: 'never answers, every question comes to you',
  moderate: 'answers what the rules and the task settle',
  bypass: 'answers and allows everything, no limits',
};

function ProjectBody({ scope, project }: { scope: DeviceScope; project: string }) {
  const { octo } = scope.workspace;
  const t = useTheme();
  const view = useStore(octo.view);
  if (!view) return null;
  const found = view.projects.find(entry => entry.path === project);
  const override = found ? found.mode : null;
  const check = <Icon name="check" size={16} color={t.link} />;
  const choose = (mode: Mode | null) => {
    if (mode === override) {
      closeSheet();
      return;
    }
    if (projectBypassConfirm(view, override, mode)) {
      openConfirm(bypassAsk(project), () => octo.setProjectMode(project, mode));
      return;
    }
    octo.setProjectMode(project, mode);
    closeSheet();
  };
  return (
    <SheetScroll>
      <Group>
        <Option text={`Default (${MODE_NAMES[view.mode]})`} detail="follows the mode of Octo" tone={override === null ? 'current' : undefined} trail={override === null ? check : undefined} onPress={() => choose(null)} />
        {MODES.map(mode => (
          <Option key={mode} text={MODE_NAMES[mode]} detail={DETAILS[mode]} tone={override === mode ? 'current' : undefined} trail={override === mode ? check : undefined} onPress={() => choose(mode)} />
        ))}
      </Group>
      <Group>
        <Option icon={color => <Icon name="pencil" size={16} color={color} />} text="Instructions" detail="rules Octo reads for this project" chevron onPress={() => openInstructions(scope, project)} />
      </Group>
    </SheetScroll>
  );
}

export function openOctoProject(scope: DeviceScope, project: string): void {
  openSheet({ title: project, sub: 'How far Octo goes in this project', agentHead: true, content: <ProjectBody scope={scope} project={project} /> });
}
