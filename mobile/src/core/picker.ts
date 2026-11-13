import * as DocumentPicker from 'expo-document-picker';
import * as ImagePicker from 'expo-image-picker';

import type { Picked } from './attach';
import { picked } from './attach';
import { fileSize } from './files';

export type PickFrom = 'library' | 'camera' | 'file';

export type Pick = {
  files: Picked[];
  denied: boolean;
};

const NOTHING: Pick = { files: [], denied: false };
const DENIED: Pick = { files: [], denied: true };
const ORIGINAL = 1;

function photos(result: ImagePicker.ImagePickerResult): Pick {
  if (result.canceled) return NOTHING;
  return { files: picked(result.assets.map(asset => ({ uri: asset.uri, name: asset.fileName, size: asset.fileSize })), fileSize), denied: false };
}

async function fromLibrary(room: number): Promise<Pick> {
  return photos(await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images', 'videos'], allowsMultipleSelection: true, selectionLimit: room, quality: ORIGINAL }));
}

async function fromCamera(): Promise<Pick> {
  const access = await ImagePicker.requestCameraPermissionsAsync();
  if (!access.granted) return DENIED;
  return photos(await ImagePicker.launchCameraAsync({ mediaTypes: ['images'], quality: ORIGINAL }));
}

async function fromFiles(): Promise<Pick> {
  const result = await DocumentPicker.getDocumentAsync({ multiple: true, copyToCacheDirectory: true });
  if (result.canceled) return NOTHING;
  return { files: picked(result.assets, fileSize), denied: false };
}

export function pick(from: PickFrom, room: number): Promise<Pick> {
  if (from === 'library') return fromLibrary(room);
  return from === 'camera' ? fromCamera() : fromFiles();
}
