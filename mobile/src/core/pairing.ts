export type Pairing = {
  url: string;
  key: string;
};

type Wire = {
  VERSION: number;
  UNPAIRED: number;
  OUTDATED: number;
  REPLACED: number;
  linkKey(link: string): string;
  secretFrom(value: string): Uint8Array | null;
};

export const wire: Wire = require('../../../src/pwa/web/wire.js');

const SECURE = 'https';
const LOCAL = 'http';
const ORIGIN = /^(https?):\/\/([^/?#@\s]+)(?=[/?#]|$)/i;
const SOCKET = { https: 'wss', http: 'ws' } as const;
const SOCKET_PATH = '/ws';

export function parsePairingLink(text: string, allowLocal: boolean): Pairing | null {
  const link = String(text || '').replace(/\s+/g, '');
  const origin = ORIGIN.exec(link);
  if (!origin || !link.includes('#')) return null;
  const scheme = origin[1].toLowerCase();
  if (scheme !== SECURE && !(allowLocal && scheme === LOCAL)) return null;
  const key = wire.linkKey(link);
  if (!wire.secretFrom(key)) return null;
  return { url: `${scheme}://${origin[2].toLowerCase()}`, key };
}

export function secureOrigin(text: string): string | null {
  const origin = ORIGIN.exec(String(text || '').trim());
  if (!origin || origin[1].toLowerCase() !== SECURE) return null;
  return `${SECURE}://${origin[2].toLowerCase()}`;
}

export function socketUrl(url: string): string {
  const at = url.indexOf('://');
  const scheme = url.slice(0, at) as keyof typeof SOCKET;
  return `${SOCKET[scheme]}${url.slice(at)}${SOCKET_PATH}`;
}

export function hostOf(url: string): string {
  return url.slice(url.indexOf('://') + 3);
}
