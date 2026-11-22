import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { request } from 'node:http';
import { connect } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { LIMITS, Registry, addressOf, computerOf, digest, respond } from '../relay.mjs';

const CONFIG = { host: 'octo-relay.marujo.dev', zone: 'octo.marujo.dev' };
const ID = '0123456789abcdef0123456789abcdef';
const OTHER = 'fedcba9876543210fedcba9876543210';
const TOKEN = 'a'.repeat(64);
const THIEF = 'b'.repeat(64);
const TUNNEL = 'https://quiet-river-sound-lamp.trycloudflare.com';
const MOVED = 'https://smoke-test-new-name.trycloudflare.com';
const HOME = `${ID}.octo.marujo.dev`;
const SERVER = fileURLToPath(new URL('../server.mjs', import.meta.url));

function put(registry, { id = ID, token = TOKEN, url = TUNNEL, from = '203.0.113.7', now = 1000, host = CONFIG.host } = {}) {
  return respond(registry, CONFIG, {
    method: 'PUT',
    host,
    target: `/v1/computers/${id}`,
    authorization: `Bearer ${token}`,
    from,
    now,
    body: JSON.stringify({ url }),
  });
}

function get(registry, host, target = '/') {
  return respond(registry, CONFIG, { method: 'GET', host, target, from: '198.51.100.1', now: 5000, body: '' });
}

test('a computer gets one address that never changes and follows its tunnel', () => {
  const registry = new Registry();
  const first = put(registry);
  assert.equal(first.status, 201);
  assert.deepEqual(JSON.parse(first.body), { address: `https://${HOME}` });
  assert.deepEqual(first.record, { id: ID, hash: digest(TOKEN), url: TUNNEL, at: 1000 });
  assert.equal(get(registry, HOME, '/ws').headers.Location, `${TUNNEL}/ws`);
  const moved = put(registry, { url: MOVED, now: 9000 });
  assert.equal(moved.status, 200);
  assert.deepEqual(JSON.parse(moved.body), { address: `https://${HOME}` });
  assert.equal(moved.record.url, MOVED);
  const answer = get(registry, HOME.toUpperCase() + ':443', '/ws');
  assert.equal(answer.status, 307);
  assert.equal(answer.headers.Location, `${MOVED}/ws`);
  assert.equal(answer.headers['Cache-Control'], 'no-store');
});

test('telling the same tunnel again writes nothing', () => {
  const registry = new Registry();
  put(registry);
  const again = put(registry, { now: 1001 });
  assert.equal(again.status, 200);
  assert.equal(again.record, undefined);
});

test('nobody else can take or move the address of a computer', () => {
  const registry = new Registry();
  put(registry);
  const stolen = put(registry, { token: THIEF, url: MOVED, now: 9000 });
  assert.equal(stolen.status, 403);
  assert.equal(stolen.record, undefined);
  assert.equal(get(registry, HOME).headers.Location, `${TUNNEL}/`);
});

test('only a trycloudflare tunnel, a full id and a full token are accepted', () => {
  const registry = new Registry();
  for (const url of [
    'https://evil.example.com',
    'http://quiet-river.trycloudflare.com',
    'https://quiet-river.trycloudflare.com/path',
    'https://quiet-river.trycloudflare.com.evil.example.com',
    'https://api.trycloudflare.com@evil.example.com',
    'https://a.b.trycloudflare.com',
    'https://quiet-river.trycloudflare.com\r\nSet-Cookie: a=b',
    '',
  ]) {
    assert.equal(put(registry, { url }).status, 400, url);
  }
  assert.equal(put(registry, { id: 'abc' }).status, 400);
  assert.equal(put(registry, { id: ID.toUpperCase() }).status, 400);
  assert.equal(put(registry, { token: 'short' }).status, 400);
  assert.equal(put(registry, { token: 'A'.repeat(64) }).status, 400);
  const junk = respond(registry, CONFIG, {
    method: 'PUT',
    host: CONFIG.host,
    target: `/v1/computers/${ID}`,
    authorization: `Bearer ${TOKEN}`,
    from: '203.0.113.7',
    now: 1,
    body: 'not json',
  });
  assert.equal(junk.status, 400);
  assert.equal(registry.size, 0);
});

