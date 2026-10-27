import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { request } from 'node:https';
import { Buffer } from 'node:buffer';
import { rootCertificates } from 'node:tls';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { URL } from 'node:url';
import process from 'node:process';
import { chromium } from 'playwright';

const origin = 'https://fluxkit.test:18443';
const issuer = 'https://identity.fluxkit.test:18443/realms/fluxkit-e2e';
const checks = [];
function passed(name) {
  checks.push(name);
  process.stdout.write(`PASS ${name}\n`);
}
async function http(method, url, { token, body, headers = {}, ca } = {}) {
  const content = body === undefined ? undefined : JSON.stringify(body);
  return new Promise((resolve, reject) => {
    const outgoing = request(
      url,
      {
        method,
        ca,
        ...(ca === undefined ? {} : { agent: false }),
        headers: {
          ...headers,
          ...(content === undefined ? {} : { 'Content-Type': 'application/json' }),
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
      },
      (response) => {
        let bytes = 0;
        const chunks = [];
        response.on('data', (chunk) => {
          bytes += chunk.length;
          if (bytes > 1024 * 1024) {
            outgoing.destroy(new Error('Fixture response exceeded its limit.'));
          } else {
            chunks.push(chunk);
          }
        });
        response.on('end', () =>
          resolve({
            status: response.statusCode,
            headers: response.headers,
            text: Buffer.concat(chunks).toString('utf8'),
          }),
        );
        response.on('error', reject);
      },
    );
    outgoing.setTimeout(15_000, () => outgoing.destroy(new Error('Fixture request timed out.')));
    outgoing.on('error', reject);
    outgoing.end(content);
  });
}
async function api(method, path, options = {}, expected = 200) {
  const reply = await http(method, origin + path, options);
  assert.equal(reply.status, expected, `${method} ${path}: unexpected HTTP status`);
  return reply.text ? JSON.parse(reply.text) : null;
}
async function ready(url) {
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    try {
      if ((await http('GET', url)).status === 200) {
        return;
      }
    } catch {
      /* Bounded startup polling of GET only. */
    }
    await delay(1000);
  }
  throw new Error('The isolated HTTPS services did not become ready.');
}

