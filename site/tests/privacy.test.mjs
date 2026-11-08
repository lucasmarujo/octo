import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const REPO = join(ROOT, '..');
const read = (file) => readFileSync(join(ROOT, file), 'utf8');
const rust = (file) => readFileSync(join(REPO, file), 'utf8');
const page = read('privacy/index.html');
const hosts = (source) => [...source.matchAll(/"https:\/\/([a-z0-9.-]+)[^"]*"/g)].map(([, host]) => host);

test('the privacy policy names every host Octo asks for plan usage', () => {
  const usage = [...rust('src/usage.rs').matchAll(/^pub const [A-Z_]+_URL: &str = "https:\/\/([a-z0-9.-]+)/gm)].map(([, host]) => host);
  assert.ok(usage.length >= 6, usage.join(', '));
  for (const host of new Set(usage)) assert.ok(page.includes(`<code>${host}</code>`), host);
});

test('the privacy policy names every host Octo asks for the version of an agent', () => {
  const versions = hosts(rust('desktop/src/agent_updates.rs').split('#[cfg(test)]')[0]).filter((host) => !host.endsWith('.run.app'));
  assert.ok(versions.length >= 5, versions.join(', '));
  for (const host of new Set(versions)) assert.ok(page.includes(`<code>${host}</code>`), host);
  assert.match(rust('desktop/src/agent_updates.rs'), /antigravity-cli-auto-updater-[\w-]+\.us-central1\.run\.app/);
  assert.match(page, /Google's update server for Antigravity, whose address names your operating system and processor type/);
});

test('the privacy note of both READMEs points at the policy and names the same agent version hosts', () => {
  const versions = hosts(rust('desktop/src/agent_updates.rs').split('#[cfg(test)]')[0]).filter((host) => !host.endsWith('.run.app'));
  for (const file of ['README.md', 'README.pt-BR.md']) {
    const readme = rust(file);
    assert.ok(readme.includes('[octo.marujo.dev/privacy](https://octo.marujo.dev/privacy/)'), file);
    for (const host of new Set(versions)) assert.ok(readme.includes(`\`${host}\``), `${file}: ${host}`);
  }
});

test('the privacy policy says the Store Octo leaves its updates to the Store, as the app does', () => {
  assert.match(rust('desktop/src/updates.rs'), /&& !from_store\(\)/);
  assert.match(page, /The Octo from the Microsoft Store never does this: the Microsoft Store installs its updates\./);
  assert.match(page, /no telemetry, no analytics, no ads and no crash reporting/);
});

test('the privacy policy lives where the Store listing points and the site links to it', () => {
  assert.ok(existsSync(join(ROOT, 'privacy/index.html')), 'nginx serves /privacy from privacy/index.html');
  assert.match(page, /<link rel="canonical" href="https:\/\/octo\.marujo\.dev\/privacy\/">/);
  assert.match(read('index.html'), /<a href="\/privacy\/">privacy policy<\/a>/);
  assert.match(read('sitemap.xml'), /<loc>https:\/\/octo\.marujo\.dev\/privacy\/<\/loc>/);
  assert.match(page, /<p class="legal-date">Last updated on [A-Z][a-z]+ \d{1,2}, \d{4}<\/p>/);
});

test('the privacy policy keeps to the content security policy of the site', () => {
  assert.doesNotMatch(page, /<script/);
  assert.doesNotMatch(page, /\sstyle="/);
  assert.doesNotMatch(page, /<style/);
  const local = [...page.matchAll(/(?:src|href)="(\/[^"#]*)"/g)].map(([, path]) => path).filter((path) => path !== '/' && path !== '/privacy/');
  for (const path of local) assert.ok(existsSync(join(ROOT, path)), path);
});
