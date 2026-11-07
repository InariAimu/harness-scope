import { test } from 'node:test';
import assert from 'node:assert/strict';
import { expectedAssets, validateBuildInfo } from './release.mjs';

const context = { tag: 'v0.2.0', commit: 'abc123', target: 'x86_64-pc-windows-msvc' };
const build = { version: '0.2.0', displayVersion: '0.2.0', channel: 'release', dirty: false, profile: 'release', gitCommit: 'abc123', target: context.target };

test('publication rejects development, dirty, wrong-commit and wrong-architecture builds', () => {
  assert.doesNotThrow(() => validateBuildInfo(build, context));
  for (const change of [{ channel: 'dev' }, { dirty: true }, { dirty: null }, { gitCommit: 'other' }, { target: 'aarch64-apple-darwin' }, { profile: 'debug' }, { displayVersion: '0.2.0-dev.123' }, { version: '0.1.0' }]) {
    assert.throws(() => validateBuildInfo({ ...build, ...change }, context));
  }
});

test('the release requires all desktop platforms and the separate Linux server', () => {
  assert.deepEqual(expectedAssets('linux', 'v0.2.0'), [
    'harness-scope-v0.2.0-linux-x86_64.AppImage',
    'harness-scope-v0.2.0-linux-amd64.deb',
    'harness-scope-v0.2.0-service-linux-x86_64.tar.gz',
  ]);
  assert.equal(expectedAssets('windows', 'v0.2.0').length, 1);
  assert.deepEqual(expectedAssets('macos', 'v0.2.0'), ['harness-scope-v0.2.0-macos-universal.dmg']);
});

test('only supported platforms and stable version tags become filenames', () => {
  for (const tag of ['../../v0.2.0', 'v0.2.0-rc.1', 'v0.2.0; echo unsafe', '0.2.0']) {
    assert.throws(() => expectedAssets('linux', tag));
  }
  assert.throws(() => expectedAssets('unknown', 'v0.2.0'));
});
