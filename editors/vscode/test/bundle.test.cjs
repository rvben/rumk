const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');

test('maps supported native hosts and rejects unsupported architectures', async () => {
  const { hostTarget, targets, binaryName } = await import('../scripts/bundle-utils.mjs');
  for (const target of Object.keys(targets)) {
    const [platform, arch] = target.split('-');
    assert.equal(hostTarget(platform, arch), target);
  }
  assert.equal(binaryName('win32-x64'), 'rumk.exe');
  assert.throws(() => hostTarget('win32', 'arm64'), /No bundled/);
  assert.throws(() => hostTarget('freebsd', 'x64'), /No bundled/);
});

test('rejects missing, corrupted, and mismatched native bundles', async () => {
  const { verifyBundle, sha256, targets, hostTarget, binaryName } = await import('../scripts/bundle-utils.mjs');
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'rumk-bundle-'));
  const target = hostTarget();
  const binary = path.join(directory, binaryName(target));
  const metadata = { target, rustTarget: targets[target], version: 'rumk test', sha256: sha256(Buffer.from('test executable')) };
  try {
    await assert.rejects(verifyBundle(directory), /ENOENT/);
    await fs.writeFile(binary, 'test executable', { mode: 0o755 });
    await fs.writeFile(path.join(directory, 'THIRD_PARTY_NOTICES.txt'), 'test license');
    await fs.writeFile(path.join(directory, 'metadata.json'), JSON.stringify(metadata));
    assert.deepEqual(await verifyBundle(directory), metadata);
    const other = target === 'linux-x64' ? 'darwin-arm64' : 'linux-x64';
    await assert.rejects(verifyBundle(directory, other), /target does not match/);
    await fs.writeFile(binary, 'different executable');
    await assert.rejects(verifyBundle(directory), /checksum mismatch/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});