test('an address only answers with a redirect and keeps the path the phone asked for', () => {
  const registry = new Registry();
  put(registry);
  assert.equal(get(registry, HOME, '/ws?x=1').headers.Location, `${TUNNEL}/ws?x=1`);
  assert.equal(get(registry, HOME, '/a b').headers.Location, `${TUNNEL}/`);
  assert.equal(get(registry, HOME, '//evil.example.com').headers.Location, `${TUNNEL}//evil.example.com`);
  assert.equal(get(registry, HOME, `/${'a'.repeat(LIMITS.targetLength)}`).headers.Location, `${TUNNEL}/`);
  assert.equal(get(registry, HOME, '/x\r\nSet-Cookie: a=b').headers.Location, `${TUNNEL}/`);
  assert.equal(get(registry, `${OTHER}.octo.marujo.dev`).status, 404);
  assert.equal(put(registry, { host: HOME }).status, 307, 'an address never registers anything');
});

test('the certificate question is answered yes only for a computer that exists', () => {
  const registry = new Registry();
  put(registry);
  assert.equal(get(registry, 'octo-relay:8080', `/ask?domain=${HOME}`).status, 200);
  assert.equal(get(registry, 'octo-relay:8080', `/ask?domain=${OTHER}.octo.marujo.dev`).status, 404);
  assert.equal(get(registry, 'octo-relay:8080', '/ask?domain=mail.marujo.dev').status, 404);
  assert.equal(get(registry, 'octo-relay:8080', `/ask?domain=${ID}.octo.example.com`).status, 404);
  assert.equal(get(registry, 'octo-relay:8080', '/ask').status, 404);
  assert.equal(get(registry, CONFIG.host, `/ask?domain=${HOME}`).status, 404, 'the question is not public');
  assert.equal(get(registry, 'octo-relay:8080', '/health').status, 200);
  assert.equal(get(registry, CONFIG.host, '/health').body, 'octo-relay');
  assert.equal(get(registry, CONFIG.host, '/').status, 404);
  assert.equal(get(registry, CONFIG.host, `/v1/computers/${ID}`).status, 405);
});

test('one network cannot flood the service with new computers, and the list stays bounded', () => {
  const limits = { ...LIMITS, newcomers: 2, computers: 3, addresses: 2 };
  const registry = new Registry(limits);
  const id = index => String(index).padStart(32, '0');
  assert.equal(put(registry, { id: id(1) }).status, 201);
  assert.equal(put(registry, { id: id(2) }).status, 201);
  assert.equal(put(registry, { id: id(3) }).status, 429);
  assert.equal(put(registry, { id: id(1), url: MOVED, now: 5000 }).status, 200, 'a known computer is never limited');
  assert.equal(put(registry, { id: id(3), from: '203.0.113.8' }).status, 201);
  assert.equal(put(registry, { id: id(4), from: '203.0.113.9' }).status, 503);
  const later = 1000 + limits.newcomerWindowMs;
  const next = new Registry(limits);
  put(next, { id: id(1), from: 'a' });
  put(next, { id: id(2), from: 'b' });
  assert.equal(put(next, { id: id(3), from: 'c' }).status, 429, 'the list of networks is bounded too');
  assert.equal(put(next, { id: id(3), from: 'c', now: later }).status, 201, 'and frees itself a day later');
});

test('a tunnel that moves twice in a moment is asked to wait', () => {
  const registry = new Registry();
  put(registry);
  assert.equal(put(registry, { url: MOVED, now: 1000 + LIMITS.moveGapMs - 1 }).status, 429);
  assert.equal(put(registry, { url: MOVED, now: 1000 + LIMITS.moveGapMs }).status, 200);
});

test('saved computers come back and junk in the file is skipped', () => {
  const registry = new Registry();
  assert.equal(registry.restore({ id: ID, hash: digest(TOKEN), url: TUNNEL, at: 7 }), true);
  assert.equal(registry.restore({ id: OTHER, hash: 'nope', url: TUNNEL, at: 7 }), false);
  assert.equal(registry.restore({ id: OTHER, hash: digest(TOKEN), url: 'https://evil.example.com', at: 7 }), false);
  assert.equal(registry.restore(null), false);
  assert.deepEqual(registry.records(), [{ id: ID, hash: digest(TOKEN), url: TUNNEL, at: 7 }]);
  assert.equal(put(registry, { url: MOVED, now: 9000 }).status, 200);
});

