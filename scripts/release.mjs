import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ROOT, versions } from './version.mjs';

const platforms = ['windows', 'linux', 'macos'];
const targets = {
  windows: ['x86_64-pc-windows-msvc'],
  linux: ['x86_64-unknown-linux-gnu'],
  macos: ['aarch64-apple-darwin', 'x86_64-apple-darwin'],
};
const git = (...args) => execFileSync('git', args, { cwd: ROOT, encoding: 'utf8' }).trim();
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');

export function expectedAssets(platform, tag) {
  assert.match(tag, /^v\d+\.\d+\.\d+$/);
  const prefix = `harness-scope-${tag}`;
  const assets = {
    windows: [`${prefix}-windows-x86_64.exe`],
    linux: [`${prefix}-linux-x86_64.AppImage`, `${prefix}-linux-amd64.deb`, `${prefix}-service-linux-x86_64.tar.gz`],
    macos: [`${prefix}-macos-universal.dmg`],
  };
  assert.ok(assets[platform], `Unknown platform: ${platform}`);
  return assets[platform];
}

export function validateBuildInfo(info, { tag, commit, target }) {
  assert.equal(info.version, tag.slice(1), 'Build version mismatch');
  assert.equal(info.displayVersion, tag.slice(1), 'Development build cannot be published');
  assert.equal(info.channel, 'release', 'Build is not tagged as a release');
  assert.equal(info.dirty, false, 'Build workspace was not clean');
  assert.equal(info.profile, 'release', 'Build is not optimized');
  assert.equal(info.gitCommit, commit, 'Build commit mismatch');
  assert.equal(info.target, target, 'Build architecture mismatch');
}

export function validateManifest(manifest, { tag, commit, platform, directory }) {
  assert.equal(manifest.tag, tag);
  assert.equal(manifest.commit, commit);
  assert.equal(manifest.platform, platform);
  assert.deepEqual(manifest.assets.map(a => a.name).sort(), expectedAssets(platform, tag).sort());
  assert.deepEqual(manifest.builds.map(b => b.target).sort(), [...targets[platform]].sort());
  for (const build of manifest.builds) validateBuildInfo(build, { tag, commit, target: build.target });
  for (const asset of manifest.assets) {
    const path = join(directory, asset.name);
    assert.equal(statSync(path).size, asset.bytes, `Size mismatch: ${asset.name}`);
    assert.equal(hash(path), asset.sha256, `Checksum mismatch: ${asset.name}`);
  }
}

function context() {
  const tag = process.env.RELEASE_TAG;
  assert.match(tag ?? '', /^v\d+\.\d+\.\d+$/, 'Set RELEASE_TAG to vX.Y.Z');
  assert.equal(tag, `v${versions()}`, 'Tag must match all version manifests');
  const commit = git('rev-parse', 'HEAD');
  if (process.env.GITHUB_SHA) assert.equal(commit, process.env.GITHUB_SHA);
  return { tag, commit };
}

function prepare() {
  const { tag, commit } = context();
  assert.equal(git('status', '--porcelain', '--untracked-files=normal'), '', 'Release checkout must be clean');
  if (git('tag', '--list', tag)) {
    assert.equal(git('rev-list', '-n', '1', tag), commit, 'Existing tag belongs to another commit');
  } else {
    // Local build identity only. The remote tag is created after all validation passes.
    git('tag', tag, commit);
  }
  console.log(`Prepared ${tag} at ${commit}; no remote tag or release was changed.`);
}

function oneFile(directory, suffix) {
  const files = readdirSync(directory).filter(name => name.endsWith(suffix));
  assert.equal(files.length, 1, `Expected one ${suffix} in ${directory}`);
  return join(directory, files[0]);
}

