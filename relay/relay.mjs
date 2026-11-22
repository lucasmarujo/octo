import { createHash, timingSafeEqual } from 'node:crypto';

export const LIMITS = {
  computers: 200000,
  newcomers: 20,
  newcomerWindowMs: 24 * 60 * 60 * 1000,
  addresses: 20000,
  moveGapMs: 1000,
  targetLength: 2048,
};

export const HEALTH_BODY = 'octo-relay';
const ID = /^[0-9a-f]{32}$/;
const TOKEN = /^[0-9a-f]{64}$/;
const TUNNEL = /^https:\/\/[a-z0-9]+(-[a-z0-9]+)*\.trycloudflare\.com$/;
const TARGET = /^\/[\x21-\x7e]*$/;
const COMPUTERS = /^\/v1\/computers\/([^/?#]+)$/;
const BEARER = /^Bearer ([0-9a-f]+)$/;
const HASH = /^[0-9a-f]{64}$/;
const HEALTH = '/health';
const ASK = '/ask';
const TEXT = { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' };
const JSON_TYPE = { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' };
const REFUSED = {
  invalid: [400, 'the id, the token or the tunnel address is not valid'],
  forbidden: [403, 'this address belongs to another computer'],
  hurried: [429, 'the address moved a moment ago, try again shortly'],
  crowded: [429, 'too many new computers from this network today'],
  full: [503, 'no room for new computers'],
};

export function digest(token) {
  return createHash('sha256').update(token).digest('hex');
}

function same(left, right) {
  const a = Buffer.from(left, 'hex');
  const b = Buffer.from(right, 'hex');
  return a.length === b.length && timingSafeEqual(a, b);
}

export class Registry {
  constructor(limits = LIMITS) {
    this.limits = limits;
    this.computers = new Map();
    this.arrivals = new Map();
  }

  get size() {
    return this.computers.size;
  }

  records() {
    return [...this.computers].map(([id, computer]) => ({ id, ...computer }));
  }

  restore(record) {
    if (!record || !ID.test(record.id) || !HASH.test(record.hash) || !TUNNEL.test(record.url)) return false;
    this.computers.set(record.id, { hash: record.hash, url: record.url, at: Number(record.at) || 0 });
    return true;
  }

  find(id) {
    return this.computers.get(id) || null;
  }

  publish({ id, token, url, from, now }) {
    if (!ID.test(id) || !TOKEN.test(token) || !TUNNEL.test(url)) return { outcome: 'invalid' };
    const known = this.computers.get(id);
    if (known) {
      if (!same(known.hash, digest(token))) return { outcome: 'forbidden' };
      if (known.url === url) return { outcome: 'kept' };
      if (now - known.at < this.limits.moveGapMs) return { outcome: 'hurried' };
      const moved = { hash: known.hash, url, at: now };
      this.computers.set(id, moved);
      return { outcome: 'moved', record: { id, ...moved } };
    }
    if (this.computers.size >= this.limits.computers) return { outcome: 'full' };
    if (!this.admit(from, now)) return { outcome: 'crowded' };
    const created = { hash: digest(token), url, at: now };
    this.computers.set(id, created);
    return { outcome: 'created', record: { id, ...created } };
  }

  admit(from, now) {
    const seen = this.arrivals.get(from);
    if (seen && now < seen.until) {
      if (seen.count >= this.limits.newcomers) return false;
      seen.count += 1;
      return true;
    }
    if (!seen && this.arrivals.size >= this.limits.addresses) {
      for (const [address, arrival] of this.arrivals) if (now >= arrival.until) this.arrivals.delete(address);
      if (this.arrivals.size >= this.limits.addresses) return false;
    }
    this.arrivals.set(from, { count: 1, until: now + this.limits.newcomerWindowMs });
    return true;
  }
}

export function hostOf(header) {
  return String(header || '').toLowerCase().replace(/:\d+$/, '');
}

export function addressOf(id, zone) {
  return `https://${id}.${zone}`;
}

export function computerOf(host, zone) {
  const suffix = `.${zone}`;
  if (!host.endsWith(suffix)) return null;
  const id = host.slice(0, host.length - suffix.length);
  return ID.test(id) ? id : null;
}

function text(status, body) {
  return { status, headers: TEXT, body };
}

function pathOf(target) {
  return String(target || '').split(/[?#]/)[0];
}

function register(registry, config, request, id) {
  const bearer = BEARER.exec(String(request.authorization || ''));
  let url = null;
  try {
    url = JSON.parse(request.body || 'null')?.url;
  } catch {
    url = null;
  }
  const result = registry.publish({
    id,
    token: bearer ? bearer[1] : '',
    url: typeof url === 'string' ? url : '',
    from: request.from,
    now: request.now,
  });
  const refused = REFUSED[result.outcome];
  if (refused) return text(refused[0], refused[1]);
  return {
    status: result.outcome === 'created' ? 201 : 200,
    headers: JSON_TYPE,
    body: JSON.stringify({ address: addressOf(id, config.zone) }),
    record: result.record,
    outcome: result.outcome,
  };
}

function redirect(registry, config, request, id) {
  const computer = registry.find(id);
  if (!computer) return text(404, 'no computer lives at this address');
  const target = String(request.target || '/');
  const kept = TARGET.test(target) && target.length <= registry.limits.targetLength ? target : '/';
  return { status: 307, headers: { Location: `${computer.url}${kept}`, 'Cache-Control': 'no-store' }, body: '' };
}

function ask(registry, config, request) {
  const domain = new URLSearchParams(String(request.target).split('?')[1] || '').get('domain') || '';
  const id = computerOf(hostOf(domain), config.zone);
  return id && registry.find(id) ? text(200, 'known') : text(404, 'unknown');
}

export function respond(registry, config, request) {
  const host = hostOf(request.host);
  const path = pathOf(request.target);
  const reads = request.method === 'GET' || request.method === 'HEAD';
  const id = computerOf(host, config.zone);
  if (id) return redirect(registry, config, request, id);
  if (reads && path === HEALTH) return text(200, HEALTH_BODY);
  if (host === config.host) {
    const computer = COMPUTERS.exec(path);
    if (!computer) return text(404, 'not found');
    if (request.method !== 'PUT') return text(405, 'method not allowed');
    return register(registry, config, request, computer[1]);
  }
  if (reads && path === ASK) return ask(registry, config, request);
  return text(404, 'not found');
}
