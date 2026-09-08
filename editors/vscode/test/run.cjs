const path = require('node:path');
const fs = require('node:fs/promises');
const os = require('node:os');
const { runTests } = require('@vscode/test-electron');

(async () => {
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'rumk-vscode-'));
  try {
    const first = path.join(temporary, 'first project');
    const second = path.join(temporary, 'second project');
    await fs.mkdir(first);
    await fs.mkdir(second);
    await fs.writeFile(path.join(first, 'Makefile'), 'all:\n    echo hello\n');
    await fs.writeFile(path.join(second, 'Makefile'), 'other:\n    echo world\n');
    await require('./fixes.cjs').prepare(first);
    const binary = process.env.RUMK_TEST_BINARY;
    const workspace = path.join(temporary, 'test.code-workspace');
    await fs.writeFile(workspace, JSON.stringify({ folders: [{ path: first }, { path: second }], settings: binary ? { 'rumk.path': binary } : {} }));
    await runTests({
      ...(process.env.VSCODE_EXECUTABLE_PATH ? { vscodeExecutablePath: process.env.VSCODE_EXECUTABLE_PATH } : { version: '1.91.1' }),
      extensionDevelopmentPath: path.resolve(__dirname, '..'),
      extensionTestsPath: path.join(__dirname, 'suite.cjs'),
      launchArgs: [workspace, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes', '--user-data-dir', path.join(temporary, 'user'), '--extensions-dir', path.join(temporary, 'extensions')],
    });
  } finally {
    await fs.rm(temporary, { recursive: true, force: true });
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
