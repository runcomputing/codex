# Using This Codex Fork

This fork carries the MCP channel notification changes plus a LaunchAgent plist
that points the Codex desktop app at a locally built Codex CLI.

## Build the fork

```sh
git clone git@github.com:suparngp/codex.git
cd codex
git switch main
cd codex-rs
cargo build -p codex-cli --bin codex
```

The debug binary will be:

```text
/Users/suparngupta/Code/codex/codex-rs/target/debug/codex
```

Run it directly with:

```sh
/Users/suparngupta/Code/codex/codex-rs/target/debug/codex
```

## Use the fork from Codex desktop

The LaunchAgent in this folder sets `CODEX_CLI_PATH` for GUI apps launched from
your macOS user session.

Install or refresh it with:

```sh
mkdir -p "$HOME/Library/LaunchAgents"
cp extras/com.suparn.codexenv.plist "$HOME/Library/LaunchAgents/com.suparn.codexenv.plist"
launchctl bootout "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.suparn.codexenv.plist" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.suparn.codexenv.plist"
launchctl kickstart -k "gui/$(id -u)/com.suparn.codexenv"
launchctl getenv CODEX_CLI_PATH
```

Then fully quit and reopen Codex desktop. It should launch app-server through
the local debug binary instead of the bundled CLI.

## Configure Talk MCP

Add Talk to `~/.codex/config.toml`:

```toml
[mcp_servers.runcomputing-talk]
enabled = true
command = "sh"
args = [
  "-c",
  'exec npx -y @runcomputing/talk@latest mcp start --token "$(npx -y @runcomputing/talk@latest auth login)"',
]
env_vars = ["GITHUB_TOKEN"]

[mcp_servers.runcomputing-talk.env]
TALK_BROKER_URL = "http://127.0.0.1:18787"
```

Restart Codex after changing MCP config.

## Expected channel behavior

An MCP stdio server that advertises experimental capability `codex/channel` can
send notifications with method `notifications/codex/channel` and params:

```json
{
  "content": "hello",
  "meta": {
    "id": "1780621571436",
    "arrived_on_channel": "codex",
    "reply_to_channel": "user",
    "from": "user",
    "sent_at": "1780621571436",
    "ack_required": "true"
  }
}
```

Codex injects that as a user message shaped like:

```xml
<channel source="runcomputing-talk" ack_required="true" arrived_on_channel="codex" from="user" id="1780621571436" reply_to_channel="user" sent_at="1780621571436">hello</channel>
```

For Talk messages with `ack_required="true"`, call Talk's `ack` tool with the
message id.
