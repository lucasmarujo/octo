import { File, FileMode } from 'expo-file-system';

import type { Source } from './uploads';

export function openFile(uri: string): Source {
  const handle = new File(uri).open(FileMode.ReadOnly);
  return {
    read: async (offset, length) => {
      handle.offset = offset;
      return handle.readBytes(length);
    },
    close: () => handle.close(),
  };
}

export function fileSize(uri: string): number {
  return new File(uri).size;
}
