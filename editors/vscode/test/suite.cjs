const assert = require('node:assert/strict');
const vscode = require('vscode');

async function until(callback, message) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    const value = await callback();
    if (value) return value;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(message);
}

// VS Code cancels every in-flight code-action request when a provider matching
// that document registers. Core registers one for every language ("*") once,
// a few seconds after startup, which would cancel the fix requests asserted
// below at random. A pending request on a plaintext canary can only be
// cancelled by such a registration, so its cancellation marks the point from
// which code actions are stable. The registration waits for the workbench to
// go idle after startup, so it always follows the canary, but a loaded machine
// can delay it by tens of seconds. Continuing without it would let it cancel an
// asserted request later, so a missing registration fails the run instead.
async function codeActionProvidersSettled() {
  const canary = await vscode.workspace.openTextDocument({ language: 'plaintext', content: 'canary' });
  let cancelled;
  const settled = new Promise(resolve => { cancelled = resolve; });
  const provider = vscode.languages.registerCodeActionsProvider({ language: 'plaintext', scheme: 'untitled' }, {
    provideCodeActions: (_document, _range, _context, token) => new Promise(resolve => {
      token.onCancellationRequested(() => { cancelled(true); resolve([]); });
    }),
  });
  try {
    const request = vscode.commands.executeCommand('vscode.executeCodeActionProvider', canary.uri, new vscode.Range(0, 0, 0, 0));
    request.then(undefined, () => {});
    const started = Date.now();
    const observed = await Promise.race([settled, new Promise(resolve => setTimeout(resolve, 120000, false))]);
    assert.ok(observed, 'VS Code registered its code-action provider for every language within 120s');
    console.log(`code-action providers settled after ${Date.now() - started}ms`);
  } finally {
    provider.dispose();
  }
}

