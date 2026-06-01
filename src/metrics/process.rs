use sysinfo::System;

#[derive(serde::Serialize, Debug)]
pub struct ProcessMetrics {
    pub active_processes: usize,
    pub total_processes: usize,
}

pub fn collect_processes(system: &System) -> ProcessMetrics {
    let total_processes = system.processes().len();
    let active_processes = system
        .processes()
        .values()
        .filter(|p| matches!(p.status(), sysinfo::ProcessStatus::Run))
        .count();

    ProcessMetrics {
        active_processes,
        total_processes,
    }
}
