import { MAX_ATTACHMENTS, MAX_FILE_BYTES, refusalText, sizeLabel } from '../core/attach';
import type { PickFrom } from '../core/picker';
import { pick } from '../core/picker';
import type { DeviceScope } from '../screens/device/context';
import type { IconName } from '../ui/Icon';
import { Icon } from '../ui/Icon';
import { Group, Option } from '../ui/Option';
import { closeSheet, openSheet, SheetScroll } from '../ui/Sheet';
import { toast } from '../ui/Toast';

type Choice = {
  from: PickFrom;
  icon: IconName;
  text: string;
  detail: string;
};

const CHOICES: Choice[] = [
  { from: 'library', icon: 'photo', text: 'Photo or video', detail: 'from your library' },
  { from: 'camera', icon: 'camera', text: 'Camera', detail: 'take a photo now' },
  { from: 'file', icon: 'file', text: 'File', detail: 'any kind of file' },
];
const FULL = `A task takes up to ${MAX_ATTACHMENTS} attached files.`;
const NO_CAMERA = 'Allow the camera for Octo in the settings of the phone to take a photo.';
const NOT_OPENED = 'The phone could not open that.';

function AttachBody({ scope }: { scope: DeviceScope }) {
  const { uploads } = scope.workspace;
  const choose = (from: PickFrom) => {
    closeSheet();
    const room = MAX_ATTACHMENTS - uploads.store.get().length;
    if (room <= 0) {
      toast(FULL, 'warn');
      return;
    }
    pick(from, room).then(
      result => {
        const note = result.denied ? NO_CAMERA : refusalText(uploads.add(result.files));
        if (note) toast(note, 'warn');
      },
      () => toast(NOT_OPENED, 'error'),
    );
  };
  return (
    <SheetScroll>
      <Group>
        {CHOICES.map(choice => (
          <Option key={choice.from} icon={color => <Icon name={choice.icon} size={16} color={color} />} text={choice.text} detail={choice.detail} chevron onPress={() => choose(choice.from)} />
        ))}
      </Group>
    </SheetScroll>
  );
}

export function openAttach(scope: DeviceScope): void {
  openSheet({ title: 'Attach', sub: `Up to ${MAX_ATTACHMENTS} files, ${sizeLabel(MAX_FILE_BYTES)} each`, content: <AttachBody scope={scope} /> });
}
