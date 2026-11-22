import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { appendFile, rename, writeFile } from 'node:fs/promises';
import { createServer, STATUS_CODES } from 'node:http';
import { dirname } from 'node:path';

import { Registry, respond } from './relay.mjs';

const PORT = Number(process.env.PORT || 8080);
const DATA = process.env.RELAY_DATA || '/data/computers.jsonl';
const CONFIG = {
  host: (process.env.RELAY_HOST || 'octo-relay.marujo.dev').toLowerCase(),
  zone: (process.env.RELAY_ZONE || 'octo.marujo.dev').toLowerCase(),
};
const MAX_BODY_BYTES = 1024;
const HEAD_TIMEOUT_MS = 10000;
const COMPACT_AFTER = 1000;
const FORWARDED = 'x-forwarded-for';

const registry = new Registry();
let appended = 0;
let writing = Promise.resolve();

function lines() {
  return registry.records().map(record => `${JSON.stringify(record)}\n`).join('');
}

function load() {
  mkdirSync(dirname(DATA), { recursive: true });
  let saved = '';
  try {
    saved = readFileSync(DATA, 'utf8');
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  for (const line of saved.split('\n')) {
    if (!line) continue;
    try {
      registry.restore(JSON.parse(line));
    } catch {
      continue;
    }
  }
  writeFileSync(`${DATA}.tmp`, lines());
  renameSync(`${DATA}.tmp`, DATA);
}

function keep(record) {
  writing = writing.catch(() => {}).then(async () => {
    await appendFile(DATA, `${JSON.stringify(record)}\n`);
    appended += 1;
    if (appended < Math.max(COMPACT_AFTER, registry.size)) return;
    await writeFile(`${DATA}.tmp`, lines());
    await rename(`${DATA}.tmp`, DATA);
    appended = 0;
  });
  return writing;
}

function caller(request) {
  const forwarded = String(request.headers[FORWARDED] || '').split(',').pop().trim();
  return forwarded || request.socket.remoteAddress || '';
}

function asked(request, body) {
  return {
    method: request.method,
    host: request.headers.host,
    target: request.url,
    authorization: request.headers.authorization,
    from: caller(request),
    now: Date.now(),
    body,
  };
}

function read(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    request.on('data', chunk => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        reject(new Error('too large'));
        request.removeAllListeners('data');
        request.resume();
        return;
      }
      chunks.push(chunk);
    });
    request.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    request.on('error', reject);
  });
}

async function serve(request, response) {
  let body = '';
  try {
    body = await read(request);
  } catch {
    response.writeHead(413, { 'Content-Type': 'text/plain; charset=utf-8', Connection: 'close' }).end('too large');
    return;
  }
  const answer = respond(registry, CONFIG, asked(request, body));
  if (answer.record) {
    try {
      await keep(answer.record);
    } catch (error) {
      console.error(JSON.stringify({ event: 'not-kept', error: String(error) }));
      response.writeHead(503, { 'Content-Type': 'text/plain; charset=utf-8' }).end('could not keep the address');
      return;
    }
    if (answer.outcome === 'created') console.log(JSON.stringify({ event: 'created', computers: registry.size }));
  }
  response.writeHead(answer.status, answer.headers).end(request.method === 'HEAD' ? '' : answer.body);
}

function upgrade(request, socket) {
  const answer = respond(registry, CONFIG, asked(request, ''));
  const headers = { ...answer.headers, 'Content-Length': Buffer.byteLength(answer.body), Connection: 'close' };
  const head = Object.entries(headers).map(([name, value]) => `${name}: ${value}\r\n`).join('');
  socket.on('error', () => {});
  socket.end(`HTTP/1.1 ${answer.status} ${STATUS_CODES[answer.status]}\r\n${head}\r\n${answer.body}`);
}

load();
const server = createServer((request, response) => {
  serve(request, response).catch(() => response.destroy());
});
server.on('upgrade', upgrade);
server.headersTimeout = HEAD_TIMEOUT_MS;
server.requestTimeout = HEAD_TIMEOUT_MS;
server.listen(PORT, () => console.log(JSON.stringify({ event: 'listening', port: server.address().port, computers: registry.size })));
for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => writing.finally(() => process.exit(0)));
