use sysinfo::{Disks, System};

#[derive(serde::Serialize, Debug)]
pub struct DiskMetrics {
    pub disk_used_kb: u64,
    pub disk_used_percent: f32,
    pub disk_read_kb_per_sec: u64,
    pub disk_write_kb_per_sec: u64,
}

pub fn collect_disk(disks: &Disks, system: &System, interval_secs: u64) -> DiskMetrics {
    let (disk_used_kb, disk_used_percent) = disk_space(disks);
    let (disk_read_kb_per_sec, disk_write_kb_per_sec) = disk_io(system, interval_secs);

    DiskMetrics {
        disk_used_kb,
        disk_used_percent,
        disk_read_kb_per_sec,
        disk_write_kb_per_sec,
    }
}

fn disk_space(disks: &Disks) -> (u64, f32) {
    let mut used_kb = 0u64;
    let mut total_kb = 0u64;

    for disk in disks.iter() {
        total_kb += disk.total_space() / 1024;
        used_kb += (disk.total_space() - disk.available_space()) / 1024;
    }

    let used_percent = if total_kb > 0 {
        (used_kb as f32 / total_kb as f32) * 100.0
    } else {
        0.0
    };

    (used_kb, used_percent)
}

fn disk_io(system: &System, interval_secs: u64) -> (u64, u64) {
    let (mut read_bytes, mut write_bytes) = (0u64, 0u64);

    for process in system.processes().values() {
        let usage = process.disk_usage();
        read_bytes += usage.read_bytes;
        write_bytes += usage.written_bytes;
    }

    let safe_interval = interval_secs.max(1);
    (
        (read_bytes / 1024) / safe_interval,
        (write_bytes / 1024) / safe_interval,
    )
}
