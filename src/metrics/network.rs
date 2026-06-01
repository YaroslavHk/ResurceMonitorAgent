use sysinfo::Networks;

#[derive(serde::Serialize, Debug)]
pub struct NetworkMetrics {

    pub network_rx_kb: u64,

    pub network_tx_kb: u64,

    pub network_rx_kb_per_sec: u64,

    pub network_tx_kb_per_sec: u64,
}

pub fn collect_network(networks: &Networks, interval_secs: u64) -> NetworkMetrics {
    let mut rx_total = 0u64;
    let mut tx_total = 0u64;
    let mut rx_delta = 0u64;
    let mut tx_delta = 0u64;

    for (_iface, data) in networks.iter() {
        rx_total += data.total_received();
        tx_total += data.total_transmitted();
        rx_delta += data.received();
        tx_delta += data.transmitted();
    }

    let safe_interval = interval_secs.max(1);
    NetworkMetrics {
        network_rx_kb: rx_total / 1024,
        network_tx_kb: tx_total / 1024,
        network_rx_kb_per_sec: (rx_delta / 1024) / safe_interval,
        network_tx_kb_per_sec: (tx_delta / 1024) / safe_interval,
    }
}
