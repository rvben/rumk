# Changelog

## 0.0.1

- Bundle the native Rumk executable in platform-specific VSIX packages; no separate installation is required.
- Build the bundled server from the same checkout and verify its checksum and target before packaging.
- Use `rumk.path` only as an explicit override for the bundled executable.

- Connect VS Code's Makefile language to the native `rumk server`.
- Provide live diagnostics, quick fixes, fix-all, formatting, and target/variable symbols.
- Add language status, restart, output, and settings commands.
- Support local and remote filesystem workspaces with workspace trust enforcement.
- Include Makefile search metadata in Debuggers, Linters, and Formatters categories.
