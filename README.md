# widget-usage-windows

> **Name is provisional.** `widget-usage-windows` is a working name for the repository and app; it may change before the first release.

A tiny, always-visible strip pinned over the **left corner of the Windows 11 taskbar** that shows, at a glance:

1. **Hardware usage**, like the *Performance* tab of Task Manager: CPU, RAM, GPU, and the active time of each physical disk.
2. **Claude Code and Codex subscription limits**: the percentage **used** of the 5-hour window and of the weekly window.

No panel to open, no window to switch to. Just glance at the taskbar.

<!-- TODO: add a screenshot (e.g. docs/images/screenshot.png) once the first build exists. -->
> **Screenshot coming soon.** Planned look: a compact strip where each item has a small label on top, the value in bold, and a thin progress bar underneath, e.g. `CPU 12%` · `RAM 48%` · `GPU 5%` · `C: 3%` | `Claude 5h 62%` · `wk 31%` | `Codex 5h 20%` · `wk 9%`.

## Status

**Early development / design phase.** There is no runnable build yet. The current design (written in Portuguese) lives in [`docs/superpowers/specs/`](docs/superpowers/specs/). Feedback on the design is welcome via issues.

## Why

Know, without interrupting your work, whether your PC is under load and how much of your AI coding quota is left before you start a big task.

## Goals

- **Truly lightweight**: a single small `.exe` (a few MB), no runtime to install, roughly 10–20 MB of RAM and near-zero CPU when idle.
- **Standalone**: download and run. The app itself offers "Start with Windows".
- **Native**: written in Rust using raw Win32 via the [`windows`](https://crates.io/crates/windows) crate, drawn with Direct2D/DirectWrite.
- **Honest numbers**: an expired token or a broken endpoint shows a clear error state, never a fake `0%` or silently stale data.

## Planned features

- **Hardware items**: CPU, RAM, GPU (busiest engine, like Task Manager), and per-disk active time (one item per physical disk, labeled with its drive letters).
- **Subscription limits**:
  - **Claude Code**: 5-hour and weekly usage, using your existing `claude` CLI login.
  - **Codex**: 5-hour and weekly usage, using your existing `codex` CLI login.
- **Toggles**: every item and every provider can be turned on or off independently. Disabled items disappear and the strip shrinks.
- **Color cues**: progress bars turn yellow near the limit and red when close to it.
- **Hover** for a tooltip with reset times, last update time, and error details.
- **Right-click** for a menu: **Settings**, **Refresh now**, **Start with Windows**, **Exit**.
- **Plays nice with the taskbar**: hides for full-screen apps, follows auto-hide, follows light/dark theme, and is per-monitor DPI aware.

### Not planned (for v1)

- Windows 10, macOS, or Linux support.
- A Windows Widgets board (Win+W) widget.
- Its own login screen or OAuth flow.
- Usage history or charts over time.

## Requirements

- Windows 11 (x64). ARM64 is desirable but not guaranteed for v1.
- No administrator privileges required.
- For the limits items: a logged-in [Claude Code](https://docs.anthropic.com/en/docs/claude-code) and/or Codex CLI on the same Windows user account.

## Installation (planned)

Nothing to install yet. The plan is:

1. **GitHub Releases**: download a single `.exe` and run it. No installer, no runtime dependencies (the CRT is statically linked).
2. **winget**: a package will follow after the first stable releases.

Use **Start with Windows** in the right-click menu to launch it at sign-in (stored under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).

## Privacy

- **No telemetry.** The app does not phone home, collect analytics, or check for updates in the background.
- **Network access is limited** to the providers' own usage endpoints, and only for the providers you enable.
- **Credentials are read-only.** The app reads `~/.claude/.credentials.json` and `~/.codex/auth.json` (respecting `CODEX_HOME`) but never writes to them, never refreshes tokens, and never competes with the official CLIs.
- **Credentials are never logged, copied, or sent anywhere** except to the corresponding provider's own endpoint.
- Settings are stored locally in `%APPDATA%`.

## Disclaimer

- This project is **not affiliated with, endorsed by, or sponsored by Anthropic or OpenAI**. "Claude", "Claude Code", "Codex", and related names are trademarks of their respective owners.
- The subscription-limit data comes from **unofficial, undocumented endpoints** (the same ones the official CLIs use for their own usage/status commands). They **may change or break at any time** without notice. When that happens, the app shows an error state rather than wrong numbers.
- Use at your own risk, and make sure your usage complies with the terms of service of each provider.

## Contributing

Contributions of all kinds are welcome: bug reports, design feedback, docs, and code. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md) first. For security issues, see [SECURITY.md](SECURITY.md) and do **not** open a public issue.

## License

[MIT](LICENSE) © Pablo Chiaro Rosa
