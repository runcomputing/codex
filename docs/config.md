# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Add a custom status-line command

To show output from an executable in the Codex TUI status line, add the
`custom-command` item and its argv vector to your user-level
`~/.codex/config.toml`:

```toml
[tui]
status_line = ["model-with-reasoning", "current-dir", "custom-command"]
status_line_command = ["my-status-provider", "--format=plain"]
```

The first value is executed directly and the remaining values are its arguments;
Codex does not invoke a shell. The command runs from the active working directory
and receives `CODEX_STATUS_LINE_CWD` plus `CODEX_THREAD_ID` when a thread is active.
Print a single status line to standard output. Codex refreshes the value periodically
and omits it if the command fails or times out.

For safety, enable `custom-command` and configure `status_line_command` only in
user-level configuration. Project-local configuration removes `custom-command`
from `status_line` and ignores `status_line_command`.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
