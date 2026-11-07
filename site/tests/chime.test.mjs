import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import * as chime from '../assets/chime.js';

const REPO = fileURLToPath(new URL('../../', import.meta.url));
const sound = readFileSync(join(REPO, 'src/sound.rs'), 'utf8');
const RATE = Number(sound.match(/pub const SAMPLE_RATE: u32 = ([\d_]+);/)[1].replace(/_/g, ''));
const rust = (name) => Number(sound.match(new RegExp(`const ${name}: f64 = ([\\d.]+);`))[1]);

test('o som do site usa as mesmas constantes do src/sound.rs', () => {
  const pairs = {
    CHIME_PEAK: 'PEAK',
    CHIME_ATTACK: 'ATTACK',
    CHIME_DECAY: 'DECAY',
    CHIME_RELEASE: 'RELEASE',
    CHIME_MODULATOR_RATIO: 'MODULATOR_RATIO',
    CHIME_MODULATION_INDEX: 'MODULATION_INDEX',
    CHIME_MODULATION_DECAY: 'MODULATION_DECAY',
  };
  for (const [site, app] of Object.entries(pairs)) assert.equal(chime[site], rust(app), site);
});

test('o som de pronto é o Chime::Done do app, duas notas que sobem', () => {
  const block = sound.match(/Chime::Done => \[([\s\S]*?)\],/)[1];
  const strikes = [...block.matchAll(/Strike \{ at: ([\d.]+), midi: ([\d.]+), gain: ([\d.]+), length: ([\d.]+) \}/g)].map(([, at, midi, gain, length]) => ({ at: Number(at), midi: Number(midi), gain: Number(gain), length: Number(length) }));
  assert.deepEqual(chime.CHIME_DONE, strikes);
  assert.ok(strikes[1].midi > strikes[0].midi, 'sobe');
});

test('o som começa do silêncio, tem o pico do app, é curto e termina em silêncio', () => {
  const samples = chime.chimeSamples(chime.CHIME_DONE, RATE);
  const seconds = samples.length / RATE;
  assert.ok(seconds > 1 && seconds < 1.3, `${seconds}s`);
  assert.equal(samples[0], 0);
  const peak = samples.reduce((top, s) => Math.max(top, Math.abs(s)), 0);
  assert.ok(Math.abs(peak - chime.CHIME_PEAK) < 1e-6, `pico ${peak}`);
  const tail = samples.slice(-200).reduce((top, s) => Math.max(top, Math.abs(s)), 0);
  assert.ok(tail < 0.01, `cauda ${tail}`);
});

test('o cuspe é curto, granulado em poucos níveis, começa e termina em silêncio e tem o volume do som de pronto', () => {
  const samples = chime.spitSamples(RATE);
  const seconds = samples.length / RATE;
  assert.ok(seconds > 0.2 && seconds < 0.5, `${seconds}s`);
  assert.equal(samples[0], 0);
  assert.equal(samples[samples.length - 1], 0);
  const peak = samples.reduce((top, s) => Math.max(top, Math.abs(s)), 0);
  assert.ok(Math.abs(peak - chime.CHIME_PEAK) < 1e-6, `pico ${peak}`);
  const levels = new Set(samples.map((s) => Math.round((s / chime.CHIME_PEAK) * 2 ** (chime.SPIT_BITS - 1))));
  assert.ok(levels.size <= 2 ** chime.SPIT_BITS + 1, `${levels.size} níveis, som de 8 bits tem poucos`);
  assert.deepEqual([...chime.spitSamples(RATE)], [...samples], 'o mesmo som toda vez');
});

test('o cuspe cai de tom: o começo vibra mais rápido que o fim do assobio', () => {
  const samples = chime.spitSamples(RATE);
  const crossings = (from, to) => {
    let count = 0;
    for (let i = Math.round(from * RATE) + 1; i < Math.round(to * RATE); i += 1) if (Math.sign(samples[i]) !== Math.sign(samples[i - 1]) && samples[i] !== 0) count += 1;
    return count / (to - from);
  };
  const half = chime.SPIT_SPLAT_AT / 2;
  assert.ok(chime.SPIT_SWEEP[0] > chime.SPIT_SWEEP[1]);
  assert.ok(crossings(0.002, half) > crossings(half, chime.SPIT_SPLAT_AT) * 1.05, 'antes do estalo o tom já está caindo');
});

test('o som do site bate amostra a amostra com a fórmula do Rust, em 16 bits', () => {
  const samples = chime.chimeSamples(chime.CHIME_DONE, RATE);
  const toI16 = (s) => Math.round(s * 32767);
  const strikes = chime.CHIME_DONE;
  const length = Math.ceil(Math.max(...strikes.map((s) => s.at + s.length)) * RATE);
  const mix = new Float64Array(length);
  for (const strike of strikes) {
    const frequency = 440 * 2 ** ((strike.midi - 69) / 12);
    const start = Math.round(strike.at * RATE);
    const count = Math.min(Math.trunc(strike.length * RATE), length - start);
    for (let i = 0; i < count; i += 1) {
      const t = i / RATE;
      const envelope = Math.min(t / rust('ATTACK'), 1) * Math.exp(-rust('DECAY') * t) * Math.min((strike.length - t) / rust('RELEASE'), 1);
      const index = rust('MODULATION_INDEX') * Math.exp(-rust('MODULATION_DECAY') * t);
      mix[start + i] += strike.gain * envelope * Math.sin(2 * Math.PI * frequency * t + index * Math.sin(2 * Math.PI * rust('MODULATOR_RATIO') * frequency * t));
    }
  }
  const peak = mix.reduce((top, s) => Math.max(top, Math.abs(s)), Number.EPSILON);
  for (let i = 0; i < length; i += 997) {
    assert.ok(Math.abs(toI16(samples[i]) - Math.round((mix[i] / peak) * rust('PEAK') * 32767)) <= 1, `amostra ${i}`);
  }
});
