#![windows_subsystem = "windows"]

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

fn main() {
    // SAFETY: the mutex handle lives until process exit; a second instance exits silently.
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\widget-usage-windows"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    std::process::exit(widget_usage_windows::ui::app::run());
}
