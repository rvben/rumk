const assert = require('node:assert/strict');
const vscode = require('vscode');

exports.run = async (root, until) => {
  const directory = vscode.Uri.joinPath(root, 'fix quality');
  await vscode.workspace.fs.createDirectory(directory);
  await vscode.workspace.fs.writeFile(vscode.Uri.joinPath(directory, '.rumk.toml'), Buffer.from(
    "[global]\nenable=['MK001','MK105','MK106','MK218']\n",
  ));
  const path = vscode.Uri.joinPath(directory, 'Makefile');
  const original = '# 😀 café\r\nVALUE=hello  \r\n.PHONY: all\r\nall  :\r\n    @@echo $(VALUE)\r\n';
  await vscode.workspace.fs.writeFile(path, Buffer.from(original));
  const doc = await vscode.workspace.openTextDocument(path);
  const uri = doc.uri;
  await vscode.window.showTextDocument(doc);
  const hasIndentError = () => vscode.languages.getDiagnostics(uri).some(d => (d.code?.value || d.code) === 'MK001');
  await until(hasIndentError, 'fix-quality fixture diagnostics missing');
  const quickFix = async () => {
    const actions = await vscode.commands.executeCommand('vscode.executeCodeActionProvider', uri, new vscode.Range(4, 1, 4, 1), 'quickfix');
    const action = actions.find(a => a.kind?.value === 'quickfix' && a.title === 'Replace spaces with tab');
    assert.ok(action?.edit, 'lightbulb offers an indentation fix');
    assert.equal(action.isPreferred, true, 'safe quick fix is preferred');
    return action;
  };
  const action = await quickFix();
  assert.equal(await vscode.workspace.applyEdit(action.edit), true);
  assert.equal(doc.getText(), original.replace('    @@', '\t@@'), 'individual fix preserves unrelated violations, Unicode, CRLF, and meaningful trailing spaces');
  assert.equal(Buffer.from(await vscode.workspace.fs.readFile(uri)).toString(), original, 'quick fix does not write to disk');
  await vscode.commands.executeCommand('undo');
  assert.equal(doc.getText(), original, 'one undo restores the entire quick fix');
  await until(hasIndentError, 'undo restores the diagnostic');

  // Request an action, then edit before invoking the actual editor command.
  await quickFix();
  const userEdit = new vscode.WorkspaceEdit();
  userEdit.insert(uri, new vscode.Position(0, 0), '# newer edit\r\n');
  assert.equal(await vscode.workspace.applyEdit(userEdit), true);
  const newer = doc.getText();
  await vscode.commands.executeCommand('rumk.fixAll');
  const expected = newer.replace('VALUE=hello', 'VALUE = hello').replace('all  :', 'all:').replace('    @@', '\t@');
  await until(() => doc.getText() === expected, 'Fix All command did not apply to the latest buffer');
  assert.equal(doc.getText(), expected, 'fix-all stabilizes interacting syntax and recipe-prefix fixes without losing whitespace or user edits');
  const repeated = await vscode.commands.executeCommand('vscode.executeCodeActionProvider', uri, new vscode.Range(0, 0, 0, 0), 'source.fixAll.rumk');
  assert.equal(repeated.length, 0, 'fix-all is idempotent');
  await vscode.commands.executeCommand('undo');
  assert.equal(doc.getText(), newer, 'one undo restores all fixes while retaining the preceding user edit');

  // Every currently fixable rule must reach the editor through the same policy.
  const cases = [
    ['MK001', 'all:\n    echo hello\n', '\techo hello', false],
    ['MK101', '.PHONY: alpha beta gamma delta epsilon\n', '\\\n', false, '[MK101]\nline-length=24\n'],
    ['MK105', 'VALUE=hello  \n', 'VALUE = hello  \n', false],
    ['MK106', 'all  : ;\n', 'all: ;\n', false],
    ['MK218', 'all:\n\t@@echo hello\n', '\t@echo hello', false],
    ['MK201', 'clean:;\n', '.PHONY: clean', true],
    ['MK203', 'all:\n\tmake -C sub && make -C other\n', '$(MAKE) -C sub && $(MAKE) -C other', true],
    ['MK211', 'NAME=hello\nVALUE=$NAME\n', 'VALUE=$(NAME)', true],
  ];
  for (const [rule, input, fragment, unsafe, extra = ''] of cases) {
    for (const policy of ['default', 'enabled', 'unfixable']) {
      if (!unsafe && policy === 'enabled') continue;
      const folder = vscode.Uri.joinPath(root, `${rule}-${policy}`);
      await vscode.workspace.fs.createDirectory(folder);
      const allowed = policy !== 'unfixable' && (!unsafe || policy === 'enabled');
      await vscode.workspace.fs.writeFile(vscode.Uri.joinPath(folder, '.rumk.toml'), Buffer.from(
        `[global]\nenable=['${rule}']\nunsafe-fixes=${policy !== 'default'}\n` +
        (policy === 'unfixable' ? `unfixable=['${rule}']\n` : '') + extra,
      ));
      const path = vscode.Uri.joinPath(folder, 'Makefile');
      await vscode.workspace.fs.writeFile(path, Buffer.from(input));
      const document = await vscode.workspace.openTextDocument(path);
      await vscode.window.showTextDocument(document);
      const file = document.uri;
      await until(() => vscode.languages.getDiagnostics(file).some(d => (d.code?.value || d.code) === rule), `${rule}/${policy}: diagnostic missing`);
      const range = new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length));
      const actions = await vscode.commands.executeCommand('vscode.executeCodeActionProvider', file, range, 'quickfix');
      assert.equal(actions.length, allowed ? 1 : 0, `${rule}/${policy}: quick-fix policy`);
      const fixes = await vscode.commands.executeCommand('vscode.executeCodeActionProvider', file, range, 'source.fixAll.rumk');
      assert.equal(fixes.length, allowed ? 1 : 0, `${rule}/${policy}: fix-all policy`);
      if (allowed) {
        assert.equal(actions[0].isPreferred, !unsafe, `${rule}: only safe actions are preferred`);
        assert.equal(await vscode.workspace.applyEdit(actions[0].edit), true);
        assert.ok(document.getText().includes(fragment), `${rule}: expected correction`);
        await until(() => !vscode.languages.getDiagnostics(file).some(d => (d.code?.value || d.code) === rule), `${rule}: fix clears its diagnostic`);
      }
      assert.equal(Buffer.from(await vscode.workspace.fs.readFile(file)).toString(), input, `${rule}/${policy}: no unsolicited disk writes`);
    }
  }
};
