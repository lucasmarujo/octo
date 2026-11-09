import { Platform } from 'react-native';

import { version } from '../../package.json';
import { newId } from './devices';
import type { Identity } from './link';
import { load, save } from './storage';

const NAME = 'client';
const APPLE_PHONE = 'iPhone';
const APPLE_TABLET = 'iPad';

type AndroidFacts = {
  Model?: string;
  Release?: string;
};

type AppleFacts = {
  osVersion?: string;
};

export function androidAgent(release: string, model: string): string {
  return `Octo/${version} (Linux; Android ${release}; ${model}) Mobile`;
}

export function appleAgent(osVersion: string, tablet: boolean): string {
  const os = osVersion.replace(/\./g, '_');
  return tablet ? `Octo/${version} (${APPLE_TABLET}; CPU OS ${os} like Mac OS X)` : `Octo/${version} (${APPLE_PHONE}; CPU ${APPLE_PHONE} OS ${os} like Mac OS X)`;
}

async function client(): Promise<string> {
  const saved = await load<string>(NAME);
  if (typeof saved === 'string' && saved) return saved;
  const created = newId();
  await save(NAME, created);
  return created;
}

export async function loadIdentity(): Promise<Identity> {
  const id = await client();
  if (Platform.OS === 'ios') {
    const osVersion = (Platform.constants as AppleFacts).osVersion || String(Platform.Version);
    const tablet = Platform.isPad;
    return {
      agent: appleAgent(osVersion, tablet),
      device: { type: 'device', model: tablet ? APPLE_TABLET : APPLE_PHONE, osVersion, standalone: true, client: id },
    };
  }
  const facts = Platform.constants as AndroidFacts;
  const model = facts.Model || 'Android';
  const release = facts.Release || String(Platform.Version);
  return {
    agent: androidAgent(release, model),
    device: { type: 'device', model, osVersion: release, standalone: true, client: id },
  };
}
