import { AppState } from 'react-native';

import Link from '../../modules/octo-link';
import { loadDevices } from './devices';
import { loadIdentity } from './identity';
import { Links } from './links';
import { loadSettings } from './settings';

export const links = new Links(Link);

export async function boot(): Promise<void> {
  const [identity] = await Promise.all([loadIdentity(), loadSettings(), loadDevices()]);
  Link.addListener('onReady', event => links.onReady(event.id));
  Link.addListener('onMessage', event => links.onMessage(event.id, event.text));
  Link.addListener('onClosed', event => links.onClosed(event.id, event.code));
  AppState.addEventListener('change', state => {
    if (state === 'active') links.foreground(Date.now());
    else if (state === 'background') links.background(Date.now());
  });
  links.start(identity);
}
