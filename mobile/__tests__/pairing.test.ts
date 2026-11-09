import { hostOf, parsePairingLink, socketUrl } from '../src/core/pairing';

const KEY = 'Qm9vdHN0cmFwcGluZyB0aGUgb3JrYWkgcGFpcmluZyE';
const LINK = `https://quiet-river-sound-lamp.trycloudflare.com/#k=${KEY}`;

describe('a pairing link', () => {
  it('gives the address of the computer and its key', () => {
    expect(parsePairingLink(LINK, false)).toEqual({ url: 'https://quiet-river-sound-lamp.trycloudflare.com', key: KEY });
  });

  it('survives being copied from a wrapped terminal', () => {
    const wrapped = `  ${LINK.slice(0, 30)}\r\n          ${LINK.slice(30)}  \n`;
    expect(parsePairingLink(wrapped, false)).toEqual({ url: 'https://quiet-river-sound-lamp.trycloudflare.com', key: KEY });
  });

  it('keeps only the origin of a tailnet address', () => {
    expect(parsePairingLink(`https://Lucas-PC.tail1234.ts.net/?x=1#k=${KEY}`, false)).toEqual({ url: 'https://lucas-pc.tail1234.ts.net', key: KEY });
  });

  it('is refused when it is cut, bare or not a link', () => {
    expect(parsePairingLink(LINK.slice(0, -6), false)).toBeNull();
    expect(parsePairingLink('https://quiet-river-sound-lamp.trycloudflare.com/', false)).toBeNull();
    expect(parsePairingLink(KEY, false)).toBeNull();
    expect(parsePairingLink('', false)).toBeNull();
    expect(parsePairingLink(`https://user@evil.example#k=${KEY}`, false)).toBeNull();
  });

  it('only takes a plain http address while developing', () => {
    const local = `http://10.0.2.2:7471/#k=${KEY}`;
    expect(parsePairingLink(local, false)).toBeNull();
    expect(parsePairingLink(local, true)).toEqual({ url: 'http://10.0.2.2:7471', key: KEY });
  });
});

describe('the socket of a computer', () => {
  it('is the encrypted socket path on the same origin', () => {
    expect(socketUrl('https://a-b.trycloudflare.com')).toBe('wss://a-b.trycloudflare.com/ws');
    expect(socketUrl('http://10.0.2.2:7471')).toBe('ws://10.0.2.2:7471/ws');
    expect(hostOf('https://a-b.trycloudflare.com')).toBe('a-b.trycloudflare.com');
  });
});
