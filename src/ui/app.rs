//! Application state and wiring: config, sampler, limits worker and the strip window.

use super::layout::{Layout, layout};
use super::render::{GAP, Renderer, SEP_W, to_px};
use super::strip;
use super::taskbar::{self, TaskbarInfo, available_width, is_fullscreen};
use super::view::{ItemView, build_items, tooltip_text};
use crate::config::{self, Config};
use crate::hw::{HwSnapshot, Sampler};
use crate::i18n::{Lang, detect_lang};
use crate::limits::claude::ClaudeProvider;
use crate::limits::codex::CodexProvider;
use crate::limits::state::next_status;
use crate::limits::worker::{Control, Fetcher, LimitResult, spawn_with};
use crate::limits::{Provider, ProviderStatus};
use crate::model::Kind;
use crate::timeutil::now_unix;
use std::path::PathBuf;
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowRect, PostMessageW,
};
use windows::core::HSTRING;

/// Strip offset from the taskbar's left edge, in DIPs.
const MARGIN_DIP: f32 = 8.0;
/// Re-run the UI Automation lookup of the taskbar icons every N topmost ticks (2 s each).
const ICONS_EVERY_TICKS: u32 = 5;

pub struct App {
    pub cfg: Config,
    pub cfg_path: PathBuf,
    pub lang: Lang,
    sampler: Sampler,
    pub hw: HwSnapshot,
    pub claude: ProviderStatus,
    pub codex: ProviderStatus,
    pub control: Arc<Control>,
    renderer: Renderer,
    taskbar: Option<TaskbarInfo>,
    icons_left: Option<i32>,
    icon_ticks: u32,
    items: Vec<ItemView>,
    layout: Option<Layout>,
    light: bool,
    pub shown: bool,
}

fn light_theme() -> bool {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: data/size describe a valid u32 buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            &HSTRING::from("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    };
    status.is_ok() && data == 1
}

fn foreground_is_fullscreen(monitor: taskbar::Rect) -> bool {
    // SAFETY: read-only shell query.
    if let Ok(state) = unsafe { SHQueryUserNotificationState() }
        && [
            QUNS_BUSY,
            QUNS_RUNNING_D3D_FULL_SCREEN,
            QUNS_PRESENTATION_MODE,
        ]
        .contains(&state)
    {
        return true;
    }
    // SAFETY: plain window queries; failures fall through to "not fullscreen".
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        let is_shell = matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd");
        let mut r = RECT::default();
        GetWindowRect(fg, &mut r).is_ok() && is_fullscreen(r.into(), monitor, is_shell)
    }
}

impl App {
    pub fn new() -> windows::core::Result<App> {
        let cfg_path = config::default_path();
        let mut cfg = Config::load_from(&cfg_path);
        if !cfg.first_run_done {
            cfg.claude_enabled = claude_provider().has_login();
            cfg.codex_enabled = codex_provider().has_login();
            cfg.first_run_done = true;
            let _ = cfg.save_to(&cfg_path);
        }
        let control = Control::new(cfg.limits_interval_secs);
        let mut app = App {
            lang: detect_lang(),
            sampler: Sampler::new(),
            hw: HwSnapshot::default(),
            claude: ProviderStatus::Disabled,
            codex: ProviderStatus::Disabled,
            control,
            renderer: Renderer::new()?,
            taskbar: None,
            icons_left: None,
            icon_ticks: 0,
            items: Vec::new(),
            layout: None,
            light: light_theme(),
            shown: false,
            cfg,
            cfg_path,
        };
        app.apply_provider_flags();
        Ok(app)
    }

    /// Syncs worker flags and statuses with the config's provider switches.
    pub fn apply_provider_flags(&mut self) {
        for (kind, on, status) in [
            (Kind::Claude, self.cfg.claude_enabled, &mut self.claude),
            (Kind::Codex, self.cfg.codex_enabled, &mut self.codex),
        ] {
            self.control.set_enabled(kind, on);
            if !on {
                *status = ProviderStatus::Disabled;
            } else if *status == ProviderStatus::Disabled {
                *status = ProviderStatus::Loading;
            }
        }
        self.control.set_interval(self.cfg.limits_interval_secs);
    }

    pub fn apply_config(&mut self, cfg: Config) {
        self.cfg = cfg.clamped();
        let _ = self.cfg.save_to(&self.cfg_path);
        self.apply_provider_flags();
        self.control.wake();
    }

    pub fn sample_hw(&mut self) {
        self.hw = self.sampler.sample();
    }

    pub fn refresh_instances(&mut self) {
        self.sampler.refresh_instances();
    }

