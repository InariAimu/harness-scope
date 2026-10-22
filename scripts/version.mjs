import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crates = ['harness-scope-core', 'harness-scope-service', 'harness-scope-desktop'];
export function readVersion(root = ROOT) {
  const section = readFileSync(resolve(root, 'Cargo.toml'), 'utf8').match(/\[workspace\.package\]([^]*?)(?=\n\[|$)/)?.[1];
  const version = section?.match(/^version\s*=\s*"(\d+\.\d+\.\d+)"/m)?.[1];
  if (!version) throw new Error('Missing workspace.package.version (X.Y.Z) in Cargo.toml');
  return version;
}
export function versions(root = ROOT, sync = false) {
  const version = readVersion(root);
  const errors = [];
  for (const file of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
    const path = resolve(root, file);
    const value = JSON.parse(readFileSync(path, 'utf8'));
    if (value.version !== version || (file === 'package-lock.json' && value.packages[''].version !== version)) errors.push(file);
    if (sync) {
      value.version = version;
      if (file === 'package-lock.json') value.packages[''].version = version;
      writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
    }
  }
  const lockPath = resolve(root, 'Cargo.lock');
  const lock = readFileSync(lockPath, 'utf8');
  const updated = lock.split('[[package]]').map((block) => {
    const name = block.match(/^name = "([^"]+)"/m)?.[1];
    if (!crates.includes(name)) return block;
    if (block.match(/^version = "([^"]+)"/m)?.[1] !== version) errors.push(`Cargo.lock:${name}`);
    return block.replace(/^version = "[^"]+"/m, `version = "${version}"`);
  }).join('[[package]]');
  if (sync) writeFileSync(lockPath, updated);
  if (!sync && errors.length) throw new Error(`Version mismatch: ${errors.join(', ')}. Run npm run version:sync.`);
  return version;
}
function git(args, root) {
  try { return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); }
  catch { return null; }
}
export function describeBuild({ version, now = new Date(), commit = null, dirty = null, tag = null }) {
  const channel = dirty === false && tag === `v${version}` ? 'release' : 'dev';
  const buildId = now.toISOString().replace(/[-:.TZ]/g, '');
  return { version, channel, buildId, builtAt: now.toISOString(), gitCommit: commit, dirty,
    displayVersion: channel === 'release' ? version : `${version}-dev.${buildId}` };
}
export function frontendBuild(root = ROOT) {
  versions(root);
  const status = git(['status', '--porcelain', '--untracked-files=normal'], root);
  return describeBuild({ version: readVersion(root), commit: git(['rev-parse', 'HEAD'], root),
    dirty: status === null ? null : status.length > 0, tag: git(['tag', '--points-at', 'HEAD', '--list', `v${readVersion(root)}`], root) });
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log(`Version ${versions(ROOT, process.argv.includes('--sync'))}: ${process.argv.includes('--sync') ? 'synchronized' : 'consistent'}`); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
