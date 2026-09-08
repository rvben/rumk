import * as path from 'node:path';

/** Arguments are passed directly to spawn, never interpreted by a shell. */
export function serverCommand(configured: string, workspaceFolder?: string): string {
  let command = configured.trim();
  if (!command) throw new Error('Set rumk.path to a Rumk executable or a command on PATH.');
  if (command.includes('${workspaceFolder}')) {
    if (!workspaceFolder) throw new Error('rumk.path uses ${workspaceFolder}, but no workspace folder is open.');
    command = command.replaceAll('${workspaceFolder}', workspaceFolder);
  }
  if (!path.isAbsolute(command) && (command.includes('/') || command.includes('\\'))) {
    if (!workspaceFolder) throw new Error('Use an absolute rumk.path when no workspace folder is open.');
    command = path.resolve(workspaceFolder, command);
  }
  return command;
}

/** Prefer the tested, bundled server unless the user explicitly overrides it. */
export function resolveServerCommand(configured: string, extensionPath: string, workspaceFolder?: string, platform = process.platform): string {
  return configured.trim()
    ? serverCommand(configured, workspaceFolder)
    : path.join(extensionPath, 'bundled', platform === 'win32' ? 'rumk.exe' : 'rumk');
}
