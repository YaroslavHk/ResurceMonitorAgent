pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod process;

use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{Components, Disks, Networks, System};

use cpu::collect_cpu;
use disk::collect_disk;
use memory::collect_memory;
use network::collect_network;
use process::collect_processes;

pub use cpu::CpuMetrics;
pub use disk::DiskMetrics;
pub use gpu::{GpuHandle, GpuMetrics};
pub use memory::MemoryMetrics;
pub use network::NetworkMetrics;
pub use process::ProcessMetrics;

#[derive(serde::Serialize)]
pub struct SystemMetrics {
    // --- Meta ---
    pub message_type: String,
    pub node_id: String,
    pub timestamp: u64,
    pub uptime_seconds: u64,

    #[serde(flatten)]
    pub cpu: CpuMetrics,

    #[serde(flatten)]
    pub gpu: GpuMetrics,

    #[serde(flatten)]
    pub memory: MemoryMetrics,

    #[serde(flatten)]
    pub disk: DiskMetrics,

    #[serde(flatten)]
    pub network: NetworkMetrics,

    #[serde(flatten)]
    pub processes: ProcessMetrics,
}

pub fn collect_metrics(
    node_id: &str,
    interval_secs: u64,
    system: &System,
    networks: &Networks,
    disks: &Disks,
    components: &Components,
    gpu: &GpuHandle,
) -> SystemMetrics {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    SystemMetrics {
        message_type: "metrics".to_string(),
        node_id: node_id.to_string(),
        timestamp,
        uptime_seconds: System::uptime(),

        cpu: collect_cpu(system, components),
        gpu: gpu.collect(),
        memory: collect_memory(system),
        disk: collect_disk(disks, system, interval_secs),
        network: collect_network(networks, interval_secs),
        processes: collect_processes(system),
    }
}
