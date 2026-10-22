import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { versions, describeBuild } from './version.mjs';

test('sync updates application manifests and local crates without changing dependencies', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'scope-version-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'src-tauri'));
  writeFileSync(join(root, 'Cargo.toml'), '[workspace.package]\nversion = "0.2.0"\n');
  for (const file of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
    writeFileSync(join(root, file), JSON.stringify({ version: '0.1.0', packages: { '': { version: '0.1.0' }, example: { version: '3.2.1' } } }));
  }
  const external = '\nname = "external"\nversion = "3.2.1"\n';
  writeFileSync(join(root, 'Cargo.lock'), ['harness-scope-core', 'harness-scope-service', 'harness-scope-desktop'].map(name => `[[package]]\nname = "${name}"\nversion = "0.1.0"\n`).join('') + '[[package]]' + external);
  assert.throws(() => versions(root), /Version mismatch/);
  assert.equal(versions(root, true), '0.2.0');
  assert.equal(versions(root), '0.2.0');
  assert.ok(readFileSync(join(root, 'Cargo.lock'), 'utf8').endsWith(external));
  assert.equal(JSON.parse(readFileSync(join(root, 'package-lock.json'))).packages.example.version, '3.2.1');
  const before = readFileSync(join(root, 'Cargo.lock'), 'utf8');
  versions(root, true);
  assert.equal(readFileSync(join(root, 'Cargo.lock'), 'utf8'), before);
});

test('only a clean matching tag is a release; development IDs record the build time', () => {
  const base = { version: '0.2.0', now: new Date('2026-10-23T00:01:02.003Z'), commit: 'abc123', dirty: false, tag: 'v0.2.0' };
  assert.equal(describeBuild(base).displayVersion, '0.2.0');
  for (const override of [{ dirty: true }, { dirty: null }, { tag: null }, { tag: 'v0.1.0' }]) {
    const build = describeBuild({ ...base, ...override });
    assert.equal(build.channel, 'dev');
    assert.equal(build.displayVersion, '0.2.0-dev.20261023000102003');
    assert.equal(build.gitCommit, 'abc123');
  }
});
