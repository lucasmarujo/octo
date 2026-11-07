export const CHIME_PEAK = 0.45;
export const CHIME_ATTACK = 0.002;
export const CHIME_DECAY = 3.1;
export const CHIME_RELEASE = 0.2;
export const CHIME_MODULATOR_RATIO = 3.5;
export const CHIME_MODULATION_INDEX = 2.1;
export const CHIME_MODULATION_DECAY = 6.0;
export const CHIME_DONE = [{ at: 0.0, midi: 84.0, gain: 1.0, length: 1.1 }, { at: 0.075, midi: 88.0, gain: 1.0, length: 1.1 }];
export const SPIT_LENGTH = 0.38;
export const SPIT_SWEEP = [880, 110];
export const SPIT_SWEEP_LENGTH = 0.16;
export const SPIT_TONE_DECAY = 16;
export const SPIT_SPLAT_AT = 0.03;
export const SPIT_SPLAT_DECAY = 12;
export const SPIT_HOLD_HZ = 6000;
export const SPIT_BITS = 4;
export const SPIT_FADE = 0.02;

export function spitSamples(rate) {
  const out = new Float32Array(Math.ceil(SPIT_LENGTH * rate));
  const hold = Math.max(1, Math.round(rate / SPIT_HOLD_HZ));
  let seed = 1;
  let phase = 0;
  let grain = 0;
  for (let i = 0; i < out.length; i += 1) {
    const t = i / rate;
    const sweep = Math.min(t / SPIT_SWEEP_LENGTH, 1);
    phase = (phase + (SPIT_SWEEP[0] * (SPIT_SWEEP[1] / SPIT_SWEEP[0]) ** sweep) / rate) % 1;
    const tone = (phase < 0.5 ? 1 : -1) * Math.min(t / 0.004, 1) * Math.exp(-SPIT_TONE_DECAY * t);
    if (i % hold === 0) {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      grain = seed / 1073741824 - 1;
    }
    const splat = t < SPIT_SPLAT_AT ? 0 : grain * Math.exp(-SPIT_SPLAT_DECAY * (t - SPIT_SPLAT_AT));
    out[i] = (tone * 0.6 + splat) * Math.min((out.length - 1 - i) / (SPIT_FADE * rate), 1);
  }
  const peak = out.reduce((top, sample) => Math.max(top, Math.abs(sample)), Number.EPSILON);
  const levels = 2 ** (SPIT_BITS - 1);
  return out.map((sample) => (Math.round((sample / peak) * levels) / levels) * CHIME_PEAK);
}

export function chimeSamples(strikes, rate) {
  const length = strikes.reduce((end, strike) => Math.max(end, strike.at + strike.length), 0);
  const mix = new Float32Array(Math.ceil(length * rate));
  for (const strike of strikes) {
    const frequency = 440 * 2 ** ((strike.midi - 69) / 12);
    const start = Math.round(strike.at * rate);
    const count = Math.min(Math.floor(strike.length * rate), mix.length - start);
    for (let i = 0; i < count; i += 1) {
      const t = i / rate;
      const envelope = Math.min(t / CHIME_ATTACK, 1) * Math.exp(-CHIME_DECAY * t) * Math.min((strike.length - t) / CHIME_RELEASE, 1);
      const index = CHIME_MODULATION_INDEX * Math.exp(-CHIME_MODULATION_DECAY * t);
      const modulator = Math.sin(2 * Math.PI * CHIME_MODULATOR_RATIO * frequency * t);
      mix[start + i] += strike.gain * envelope * Math.sin(2 * Math.PI * frequency * t + index * modulator);
    }
  }
  const peak = mix.reduce((top, sample) => Math.max(top, Math.abs(sample)), Number.EPSILON);
  return mix.map((sample) => (sample / peak) * CHIME_PEAK);
}
