#![windows_subsystem = "windows"]

use claude_codex_taskbar::ui::strip::WM_APP_OPEN_SETTINGS;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
use windows::core::w;

/// v0.1.x ran as `widget-usage-windows` with the same window class. Close it on upgrade so
/// two strips never share the taskbar.
unsafe fn close_legacy_instance() {
    // SAFETY: plain handle and window calls; every handle opened here is closed.
    unsafe {
        let Ok(legacy) = OpenMutexW(
            SYNCHRONIZATION_SYNCHRONIZE,
            false,
            w!("Local\\widget-usage-windows"),
        ) else {
            return;
        };
        if let Ok(hwnd) = FindWindowW(w!("WidgetUsageStrip"), None) {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        // The legacy mutex is released (abandoned) when the old process exits.
        let _ = WaitForSingleObject(legacy, 3000) == WAIT_OBJECT_0;
        let _ = CloseHandle(legacy);
    }
}

fn main() {
    // SAFETY: the mutex handle lives until process exit.
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\claude-codex-taskbar"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Launching again opens Settings in the running instance (useful when the
            // strip is hidden, e.g. on a left-aligned taskbar).
            if let Ok(hwnd) = FindWindowW(w!("WidgetUsageStrip"), None) {
                let _ = PostMessageW(Some(hwnd), WM_APP_OPEN_SETTINGS, WPARAM(0), LPARAM(0));
            }
            return;
        }
        close_legacy_instance();
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    claude_codex_taskbar::migrate::run();
    std::process::exit(claude_codex_taskbar::ui::app::run());
}
