# Taskbar Usage Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single small Rust `.exe` that pins a compact strip over the left corner of the Windows 11 taskbar showing CPU/RAM/GPU/per-disk activity and Claude Code / Codex 5h + weekly limit usage.

**Architecture:** One binary crate with a `lib` target so all pure logic (parsers, state machine, layout, colors, config, i18n) is unit-tested without Win32. Win32 shell code (window, Direct2D drawing, taskbar geometry, PDH sampling, settings dialog) is thin and verified manually. The UI thread owns the window and samples hardware on a 1 s timer; one background thread fetches limits and posts results to the window with `PostMessageW(WM_APP_LIMITS)`.

**Tech Stack:** Rust stable (MSVC target `x86_64-pc-windows-msvc`), `windows` crate (Win32 + Direct2D/DirectWrite + PDH + UI Automation), `serde` + `serde_json`, `ureq` (rustls, no default features beyond `tls`/`json`). No other runtime dependencies.

**Spec:** `docs/superpowers/specs/2026-10-30-taskbar-usage-widget-design.md` (Portuguese; section numbers below refer to it).

## Global Constraints

- Windows 11 x64; no admin rights; no runtime dependency (.NET, VC++ redist, WebView2). Static CRT: `.cargo/config.toml` → `[target.x86_64-pc-windows-msvc] rustflags = ["-C", "target-feature=+crt-static"]`.
- Release profile: `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`. Target: release `.exe` ≤ 4 MB; idle RAM ≈ 10–20 MB; idle CPU ≈ 0%.
- Credentials are **read-only**: never write `.claude/.credentials.json` or `.codex/auth.json`, never refresh tokens, never log/print/persist tokens, send them only to the provider's own endpoint.
- Network: only `https://api.anthropic.com/api/oauth/usage` and `https://chatgpt.com/backend-api/wham/usage`. No telemetry.
- Never show `0%` for an error: error states render value `—` with a grey bar.
- Colors: hardware `#4cc2ff`, Claude `#d97757`, Codex `#10a37f`; bar ≥ 75% → amber `#e8a33d`, ≥ 90% → red `#e5484d`; error grey `#5a5a5a`.
- Intervals: hardware default 1 s (1–5 s), limits default 120 s (min 60 s). Stale window: 15 min.
- Config at `%APPDATA%\widget-usage-windows\config.json`. Autostart value name `widget-usage-windows` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- UI strings in English and pt-BR, picked from the Windows UI language (`GetUserDefaultUILanguage`, primary lang `LANG_PORTUGUESE` → pt-BR, else English). All user-facing text goes through `i18n::t`.
- Code, comments, commits in English; Conventional Commits; small incremental commits; no AI co-author trailers (the maintainer is the sole author).
- Every task must pass `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` before its commit.

## Review Focus

1. **Malformed or partial credential files** (UTF-8 BOM, missing `claudeAiOauth`/`tokens`, empty file, file locked while the CLI writes it): expect `NoLogin` or `Error`, never a panic. Tests in Tasks 4 and 5.
2. **Network down / slow endpoint / non-JSON 5xx body:** UI must never freeze and values degrade to `Stale` then `—`. Pure tests in Task 3; the fetch runs off the UI thread with a 10 s timeout (Task 9).
3. **Disks without a drive letter or hot-plugged** (PDH instance `"2"`, USB added later): label `Disk 2`, list refreshed when the PDH instance set changes. Test in Task 6.
4. **Out-of-range readings** (GPU engine sum > 100, disk idle > 100 giving negative activity, Claude `utilization` > 100): clamp to 0–100. Tests in Tasks 4 and 6.
5. **Explorer restart, DPI or resolution change, taskbar auto-hide, fullscreen apps:** strip repositions, hides and reappears correctly. Pure predicate test in Task 8 and a manual checklist in Task 9.

---

## File Structure

```
Cargo.toml                 crate + release profile + deps
.cargo/config.toml         static CRT
build.rs                   embeds app manifest (PerMonitorV2 DPI, Common Controls v6) + icon
res/app.manifest, res/app.ico, res/app.rc
src/main.rs                entry: single-instance mutex, COM init, run ui::app
src/lib.rs                 pub mod declarations
src/i18n.rs                Lang, Key, t()
src/config.rs              Config, load/save, defaults
src/timeutil.rs            parse_rfc3339, format_local_hhmm (Win32)
src/model.rs               ItemId, Kind, ItemView, bar_color()
src/limits/mod.rs          WindowUsage, ProviderUsage, ProviderStatus, FetchError, Provider trait
src/limits/state.rs        next_status()
src/limits/http.rs         get_json()
src/limits/claude.rs       credentials + usage parsing + fetch
src/limits/codex.rs        auth parsing + usage parsing + session-log fallback + fetch
src/limits/worker.rs       background thread loop
src/hw/mod.rs              HwSnapshot, Sampler (PDH + GlobalMemoryStatusEx)
src/hw/parse.rs            gpu_max_engine(), disk_label(), disk_active()
src/ui/layout.rs           layout()
src/ui/taskbar.rs          TaskbarInfo, query(), is_fullscreen(), available_width()
src/ui/render.rs           Direct2D/DirectWrite drawing into a layered window
src/ui/strip.rs            strip window proc, timers, tooltip, context menu
src/ui/settings.rs         settings window
src/ui/app.rs              wiring: config, sampler, worker, strip
src/autostart.rs           enable/disable/is_enabled
tests/fixtures/            redacted JSON/JSONL samples
.github/workflows/ci.yml, release.yml
```

