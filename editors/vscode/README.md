# Rumk for Visual Studio Code

**Find the Makefile bug before you run Make.**

Rumk brings its native Makefile analyzer into VS Code: live diagnostics, quick
fixes, formatting, and an outline of targets and variables. Analysis follows
includes and understands unsaved Makefile buffers across workspace folders.
It never invokes Make or runs recipes.

## Get started

1. Install **Rumk — Makefile Diagnostics & Formatting** from the VS Code Marketplace.
2. Open a Makefile in a trusted workspace.

That’s it. The extension includes its own Rumk executable: no Cargo, Python,
separate CLI installation, or startup download is required. Problems appear in
the editor and Problems panel; use the lightbulb for fixes and **Format Document**
for layout. For a downloaded VSIX, use **Extensions: Install from VSIX**.

Platform packages cover macOS (Intel and Apple Silicon), Linux (x64 and ARM64),
and Windows (x64). Linux binaries are statically linked with musl. VS Code selects
the package for the machine running the extension, including remote workspaces.
Windows ARM64 and Alpine-specific Marketplace packages are not currently built.

## Makefiles, built right

- **Live diagnostics:** see syntax problems, suspicious dependencies, portability
  issues, and enabled style checks while editing.
- **Quick fixes:** apply individual fixes with the lightbulb. Fixes are versioned
  so they cannot overwrite newer edits.
- **Fix all:** run **Rumk: Fix All Applicable Problems**, use the editor context
  menu, or opt into fixes on save.
- **Formatting:** use **Format Document**, **Format Document With…**, or enable
  format on save. Rumk preserves meaningful whitespace and line endings.
- **Outline:** browse targets and variables in VS Code's Outline and Go to Symbol.
- **Includes:** open file buffers participate in include analysis, and changes
  to files or configuration refresh diagnostics.
- **Server status:** the editor's language status shows readiness or errors;
  **Rumk: Show Output** opens the server log.

Rumk uses the same project configuration and fix policy as its CLI. Fixes are
safe by default; a project that explicitly enables unsafe fixes also enables
those applicable actions in the editor. Edits stay in the buffer until you save.

This extension helps diagnose Makefile bugs. It does not provide breakpoints,
recipe stepping, or a Debug Adapter Protocol implementation.

## Settings

```jsonc
{
  "rumk.path": "",
  "[makefile]": {
    "editor.defaultFormatter": "rvben.rumk",
    "editor.formatOnSave": true,
    "editor.codeActionsOnSave": {
      "source.fixAll.rumk": "explicit"
    }
  }
}
```

| Setting | Default | Purpose |
| --- | --- | --- |
| `rumk.enable` | `true` | Start or stop all Rumk language features. |
| `rumk.path` | empty | Use bundled Rumk; set a path or command to override. |
| `rumk.trace.server` | `off` | Protocol logging: `off`, `messages`, or `verbose`. |

The bundled executable is always used when `rumk.path` is empty, even if an older
Rumk is installed elsewhere. Set `"rumk.path": "rumk"` to explicitly use `PATH`, or
supply the path to another executable supporting `rumk server`.

A relative executable path and `${workspaceFolder}` resolve against the first
workspace folder. One server handles all folders, with configuration discovered
separately from each Makefile. Folder-specific executable settings are not used.
Paths containing spaces work without quotes; arguments, `~`, and shell variables
are not expanded. Use Rumk's TOML configuration for rule selection and fix policy.

The extension uses VS Code's built-in `makefile` language. If a custom include
filename opens as plain text, select **Makefile** in the language picker or add a
`files.associations` entry. Save untitled documents to a file to enable analysis.

## Troubleshooting

Run **Rumk: Show Output** to see the executable path and server log. If you set a
custom `rumk.path`, clear it to return to the bundled server, or verify that your
executable supports `rumk server`. Changes restart the server automatically.
**Rumk: Restart Language Server** reconnects manually. Reinstall the extension
if its bundled executable is missing or damaged.

In SSH, WSL, and containers, install the extension on the remote side: VS Code
selects the matching package and its bundled executable. A custom `rumk.path`
must refer to an executable in that remote environment. Read-only virtual workspaces and
untrusted workspaces are unsupported. Protocol tracing is off by default;
verbose logs contain buffer contents, so review them before sharing.

## Develop and package

From the repository root:

```sh
cd editors/vscode
npm ci
npm test
npm run bundle
npm run test:integration
npm run package:pre-release
```

Node.js 22 or newer and a Rust toolchain are required for development. On Linux,
install `musl-tools` (or your distribution’s musl compiler) and the matching Rust
target: `rustup target add x86_64-unknown-linux-musl` or
`rustup target add aarch64-unknown-linux-musl`.

`npm run bundle` builds the Rust server from this checkout in release mode for
the current host, records its version, source revision, dirty-source status, and
SHA-256 checksum, and collects its dependency license notices. The generated
`bundled/` and `.build/` directories are local build output and must not be committed.

`npm run package` and `npm run package:pre-release` rebuild and verify the bundle,
then produce `rumk-<version>-<platform>-<architecture>.vsix`. Build each package
on its native host; do not publish a platform binary in a universal VSIX.
The extension CI builds and tests all five supported platforms.

Integration tests launch an
isolated VS Code profile with the actual extension and bundled Rust server,
with no executable override by default. They verify
multi-folder diagnostics, fixes to unsaved buffers, formatting, symbols,
restart, disable/enable behavior, and recovery from an invalid executable path. Set `RUMK_TEST_BINARY` to test another
binary, or `VSCODE_EXECUTABLE_PATH` to use an existing VS Code executable.
The default test host is the minimum supported VS Code 1.91.1.

The VSIX includes the extension code, language client, native Rumk executable,
and dependency license notices. The extension has an independent version and
changelog. Marketplace publication is a separate, explicit step.
The `rvben` publisher must exist and be authorized before publishing.

The manifest includes the `Debuggers` category and Makefile keywords for
`@category:debuggers Makefile` discovery after publication and indexing.
Marketplace ranking and exact search placement are not guaranteed.

### Publish a preview

The **VS Code extension** GitHub Actions workflow supports manual dispatch.
It builds and tests all five native platforms before retaining their VSIX files.
With **publish** enabled, it verifies that the complete package set contains the
same clean source revision, then publishes to Visual Studio Marketplace using
the repository's `VSCE_PAT` secret. Ordinary pushes and pull requests never publish.
Use the existing publisher credential; do not paste tokens into issues or logs.
