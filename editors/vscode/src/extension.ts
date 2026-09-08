import * as vscode from 'vscode';
import { LanguageClient, State, RevealOutputChannelOn } from 'vscode-languageclient/node';
import { resolveServerCommand } from './settings';

let client: LanguageClient | undefined;
let pending: Promise<void> = Promise.resolve();
let disposed = false;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  disposed = false;
  const output = vscode.window.createOutputChannel('Rumk', { log: true });
  const status = vscode.languages.createLanguageStatusItem('rumk.server', { language: 'makefile', scheme: 'file' });
  status.name = 'Rumk';
  status.command = { command: 'rumk.showOutput', title: 'Show Rumk Output' };
  context.subscriptions.push(output, status);

  function showStatus(text: string, detail: string, severity = vscode.LanguageStatusSeverity.Information): void {
    status.text = text;
    status.detail = detail;
    status.severity = severity;
    status.busy = text === 'Rumk: starting';
  }

  async function restart(): Promise<void> {
    if (client) {
      const previous = client;
      client = undefined;
      await previous.dispose();
    }
    if (disposed) return;
    if (!vscode.workspace.isTrusted) {
      showStatus('Rumk: restricted', 'Trust this workspace to start Rumk.');
      return;
    }
    const config = vscode.workspace.getConfiguration('rumk');
    if (!config.get<boolean>('enable', true)) {
      showStatus('Rumk: disabled', 'Enable Rumk in settings.');
      return;
    }
    if (vscode.workspace.workspaceFolders?.some(folder => folder.uri.scheme !== 'file')) {
      showStatus('Rumk: unavailable', 'Rumk requires a workspace backed by local files.');
      return;
    }
    const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    const command = resolveServerCommand(config.get<string>('path', ''), context.extensionPath, cwd);
    output.info(`Starting ${command} server`);
    showStatus('Rumk: starting', 'Starting the Makefile language server.');
    const next = new LanguageClient('rumk', 'Rumk', {
      command, args: ['server'], options: { cwd, shell: false },
    }, {
      documentSelector: [{ scheme: 'file', language: 'makefile' }],
      outputChannel: output,
      traceOutputChannel: output,
      revealOutputChannelOn: RevealOutputChannelOn.Never,
      initializationFailedHandler: () => false,
    });
    client = next;
    const stateListener = next.onDidChangeState(event => {
      if (client !== next || disposed) return;
      if (event.newState === State.Running) showStatus('Rumk', 'Makefile diagnostics, fixes, formatting, and symbols are ready.');
      if (event.newState === State.Stopped) showStatus('Rumk: stopped', 'Use Rumk: Restart Language Server to reconnect.', vscode.LanguageStatusSeverity.Error);
    });
    try {
      await next.start();
    } catch (error) {
      stateListener.dispose();
      throw error;
    }
  }

  function scheduleRestart(): Promise<void> {
    pending = pending.then(restart).catch((error: unknown) => {
      if (disposed) return;
      const message = error instanceof Error ? error.message : String(error);
      output.appendLine(`Unable to start Rumk: ${message}`);
      showStatus('Rumk: unavailable', 'Check the server log. Clear rumk.path to use the bundled server.', vscode.LanguageStatusSeverity.Error);
      void vscode.window.showErrorMessage(
        'Rumk could not start. Clear rumk.path to use the bundled server, or check that your custom executable supports “rumk server”.',
        'Open Settings', 'Show Output',
      ).then(async choice => {
        if (choice === 'Open Settings') await vscode.commands.executeCommand('rumk.openSettings');
        if (choice === 'Show Output') output.show();
      });
    });
    return pending;
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('rumk.restartServer', scheduleRestart),
    vscode.commands.registerCommand('rumk.showOutput', () => output.show()),
    vscode.commands.registerCommand('rumk.openSettings', () => vscode.commands.executeCommand('workbench.action.openSettings', '@ext:rvben.rumk')),
    vscode.commands.registerCommand('rumk.fixAll', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'makefile' || editor.document.uri.scheme !== 'file') return;
      await vscode.commands.executeCommand('editor.action.codeAction', { kind: 'source.fixAll.rumk', apply: 'first' });
    }),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration('rumk.path') || event.affectsConfiguration('rumk.enable')) void scheduleRestart();
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => { void scheduleRestart(); }),
    vscode.workspace.onDidGrantWorkspaceTrust(() => { void scheduleRestart(); }),
  );
  await scheduleRestart();
}

export async function deactivate(): Promise<void> {
  disposed = true;
  await pending;
  const previous = client;
  client = undefined;
  await previous?.dispose();
}
