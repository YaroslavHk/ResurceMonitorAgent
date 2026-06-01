use sysinfo::System;

#[derive(serde::Serialize, Debug)]
pub struct MemoryMetrics {
    pub ram_used_kb: u64,
    pub swap_used_kb: u64,
}

pub fn collect_memory(system: &System) -> MemoryMetrics {
    MemoryMetrics {
        ram_used_kb: system.used_memory() / 1024,
        swap_used_kb: system.used_swap() / 1024,
    }
}
