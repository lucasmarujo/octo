(function (root) {
  'use strict';

  const VERSION = 2;
  const UP = 'orkai-pwa/2 phone->bridge';
  const DOWN = 'orkai-pwa/2 bridge->phone';
  const SECRET_LEN = 32;
  const UNPAIRED = 4001;
  const OUTDATED = 4002;
  const subtle = root.crypto && root.crypto.subtle;
  const utf8 = new TextEncoder();
  const text = new TextDecoder();

  function encode(bytes) {
    let binary = '';
    for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i]);
    return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  }

  function decode(value) {
    const base64 = value.replace(/-/g, '+').replace(/_/g, '/');
    const binary = atob(base64 + '==='.slice((base64.length + 3) % 4));
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
    return bytes;
  }

  function concat(...parts) {
    const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
    let at = 0;
    for (const part of parts) {
      out.set(part, at);
      at += part.length;
    }
    return out;
  }

  function nonce(counter) {
    const bytes = new Uint8Array(12);
    const view = new DataView(bytes.buffer);
    view.setUint32(4, Math.floor(counter / 0x100000000));
    view.setUint32(8, counter >>> 0);
    return bytes;
  }

  function secretFrom(value) {
    try {
      const bytes = decode(value || '');
      return bytes.length === SECRET_LEN ? bytes : null;
    } catch (err) {
      return null;
    }
  }

  async function deriveKeys(secret, privateKey, phone, bridge) {
    const peer = await subtle.importKey('raw', bridge, { name: 'ECDH', namedCurve: 'P-256' }, false, []);
    const shared = await subtle.deriveBits({ name: 'ECDH', public: peer }, privateKey, 256);
    const base = await subtle.importKey('raw', shared, 'HKDF', false, ['deriveKey']);
    const derive = (label, usage) =>
      subtle.deriveKey(
        { name: 'HKDF', hash: 'SHA-256', salt: secret, info: concat(utf8.encode(label), phone, bridge) },
        base,
        { name: 'AES-GCM', length: 256 },
        false,
        [usage],
      );
    return { up: await derive(UP, 'encrypt'), down: await derive(DOWN, 'decrypt') };
  }

  class Session {
    constructor(url, secret, handlers) {
      this.handlers = handlers;
      this.secret = secret;
      this.keys = null;
      this.sent = 0;
      this.received = 0;
      this.outbox = Promise.resolve();
      this.inbox = Promise.resolve();
      this.closed = false;
      this.socket = new root.WebSocket(url);
      this.socket.binaryType = 'arraybuffer';
      this.socket.onopen = () => this.hello().catch((err) => this.fail(err));
      this.socket.onmessage = (event) => this.receive(event.data);
      this.socket.onclose = (event) => this.finish(event.code, event.reason);
      this.socket.onerror = () => {};
    }

    async hello() {
      this.pair = await subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits']);
      this.own = new Uint8Array(await subtle.exportKey('raw', this.pair.publicKey));
      this.socket.send(JSON.stringify({ type: 'hello', version: VERSION, key: encode(this.own) }));
    }

    receive(data) {
      if (typeof data === 'string') {
        if (!this.keys) this.greet(data).catch((err) => this.fail(err));
        return;
      }
      const counter = this.received;
      this.received += 1;
      const frame = new Uint8Array(data);
      this.inbox = this.inbox
        .then(async () => {
          const plain = await subtle.decrypt({ name: 'AES-GCM', iv: nonce(counter) }, this.keys.down, frame);
          return JSON.parse(text.decode(plain));
        })
        .then((message) => {
          if (!this.closed) this.handlers.message(message);
        })
        .catch((err) => this.fail(err));
    }

    async greet(data) {
      const greeting = JSON.parse(data);
      if (greeting.type !== 'hello' || greeting.version !== VERSION) {
        this.close(OUTDATED);
        return;
      }
      this.keys = await deriveKeys(this.secret, this.pair.privateKey, this.own, decode(greeting.key));
      this.send({ type: 'sync' });
      if (this.handlers.ready) this.handlers.ready();
    }

    send(message) {
      if (this.closed || !this.keys) return;
      const counter = this.sent;
      this.sent += 1;
      const plain = utf8.encode(JSON.stringify(message));
      this.outbox = this.outbox
        .then(async () => {
          const sealed = await subtle.encrypt({ name: 'AES-GCM', iv: nonce(counter) }, this.keys.up, plain);
          if (!this.closed && this.socket.readyState === 1) this.socket.send(sealed);
        })
        .catch((err) => this.fail(err));
    }

    fail(err) {
      if (this.handlers.error) this.handlers.error(err);
      this.close();
    }

    close(code) {
      if (this.closed) return;
      this.socket.close();
      this.finish(code || 1000, '');
    }

    finish(code, reason) {
      if (this.closed) return;
      this.closed = true;
      if (this.handlers.close) this.handlers.close(code, reason || '');
    }
  }

  const api = { VERSION, UNPAIRED, OUTDATED, Session, encode, decode, nonce, secretFrom, supported: Boolean(subtle) };
  root.OrkaiWire = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
