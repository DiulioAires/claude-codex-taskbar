# Provider logos and completion flashes

The strip identifies Claude and Codex with a logo before each provider's gauges, including
compact mode. The 5-hour and weekly indicators remain; account-letter badges are not drawn on the strip.
Hover for the full provider names and usage details.

A local completion hook can flash the corresponding logo **three times over three seconds**.
The other provider, quota numbers, foreground app and usage refresh schedule are unchanged.
The widget uses a short-lived Windows timer, then returns to its normal idle behavior.
Registering a hook is opt-in; no hooks are installed or settings changed automatically.

## Requirements and limits

- Run CCT on the same Windows desktop and user session as your coding tools.
- Use a version of Claude Code or Codex that supports the `Stop` command hook below. Review
  and trust the hook in the coding tool before using it.
- Claude Code's terminal and local **Code tab** sessions in Claude Desktop share hook settings.
  Claude's ordinary Chat and Cowork interfaces are not covered by this integration.
- Codex supports hooks in its CLI and local desktop sessions. Remote/cloud/SSH/WSL sessions
  need a Windows-side bridge, which is not included here.
- `Stop` signals the end of an agent response, not proof that the entire task succeeded.
  Other hooks may request further work. Interruptions and approval requests are not completion
  events. Only configure this hook for the main agent's `Stop`, not `SubagentStop`.
- The provider must be enabled and have a visible gauge. The strip still respects fullscreen,
  taskbar auto-hide and available space; a flash does not force it onto the screen.

## Claude Code

Add this hook to your existing Claude Code settings, merging it with any hooks you already
have. For example, use your user-level `~/.claude/settings.json`. Replace the executable path
with your permanent CCT location. The examples use a path without spaces to work in both
PowerShell and Git Bash; paths with spaces need quoting appropriate to the hook's shell.

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "C:/Tools/CCT/claude-codex-taskbar.exe --task-complete claude",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

Review the change using `/hooks` and restart the session if needed. See the official
[Claude hooks reference](https://code.claude.com/docs/en/hooks) and
[Desktop shared configuration](https://code.claude.com/docs/en/desktop#shared-configuration).

## Codex

Merge this into `~/.codex/hooks.json` (or the equivalent under your custom `CODEX_HOME`).
Keep any existing hook entries. Do not add it again if an equivalent hook is already
configured in `config.toml` or a plugin, or you may receive duplicate completion signals.

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "C:/Tools/CCT/claude-codex-taskbar.exe --task-complete codex",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

Trust the hook through `/hooks` in the CLI or **Settings > Hooks** in the desktop app.
Older Codex versions without this hook interface need to be updated; the legacy `notify`
setting is not this command's interface. See the official
[Codex hooks documentation](https://learn.chatgpt.com/docs/hooks).

## Test the signal

With CCT already running, call either command from PowerShell:

```powershell
& 'C:\Tools\CCT\claude-codex-taskbar.exe' --task-complete claude
& 'C:\Tools\CCT\claude-codex-taskbar.exe' --task-complete codex
```

The corresponding logo flashes three times without opening Settings or taking focus.
If CCT is closed, the command is a harmless no-op and does not start it. Invalid arguments
exit with code 1. Successful hook invocations return `{}` when stdout is captured, as expected
by `Stop` hooks, and do not instruct the coding tool to continue or stop work.

## Privacy

The command sends only a fixed provider number to CCT's existing window. It does not parse
the hook payload, read transcripts, inspect credentials, make network calls, or save task
information. Logos are embedded locally; no assets are downloaded at runtime. CCT does not
need an additional process or runtime to remain running for this feature.
