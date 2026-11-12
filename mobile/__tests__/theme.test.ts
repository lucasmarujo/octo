import { ACCENT_DEFAULT, ACCENT_PRESETS } from '../src/core/accent';
import { DEFAULT_THEME, resolveTheme, THEME_CHOICES } from '../src/core/model';
import { readSettings } from '../src/core/settings';
import { hueAt } from '../src/sheets/Appearance';
import { THEMES, tinted } from '../src/theme/tokens';
import { markColors } from '../src/ui/Brand';

declare const __dirname: string;

const { readFileSync } = jest.requireActual<{ readFileSync(path: string, encoding: 'utf8'): string }>('fs');
const { join } = jest.requireActual<{ join(...segments: string[]): string }>('path');

jest.mock('../src/core/storage', () => ({ load: jest.fn(async () => null), save: jest.fn(async () => {}) }));

describe('the themes of the phone', () => {
  it('open in Octo, black with the aqua accent, and keep the look of before as Old', () => {
    expect(THEME_CHOICES).toEqual(['octo', 'gray', 'light', 'old', 'system']);
    expect(DEFAULT_THEME).toBe('octo');
    expect(THEMES.octo).toMatchObject({ bg: '#000000', surface: '#0a0b0c', surfaceRaised: '#141718', border: '#1a1d1f', accent: '#35fddc', accentInk: '#04201c', link: '#7dfee8', termBg: '#000000' });
    expect(THEMES.old).toMatchObject({ bg: '#0e0e13', accent: '#e85eb6', link: '#6e96ff' });
    expect(THEMES.gray.accent).toBe(THEMES.octo.accent);
    expect(THEMES.light).toMatchObject({ light: true, accent: '#12b8a0' });
  });

  it('are the ones of Octo on the computer', () => {
    const css = readFileSync(join(__dirname, '..', '..', 'desktop', 'ui', 'styles.css'), 'utf8');
    const root = css.split('\n:root {')[1].split('\n}')[0];
    const token = (name: string) => new RegExp(`--${name}: (#[0-9a-f]{6});`).exec(root)![1];
    expect(THEMES.octo).toMatchObject({ bg: token('color-bg'), surface: token('color-surface'), surfaceRaised: token('color-surface-raised'), border: token('color-border'), borderStrong: token('color-border-strong'), accent: token('color-accent'), accentInk: token('color-accent-ink'), link: token('color-link') });
    expect(ACCENT_DEFAULT).toBe(THEMES.octo.accent);
  });

  it('follow the system between Octo and Light', () => {
    expect(resolveTheme('system', false)).toBe('octo');
    expect(resolveTheme('system', true)).toBe('light');
    expect(resolveTheme('old', true)).toBe('old');
  });

  it('move everyone who had the dark theme of before to Octo, and forget a color that is not one', () => {
    expect(readSettings({ theme: 'dark' as never })).toMatchObject({ theme: 'octo', accent: null });
    expect(readSettings({ theme: 'old', accent: '#FF8A4C' })).toMatchObject({ theme: 'old', accent: '#ff8a4c' });
    expect(readSettings({ accent: 'orange' })).toMatchObject({ accent: null });
    expect(readSettings(null)).toMatchObject({ theme: 'octo', accent: null, layout: 'list', haptics: true, reduced: false });
  });
});

describe('the accent color', () => {
  it('repaints the accent, its ink and the links of every theme but Old', () => {
    const orange = tinted(THEMES.octo, '#ff8a4c');
    expect(orange).toMatchObject({ name: 'octo', accent: '#ff8a4c', accentInk: '#0b0c0d', bg: '#000000' });
    expect(orange.link).not.toBe(THEMES.octo.link);
    expect(tinted(THEMES.light, '#8b7bff').accent).toBe('#8b7bff');
    expect(tinted(THEMES.old, '#ff8a4c')).toBe(THEMES.old);
    expect(tinted(THEMES.octo, null)).toBe(THEMES.octo);
  });

  it('offers the same presets as the computer, the first being the color of Octo', () => {
    expect(ACCENT_PRESETS[0]).toBe(ACCENT_DEFAULT);
    expect(ACCENT_PRESETS).toHaveLength(7);
  });

  it('reads a hue from where the finger is on the slider, without leaving it', () => {
    expect(hueAt(0, 300)).toBe(0);
    expect(hueAt(150, 300)).toBe(180);
    expect(hueAt(-40, 300)).toBe(0);
    expect(hueAt(900, 300)).toBe(0);
    expect(hueAt(120, 0)).toBe(0);
  });

  it('paints the O of the app: a lighter glow, a darker shade and the crest behind', () => {
    expect(markColors('#35fddc')).toEqual({ crest: 'rgb(22, 106, 92)', glow: 'rgb(194, 254, 245)', shade: 'rgb(35, 167, 145)' });
    expect(markColors('#ff8a4c').crest).not.toBe(markColors('#35fddc').crest);
  });
});

describe('the icon of the app', () => {
  it('is the O on black, in the stores, on the home screen and in the notifications', () => {
    const app = JSON.parse(readFileSync(join(__dirname, '..', 'app.json'), 'utf8')).expo;
    expect(app.backgroundColor).toBe('#000000');
    expect(app.android.adaptiveIcon).toEqual({ backgroundColor: '#0b0c0d', foregroundImage: './assets/android-icon-foreground.png' });
    expect(app.plugins).toContainEqual(['expo-splash-screen', { image: './assets/splash-icon.png', imageWidth: 128, backgroundColor: '#000000' }]);
  });
});
