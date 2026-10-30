//! Hardware usage sampling (CPU, RAM, GPU, per-disk activity).

pub mod parse;

use parse::{disk_active, disk_label, gpu_max_engine};
use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
    PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PdhOpenQueryW, PdhRemoveCounter,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::core::{HSTRING, PCWSTR};

const CPU_UTILITY: &str = r"\Processor Information(_Total)\% Processor Utility";
const CPU_TIME: &str = r"\Processor(_Total)\% Processor Time";
const GPU_ENGINE: &str = r"\GPU Engine(*)\Utilization Percentage";
const DISK_IDLE: &str = r"\PhysicalDisk(*)\% Idle Time";

/// Doubles without the 100% cap (`PDH_FMT_NOCAP100` = 0x8000); callers clamp.
const FMT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);

#[derive(Clone, Debug, PartialEq)]
pub struct DiskReading {
    /// PDH instance name, e.g. `"0 C:"`; stable key for config.
    pub instance: String,
    pub label: String,
    pub active_pct: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HwSnapshot {
    pub cpu: Option<f32>,
    pub ram: Option<f32>,
    pub gpu: Option<f32>,
    pub disks: Vec<DiskReading>,
}

/// Owns one PDH query. Rate counters need two collections, so the first sample has `None`s.
pub struct Sampler {
    query: Option<PDH_HQUERY>,
    cpu: Option<PDH_HCOUNTER>,
    gpu: Option<PDH_HCOUNTER>,
    disk: Option<PDH_HCOUNTER>,
}

fn add_counter(query: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
    let mut counter = PDH_HCOUNTER::default();
    // SAFETY: query is open; path is a valid wide string for the call's duration.
    let status = unsafe { PdhAddEnglishCounterW(query, &HSTRING::from(path), 0, &mut counter) };
    (status == 0).then_some(counter)
}

fn single_value(counter: PDH_HCOUNTER) -> Option<f32> {
    let mut value = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: counter belongs to an open query; value is a valid out pointer.
    let status = unsafe { PdhGetFormattedCounterValue(counter, FMT, None, &mut value) };
    // SAFETY: PDH_FMT_DOUBLE fills the doubleValue union member.
    if status != 0 {
        return None;
    }
    Some(unsafe { value.Anonymous.doubleValue } as f32)
}

fn array_values(counter: PDH_HCOUNTER) -> Option<Vec<(String, f64)>> {
    let (mut size, mut count) = (0u32, 0u32);
    // SAFETY: first call only asks for the required buffer size.
    let status = unsafe { PdhGetFormattedCounterArrayW(counter, FMT, &mut size, &mut count, None) };
    if status != PDH_MORE_DATA {
        return (status == 0).then(Vec::new);
    }
    // u64 backing keeps the item array suitably aligned.
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    // SAFETY: buf holds at least `size` bytes as PDH requested.
    let status =
        unsafe { PdhGetFormattedCounterArrayW(counter, FMT, &mut size, &mut count, Some(items)) };
    if status != 0 {
        return None;
    }
    // SAFETY: PDH wrote `count` items; names point into the same buffer.
    let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
    Some(
        items
            .iter()
            .filter(|item| item.FmtValue.CStatus == 0)
            .map(|item| {
                let name = unsafe { PCWSTR(item.szName.0).to_string() }.unwrap_or_default();
                (name, unsafe { item.FmtValue.Anonymous.doubleValue })
            })
            .collect(),
    )
}

fn ram_load() -> Option<f32> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength is set as the API requires.
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    let used = status.ullTotalPhys.saturating_sub(status.ullAvailPhys);
    Some((used as f64 / status.ullTotalPhys.max(1) as f64 * 100.0) as f32)
}

impl Sampler {
    /// Never fails: counters that cannot be added simply report `None`.
    pub fn new() -> Sampler {
        let mut query = PDH_HQUERY::default();
        // SAFETY: null data source means live data.
        let opened = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } == 0;
        if !opened {
            return Sampler {
                query: None,
                cpu: None,
                gpu: None,
                disk: None,
            };
        }
        let cpu = add_counter(query, CPU_UTILITY).or_else(|| add_counter(query, CPU_TIME));
        let mut sampler = Sampler {
            query: Some(query),
            cpu,
            gpu: None,
            disk: None,
        };
        sampler.refresh_instances();
        sampler
    }

    /// Re-adds the wildcard counters so new GPU processes and hot-plugged disks are picked up.
    /// Call every ~30 s.
    pub fn refresh_instances(&mut self) {
        let Some(query) = self.query else { return };
        for counter in [self.gpu.take(), self.disk.take()].into_iter().flatten() {
            // SAFETY: counter was added to this query.
            unsafe { PdhRemoveCounter(counter) };
        }
        self.gpu = add_counter(query, GPU_ENGINE);
        self.disk = add_counter(query, DISK_IDLE);
        // Prime the new rate counters so the next sample has values.
        // SAFETY: query is open.
        unsafe { PdhCollectQueryData(query) };
    }

    pub fn sample(&mut self) -> HwSnapshot {
        let ram = ram_load();
        let Some(query) = self.query else {
            return HwSnapshot {
                ram,
                ..Default::default()
            };
        };
        // SAFETY: query is open.
        if unsafe { PdhCollectQueryData(query) } != 0 {
            return HwSnapshot {
                ram,
                ..Default::default()
            };
        }
        let cpu = self.cpu.and_then(single_value).map(|v| v.clamp(0.0, 100.0));
        let gpu = self.gpu.and_then(array_values).map(|s| gpu_max_engine(&s));
        let mut disks: Vec<DiskReading> = self
            .disk
            .and_then(array_values)
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| name != "_Total")
            .map(|(instance, idle)| DiskReading {
                label: disk_label(&instance),
                active_pct: disk_active(idle),
                instance,
            })
            .collect();
        disks.sort_by(|a, b| a.instance.cmp(&b.instance));
        HwSnapshot {
            cpu,
            ram,
            gpu,
            disks,
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        if let Some(query) = self.query.take() {
            // SAFETY: query was opened by this sampler and is closed once.
            unsafe { PdhCloseQuery(query) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads live performance counters"]
    fn sampler_smoke() {
        let mut s = Sampler::new();
        s.sample();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let snap = s.sample();
        println!("{snap:?}");
        assert!(snap.cpu.is_some_and(|v| (0.0..=100.0).contains(&v)));
        assert!(snap.ram.is_some_and(|v| (0.0..=100.0).contains(&v)));
        assert!(snap.gpu.is_some_and(|v| (0.0..=100.0).contains(&v)));
        assert!(!snap.disks.is_empty());
    }
}