---

### Task 1: Toolchain, crate skeleton and CI

**Files:**
- Create: `Cargo.toml`, `.cargo/config.toml`, `build.rs`, `res/app.manifest`, `res/app.rc`, `res/app.ico`, `src/main.rs`, `src/lib.rs`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces: crate name `widget_usage_windows` (lib) and binary `widget-usage-windows.exe`; `windows_subsystem = "windows"` in `main.rs`.

- [ ] **Step 1: Install Rust** (MSVC Build Tools 2022 are already present). Ask the user before installing: `winget install --id Rustlang.Rustup -e`, then `rustup default stable-x86_64-pc-windows-msvc` and `rustup component add clippy rustfmt`. Verify: `cargo --version` prints a version.
- [ ] **Step 2: Create the crate files.** Dependencies: `windows` (pin a current 0.6x version; features grow per task), `serde = { features = ["derive"] }`, `serde_json`, `ureq = { default-features = false, features = ["rustls", "json"] }`; dev-dependency none. `build.rs` compiles `res/app.rc` with `embed-resource`(build-dependency) embedding the manifest (`dpiAwareness` = `PerMonitorV2`, Common Controls 6.0) and the icon. `main.rs` just returns for now.
- [ ] **Step 3: Verify the release build and size.** Run `cargo build --release`; expected: success and `target/release/widget-usage-windows.exe` exists. Run `(Get-Item target\release\widget-usage-windows.exe).Length` — expected < 1 MB at this stage. Check static CRT: `dumpbin /dependents` (from the Build Tools dev shell) lists no `VCRUNTIME140.dll`.
- [ ] **Step 4: Add `ci.yml`**: on push/PR, `windows-latest`, steps `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`.
- [ ] **Step 5: Commit** `chore: add Rust crate skeleton, manifest and CI`.

### Task 2: Config and i18n

**Files:**
- Create: `src/config.rs`, `src/i18n.rs`
- Test: inline `#[cfg(test)]` modules

**Interfaces:**
- Produces:
  - `pub enum ItemId { Cpu, Ram, Gpu, Disk(String), ClaudeFive, ClaudeWeek, CodexFive, CodexWeek }` (lives in `model.rs`, created here as needed; `Disk(String)` key = PDH instance name like `"0 C:"`).
  - `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct Config { hw_interval_secs: u32, limits_interval_secs: u32, claude_enabled: bool, codex_enabled: bool, hidden_items: Vec<String>, first_run_done: bool }` with `#[serde(default)]` on the struct.
  - `Config::default()` → `1, 120, false, false, vec![], false`.
  - `Config::load_from(path: &Path) -> Config` (missing/corrupt → default, never panic), `Config::save_to(&self, path: &Path) -> io::Result<()>` (write to `*.tmp` then rename), `config::default_path() -> PathBuf`.
  - `Config::clamped(self) -> Config`: hw 1–5, limits ≥ 60.
  - `Config::is_visible(&self, id: &ItemId) -> bool` using `item_key(id)`: `"cpu"`, `"ram"`, `"gpu"`, `"disk:<instance>"`, `"claude5h"`, `"claudeweek"`, `"codex5h"`, `"codexweek"`.
  - `pub enum Lang { En, PtBr }`, `pub fn detect_lang() -> Lang`, `pub enum Key {...}`, `pub fn t(lang: Lang, key: Key) -> &'static str`.
