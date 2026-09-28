# Changelog

## 0.0.2

- Bundle Rumk 0.1.0. See the [Rumk changelog](https://github.com/rvben/rumk/blob/main/CHANGELOG.md#010---2026-09-28) for new rules, per-Makefile configuration and `pyproject.toml` support.
- Keep formatting, quick fixes, symbols and diagnostics working while builds or other tools write files in the workspace. Disk changes no longer cancel pending requests.
- Report an unknown rule in a `# rumk-` comment as an error on that line instead of clearing every document's diagnostics and showing an error popup while the rule name is being typed.

## 0.0.1

- Bundle the native Rumk executable in platform-specific VSIX packages; no separate installation is required.
- Build the bundled server from the same checkout and verify its checksum and target before packaging.
- Use `rumk.path` only as an explicit override for the bundled executable.

- Connect VS Code's Makefile language to the native `rumk server`.
- Provide live diagnostics, quick fixes, fix-all, formatting, and target/variable symbols.
- Add language status, restart, output, and settings commands.
- Support local and remote filesystem workspaces with workspace trust enforcement.
- Include Makefile search metadata in the Linters, Formatters, and Programming Languages categories.
