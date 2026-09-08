# Native editor integration

Run `rumk server` as a stdio Language Server Protocol 3.17 server. Connect your
editor's Makefile LSP client to that command; stdout contains only framed JSON-RPC.
The server provides diagnostics, quick fixes, `source.fixAll.rumk`, document
formatting, and target/variable symbols. It uses the same configuration discovery,
rules, suppressions, fix allowlists, and safety policy as the CLI. Start with
`rumk --config path/to/rumk.toml server` or `rumk --no-config server` to override
discovery. Formatting uses the safe layout rules selected in your configuration.

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

Analysis runs on a background worker with cached results for the current buffer
generation. New diagnostic jobs coalesce, stale responses are discarded, and
request cancellation is handled without waiting for analysis. Requests are
bounded to 64 pending jobs. Limits are 128 open documents, 8 MiB per buffer or
message, and 32 MiB of open text. Only local `file:` URIs are accepted. Unsupported
requests receive the standard method-not-found response. Shutdown followed by
exit returns status 0; an unexpected EOF or exit returns status 1.
