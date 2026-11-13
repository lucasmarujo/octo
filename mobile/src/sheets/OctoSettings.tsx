import { StyleSheet, Text, View } from 'react-native';

import type { EngineSlot } from '../core/octo';
import { bypassAsk, enableBypassConfirm, engineName, GLOBAL_FILE, installedEngines, MODE_NAMES, MODE_TEXTS, MODES, needsBypassConfirm, secondaryChoices } from '../core/octo';
import type { Mode, OrchestratorView } from '../core/protocol';
import { useStore } from '../core/store';
import type { DeviceScope } from '../screens/device/context';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { CAPS, mono, sans, tracking } from '../theme/type';
import { kindColor } from '../ui/atoms';
import { Icon } from '../ui/Icon';
import { ArmedOption, Group, Note, Option } from '../ui/Option';
import { ProviderMark } from '../ui/ProviderMark';
import type { SegmentItem } from '../ui/Segmented';
import { Segmented } from '../ui/Segmented';
import { closeSheet, openSheet, SheetScroll } from '../ui/Sheet';
import { Switch } from './Appearance';
import { openConfirm } from './Confirm';
import { openInstructions } from './Instructions';

const OPTION_ICON = 18;

export const MODE_ITEMS: SegmentItem<Mode>[] = MODES.map(mode => ({ value: mode, text: MODE_NAMES[mode], tone: mode === 'bypass' ? 'waiting' : 'accent' }));

const styles = (t: Theme) =>
  StyleSheet.create({
    section: { gap: 8 },
    caps: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.textFaint, paddingHorizontal: 4 },
    group: { borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    setting: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 13, paddingHorizontal: 14, minHeight: 52 },
    label: { flex: 1 },
    labelText: { ...sans(16, 600), color: t.text },
    small: { ...sans(13), color: t.textFaint },
    mark: mono(15),
  });

export function pickMode(scope: DeviceScope, view: OrchestratorView, mode: Mode): void {
  if (mode === view.mode) return;
  if (needsBypassConfirm(mode, view.mode)) openConfirm(bypassAsk(null), () => scope.workspace.octo.setMode(mode));
  else scope.workspace.octo.setMode(mode);
}

function Engines({ scope, view, slot }: { scope: DeviceScope; view: OrchestratorView; slot: EngineSlot }) {
  const t = useTheme();
  const s = useStyles(styles);
  const current = slot === 'primary' ? view.primary : view.secondary;
  const model = slot === 'primary' ? view.primaryModel : view.secondaryModel;
  const kinds = slot === 'primary' ? installedEngines(view) : secondaryChoices(view);
  const check = <Icon name="check" size={16} color={t.link} />;
  const choose = (kind: string | null) => {
    if (kind !== current) scope.workspace.octo.setEngine(slot, kind);
  };
  return (
    <Group>
      {kinds.map(kind => (
        <Option
          key={kind}
          icon={() => <ProviderMark kind={kind} size={OPTION_ICON} style={[s.mark, { color: kindColor(t, kind) }]} />}
          iconFill={mix(kindColor(t, kind), 14)}
          text={engineName(kind)}
          detail={kind === current && model ? model : null}
          tone={kind === current ? 'current' : undefined}
          trail={kind === current ? check : undefined}
          onPress={() => choose(kind)}
        />
      ))}
      {slot === 'secondary' ? <Option text="None" detail="nothing takes over when the first fails" tone={current === null ? 'current' : undefined} trail={current === null ? check : undefined} onPress={() => choose(null)} /> : null}
    </Group>
  );
}

function SettingsBody({ scope }: { scope: DeviceScope }) {
  const { octo } = scope.workspace;
  const s = useStyles(styles);
  const view = useStore(octo.view);
  if (!view) return null;
  return (
    <SheetScroll>
      <View style={s.group}>
        <View style={s.setting}>
          <View style={s.label}>
            <Text style={s.labelText}>Octo answers your agents</Text>
            <Text style={s.small}>{view.enabled ? 'On. It runs in Octo on the computer.' : 'Off. Nothing is watched or answered.'}</Text>
          </View>
          <Switch on={view.enabled} label="orchestrator" flip={() => (enableBypassConfirm(view) ? openConfirm(bypassAsk(null), () => octo.setEnabled(true)) : octo.setEnabled(!view.enabled))} />
        </View>
      </View>
      <View style={s.section}>
        <Text style={s.caps}>THINKS FIRST</Text>
        <Engines scope={scope} view={view} slot="primary" />
        {installedEngines(view).length ? null : <Note text="Octo thinks with Claude or Codex. Install one of them on the computer." />}
      </View>
      <View style={s.section}>
        <Text style={s.caps}>TAKES OVER WHEN THE FIRST FAILS</Text>
        <Engines scope={scope} view={view} slot="secondary" />
      </View>
      <View style={s.section}>
        <Text style={s.caps}>DEFAULT MODE</Text>
        <Segmented items={MODE_ITEMS} value={view.mode} label="default mode" stretch onPick={mode => pickMode(scope, view, mode)} />
        <Note text={`${MODE_TEXTS[view.mode]} Each project can run in its own mode, from Projects.`} />
      </View>
      <View style={s.section}>
        <Text style={s.caps}>INSTRUCTIONS</Text>
        <Group>
          <Option icon={color => <Icon name="pencil" size={16} color={color} />} text={GLOBAL_FILE} detail="rules for every project" monoLabel chevron onPress={() => openInstructions(scope, null)} />
        </Group>
        <Note text="The rules of one project are in Projects, under its mode." />
      </View>
      <Group>
        <ArmedOption
          icon={color => <Icon name="trash" size={16} color={color} />}
          text="Clear the conversation"
          armed="Tap again to clear"
          action={() => {
            octo.clearChat();
            closeSheet();
          }}
        />
      </Group>
    </SheetScroll>
  );
}

export function openOctoSettings(scope: DeviceScope): void {
  openSheet({ title: 'Octo', sub: 'Which agent thinks for it, how far it goes, what it follows', content: <SettingsBody scope={scope} /> });
}
