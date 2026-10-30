#![windows_subsystem = "windows"]

use widget_usage_windows::ui::strip::WM_APP_OPEN_SETTINGS;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
use windows::core::w;

fn main() {
    // SAFETY: the mutex handle lives until process exit.
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\widget-usage-windows"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Launching again opens Settings in the running instance (useful when the
            // strip is hidden, e.g. on a left-aligned taskbar).
            if let Ok(hwnd) = FindWindowW(w!("WidgetUsageStrip"), None) {
                let _ = PostMessageW(Some(hwnd), WM_APP_OPEN_SETTINGS, WPARAM(0), LPARAM(0));
            }
            return;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    std::process::exit(widget_usage_windows::ui::app::run());
}
