import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('../src/updater.ts', import.meta.url), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const { UpdateController } = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));

test('no update is shown only after a successful check; network errors can be retried', async () => {
  let attempts = 0;
  const c = new UpdateController({ check: async () => { if (++attempts === 1) throw new Error('offline'); return null; }, restart: async () => {} });
  await c.check();
  assert.equal(c.state.phase, 'error');
  assert.match(c.state.error, /offline/);
  await c.check();
  assert.equal(c.state.phase, 'current');
});

test('shows release notes and byte progress, installs only once, and restarts on demand', async () => {
  const snapshots = [];
  let installs = 0, restarts = 0;
  const update = { version: '0.6.1', body: '修复角色导入', async downloadAndInstall(emit) {
    installs++;
    emit({ event: 'Started', data: { contentLength: 100 } });
    emit({ event: 'Progress', data: { chunkLength: 40 } });
    emit({ event: 'Progress', data: { chunkLength: 60 } });
    emit({ event: 'Finished' });
  } };
  const c = new UpdateController({ check: async () => update, restart: async () => { restarts++; } }, state => snapshots.push({ ...state }));
  await c.check();
  assert.equal(c.state.phase, 'available');
  assert.equal(c.state.version, '0.6.1');
  assert.equal(c.state.notes, '修复角色导入');
  await c.install();
  assert.ok(snapshots.some(s => s.phase === 'downloading' && s.downloaded === 40 && s.total === 100));
  assert.equal(c.state.phase, 'ready');
  assert.equal(restarts, 0);
  await c.install();
  assert.equal(installs, 1);
  await c.restart();
  assert.equal(restarts, 1);
});

test('unknown download length stays indeterminate and failed downloads can retry', async () => {
  let attempts = 0;
  const c = new UpdateController({ check: async () => ({ version: '0.6.1', async downloadAndInstall(emit) {
    emit({ event: 'Started', data: {} });
    emit({ event: 'Progress', data: { chunkLength: 123 } });
    if (++attempts === 1) throw new Error('download failed');
  } }), restart: async () => {} });
  await c.check();
  await c.install();
  assert.equal(c.state.phase, 'error');
  assert.equal(c.state.operation, 'install');
  assert.equal(c.state.total, undefined);
  assert.equal(c.state.downloaded, 123);
  await c.install();
  assert.equal(c.state.phase, 'ready');
});

test('overlapping checks/downloads are ignored and pending plugin handles are closed before rechecking', async () => {
  let resolve, checks = 0, closes = 0;
  const update = { version: '0.6.1', downloadAndInstall: async () => {}, close: async () => { closes++; } };
  const c = new UpdateController({ check: () => { checks++; return new Promise(r => { resolve = r; }); }, restart: async () => {} });
  const first = c.check();
  await c.check();
  await c.install();
  assert.equal(checks, 1);
  resolve(update); await first;
  const next = c.check();
  await new Promise(setImmediate);
  assert.equal(closes, 1);
  resolve(null); await next;
  assert.equal(c.state.phase, 'current');
});

test('restart failure remains retryable without reinstalling', async () => {
  let restarts = 0;
  const c = new UpdateController({ check: async () => ({ version: '0.6.1', downloadAndInstall: async () => {} }), restart: async () => { if (++restarts === 1) throw new Error('restart failed'); } });
  await c.check(); await c.install(); await c.restart();
  assert.equal(c.state.operation, 'restart');
  assert.match(c.state.error, /restart failed/);
  await c.restart();
  assert.equal(restarts, 2);
});