test('the address and the host of a computer mirror each other', () => {
  assert.equal(addressOf(ID, CONFIG.zone), `https://${HOME}`);
  assert.equal(computerOf(HOME, CONFIG.zone), ID);
  assert.equal(computerOf(`${ID}.octo.evil.dev`, CONFIG.zone), null);
  assert.equal(computerOf('octo.marujo.dev', CONFIG.zone), null);
  assert.equal(computerOf('relay.octo.marujo.dev', CONFIG.zone), null);
  assert.equal(computerOf(`x.${ID}.octo.marujo.dev`, CONFIG.zone), null);
});

function start(data) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [SERVER], {
      env: { ...process.env, PORT: '0', RELAY_DATA: data },
      stdio: ['ignore', 'pipe', 'inherit'],
    });
    let seen = '';
    child.stdout.on('data', chunk => {
      seen += chunk;
      const line = seen.split('\n').find(text => text.includes('"listening"'));
      if (line) resolve({ child, port: JSON.parse(line).port });
    });
    child.on('exit', code => reject(new Error(`the relay stopped with ${code}`)));
  });
}

function stop(child) {
  return new Promise(resolve => {
    child.removeAllListeners('exit');
    child.on('exit', resolve);
    child.kill();
  });
}

function call(port, { method = 'GET', host, path = '/', token, body }) {
  return new Promise((resolve, reject) => {
    const headers = { Host: host };
    if (token) headers.Authorization = `Bearer ${token}`;
    const sent = request({ port, host: '127.0.0.1', method, path, headers }, response => {
      let text = '';
      response.on('data', chunk => (text += chunk));
      response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, body: text }));
    });
    sent.on('error', reject);
    sent.end(body);
  });
}

function upgrade(port, host) {
  return new Promise((resolve, reject) => {
    const socket = connect(port, '127.0.0.1', () => {
      socket.write(
        `GET /ws HTTP/1.1\r\nHost: ${host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n`,
      );
    });
    let text = '';
    socket.on('data', chunk => (text += chunk));
    socket.on('end', () => resolve(text));
    socket.on('error', reject);
  });
}

test('the running service registers, redirects a phone socket and remembers after a restart', async () => {
  const folder = mkdtempSync(join(tmpdir(), 'octo-relay-'));
  const data = join(folder, 'computers.jsonl');
  let relay = await start(data);
  try {
    const body = JSON.stringify({ url: TUNNEL });
    const created = await call(relay.port, { method: 'PUT', host: CONFIG.host, path: `/v1/computers/${ID}`, token: TOKEN, body });
    assert.equal(created.status, 201);
    assert.deepEqual(JSON.parse(created.body), { address: `https://${HOME}` });
    const socket = await upgrade(relay.port, HOME);
    assert.match(socket, /^HTTP\/1\.1 307 Temporary Redirect\r\n/);
    assert.match(socket, new RegExp(`\r\nLocation: ${TUNNEL}/ws\r\n`));
    assert.match(socket, /\r\nConnection: close\r\n/);
    const page = await call(relay.port, { host: HOME, path: '/' });
    assert.equal(page.status, 307);
    assert.equal(page.headers.location, `${TUNNEL}/`);
    const large = await call(relay.port, {
      method: 'PUT',
      host: CONFIG.host,
      path: `/v1/computers/${OTHER}`,
      token: TOKEN,
      body: JSON.stringify({ url: TUNNEL, pad: 'x'.repeat(4096) }),
    });
    assert.equal(large.status, 413);
    await stop(relay.child);
    assert.equal(readFileSync(data, 'utf8').trim().split('\n').length, 1);
    relay = await start(data);
    const after = await call(relay.port, { host: HOME, path: '/ws' });
    assert.equal(after.headers.location, `${TUNNEL}/ws`);
    const stolen = await call(relay.port, {
      method: 'PUT',
      host: CONFIG.host,
      path: `/v1/computers/${ID}`,
      token: THIEF,
      body: JSON.stringify({ url: MOVED }),
    });
    assert.equal(stolen.status, 403);
  } finally {
    await stop(relay.child);
    rmSync(folder, { recursive: true, force: true });
  }
});