- Keys and copy (en / pt-BR) — exact text:
  - `MenuSettings` "Settings" / "Configurações"; `MenuRefresh` "Refresh now" / "Atualizar agora"; `MenuAutostart` "Start with Windows" / "Iniciar com o Windows"; `MenuExit` "Exit" / "Sair"
  - `LabelWeek` "wk" / "sem"
  - `ClaudeExpired` "Claude login expired — open Claude Code to renew" / "Login do Claude expirado — abra o Claude Code para renovar"
  - `ClaudeNoLogin` "Log in by running `claude`" / "Faça login rodando `claude`"
  - `CodexExpired` "Codex login expired — run `codex login`" / "Login do Codex expirado — rode `codex login`"
  - `CodexNoLogin` "Log in by running `codex login`" / "Faça login rodando `codex login`"
  - `NoSubscription` "No subscription limits (API key login)" / "Sem limites de assinatura (login por API key)"
  - `StaleFmt` "Update failed; last data at {}" / "Falha ao atualizar; último dado às {}"
  - `SessionDataFmt` "Data from {} (last session)" / "Dado de {} (última sessão)"
  - `ResetsFmt` "resets at {}" / "reseta às {}"; `UpdatedFmt` "updated at {}" / "atualizado às {}"
  - `NoRecentData` "No recent data" / "Sem dado recente"
  - `HintCenterTaskbar` "Center the taskbar icons for the strip to fit (Settings → Personalization → Taskbar → Taskbar behaviors)" / "Centralize os ícones da barra para a faixa caber (Configurações → Personalização → Barra de tarefas → Comportamentos)"
  - `HintWidgetsButton` "Turn off the Windows Widgets button to free this corner (Settings → Personalization → Taskbar → Widgets)" / "Desative o botão de Widgets do Windows para liberar este canto (Configurações → Personalização → Barra de tarefas → Widgets)"
  - `LoginFound` "login found" / "login encontrado"; settings labels for every toggle and interval field.

- [ ] **Step 1: Write failing tests**: `config_roundtrip` (save then load equals), `config_corrupt_file_gives_default` (file contains `"{nope"`), `config_missing_fields_use_defaults` (`{"claude_enabled":true}` → other fields default), `config_clamps_intervals` (`hw 0 → 1`, `hw 9 → 5`, `limits 10 → 60`), `visibility_respects_hidden_items` (`hidden_items = ["disk:1 D:"]` → `Disk("1 D:")` hidden, `Cpu` visible), `every_key_has_both_languages` (iterate all `Key` variants, both strings non-empty).
- [ ] **Step 2: Run** `cargo test config i18n` — expected FAIL (not defined).
- [ ] **Step 3: Implement** the signatures above. `detect_lang` uses `GetUserDefaultUILanguage() & 0x3ff == 0x16`.
- [ ] **Step 4: Run** `cargo test` — expected PASS.
- [ ] **Step 5: Commit** `feat: add config persistence and en/pt-BR strings`.

### Task 3: Limits model, status state machine and colors

**Files:**
- Create: `src/limits/mod.rs`, `src/limits/state.rs`, `src/model.rs` (add `Kind`, `bar_color`)

**Interfaces:**
- Produces:
  - `pub struct WindowUsage { pub used_pct: f32, pub resets_at: Option<i64> /* unix secs */ }`
  - `pub struct ProviderUsage { pub five_hour: Option<WindowUsage>, pub weekly: Option<WindowUsage> }`
  - `pub enum FetchError { NoLogin, Expired, NoSubscription, Network(String), BadResponse(String) }`
  - `pub enum ProviderStatus { Disabled, Loading, Ok { usage: ProviderUsage, at: i64 }, Stale { usage: ProviderUsage, at: i64, err: FetchError }, FromSession { usage: ProviderUsage, at: i64 }, Failed(FetchError) }`
  - `pub fn next_status(prev: &ProviderStatus, result: Result<ProviderUsage, FetchError>, now: i64) -> ProviderStatus`
  - `pub trait Provider: Send { fn fetch(&self) -> Result<ProviderUsage, FetchError>; fn has_login(&self) -> bool; }`
  - `pub enum Kind { Hardware, Claude, Codex }`, `pub fn bar_color(kind: Kind, pct: Option<f32>) -> u32 /* 0xRRGGBB */`
- Rules for `next_status`: `Ok(u)` → `Ok{u, now}`. `Err(NoLogin|Expired|NoSubscription)` → `Failed(e)` (auth problems are never masked by stale data). `Err(Network|BadResponse)` with prev `Ok`/`Stale` whose `at` ≥ `now − 900` → `Stale{prev usage, prev at, err}`; otherwise `Failed(e)`.

