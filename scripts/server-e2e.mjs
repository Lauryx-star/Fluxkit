import { spawn } from 'node:child_process';
import { mkdtemp, rm, chmod } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import { randomUUID } from 'node:crypto';
import process from 'node:process';

const args = process.argv.slice(2);
if (args.includes('--help')) {
  process.stdout.write(
    'Usage: npm run test:server:e2e [-- --server-only]\nDefault: HTTPS/Keycloak/browser tests plus the native Rust crypto protocol test.\nRequires Docker Compose, and the documented Rust desktop prerequisites unless --server-only is selected.\n',
  );
  process.exit(0);
}
if (args.some((arg) => arg !== '--server-only')) {
  throw new Error('Unsupported option. Use --help for supported options.');
}
const repository = fileURLToPath(new URL('..', import.meta.url));
const file = join(repository, 'server/e2e/compose.yml');
const project = 'fluxkit-e2e-' + randomUUID().replaceAll('-', '').slice(0, 12);
const runner = project + '-runner';
const port = process.env.FLUXKIT_E2E_HTTPS_PORT ?? '18443';
if (!/^\d+$/.test(port) || Number(port) < 1024 || Number(port) > 65535) {
  throw new Error('FLUXKIT_E2E_HTTPS_PORT must be an unprivileged TCP port (1024–65535).');
}
const env = { ...process.env, FLUXKIT_E2E_HTTPS_PORT: port };
if (
  process.platform === 'darwin' &&
  !env.DEVELOPER_DIR &&
  existsSync('/Library/Developer/CommandLineTools')
) {
  env.DEVELOPER_DIR = '/Library/Developer/CommandLineTools';
}
let active;
let interrupted = false;
function stop() {
  interrupted = true;
  active?.kill('SIGTERM');
}
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
async function run(command, commandArgs, extraEnv = {}, allowFailure = false) {
  if (interrupted && !allowFailure) {
    throw new Error('E2E run interrupted.');
  }
  return new Promise((resolve, reject) => {
    const child = spawn(command, commandArgs, {
      cwd: repository,
      env: { ...env, ...extraEnv },
      stdio: 'inherit',
    });
    active = child;
    child.once('error', () => {
      if (allowFailure) {
        resolve(false);
      } else {
        reject(new Error(`Could not start ${command}. Check its installation.`));
      }
    });
    child.once('exit', (code) => {
      active = undefined;
      if (code === 0 || allowFailure) {
        resolve(code === 0);
      } else {
        reject(new Error(`${command} exited with ${code ?? 'a signal'}.`));
      }
    });
  });
}
const compose = (args, allowFailure = false) =>
  run('docker', ['compose', '-p', project, '-f', file, ...args], {}, allowFailure);
let temporary;
try {
  temporary = await mkdtemp(join(tmpdir(), 'fluxkit-e2e-'));
  await chmod(temporary, 0o700);
  process.stdout.write(`Starting disposable HTTPS/OIDC fixtures (${project}).\n`);
  await compose(['build', 'server', 'runner']);
  await compose(['up', '-d', 'server', 'identity']);
  await compose(['run', '--no-deps', '--name', runner, 'runner']);
  if (!args.includes('--server-only')) {
    await run('docker', ['cp', `${runner}:/results/session.json`, join(temporary, 'session.json')]);
    await run('docker', ['cp', `${runner}:/public-ca/root.crt`, join(temporary, 'root.crt')]);
    await chmod(join(temporary, 'session.json'), 0o600);
    await run(
      'cargo',
      [
        'test',
        '--manifest-path',
        'src-tauri/Cargo.toml',
        '--locked',
        'rust_crypto_and_value_protocol_roundtrip_through_the_real_backend',
        '--',
        '--ignored',
      ],
      {
        FLUXKIT_VALUE_TEST_URL: `https://fluxkit.test:${port}`,
        FLUXKIT_E2E_SESSION: join(temporary, 'session.json'),
        FLUXKIT_E2E_CA: join(temporary, 'root.crt'),
      },
    );
    process.stdout.write('Native HTTPS crypto/value protocol passed.\n');
  } else {
    process.stdout.write('Native protocol phase explicitly excluded (--server-only).\n');
  }
  process.stdout.write('E2E validation passed. Removing disposable fixtures.\n');
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : 'E2E run failed.'}\n`);
  // Only fixture services: no production projects, request bodies or browser traces are collected.
  await compose(
    ['logs', '--no-color', '--tail', '40', 'server', 'identity', 'proxy', 'certificates'],
    true,
  );
  process.exitCode = 1;
} finally {
  await run('docker', ['rm', '-f', runner], {}, true);
  const cleaned = await compose(['down', '--volumes', '--remove-orphans'], true);
  if (!cleaned) {
    process.stderr.write(
      `Cleanup incomplete. Remove only this fixture project: docker compose -p ${project} -f server/e2e/compose.yml down --volumes --remove-orphans\n`,
    );
    process.exitCode = 1;
  }
  if (temporary) {
    await rm(temporary, { recursive: true, force: true });
  }
  process.off('SIGINT', stop);
  process.off('SIGTERM', stop);
}