function buildInfo(target, ctx) {
  const base = target.endsWith('apple-darwin') ? join(ROOT, 'target', target) : join(ROOT, 'target');
  const directory = join(base, 'release', 'build');
  const matches = [];
  for (const name of readdirSync(directory).filter(n => n.startsWith('harness-scope-core-'))) {
    const path = join(directory, name, 'out', 'build-info.json');
    if (!existsSync(path)) continue;
    const info = JSON.parse(readFileSync(path, 'utf8'));
    try { validateBuildInfo(info, { ...ctx, target }); matches.push(info); } catch { /* Ignore cached builds from other revisions. */ }
  }
  assert.ok(matches.length, `No verified release build metadata for ${target}`);
  return matches.sort((a, b) => b.builtAt.localeCompare(a.builtAt))[0];
}

function stage(platform) {
  const ctx = context();
  assert.equal(git('status', '--porcelain', '--untracked-files=normal'), '', 'Build modified the checkout');
  const names = expectedAssets(platform, ctx.tag);
  const builds = targets[platform].map(target => buildInfo(target, ctx));
  const directory = join(ROOT, 'output', 'release-stage');
  mkdirSync(directory, { recursive: true });
  if (platform === 'windows') {
    copyFileSync(join(ROOT, 'target', 'release', 'harness-scope-desktop.exe'), join(directory, names[0]));
  } else if (platform === 'linux') {
    const bundles = join(ROOT, 'target', 'release', 'bundle');
    copyFileSync(oneFile(join(bundles, 'appimage'), '.AppImage'), join(directory, names[0]));
    copyFileSync(oneFile(join(bundles, 'deb'), '.deb'), join(directory, names[1]));
    execFileSync('tar', ['-czf', join(directory, names[2]), '-C', join(ROOT, 'target', 'release'), 'harness-scope-service']);
  } else {
    copyFileSync(oneFile(join(ROOT, 'target', 'universal-apple-darwin', 'release', 'bundle', 'dmg'), '.dmg'), join(directory, names[0]));
  }
  const assets = names.map(name => ({ name, bytes: statSync(join(directory, name)).size, sha256: hash(join(directory, name)) }));
  const manifest = { ...ctx, platform, builds, assets };
  validateManifest(manifest, { ...ctx, platform, directory });
  writeFileSync(join(directory, `build-manifest-${platform}.json`), JSON.stringify(manifest, null, 2) + '\n');
  console.log(JSON.stringify(manifest, null, 2));
}

function collect() {
  const ctx = context();
  const source = join(ROOT, 'output', 'release-stage');
  const destination = join(ROOT, 'output', 'release-assets');
  const manifests = platforms.map(platform => {
    const manifest = JSON.parse(readFileSync(join(source, `build-manifest-${platform}.json`), 'utf8'));
    validateManifest(manifest, { ...ctx, platform, directory: source });
    return manifest;
  });
  mkdirSync(destination, { recursive: true });
  assert.equal(readdirSync(destination).length, 0, 'Asset destination must be empty');
  for (const manifest of manifests) {
    for (const asset of manifest.assets) copyFileSync(join(source, asset.name), join(destination, asset.name));
  }
  writeFileSync(join(destination, 'build-manifest.json'), JSON.stringify({ ...ctx, platforms: manifests }, null, 2) + '\n');
  const licenses = ['LICENSE', 'third_party/CC-Switch-LICENSE', 'third_party/UPSTREAM.md'];
  writeFileSync(join(destination, 'LICENSES.txt'), licenses.map(path => `${path}\n${'='.repeat(path.length)}\n${readFileSync(join(ROOT, path), 'utf8')}`).join('\n\n'));
  const checksums = readdirSync(destination).sort().map(name => `${hash(join(destination, name))}  ${basename(name)}`);
  writeFileSync(join(destination, 'SHA256SUMS.txt'), checksums.join('\n') + '\n');
  console.log(`Verified ${manifests.length} platforms and ${checksums.length} assets for ${ctx.tag} at ${ctx.commit}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  if (command === 'prepare') prepare();
  else if (command === 'stage') stage(process.argv[3]);
  else if (command === 'collect') collect();
  else throw new Error('Usage: node scripts/release.mjs prepare|stage <windows|linux|macos>|collect');
}
