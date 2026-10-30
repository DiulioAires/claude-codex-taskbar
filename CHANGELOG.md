# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-10-30

### Changed
- Renamed to **CCT (claude-codex-taskbar)**: the executable is now `claude-codex-taskbar.exe`.
- Windows 10 is supported (validated), in addition to Windows 11.

### Added
- Automatic upgrade from v0.1.x: closes the old `widget-usage-windows.exe`, copies its settings
  and moves *Start with Windows* to the new executable.
- Experimental, opt-in *Keep login active* for Codex (`codex login status`), alongside Claude Code.

## [0.1.0] - 2026-10-30

### Added
- First release: taskbar strip with CPU, RAM, GPU and per-disk activity, Claude Code and Codex
  5-hour/weekly limits, tooltip with reset times, settings window, start with Windows,
  English and Brazilian Portuguese UI.

[0.2.0]: https://github.com/chiarorosa/claude-codex-taskbar/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/chiarorosa/claude-codex-taskbar/releases/tag/v0.1.0
