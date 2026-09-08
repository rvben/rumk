const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { transformSync } = require('esbuild');
const fs = require('node:fs');
const source = transformSync(fs.readFileSync(path.join(__dirname, '../src/settings.ts'), 'utf8'), { loader: 'ts', format: 'cjs' }).code;
const loaded = { exports: {} };
new Function('require', 'module', 'exports', source)(require, loaded, loaded.exports);
const { serverCommand } = loaded.exports;

test('resolves workspace paths, including spaces, without splitting shell arguments', () => {
  const root = path.resolve('project with spaces');
  assert.equal(serverCommand('${workspaceFolder}/bin/rumk', root), path.join(root, 'bin/rumk'));
  assert.equal(serverCommand('./bin/rumk', root), path.join(root, 'bin/rumk'));
  assert.equal(serverCommand(' rumk '), 'rumk');
  assert.equal(serverCommand('rumk;echo-secret'), 'rumk;echo-secret');
});
test('rejects ambiguous executable paths instead of using the host working directory', () => {
  assert.throws(() => serverCommand('  '), /executable/);
  assert.throws(() => serverCommand('${workspaceFolder}/rumk'), /no workspace/);
  assert.throws(() => serverCommand('./rumk'), /absolute/);
});

test('uses the bundled executable by default and honors explicit overrides', () => {
  const { resolveServerCommand } = loaded.exports;
  const extension = path.resolve('extension with spaces');
  assert.equal(resolveServerCommand('', extension), path.join(extension, 'bundled', process.platform === 'win32' ? 'rumk.exe' : 'rumk'));
  assert.equal(resolveServerCommand('   ', extension, undefined, 'win32'), path.join(extension, 'bundled', 'rumk.exe'));
  assert.equal(resolveServerCommand('rumk', extension), 'rumk');
  assert.equal(resolveServerCommand('./my-rumk', extension, extension), path.join(extension, 'my-rumk'));
});