exports.run = async () => {
  await codeActionProvidersSettled();
  const extension = vscode.extensions.getExtension('rvben.rumk');
  assert.ok(extension, 'extension is installed');
  await extension.activate();
  const folders = vscode.workspace.workspaceFolders;
  const first = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(folders[0].uri, 'Makefile'));
  const second = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(folders[1].uri, 'Makefile'));
  assert.equal(first.languageId, 'makefile');
  await vscode.window.showTextDocument(first);
  for (const doc of [first, second]) {
    await until(() => vscode.languages.getDiagnostics(doc.uri).find(d => d.source === 'rumk' && (d.code?.value || d.code) === 'MK001'), 'diagnostics missing in ' + doc.uri.fsPath);
  }
  const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', first.uri);
  assert.ok(symbols.some(s => s.name === 'all'), 'target symbols appear in Outline');
  const actions = await vscode.commands.executeCommand('vscode.executeCodeActionProvider', first.uri, new vscode.Range(0, 0, 2, 0), 'source.fixAll.rumk');
  const fix = actions.find(a => a.kind?.value === 'source.fixAll.rumk');
  assert.ok(fix?.edit, 'fix-all returns a workspace edit');
  assert.equal(await vscode.workspace.applyEdit(fix.edit), true);
  assert.ok(first.getText().includes('\techo hello'), 'fix changes the live buffer');
  assert.equal(first.isDirty, true, 'fix does not save the file');
  await until(() => !vscode.languages.getDiagnostics(first.uri).some(d => (d.code?.value || d.code) === 'MK001'), 'stale diagnostics remain');
  // Exercise VS Code's actual save participants, not a direct code-action request.
  const editorSettings = vscode.workspace.getConfiguration('editor', {
    uri: first.uri, languageId: 'makefile',
  });
  await editorSettings.update('formatOnSave', false, vscode.ConfigurationTarget.Workspace, true);
  const replaceFirst = async text => {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(first.uri, new vscode.Range(first.positionAt(0), first.positionAt(first.getText().length)), text);
    assert.equal(await vscode.workspace.applyEdit(edit), true);
  };
  const diskText = async () => Buffer.from(await vscode.workspace.fs.readFile(first.uri)).toString('utf8');
  const broken = '.PHONY: all\nall:\n    echo saved\n';
  await editorSettings.update('codeActionsOnSave', { 'source.fixAll.rumk': 'never' }, vscode.ConfigurationTarget.Workspace, true);
  await replaceFirst(broken);
  assert.equal(await first.save(), true);
  assert.equal(await diskText(), broken, 'save leaves fixes opt-in');
  await editorSettings.update('codeActionsOnSave', { 'source.fixAll.rumk': 'explicit' }, vscode.ConfigurationTarget.Workspace, true);
  // Save immediately after editing: fixes must use the latest buffer even if
  // diagnostics for that version have not arrived yet.
  await replaceFirst(broken.replace('saved', 'latest'));
  assert.equal(await first.save(), true);
  const expected = '.PHONY: all\nall:\n\techo latest\n';
  assert.equal(first.getText(), expected, 'save fixes the latest buffer');
  assert.equal(await diskText(), expected, 'save writes the fixed text to disk');
  assert.equal(first.isDirty, false, 'fixed document is saved');
  await until(() => !vscode.languages.getDiagnostics(first.uri).some(d => (d.code?.value || d.code) === 'MK001'), 'saved fix leaves stale diagnostics');
  await replaceFirst(expected + '# save again\n');
  assert.equal(await first.save(), true);
  assert.equal(await diskText(), expected + '# save again\n', 'saving clean content preserves it');
  await editorSettings.update('codeActionsOnSave', undefined, vscode.ConfigurationTarget.Workspace, true);
  await editorSettings.update('formatOnSave', undefined, vscode.ConfigurationTarget.Workspace, true);
  const formatting = await vscode.commands.executeCommand('vscode.executeFormatDocumentProvider', second.uri, { tabSize: 4, insertSpaces: false });
  let formatted = second.getText();
  for (const edit of [...formatting].sort((a, b) => second.offsetAt(b.range.start) - second.offsetAt(a.range.start))) {
    formatted = formatted.slice(0, second.offsetAt(edit.range.start)) + edit.newText + formatted.slice(second.offsetAt(edit.range.end));
  }
  assert.ok(formatted.includes('\techo world'), 'formatting uses Rumk');
  // A restarted client opens the visible document and the hidden one through
  // separate paths; requests sent the moment it starts must still reach a
  // server that knows the visible document, including its unsaved text.
  await replaceFirst('all:\n    echo restarted\n');
  // Hidden documents are opened ahead of the visible one, which widens the
  // window in which a request could overtake it.
  for (let index = 0; index < 30; index++) {
    const hidden = vscode.Uri.joinPath(folders[1].uri, `hidden ${index}`, 'Makefile');
    await vscode.workspace.fs.writeFile(hidden, Buffer.from(`hidden${index}:\n\techo ${index}\n`));
    await vscode.workspace.openTextDocument(hidden);
  }
  const afterRestart = [
    ['symbols', () => vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', first.uri),
      result => result?.some(s => s.name === 'all')],
    ['fix-all', () => vscode.commands.executeCommand('vscode.executeCodeActionProvider', first.uri, new vscode.Range(0, 0, 2, 0), 'source.fixAll.rumk'),
      result => result?.some(a => a.kind?.value === 'source.fixAll.rumk' && a.edit)],
    ['formatting', () => vscode.commands.executeCommand('vscode.executeFormatDocumentProvider', first.uri, { tabSize: 4, insertSpaces: false }),
      result => result?.some(e => e.newText.includes('\t'))],
  ];
  for (const [name, request, valid] of afterRestart) {
    await vscode.commands.executeCommand('rumk.restartServer');
    const result = await request();
    assert.ok(valid(result), `${name} is available as soon as the server restarts, found ${JSON.stringify(result)}`);
  }
  await vscode.commands.executeCommand('rumk.restartServer');
  await replaceFirst('edited:\n    echo restarted\n');
  const editedSymbols = (await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', first.uri))?.map(s => s.name);
  assert.deepEqual(editedSymbols, ['edited'], `an edit made as soon as the server restarts reaches it, found ${JSON.stringify(editedSymbols)}`);
  await replaceFirst(expected + '# save again\n');
  assert.equal(await first.save(), true);
  await vscode.commands.executeCommand('rumk.restartServer');
  await until(() => vscode.languages.getDiagnostics(second.uri).some(d => (d.code?.value || d.code) === 'MK001'), 'restart did not resynchronize open documents');
  await vscode.workspace.getConfiguration('rumk').update('enable', false, vscode.ConfigurationTarget.Workspace);
  await until(() => !vscode.languages.getDiagnostics(second.uri).some(d => d.source === 'rumk'), 'disable did not clear diagnostics');
  await vscode.workspace.getConfiguration('rumk').update('enable', true, vscode.ConfigurationTarget.Workspace);
  await until(() => vscode.languages.getDiagnostics(second.uri).some(d => (d.code?.value || d.code) === 'MK001'), 'enable did not restart server');
  const settings = vscode.workspace.getConfiguration('rumk');
  const originalPath = settings.get('path');
  await settings.update('path', vscode.Uri.joinPath(folders[0].uri, 'missing-rumk').fsPath, vscode.ConfigurationTarget.Workspace);
  await vscode.commands.executeCommand('rumk.restartServer');
  assert.ok(!vscode.languages.getDiagnostics(second.uri).some(d => d.source === 'rumk'), 'failed startup clears old diagnostics');
  await settings.update('path', originalPath, vscode.ConfigurationTarget.Workspace);
  await vscode.commands.executeCommand('rumk.restartServer');
  await until(() => vscode.languages.getDiagnostics(second.uri).some(d => (d.code?.value || d.code) === 'MK001'), 'correcting executable path did not recover');
  await require('./fixes.cjs').run(folders[0].uri, until);
  // Any other extension registering a code-action provider would cancel the
  // requests asserted above at random, so nothing else may have activated.
  const active = vscode.extensions.all.filter(e => e.isActive).map(e => e.id);
  assert.deepEqual(active, ['rvben.rumk'], `only Rumk activates during the run, found ${active.join(', ')}`);
  console.log('Rumk extension integration checks passed.');
};