- [ ] **Step 1: Write failing tests**: `ok_result_sets_ok`; `network_error_within_15min_is_stale` (prev Ok at `t`, error at `t+899` → `Stale` keeping `at = t`); `network_error_after_15min_fails` (`t+901` → `Failed(Network)`); `expired_never_stale` (prev Ok at `t`, `Expired` at `t+1` → `Failed(Expired)`); colors: `bar_color(Hardware, Some(10.0)) == 0x4cc2ff`, `bar_color(Claude, Some(74.9)) == 0xd97757`, `bar_color(Codex, Some(75.0)) == 0xe8a33d`, `bar_color(Claude, Some(90.0)) == 0xe5484d`, `bar_color(Codex, None) == 0x5a5a5a`.
- [ ] **Step 2: Run** `cargo test limits::state model` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test` — PASS.
- [ ] **Step 5: Commit** `feat: add limits status model and bar colors`.

### Task 4: Claude provider

**Files:**
- Create: `src/limits/claude.rs`, `src/limits/http.rs`, `src/timeutil.rs`, `tests/fixtures/claude_credentials.json`, `tests/fixtures/claude_usage.json`

**Interfaces:**
- Consumes: Task 3 types.
- Produces:
  - `pub fn parse_rfc3339(s: &str) -> Option<i64>` (accepts `Z`, `+hh:mm`, optional fractional seconds); `pub fn format_local_hhmm(unix: i64) -> String` and `format_local_day_hhmm(unix: i64) -> String` (Win32 `SystemTimeToTzSpecificLocalTime` + `GetTimeFormatEx`/`GetDateFormatEx` with `LOCALE_NAME_USER_DEFAULT`).
  - `pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, FetchError>`: `ureq` agent, 10 s global timeout, `User-Agent: widget-usage-windows/<version>`. 401/403 → `Expired`; other non-2xx → `BadResponse("HTTP <code>")`; transport error → `Network(msg)`; body not JSON → `BadResponse`. Error strings never include headers.
  - `pub struct ClaudeCreds { pub access_token: String, pub expires_at_ms: Option<i64> }`
  - `pub fn parse_claude_creds(text: &str) -> Result<ClaudeCreds, FetchError>` (strips UTF-8 BOM; missing `claudeAiOauth.accessToken` → `NoLogin`).
  - `pub fn parse_claude_usage(v: &serde_json::Value) -> Result<ProviderUsage, FetchError>` (`five_hour`/`seven_day` → `utilization` clamped 0–100, `resets_at` via `parse_rfc3339`; a window that is `null` → `None`; both missing → `BadResponse`).
  - `pub struct ClaudeProvider { pub creds_path: PathBuf }` implementing `Provider`; `ClaudeProvider::default_path() -> PathBuf` = `%USERPROFILE%\.claude\.credentials.json`. `fetch`: file missing → `NoLogin`; `expires_at_ms <= now_ms` → `Expired` **without calling the network**; else GET with `Authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20`.

- [ ] **Step 1: Confirm the real response shape.** Write a throwaway PowerShell script in the session scratchpad (not the repo) that reads the token and calls the endpoint, then prints the JSON with any key containing `token` removed. Save the output, with values changed to fake ones, as `tests/fixtures/claude_usage.json`. Fake `claude_credentials.json`: `{"claudeAiOauth":{"accessToken":"sk-ant-oat01-FAKE","refreshToken":"FAKE","expiresAt":1893456000000}}`. If the real shape differs from the spec (§6.2), adapt `parse_claude_usage` and note it in the commit body.
- [ ] **Step 2: Write failing tests**: `rfc3339_variants` (`"2026-10-30T15:00:00Z"` → `1793372400`; same with `.123+00:00` and `-03:00` → `1793383200`); `creds_ok` (fixture → token `"sk-ant-oat01-FAKE"`, `expires_at_ms = Some(1893456000000)`); `creds_with_bom`; `creds_missing_key_is_no_login` (`{}`); `creds_empty_file_is_no_login` (`""`); `usage_fixture_parses` (values equal fixture); `usage_clamps_over_100` (`utilization: 130` → `100.0`); `usage_null_weekly` (`seven_day: null` → `weekly == None`); `usage_garbage_is_bad_response` (`{"x":1}`); `expired_token_skips_network` (temp creds file with `expiresAt` in the past → `Err(Expired)`; works offline).
- [ ] **Step 3: Run** `cargo test claude timeutil` — FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test` — PASS. Also `cargo test -- --ignored live_claude` with an `#[ignore]` test that calls the real endpoint and asserts `Ok`; run it once manually if the local login is valid.
- [ ] **Step 6: Commit** `feat: add Claude Code limits provider`.

### Task 5: Codex provider with session-log fallback

**Files:**
- Create: `src/limits/codex.rs`, `tests/fixtures/codex_auth.json`, `tests/fixtures/codex_usage.json`, `tests/fixtures/codex_session.jsonl`

