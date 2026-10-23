import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath, URL } from 'node:url';

const env = { ...process.env };
const commandLineTools = '/Library/Developer/CommandLineTools';
// Prefer the standalone SDK without changing the user's system-wide Xcode selection.
if (process.platform === 'darwin' && !env.DEVELOPER_DIR && existsSync(commandLineTools)) {
  env.DEVELOPER_DIR = commandLineTools;
}
const cli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url));
const child = spawn(process.execPath, [cli, ...process.argv.slice(2)], { stdio: 'inherit', env });
child.on('error', () => {
  process.stderr.write('Unable to start the local Tauri CLI. Run npm ci first.\n');
  process.exitCode = 1;
});
child.on('exit', (code) => {
  process.exitCode = code ?? 1;
});
