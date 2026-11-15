#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..');

function detectHostTriple() {
  const rustcOut = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const match = rustcOut.match(/^host:\s*(.+)$/m);
  if (!match) {
    throw new Error('Could not detect rustc host target triple');
  }
  return match[1].trim();
}

function parseArg(flag) {
  const idx = process.argv.indexOf(flag);
  if (idx >= 0 && idx + 1 < process.argv.length) {
    return process.argv[idx + 1];
  }
  return null;
}

const isDebug = process.argv.includes('--debug');
const profile = isDebug ? 'debug' : 'release';
const targetTriple =
  parseArg('--target') || process.env.TAURI_ENV_TARGET_TRIPLE || detectHostTriple();
const isWindows = targetTriple.includes('windows');
const exeExt = isWindows ? '.exe' : '';

const cargoArgs = ['build', '--bin', 'psalm-stage-cli', '--target', targetTriple];
if (!isDebug) {
  cargoArgs.push('--release');
}

console.log(`==> Building psalm-stage-cli sidecar (${targetTriple}, ${profile})...`);
execFileSync('cargo', cargoArgs, { cwd: repoRoot, stdio: 'inherit' });

const builtBinary = path.join(
  repoRoot,
  'target',
  targetTriple,
  profile,
  `psalm-stage-cli${exeExt}`,
);
if (!fs.existsSync(builtBinary)) {
  throw new Error(`Expected built sidecar at ${builtBinary}`);
}

const binariesDir = path.join(repoRoot, 'src-tauri', 'binaries');
fs.mkdirSync(binariesDir, { recursive: true });
const destBinary = path.join(binariesDir, `psalm-stage-cli-${targetTriple}${exeExt}`);
fs.copyFileSync(builtBinary, destBinary);
if (!isWindows) {
  fs.chmodSync(destBinary, 0o755);
}
console.log(`==> Sidecar ready: ${destBinary}`);

