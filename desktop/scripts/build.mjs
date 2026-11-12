#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const desktopDir = fileURLToPath(new URL('../', import.meta.url));
const config = JSON.parse(readFileSync(path.join(desktopDir, 'src-tauri/tauri.conf.json'), 'utf8'));
const args = process.argv.slice(2);
let name = process.env.AVARI_APP_NAME ?? config.productName ?? 'Avari Mieru';
let printConfig = false;
const buildArgs = [];

function fail(message) {
  console.error(message);
  process.exit(1);
}

for (let i = 0; i < args.length; i++) {
  const arg = args[i];
  if (arg === '--help' || arg === '-h') {
    console.log(`Usage: node desktop/scripts/build.mjs [--name "Avari Mieru"] [Tauri build options]

Examples:
  node desktop/scripts/build.mjs --debug --bundles app
  node desktop/scripts/build.mjs --name "My VPN" --debug --bundles app
  AVARI_APP_NAME="My VPN" node desktop/scripts/build.mjs --debug --bundles app

--name overrides AVARI_APP_NAME and the default productName.
--print-config prints the resolved branding without building.
Other options are passed to Tauri build. The application identifier and profile storage stay unchanged.`);
    process.exit(0);
  } else if (arg === '--name') {
    if (!args[i + 1] || args[i + 1].startsWith('--')) fail('--name requires an application name.');
    name = args[++i];
  } else if (arg.startsWith('--name=')) {
    name = arg.slice('--name='.length);
  } else if (arg === '--print-config') {
    printConfig = true;
  } else {
    buildArgs.push(arg);
  }
}

name = name.trim();
if (!name || name.length > 80 || /[\x00-\x1f\x7f/\\:*?"<>|]/.test(name) || /[. ]$/.test(name)) {
  fail('Use an application name of 1–80 characters without control characters, / \\ : * ? " < > | or a trailing dot.');
}

// Tauri merges arrays as a whole. Preserve every existing window option.
const overlay = {
  productName: name,
  app: {
    windows: config.app.windows.map(window => window.label === 'main' ? { ...window, title: name } : window),
  },
};

if (printConfig) {
  console.log(JSON.stringify({ name, VITE_APP_NAME: name, config: overlay }, null, 2));
  process.exit(0);
}

const cli = path.join(desktopDir, 'ui/node_modules/@tauri-apps/cli/tauri.js');
const result = spawnSync(process.execPath, [cli, 'build', ...buildArgs, '--config', JSON.stringify(overlay)], {
  cwd: desktopDir,
  env: { ...process.env, VITE_APP_NAME: name },
  stdio: 'inherit',
});
if (result.error) console.error(`Could not start Tauri: ${result.error.message}`);
process.exit(result.status ?? 1);