**Interfaces:**
- Consumes: Task 3 types; `get_json` from Task 4.
- Produces:
  - `pub struct CodexCreds { pub access_token: String, pub account_id: Option<String>, pub api_key_only: bool }`
  - `pub fn parse_codex_auth(text: &str) -> Result<CodexCreds, FetchError>` (BOM-safe; no `tokens.access_token` but `OPENAI_API_KEY` present → `NoSubscription`; neither → `NoLogin`).
  - `pub fn parse_codex_usage(v: &Value) -> Result<ProviderUsage, FetchError>`: `rate_limit.primary_window` → five_hour, `secondary_window` → weekly; `used_percent`; reset = `reset_at` (unix) or `now + reset_after_seconds`.
  - `pub fn latest_session_limits(sessions_dir: &Path, now: i64) -> Option<(ProviderUsage, i64)>`: scan `**/*.jsonl` newest-modified first (stop after the first file containing a match), take the last line with `payload.rate_limits`. Map `primary`/`secondary` (`used_percent`, `resets_at` unix or `resets_in_seconds` relative to the line's `timestamp`). A window whose reset time is `<= now` becomes `None` (spec §6.7). Returns `(usage, line_timestamp)`; `None` if both windows are `None`.
  - `pub struct CodexProvider { pub home: PathBuf }` implementing `Provider`; `CodexProvider::default_home()` = `%CODEX_HOME%` or `%USERPROFILE%\.codex`. Headers: `Authorization: Bearer <token>`, `ChatGPT-Account-Id: <account_id>` (omitted if absent).
  - `pub fn fetch_with_fallback(p: &CodexProvider, now: i64) -> Result<(ProviderUsage, bool /*from_session*/ , i64), FetchError>`: on `Network`/`BadResponse`, try `latest_session_limits`; on success return `from_session = true`.
- The worker (Task 9) maps `from_session = true` to `ProviderStatus::FromSession`.

- [ ] **Step 1: Confirm real shapes** the same way as Task 4 Step 1 (endpoint and the newest line containing `rate_limits` in `~/.codex/sessions`). Remove tokens, account IDs and prompt content, and use fake values in fixtures.
- [ ] **Step 2: Write failing tests**: `auth_ok`, `auth_api_key_only_is_no_subscription`, `auth_empty_is_no_login`, `auth_with_bom`; `usage_fixture_parses`; `usage_reset_after_seconds` (`reset_after_seconds: 600`, now `1000` → `resets_at = Some(1600)`); `session_fallback_picks_last_line` (two matching lines → values of the second); `session_fallback_drops_expired_window` (primary reset `< now`, secondary future → `five_hour None`, weekly `Some`); `session_fallback_all_expired_is_none`; `session_fallback_ignores_bad_lines` (a non-JSON line in the middle doesn't break parsing).
- [ ] **Step 3: Run** `cargo test codex` — FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test` — PASS (plus an `#[ignore]` `live_codex` test, run once manually).
- [ ] **Step 6: Commit** `feat: add Codex limits provider with session-log fallback`.

### Task 6: Hardware sampler

**Files:**
- Create: `src/hw/mod.rs`, `src/hw/parse.rs`

**Interfaces:**
- Produces:
  - `pub struct DiskReading { pub instance: String, pub label: String, pub active_pct: f32 }`
  - `pub struct HwSnapshot { pub cpu: Option<f32>, pub ram: Option<f32>, pub gpu: Option<f32>, pub disks: Vec<DiskReading> }`
  - `pub struct Sampler` with `Sampler::new() -> Sampler` (never fails; missing counters yield `None`) and `fn sample(&mut self) -> HwSnapshot`.
  - `parse.rs`: `pub fn gpu_max_engine(samples: &[(String, f64)]) -> f32`, `pub fn disk_label(instance: &str) -> String`, `pub fn disk_active(idle_pct: f64) -> f32`.
- Counters (PDH, English names via `PdhAddEnglishCounterW`, one query, `PdhCollectQueryData` each tick; first tick returns `None` for rate counters):
  - CPU `\Processor Information(_Total)\% Processor Utility` (fallback `\Processor(_Total)\% Processor Time`), clamp 0–100.
  - GPU `\GPU Engine(*)\Utilization Percentage` via `PdhGetFormattedCounterArrayW`. `gpu_max_engine` groups instances by the part from `luid_` through `engtype_<x>` (dropping `pid_<n>_`), sums each group, and returns the max clamped to 0–100.
  - Disk `\PhysicalDisk(*)\% Idle Time`, skipping `_Total`. `disk_active = clamp(100 − idle, 0, 100)`. `disk_label("0 C: D:") == "C: D:"`, `disk_label("2") == "Disk 2"`. Rebuild the disk list when the instance set changes: re-enumerate with `PdhEnumObjectItemsW` every 30 s.
  - RAM `GlobalMemoryStatusEx().dwMemoryLoad`.

- [ ] **Step 1: Write failing tests**: `gpu_groups_by_engine_across_pids` (`pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D` = 30 and `pid_2_..._eng_0_engtype_3D` = 25 and `..._eng_3_engtype_VideoDecode` = 40 → `55.0`); `gpu_clamps` (sum 140 → `100.0`); `gpu_empty_is_zero`; `disk_label_letters`, `disk_label_no_letter`; `disk_active_clamps` (`idle 104 → 0.0`, `idle 25 → 75.0`).
- [ ] **Step 2: Run** `cargo test hw` — FAIL.
- [ ] **Step 3: Implement** `parse.rs`, then `Sampler`.
- [ ] **Step 4: Run** `cargo test` — PASS. Add an `#[ignore]` `sampler_smoke` test: two samples 1 s apart; `cpu`/`ram` are `Some` in `0..=100`, `disks` non-empty. Run `cargo test -- --ignored sampler_smoke` — PASS. Compare once by eye with Task Manager (CPU/GPU/disk within a few points).
- [ ] **Step 5: Commit** `feat: add PDH hardware sampler`.

### Task 7: Strip layout

**Files:**
- Create: `src/ui/layout.rs`

**Interfaces:**
- Consumes: `ItemId`, `Kind`.
- Produces:
  - `pub struct ItemMetrics { pub full_w: i32, pub compact_w: i32 }` (measured by the renderer with DirectWrite)
  - `pub enum Mode { Full, Compact }`
  - `pub struct Layout { pub mode: Mode, pub visible: Vec<usize> /* indices into input */, pub separator_after: Option<usize>, pub overflow: bool, pub width: i32 }`
  - `pub fn layout(items: &[(Kind, ItemMetrics)], available: i32, gap: i32, sep_w: i32, ellipsis_w: i32) -> Layout`
- Algorithm (spec §6.7): try Full with all items; if it doesn't fit, try Compact with all; if it still doesn't fit, Compact while dropping items from the end until `items + "…" fits`; `overflow = true` when anything was dropped. A separator sits between the last Hardware item and the first Claude/Codex item, only when both groups are visible. Zero items → `width = 0`.

- [ ] **Step 1: Write failing tests**: `fits_full`; `falls_back_to_compact`; `drops_from_end_with_ellipsis` (assert `visible`, `overflow`, `width <= available`); `separator_only_between_groups` (hardware-only → `None`); `empty_items_zero_width`; `nothing_fits_shows_only_ellipsis_or_nothing` (`available` smaller than the ellipsis → `visible` empty, `width == 0`).
- [ ] **Step 2: Run** `cargo test layout` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test` — PASS.
- [ ] **Step 5: Commit** `feat: add strip layout with compact and overflow modes`.

### Task 8: Taskbar geometry and environment

**Files:**
- Create: `src/ui/taskbar.rs`

**Interfaces:**
- Produces:
  - `pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }`
  - `pub struct TaskbarInfo { pub rect: Rect, pub monitor: Rect, pub dpi: u32, pub autohide: bool, pub visible: bool, pub left_aligned: bool, pub widgets_button_on: bool, pub icons_left: Option<i32> }`
  - `pub fn query() -> Option<TaskbarInfo>`: `FindWindowW("Shell_TrayWnd")`; `GetWindowRect`; `MonitorFromWindow` + `GetMonitorInfoW`; `GetDpiForWindow`; autohide via `SHAppBarMessage(ABM_GETSTATE)`; `visible` = the taskbar rect overlaps the monitor by more than 4 px (auto-hide slid away → false); `left_aligned` from `HKCU\...\Explorer\Advanced\TaskbarAl == 0`; `widgets_button_on` from `TaskbarDa == 1` (missing → true); `icons_left` = left edge of the UI Automation element with AutomationId `StartButton` under the taskbar (any UIA failure → `None`).
  - `pub fn available_width(info: &TaskbarInfo, margin: i32) -> i32`: `icons_left − rect.left − 2*margin` when known; else `(rect.width() / 2) − 300*dpi/96 − 2*margin`; `left_aligned` → `0`; never negative.
  - `pub fn is_fullscreen(fg: Rect, monitor: Rect, fg_is_shell: bool) -> bool`: true when `!fg_is_shell` and `fg` covers `monitor` entirely. `strip.rs` also hides when `SHQueryUserNotificationState` returns `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE`. `fg_is_shell` = the foreground window class is `Progman`, `WorkerW` or `Shell_TrayWnd`.

- [ ] **Step 1: Write failing tests**: `available_uses_icons_left` (rect 0..1920, icons_left 700, margin 8 → `684`); `available_fallback_estimate` (1920 wide, dpi 96, none → `960 − 300 − 16 = 644`); `available_left_aligned_is_zero`; `available_never_negative`; `fullscreen_true_when_covering`; `fullscreen_false_for_desktop` (`fg_is_shell = true`); `fullscreen_false_when_smaller`.
- [ ] **Step 2: Run** `cargo test taskbar` — FAIL.
- [ ] **Step 3: Implement** the pure functions, then `query()`.
- [ ] **Step 4: Run** `cargo test` — PASS. Add an `#[ignore]` `query_smoke` test (`query()` is `Some`, rect non-empty, dpi ≥ 96) and run it.
- [ ] **Step 5: Commit** `feat: add taskbar geometry and fullscreen detection`.

### Task 9: Strip window, rendering, tooltip, menu and wiring

**Files:**
- Create: `src/ui/render.rs`, `src/ui/strip.rs`, `src/ui/app.rs`, `src/limits/worker.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `pub fn run() -> i32` in `ui/app.rs` (called from `main`).
  - `worker::spawn(hwnd: isize, providers: Vec<(Kind, Box<dyn Provider>)>, interval: Arc<AtomicU32>, wake: Arc<(Mutex<bool>, Condvar)>) -> JoinHandle<()>`: for each enabled provider, fetch (Codex via `fetch_with_fallback`), box the result into `Box<(Kind, Result<..>, i64)>` and `PostMessageW(hwnd, WM_APP_LIMITS, 0, ptr)` (UI thread reclaims the box). Waits `interval` seconds or until `wake` is notified ("Refresh now", provider toggled, config change).
  - Message constants: `WM_APP_LIMITS = WM_APP + 1`, `WM_APP_TRAY_REASSERT = WM_APP + 2`; timer IDs `TIMER_HW = 1` (hw interval), `TIMER_TOPMOST = 2` (2000 ms), `TIMER_DISKS = 3` (30 s).
- Behaviour:
  - `main.rs`: single-instance named mutex `Local\widget-usage-windows` (second instance exits silently); `CoInitializeEx(COINIT_APARTMENTTHREADED)`; `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` is also in the manifest.
  - Window: class `WidgetUsageStrip`, styles `WS_POPUP`, ex-styles `WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_NOACTIVATE`. Drawing: 32-bpp premultiplied DIB section + `ID2D1DCRenderTarget` bound to its memory DC, then `UpdateLayeredWindow` with `ULW_ALPHA` (transparent background, taskbar shows through). Hit testing: a background alpha of 1/255 over the strip rect so the mouse is captured.
  - Item look (mockup B, option B in `.superpowers/brainstorm/.../faixa-visual.html`): label 9.5 px (DIPs) grey (`#bbbbbb` dark theme / `#5f5f5f` light theme), value 11 px semibold (white / `#1a1a1a`), bar 30×5 DIPs, radius 3, track `#333333` (dark) / `#d0d0d0` (light); gap 10 DIPs; separator 1×30 DIPs `#3a3a3a`. Compact mode drops the label row. Font `Segoe UI Variable Text`, fallback `Segoe UI`. Labels: `CPU`, `RAM`, `GPU`, `disk_label`, `Claude 5h`, `t(LabelWeek)`, `Codex 5h`, `t(LabelWeek)`. Value `—` for `Failed`/`Loading`; bar dimmed to 50% alpha for `Stale`/`FromSession`.
  - Theme: re-read `SystemUsesLightTheme` on `WM_SETTINGCHANGE` with `"ImmersiveColorSet"`.
  - Position: vertically centered on `TaskbarInfo.rect`, left = `rect.left + 8*dpi/96`, width from `layout(available_width(...))`. Hide (`ShowWindow SW_HIDE`) when `!visible`, fullscreen, or `width == 0`. Recompute on `WM_DISPLAYCHANGE`, `WM_DPICHANGED`, `WM_SETTINGCHANGE`, the `RegisterWindowMessageW("TaskbarCreated")` message, and every `TIMER_TOPMOST` tick (which also calls `SetWindowPos(HWND_TOPMOST, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`).
  - Tooltip: `TOOLTIPS_CLASSW` with `TTF_TRACK` off, multiline (`TTM_SETMAXTIPWIDTH 400`), text rebuilt on hover (`TTN_GETDISPINFOW`). Content: per visible provider, "Claude — 5h 62% · resets at 15:00 / wk 34% · resets at Mon 09:00 / updated at 14:31", or the status message (`ClaudeExpired`, `StaleFmt`, `SessionDataFmt`, `NoRecentData`...); overflowed item names and values; `HintCenterTaskbar` when `left_aligned`; `HintWidgetsButton` when `widgets_button_on`.
  - Right-click: `TrackPopupMenu` with `MenuSettings`, `MenuRefresh`, `MenuAutostart` (checked from `autostart::is_enabled()`), separator, `MenuExit`.
  - First run (`!first_run_done`): `claude_enabled = ClaudeProvider.has_login()`, `codex_enabled = CodexProvider.has_login()`, then `first_run_done = true` and save.

- [ ] **Step 1: Implement `worker.rs`** with a unit test `worker_posts_and_respects_wake` using a fake `Provider` and a fake post function (make the post callback injectable: `spawn_with(post: impl Fn(Box<..>) + Send + 'static, ...)`; `spawn` wraps `PostMessageW`). Assert one result arrives right after spawn and another right after `wake`, well before `interval`.
- [ ] **Step 2: Run** `cargo test worker` — PASS after implementation.
- [ ] **Step 3: Implement `render.rs`, `strip.rs`, `app.rs`, `main.rs`.**
- [ ] **Step 4: Manual verification** (`cargo run --release`), checking each item and noting results in the commit body:
  1. The strip appears on the left of the taskbar matching mockup B; values change every second; CPU/GPU/disk are within a few points of Task Manager.
  2. Claude/Codex show %, the tooltip shows reset times and "updated at".
  3. Rename `.claude\.credentials.json` temporarily → `—` and the `ClaudeNoLogin` tooltip after "Refresh now"; restore it.
  4. Disconnect the network → after 15 min the values become `—` (or with a debug env var `WUW_STALE_SECS=30` read only in debug builds).
  5. Fullscreen video (F11 in a browser on YouTube) → the strip hides; exit → it returns.
  6. Taskbar auto-hide on → the strip hides with the taskbar and returns.
  7. `Stop-Process -Name explorer` → the strip comes back after Explorer restarts.
  8. Change display scale 100% → 150% → the strip is resized and sharp.
  9. Light/dark theme switch updates text colors.
  10. Task Manager: the process uses < 20 MB working set and ~0% CPU after 1 minute.
- [ ] **Step 5: Run** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` — all PASS.
- [ ] **Step 6: Commit** `feat: add taskbar strip window with tooltip and context menu`.

### Task 10: Settings window and autostart

**Files:**
- Create: `src/ui/settings.rs`, `src/autostart.rs`
- Modify: `src/ui/strip.rs` (menu handlers)

**Interfaces:**
- Produces:
  - `autostart::enable() -> io::Result<()>` (writes the quoted current exe path), `disable() -> io::Result<()>`, `is_enabled() -> bool` (true only if the value equals the current exe path, case-insensitive).
  - `pub fn autostart_value(exe: &Path) -> String` (pure: `"\"C:\\path\\app.exe\""`).
  - `settings::open(parent: HWND, cfg: &Config, disks: &[DiskReading], lang: Lang, on_apply: impl Fn(Config) + 'static)`: a single non-modal window (reuse it if already open) with checkboxes: Claude, Codex (each followed by `LoginFound` or the `*NoLogin` text), CPU, RAM, GPU, every current disk, Claude 5h, Claude week, Codex 5h, Codex week; two numeric edit fields (hw seconds 1–5, limits seconds ≥ 60); Start with Windows; OK / Cancel. Apply saves the config (clamped), redraws the strip and wakes the worker.

- [ ] **Step 1: Write failing test** `autostart_value_quotes_path` (`C:\a b\x.exe` → `"C:\a b\x.exe"` with quotes).
- [ ] **Step 2: Run** `cargo test autostart` — FAIL; **implement**; run — PASS.
- [ ] **Step 3: Implement** `settings.rs` and the menu handlers.
- [ ] **Step 4: Manual verification**: turning off GPU removes it from the strip immediately; turning off Codex hides both Codex items and stops its requests (no new Codex fetch after "Refresh now"); settings survive a restart; Start with Windows creates/removes the Run value (`reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v widget-usage-windows`); the pt-BR UI shows Portuguese text.
- [ ] **Step 5: Commit** `feat: add settings window and start-with-Windows`.

### Task 11: Release workflow and README

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `README.md`

- [ ] **Step 1: `release.yml`**: on tag `v*`, `windows-latest`, `cargo build --release`, compute `Get-FileHash -Algorithm SHA256`, upload `widget-usage-windows.exe` and `widget-usage-windows.exe.sha256` to the GitHub Release.
- [ ] **Step 2: README**: replace the status with real install steps (download from Releases, run, optional "Start with Windows"), a SmartScreen section ("More info → Run anyway" and how to verify the SHA-256), how to free the left corner (center icons, turn off Widgets), the Claude ~8 h login-expiry behavior, and how to toggle providers. Keep the existing unofficial-endpoints / non-affiliation / privacy sections. Add a screenshot taken from the running app as `docs/screenshot.png`.
- [ ] **Step 3: Verify** the final release size: `(Get-Item target\release\widget-usage-windows.exe).Length` ≤ 4 MB; `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` PASS.
- [ ] **Step 4: Commit** `docs: add install guide and release workflow`.
