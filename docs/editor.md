# Native editor integration

## Visual Studio Code

The [Rumk extension](https://marketplace.visualstudio.com/items?itemName=rvben.rumk)
connects VS Code to this server with live diagnostics, quick fixes, fix-all,
formatting, and target/variable symbols. Install its preview from the
Marketplace, or build a local VSIX from
`editors/vscode`; see its [README](../editors/vscode/README.md) for setup and
development instructions. Each platform-specific VSIX bundles the native
server; no separate Rumk installation is required.

Click a diagnostic's rule code in the hover or Problems panel to open the rule
documentation. Pages include manual repair guidance for findings without a
quick fix. An intentional exception can use rule configuration or an inline
suppression; a missing quick fix can also reflect the project's fix policy or
incomplete analysis context.

## Language server

Run `rumk server` as a stdio Language Server Protocol 3.17 server. Connect your
editor's Makefile LSP client to that command; stdout contains only framed JSON-RPC.
The server provides diagnostics, quick fixes, `source.fixAll.rumk`, document
formatting, and target/variable symbols. It uses the same configuration discovery,
rules, suppressions, fix allowlists, and safety policy as the CLI. Start with
`rumk --config path/to/rumk.toml server` or `rumk --no-config server` to override
discovery. Both CLI and editor resolve configuration from each Makefile's directory,
including `[tool.rumk]` in nested `pyproject.toml` files. An explicit `--config` applies to
all files. Formatting uses the safe layout rules selected in your configuration.

Open buffers override disk contents throughout the include graph, including new
unsaved files named by static includes. Wildcard discovery still uses disk. Buffer changes invalidate dependent diagnostics. Closing a
buffer restores its disk contents and clears stale findings. Watched-file events
reload includes and configuration; the server registers a workspace file watcher
when the client supports dynamic registration. Clients without that capability
can send watched-file notifications themselves; saving a document also refreshes
analysis. Unsaved configuration buffers are not configuration overrides.

Positions and incremental changes use UTF-16, including non-BMP characters.
Diagnostics carry document versions. Fix actions use versioned workspace edits
so a client can reject changes to a newer buffer, and require client support for
literal code actions and `workspaceEdit.documentChanges`. Formatting follows the
standard unversioned formatting response contract. The client applies edits;
the server never writes source files or invokes Make.

Quick fixes match the selected diagnostic or edit span, including a cursor inside
the affected text. Invalid or reversed ranges return invalid-params errors.
Fix-all reanalyzes interacting edits until stable, retaining all unsaved include
buffers and the configured fix safety policy. It returns one atomic, versioned
replacement; cycles, oversized results, and failure to stabilize within ten
passes return errors instead of partial edits.

Analysis runs on a background worker with cached results for the current buffer
generation. New diagnostic jobs coalesce, stale responses are discarded, and
request cancellation is handled without waiting for analysis. Requests are
bounded to 64 pending jobs. Limits are 128 open documents, 8 MiB per buffer or
message, and 32 MiB of open text. Only local `file:` URIs are accepted. Unsupported
requests receive the standard method-not-found response. Shutdown followed by
exit returns status 0; an unexpected EOF or exit returns status 1.

Malformed JSON inside a complete frame receives a parse-error response without
discarding open buffers or ending the session. Invalid IDs, methods, and scalar
parameters receive protocol errors before they can change server state. Lifecycle
requests and notifications retain their distinct roles. Invalid or truncated
framing remains fatal because the next message boundary cannot be trusted.