    pub fn on_limits(&mut self, r: LimitResult) {
        let now = now_unix();
        let status = match r.kind {
            Kind::Claude => &mut self.claude,
            Kind::Codex => &mut self.codex,
            Kind::Hardware => return,
        };
        if *status == ProviderStatus::Disabled {
            return;
        }
        *status = match r.result {
            Ok((usage, true, at)) => ProviderStatus::FromSession { usage, at },
            Ok((usage, false, at)) => next_status(status, Ok(usage), at),
            Err(err) => next_status(status, Err(err), now),
        };
    }

    pub fn theme_changed(&mut self) {
        self.light = light_theme();
    }

    /// Forces the UI Automation icon lookup on the next layout.
    pub fn invalidate_icons(&mut self) {
        self.icon_ticks = 0;
        self.icons_left = None;
    }

    pub fn tick_icons(&mut self) {
        self.icon_ticks = self.icon_ticks.saturating_add(1);
    }

    /// Recomputes position and layout, then paints or hides the strip.
    pub fn render(&mut self, hwnd: HWND) {
        let need_icons = self.icons_left.is_none() || self.icon_ticks >= ICONS_EVERY_TICKS;
        let Some(mut info) = taskbar::query(need_icons) else {
            strip::hide(hwnd, self);
            return;
        };
        if need_icons {
            self.icon_ticks = 0;
            self.icons_left = info.icons_left;
        }
        info.icons_left = self.icons_left;
        let dpi = info.dpi;
        self.items = build_items(&self.cfg, &self.hw, &self.claude, &self.codex, self.lang);
        let metrics = self.renderer.measure(&self.items, dpi);
        let margin = to_px(MARGIN_DIP, dpi);
        let kinds: Vec<_> = self
            .items
            .iter()
            .map(|i| i.kind)
            .zip(metrics.iter().copied())
            .collect();
        let l = layout(
            &kinds,
            available_width(&info, margin),
            to_px(GAP, dpi),
            to_px(SEP_W, dpi),
            self.renderer.ellipsis_px(dpi),
        );
        let hide = !info.visible || l.width == 0 || foreground_is_fullscreen(info.monitor);
        if hide {
            strip::hide(hwnd, self);
        } else {
            let pos = (info.rect.left + margin, info.rect.top);
            let size = (l.width, info.rect.height());
            if self
                .renderer
                .paint(hwnd, pos, size, dpi, &self.items, &metrics, &l, self.light)
                .is_ok()
            {
                strip::show(hwnd, self);
            }
        }
        self.layout = Some(l);
        self.taskbar = Some(info);
    }

    pub fn tooltip(&self) -> String {
        let hidden: Vec<ItemView> = match &self.layout {
            Some(l) if l.overflow => (0..self.items.len())
                .filter(|i| !l.visible.contains(i))
                .map(|i| self.items[i].clone())
                .collect(),
            _ => Vec::new(),
        };
        let (left, widgets) = self
            .taskbar
            .as_ref()
            .map_or((false, false), |t| (t.left_aligned, t.widgets_button_on));
        let text = tooltip_text(self.lang, &self.claude, &self.codex, &hidden, left, widgets);
        if text.is_empty() {
            "Taskbar Usage".into()
        } else {
            text
        }
    }
}

pub(crate) fn claude_provider() -> ClaudeProvider {
    ClaudeProvider {
        creds_path: ClaudeProvider::default_path(),
    }
}

pub(crate) fn codex_provider() -> CodexProvider {
    CodexProvider {
        home: CodexProvider::default_home(),
    }
}

/// Starts the limits thread; results are boxed and posted to `hwnd` as `WM_APP_LIMITS`.
pub fn spawn_worker(hwnd: HWND, control: Arc<Control>) {
    let claude = claude_provider();
    let codex = codex_provider();
    let fetchers: Vec<(Kind, Fetcher)> = vec![
        (
            Kind::Claude,
            Box::new(move |now| claude.fetch().map(|u| (u, false, now))),
        ),
        (
            Kind::Codex,
            Box::new(move |now| codex.fetch_with_fallback(now)),
        ),
    ];
    let target = hwnd.0 as isize;
    spawn_with(control, fetchers, move |result| {
        let ptr = Box::into_raw(Box::new(result));
        // SAFETY: the UI thread reclaims the box in its WM_APP_LIMITS handler.
        let posted = unsafe {
            PostMessageW(
                Some(HWND(target as _)),
                strip::WM_APP_LIMITS,
                WPARAM(0),
                LPARAM(ptr as isize),
            )
        };
        if posted.is_err() {
            // SAFETY: the message was not queued, so ownership never left this thread.
            drop(unsafe { Box::from_raw(ptr) });
        }
    });
}

pub fn run() -> i32 {
    match App::new() {
        Ok(app) => strip::run(app),
        Err(_) => 1,
    }
}