let browser;
async function signIn(username, password, checkWrongPassword = false) {
  const startReply = await http('POST', origin + '/api/auth/oidc/start');
  assert.equal(startReply.status, 200);
  assert.equal(startReply.headers['cache-control'], 'no-store');
  const start = JSON.parse(startReply.text);
  const context = await browser.newContext({ ignoreHTTPSErrors: false });
  try {
    const page = await context.newPage();
    await page.goto(`${origin}/api/auth/oidc/authorize/${start.id}`);
    assert.equal(new URL(page.url()).origin, new URL(issuer).origin);
    await page.locator('#username').fill(username);
    await page
      .locator('#password')
      .fill(checkWrongPassword ? 'SyntheticWrongPassword42!' : password);
    await page.locator('#kc-login').click();
    if (checkWrongPassword) {
      const validation = page.locator('#input-error-username, #input-error').first();
      await validation.waitFor({ state: 'visible' });
      assert.match(await validation.innerText(), /invalid/i);
      assert.equal(
        (await api('POST', '/api/auth/oidc/poll', { body: { id: start.id, secret: start.secret } }))
          .status,
        'waiting',
      );
      assert.equal(new URL(page.url()).origin, new URL(issuer).origin);
      await page.locator('#username').fill(username);
      await page.locator('#password').fill(password);
      await page.locator('#kc-login').click();
    }
    await page.waitForURL(origin + '/api/auth/oidc/callback');
    await page
      .getByText('Return to Fluxkit to see the sign-in result.', { exact: false })
      .waitFor();
    assert.equal(new URL(page.url()).search, '');
    // A bad polling secret neither obtains a session nor consumes the real result.
    const wrong = await api('POST', '/api/auth/oidc/poll', {
      body: { id: start.id, secret: Buffer.alloc(32).toString('base64') },
    });
    assert.equal(wrong.status, 'expired');
    const result = await api('POST', '/api/auth/oidc/poll', {
      body: { id: start.id, secret: start.secret },
    });
    const consumed = await api('POST', '/api/auth/oidc/poll', {
      body: { id: start.id, secret: start.secret },
    });
    assert.equal(consumed.status, 'expired');
    return result;
  } finally {
    await context.close();
  }
}
function token(result) {
  assert.equal(result.status, 'complete');
  assert.equal(typeof result.session?.accessToken, 'string');
  return result.session.accessToken;
}
async function run() {
  await Promise.all([
    ready(origin + '/health/ready'),
    ready(issuer + '/.well-known/openid-configuration'),
  ]);
  const ca = await readFile('/public-ca/root.crt');
  await assert.rejects(
    http('GET', origin + '/health/ready', { ca: rootCertificates[0] }),
    (error) =>
      [
        'UNABLE_TO_VERIFY_LEAF_SIGNATURE',
        'SELF_SIGNED_CERT_IN_CHAIN',
        'UNABLE_TO_GET_ISSUER_CERT_LOCALLY',
      ].includes(error.code),
  );
  assert.equal((await http('GET', origin + '/health/ready', { ca })).status, 200);
  assert.equal(
    JSON.parse((await http('GET', issuer + '/.well-known/openid-configuration')).text).issuer,
    issuer,
  );
  passed('HTTPS verifies the private CA; untrusted clients are rejected');
  assert.equal((await api('GET', '/api/auth/mode')).mode, 'oidc');
  await api('GET', '/api/projects', {}, 401);
  await api(
    'POST',
    '/api/auth/login',
    { body: { username: 'unused', password: 'SyntheticUnused42!' } },
    404,
  );
  await api(
    'POST',
    '/api/auth/register',
    { body: { username: 'unused', password: 'SyntheticUnused42!' } },
    404,
  );
  passed('OIDC is exclusive; anonymous project access is denied');

  browser = await chromium.launch();
  const adminToken = token(await signIn('e2e-admin', 'SyntheticAdmin42!password', true));
  const admin = { token: adminToken };
  const adminAccount = await api('GET', '/api/account', admin);
  assert.equal(adminAccount.isServerAdmin, true);
  const personal = await api('GET', '/api/projects', admin);
  assert.equal(personal.length, 1);
  passed(
    'Real Keycloak browser login enforces password and PKCE; bootstrap gets one personal project',
  );

  const pending = await signIn('e2e-member', 'SyntheticMember42!password');
  assert.equal(pending.status, 'pendingApproval');
  assert.equal(pending.session, undefined);
  const accounts = await api('GET', '/api/admin/pending-users', admin);
  assert.equal(accounts.length, 1);
  const memberId = accounts[0].id;
  await api('POST', `/api/admin/users/${memberId}/approve`, admin, 204);
  const memberToken = token(await signIn('e2e-member', 'SyntheticMember42!password'));
  const member = { token: memberToken };
  const memberAccount = await api('GET', '/api/account', member);
  assert.equal(memberAccount.id, memberId);
  assert.equal(memberAccount.isServerAdmin, false);
  const returningToken = token(await signIn('e2e-member', 'SyntheticMember42!password'));
  assert.equal((await api('GET', '/api/account', { token: returningToken })).id, memberId);
  assert.equal((await api('GET', '/api/projects', member)).length, 1);
  await api('GET', `/api/projects/${personal[0].id}`, member, 404);
  passed('Approval, repeat login, stable account identity and personal-project isolation');

  const project = await api(
    'POST',
    '/api/projects',
    { ...admin, body: { name: 'HTTPS fixture team' } },
    201,
  );
  const id = project.id;
  await api('GET', `/api/projects/${id}`, member, 404);
  await api(
    'POST',
    `/api/projects/${id}/members`,
    { ...admin, body: { username: memberAccount.userName, role: 'member' } },
    204,
  );
  const shared = randomUUID();
  const snapshot = (name) =>
    JSON.stringify({
      project: { schemaVersion: 1, id, name },
      collections: [],
      environments: {
        schemaVersion: 1,
        environments: [],
        variables: [{ id: shared, name: 'baseUrl', secret: false }],
      },
    });
  await api('PUT', `/api/projects/${id}`, {
    ...admin,
    headers: { 'If-Match': '1' },
    body: { snapshot: snapshot('HTTPS fixture team') },
  });
  await api('PUT', `/api/projects/${id}/values`, {
    ...member,
    headers: { 'If-Match': '0' },
    body: {
      projectRevision: 2,
      values: [
        { variableId: shared, environmentId: 'default', value: 'Shared synthetic HTTPS value' },
      ],
    },
  });
  assert.equal(
    (await api('GET', `/api/projects/${id}/values`, admin)).values[0].value,
    'Shared synthetic HTTPS value',
  );
  await api(
    'PUT',
    `/api/projects/${id}/values`,
    { ...admin, headers: { 'If-Match': '0' }, body: { projectRevision: 2, values: [] } },
    409,
  );
  const writes = await Promise.all(
    ['First synthetic writer', 'Second synthetic writer'].map((name) =>
      http('PUT', origin + `/api/projects/${id}`, {
        ...member,
        headers: { 'If-Match': '2' },
        body: { snapshot: snapshot(name) },
      }),
    ),
  );
  assert.deepEqual(writes.map((reply) => reply.status).sort(), [200, 409]);
  const winner = await api('GET', `/api/projects/${id}`, admin);
  assert.equal(winner.revision, 3);
  assert.ok(['First synthetic writer', 'Second synthetic writer'].includes(winner.name));
  assert.equal((await api('GET', `/api/projects/${id}/values`, member)).values.length, 1);
  passed('Team membership, shared values and competing snapshot/value revisions over HTTPS');

  const copyId = randomUUID();
  const copyVariable = randomUUID();
  const copy = {
    project: { schemaVersion: 1, id: copyId, name: 'Synthetic project transfer' },
    collections: [],
    environments: {
      schemaVersion: 1,
      environments: [],
      variables: [{ id: copyVariable, name: 'baseUrl' }],
    },
  };
  const selected = [
    { variableId: copyVariable, environmentId: 'default', value: 'Selected synthetic value' },
  ];
  const transfer = () => ({
    name: copy.project.name,
    kind: 'personal',
    snapshot: JSON.stringify(copy),
    encryptedSecrets: [],
    sharedValues: selected,
    vaultValues: [],
  });
  await api('POST', `/api/projects/${copyId}/takeover`, { ...member, body: transfer() }, 400);
  await api('GET', `/api/projects/${copyId}`, member, 404);
  copy.environments.variables[0].secret = false;
  await api('POST', `/api/projects/${copyId}/takeover`, { ...member, body: transfer() }, 201);
  assert.equal(
    (await api('GET', `/api/projects/${copyId}/values`, member)).values[0].value,
    selected[0].value,
  );
  await api('POST', `/api/projects/${copyId}/takeover`, { ...member, body: transfer() }, 409);
  await api('GET', `/api/projects/${copyId}`, admin, 404);
  passed(
    'Project transfer is atomic, preserves secret classification and rejects duplicate identities',
  );

  await api(
    'PUT',
    `/api/admin/users/${memberId}/status`,
    { ...admin, body: { status: 'disabled' } },
    204,
  );
  await api('GET', '/api/projects', member, 401);
  await api(
    'PUT',
    `/api/admin/users/${memberId}/status`,
    { ...admin, body: { status: 'active' } },
    204,
  );
  await api('DELETE', `/api/projects/${id}/members/${memberId}`, admin, 204);
  await api('GET', `/api/projects/${id}`, member, 404);
  passed('Suspension revokes API access; membership removal revokes team access');

  // Synthetic, short-lived handoffs for the optional native phase; never written to the repository.
  await writeFile('/results/session.json', JSON.stringify({ adminToken, memberToken }), {
    mode: 0o600,
  });
  await writeFile(
    '/results/report.json',
    JSON.stringify({ checks, provider: 'Keycloak 26.8.1', browser: browser.version() }, null, 2),
  );
  process.stdout.write(`Browser/server E2E passed (${checks.length} checks).\n`);
}
try {
  await run();
} catch (error) {
  // Do not dump HTTP bodies, handoff secrets, cookies, tokens or browser traces.
  process.stderr.write(
    `E2E failed: ${error instanceof Error ? error.message : 'unknown failure'}\n`,
  );
  process.exitCode = 1;
} finally {
  await browser?.close();
}
