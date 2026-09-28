const path = require('node:path');
const fs = require('node:fs/promises');
const os = require('node:os');
const { downloadAndUnzipVSCode, runTests } = require('@vscode/test-electron');

// VS Code cancels every in-flight code-action request whenever any extension
// registers a code-action provider, for any language. Built-in extensions
// activate lazily during the run, so each one that ships code is disabled; the
// declarative ones stay, including vscode.make, which contributes `makefile`.
async function builtinExtensionsWithCode(executable) {
  const candidates = [
    path.join(path.dirname(executable), 'resources', 'app', 'extensions'),
    path.join(path.dirname(executable), '..', 'Resources', 'app', 'extensions'),
  ];
  for (const directory of candidates) {
    let entries;
    try {
      entries = await fs.readdir(directory, { withFileTypes: true });
    } catch {
      continue;
    }
    const ids = [];
    for (const entry of entries.filter(entry => entry.isDirectory())) {
      let manifest;
      try {
        manifest = JSON.parse(await fs.readFile(path.join(directory, entry.name, 'package.json'), 'utf8'));
      } catch {
        continue;
      }
      if (manifest.main || manifest.browser) ids.push(`${manifest.publisher}.${manifest.name}`);
    }
    if (ids.length === 0) throw new Error(`no built-in extensions with code found in ${directory}`);
    return ids;
  }
  throw new Error(`built-in extensions not found next to ${executable}`);
}

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
    const vscodeExecutablePath = process.env.VSCODE_EXECUTABLE_PATH || await downloadAndUnzipVSCode('1.91.1');
    const disabled = (await builtinExtensionsWithCode(vscodeExecutablePath)).flatMap(id => ['--disable-extension', id]);
    await runTests({
      vscodeExecutablePath,
      extensionDevelopmentPath: path.resolve(__dirname, '..'),
      extensionTestsPath: path.join(__dirname, 'suite.cjs'),
      launchArgs: [workspace, ...disabled, '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes', '--user-data-dir', path.join(temporary, 'user'), '--extensions-dir', path.join(temporary, 'extensions')],
    });
  } finally {
    await fs.rm(temporary, { recursive: true, force: true });
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
