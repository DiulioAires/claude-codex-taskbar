//! Pure helpers that turn raw PDH counter values into Task Manager-style numbers.

use std::collections::HashMap;

/// Task Manager's GPU %: per-process engine samples summed per engine, then the busiest engine.
/// Instance names look like `pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D`.
/// `None` when no sample has valid data.
pub fn gpu_max_engine(samples: &[(String, f64)]) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }

    let mut per_engine: HashMap<&str, f64> = HashMap::new();
    for (name, value) in samples {
        let engine = name.find("luid_").map_or(name.as_str(), |i| &name[i..]);
        *per_engine.entry(engine).or_default() += value;
    }
    let max = per_engine.values().copied().fold(0.0, f64::max);
    Some((max as f32).clamp(0.0, 100.0))
}

/// `"0 C: D:"` → `"C: D:"`; a disk without letters (`"2"`) → `"Disk 2"`.
pub fn disk_label(instance: &str) -> String {
    let mut parts = instance.split_whitespace();
    let index = parts.next().unwrap_or(instance);
    let letters: Vec<&str> = parts.collect();
    if letters.is_empty() {
        format!("Disk {index}")
    } else {
        letters.join(" ")
    }
}

/// Active time from `% Idle Time`, which PDH can report slightly outside 0–100.
pub fn disk_active(idle_pct: f64) -> f32 {
    ((100.0 - idle_pct) as f32).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, v: f64) -> (String, f64) {
        (name.to_owned(), v)
    }

    #[test]
    fn gpu_groups_by_engine_across_pids() {
        let samples = [
            s(
                "pid_1_luid_0x00000000_0x0000D1A4_phys_0_eng_0_engtype_3D",
                30.0,
            ),
            s(
                "pid_2_luid_0x00000000_0x0000D1A4_phys_0_eng_0_engtype_3D",
                25.0,
            ),
            s(
                "pid_3_luid_0x00000000_0x0000D1A4_phys_0_eng_3_engtype_VideoDecode",
                40.0,
            ),
        ];
        assert_eq!(gpu_max_engine(&samples), Some(55.0));
    }

    #[test]
    fn gpu_separates_adapters() {
        let samples = [
            s(
                "pid_1_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D",
                20.0,
            ),
            s(
                "pid_2_luid_0x00000000_0x0000BBBB_phys_0_eng_0_engtype_3D",
                35.0,
            ),
        ];
        assert_eq!(gpu_max_engine(&samples), Some(35.0));
    }

    #[test]
    fn gpu_clamps() {
        let samples = [
            s("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 90.0),
            s("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 50.0),
        ];
        assert_eq!(gpu_max_engine(&samples), Some(100.0));
    }

    #[test]
    fn gpu_empty_is_none() {
        assert_eq!(gpu_max_engine(&[]), None);
    }

    #[test]
    fn gpu_idle_is_zero() {
        let samples = [s("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 0.0)];
        assert_eq!(gpu_max_engine(&samples), Some(0.0));
    }

    #[test]
    fn disk_label_letters() {
        assert_eq!(disk_label("0 C:"), "C:");
        assert_eq!(disk_label("0 C: D:"), "C: D:");
    }

    #[test]
    fn disk_label_no_letter() {
        assert_eq!(disk_label("2"), "Disk 2");
    }

    #[test]
    fn disk_active_clamps() {
        assert_eq!(disk_active(104.0), 0.0);
        assert_eq!(disk_active(25.0), 75.0);
        assert_eq!(disk_active(-3.0), 100.0);
    }
}
