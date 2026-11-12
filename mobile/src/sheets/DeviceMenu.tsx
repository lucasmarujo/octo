
import type { Device } from '../core/devices';
import { forgetDevice } from '../core/devices';
import { hostOf } from '../core/pairing';
import { Icon } from '../ui/Icon';
import { Brand } from '../ui/Brand';
import { ArmedOption, Group, Option } from '../ui/Option';
import { closeSheet, openSheet, SheetScroll } from '../ui/Sheet';

type Props = {
  device: Device;
  onPair: (replace: string) => void;
  onForgotten: () => void;
};

const LEAD = 40;

function MenuBody({ device, onPair, onForgotten }: Props) {
  return (
    <SheetScroll>
      <Group>
        <Option
          icon={color => <Icon name="recover" size={16} color={color} />}
          iconTone="link"
          text="Pair it again"
          detail="scan the code this computer shows now"
          chevron
          onPress={() => {
            closeSheet();
            onPair(device.id);
          }}
        />
      </Group>
      <Group>
        <ArmedOption
          icon={color => <Icon name="trash" size={16} color={color} />}
          text="Forget this computer"
          armed="Tap again to forget"
          action={() => {
            closeSheet();
            forgetDevice(device.id);
            onForgotten();
          }}
        />
      </Group>
    </SheetScroll>
  );
}

export function openDeviceMenu(props: Props): void {
  openSheet({ title: props.device.name, sub: hostOf(props.device.url), lead: <Brand mood="idle" size={LEAD} />, agentHead: true, content: <MenuBody {...props} /> });
}
