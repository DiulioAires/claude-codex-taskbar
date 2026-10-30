# widget-usage-windows

> **Name is provisional.** `widget-usage-windows` is a working name for the repository and app; it may change before the first release.

A tiny, always-visible strip pinned over the **left corner of the Windows 11 taskbar** that shows, at a glance:

1. **Hardware usage**, like the *Performance* tab of Task Manager: CPU, RAM, GPU, and the active time of each physical disk.
2. **Claude Code and Codex subscription limits**: the percentage **used** of the 5-hour window and of the weekly window.

No panel to open, no window to switch to. Just glance at the taskbar.

![The strip on the left corner of the Windows 11 taskbar](docs/screenshot.png)

## Status

**Early development (v0.1).** The strip, the hardware and limit readings, the tooltip, the menu, and the settings window work. Expect rough edges; bug reports are very welcome. The design (written in Portuguese) lives in [`docs/superpowers/specs/`](docs/superpowers/specs/).

## Why

Know, without interrupting your work, whether your PC is under load and how much of your AI coding quota is left before you start a big task.

## Goals

- **Truly lightweight**: a single small `.exe` (a few MB), no runtime to install, roughly 10–20 MB of RAM and near-zero CPU when idle.
- **Standalone**: download and run. The app itself offers "Start with Windows".
- **Native**: written in Rust using raw Win32 via the [`windows`](https://crates.io/crates/windows) crate, drawn with Direct2D/DirectWrite.
- **Honest numbers**: an expired token or a broken endpoint shows a clear error state, never a fake `0%` or silently stale data.

## Features

- **Hardware items**: CPU, RAM, GPU (busiest engine, like Task Manager), and per-disk active time (one item per physical disk, labeled with its drive letters).
- **Subscription limits**:
  - **Claude Code**: 5-hour and weekly usage, using your existing `claude` CLI login.
  - **Codex**: 5-hour and weekly usage, using your existing `codex` CLI login.
- **Toggles**: every item and every provider can be turned on or off independently. Disabled items disappear and the strip shrinks.
- **Color cues**: progress bars turn yellow near the limit and red when close to it.
- **Hover** for a tooltip with reset times, last update time, and error details.
- **Right-click** for a menu: **Settings**, **Refresh now**, **Start with Windows**, **Exit**.
- **Plays nice with the taskbar** (see below).

### Stays out of your way

- **Full screen:** the strip hides while a full-screen app (a YouTube video, a game, a presentation) is on the taskbar's monitor and comes back as soon as you leave full screen. Full screen on another monitor leaves it alone.
- **Display scaling:** it follows Windows display scaling (100%, 125%, 150%, …) per monitor, and stays sharp after a change.
- **Light and dark theme:** text and bar colors follow the Windows theme, switching live.
- **Taskbar icons:** it never covers the taskbar icons. When apps open and the centered icons shift, the strip shrinks (compact mode, then `…`).
- **Auto-hide and Explorer restarts:** it hides together with an auto-hidden taskbar and comes back on its own after Explorer restarts.
- **Sleep:** after resume it fetches fresh limits right away and never shows old numbers as current.

### Not planned (for v1)

- Windows 10, macOS, or Linux support.
- A Windows Widgets board (Win+W) widget.
- Its own login screen or OAuth flow.
- Usage history or charts over time.

## Requirements

- Windows 11 (x64). ARM64 is desirable but not guaranteed for v1.
- No administrator privileges required.
- For the limits items: a logged-in [Claude Code](https://docs.anthropic.com/en/docs/claude-code) and/or Codex CLI on the same Windows user account.

## Installation

1. Download `widget-usage-windows.exe` from the [latest release](../../releases/latest). It is a single file (under 1 MB): no installer, no runtime dependencies.
2. Put it somewhere permanent (for example `%LOCALAPPDATA%\Programs\widget-usage-windows\`) and run it.
3. Right-click the strip → **Start with Windows** to launch it at sign-in (stored under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).

A `winget` package will follow after the first stable releases.

### "Windows protected your PC" (SmartScreen)

Release builds are not code-signed yet, so SmartScreen may warn about an unrecognized app. Click **More info → Run anyway**. To make sure the file is the one published here, compare its hash with the `.sha256` file attached to the release:

```powershell
Get-FileHash .\widget-usage-windows.exe -Algorithm SHA256
```

### Building from source

```powershell
cargo build --release   # target\release\widget-usage-windows.exe
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the toolchain setup.

## Usage

- **Free the left corner.** The strip lives where the Windows Widgets (weather) button sits. Turn that button off in *Settings → Personalization → Taskbar → Widgets*. Keep the taskbar icons **centered** (*Taskbar behaviors → Taskbar alignment*); with left-aligned icons there is no room and the strip stays hidden.
- **Not enough room?** The strip first drops the labels (compact mode), then hides items from the end and shows `…`. Hover to see what was cut, or turn items off in **Settings**.
- **Toggle providers and items** in **Settings** (right-click → Settings). On first run, Claude and Codex are enabled automatically if their CLI login is found.
- **Claude shows `—` after a few hours?** Claude Code's login token lasts about 8 hours and is renewed only by Claude Code itself. The app never refreshes it (so it can never log the CLI out); open Claude Code and the numbers come back on the next refresh.
- **Keep login active (experimental, off by default), for Claude Code and Codex.** If you opt in (the app asks once on first run; also in **Settings**, one switch per provider), when a token has expired the app runs the CLI's own status command hidden (`claude auth status` or `codex login status`), at most every 15 minutes per provider, so the official CLI renews its own login. It sends no prompt and uses no quota. The tooltip tells you whether the renewal worked. **Feedback wanted:** please open an issue saying whether it renewed your login.
- **Codex offline?** If the endpoint fails, the strip falls back to the last limits the Codex CLI wrote to its session logs, dimmed, as long as that window has not reset yet.

## Privacy

- **No telemetry.** The app does not phone home, collect analytics, or check for updates in the background.
- **Network access is limited** to the providers' own usage endpoints, and only for the providers you enable.
- **Credentials are read-only.** The app reads `~/.claude/.credentials.json` and `~/.codex/auth.json` (respecting `CODEX_HOME`) but never writes to them, never refreshes tokens, and never competes with the official CLIs.
- **Credentials are never logged, copied, or sent anywhere** except to the corresponding provider's own endpoint.
- **No other programs are run**, unless you turn on *Keep login active*, which runs only `claude auth status` and/or `codex login status` (hidden, output discarded).
- Settings are stored locally in `%APPDATA%`.

## Disclaimer

- This project is **not affiliated with, endorsed by, or sponsored by Anthropic or OpenAI**. "Claude", "Claude Code", "Codex", and related names are trademarks of their respective owners.
- The subscription-limit data comes from **unofficial, undocumented endpoints** (the same ones the official CLIs use for their own usage/status commands). They **may change or break at any time** without notice. When that happens, the app shows an error state rather than wrong numbers.
- Use at your own risk, and make sure your usage complies with the terms of service of each provider.

## Contributing

Contributions of all kinds are welcome: bug reports, design feedback, docs, and code. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md) first. For security issues, see [SECURITY.md](SECURITY.md) and do **not** open a public issue.

## License

[MIT](LICENSE) © Pablo Chiaro Rosa
