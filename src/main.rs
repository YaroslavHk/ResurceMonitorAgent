use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use nvml_wrapper::Nvml;
use serde_json::Value;
use sysinfo::{Components, Disks, Networks, System};
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};


#[derive(Serialize, Debug)]
struct AgentMetadata {
    // --- Meta ---
    message_type: String,
    node_id: String,
    hostname: String,
    os_name: String,

    // --- CPU ---
    cpu_cores: u32,
    cpu_threads: u32,
    cpu_frequency_mhz: u32,
    cpu_brand: String,
    cpu_model: String,

    // --- GPU ---
    gpu_name: String,
    gpu_memory_total_mb: u64,

    // --- RAM & SWAP ---
    ram_total_kb: u64,
    swap_total_kb: u64,
    disk_total_kb: u64,
}
#[derive(Serialize, Debug)]
struct SystemMetrics {
    // --- Meta ---
    message_type: String,
    node_id: String,
    timestamp: u64,
    uptime_seconds: u64,

    // --- CPU ---
    cpu_usage_percent: f32,
    cpu_temperature_celsius: f32,

    // --- GPU ---
    gpu_temperature_celsius: f32,
    gpu_usage_percent: f32,
    gpu_memory_used_mb: u64,

    // --- RAM & SWAP ---
    ram_used_kb: u64,
    swap_used_kb: u64,

    // --- Disk ---
    disk_used_kb: u64,
    disk_used_percent: f32,
    disk_read_kb_per_sec: u64,
    disk_write_kb_per_sec: u64,

    // --- System / Processes ---
    active_processes: usize,
    total_processes: usize,

    // --- Network ---
    network_rx_kb: u64,
    network_tx_kb: u64,
    network_rx_kb_per_sec: u64,
    network_tx_kb_per_sec: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerComand {
    command: String,
    value: Option<Value>,
}


#[cfg(target_os = "windows")]
fn get_windows_cpu_temp() -> f32 {
    use serde::Deserialize;
    use wmi::{COMLibrary, WMIConnection};

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "PascalCase")]
    struct ThermalZone {
        current_temperature: u32,
    }

    let com_con = match COMLibrary::without_security() {
        Ok(con) => con,
        Err(_) => return 0.0,
    };

    let wmi_con = match WMIConnection::new(com_con.into()) {
        Ok(con) => con,
        Err(_) => return 0.0,
    };

    let query = "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";

    let results: Result<Vec<ThermalZone>, _> = wmi_con.raw_query(query);

    if let Ok(zones) = results {
        let max_temp = zones.iter()
            .map(|z| z.current_temperature)
            .max()
            .unwrap_or(0);

        if max_temp > 0 {
            let kelvin = max_temp as f32 / 10.0;
            let celsius = kelvin - 273.15;

            if celsius > 0.0 && celsius < 150.0 {
                return (celsius * 10.0).round() / 10.0;
            }
        }
    }
    0.0
}

#[tokio::main]
async fn main() {
    let server_url = "wss://darkened-paragraph-stump.ngrok-free.dev/ws";

    let node_id = machine_uid::get().unwrap_or_else(|_|
        {
            eprintln!("Failed to get machine UID");
            "unknown_machine_id".to_string()
        });
    let hostname = System::host_name().unwrap_or_else(|| "UnknownHost".to_string());
    let mut interval_secs = 5u64;
    let mut poll_interval = time::interval(Duration::from_secs(interval_secs));

    let mut system = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut components = Components::new_with_refreshed_list();
    let os_name = System::name().unwrap_or_else(|| "Unknown OS".to_string());

    let nvml = Nvml::init().ok();

    if nvml.is_some() {
        println!("NVIDIA GPU found, monitoring enabled.");
    } else {
        println!("NVML not initialized (possibly no NVIDIA GPU). Metrics will be zero.");
    }

    println!("--- Sensor Diagnostics ---");
    let sensors_count = components.iter().count();
    println!("Sensors found: {}", sensors_count);
    for component in components.iter() {
        println!("Sensor: '{}', Temperature: {:?}", component.label(), component.temperature());
    }
    println!("----------------------------");

    println!("Agent started");
    println!("Node ID: {}", node_id);
    println!("Hostname: {}", hostname);
    println!("Server URL: {}", server_url);
    println!("Interval: {} seconds", interval_secs);

    loop {
        println!("Attempting to connect to {}...", server_url);

        let ws_stream = loop {
            match connect_async(server_url).await {
                Ok((stream, _)) => {
                    println!("WebSocket successfully connected!");
                    break stream;
                }
                Err(e) => {
                    eprintln!("Server unavailable ({}). Retrying in 5 seconds...", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        };

        let (mut write, mut read) = ws_stream.split();

        system.refresh_all();
        system.refresh_memory();
        disks.refresh(false);

        let cpus = system.cpus();
        let mut gpu_name = "N/A".to_string();
        let mut gpu_memory_total_mb = 0;

        if let Some(ref nvml_instance) = nvml {
            if let Ok(device) = nvml_instance.device_by_index(0) {
                if let Ok(name) = device.name() { gpu_name = name; }
                if let Ok(memory) = device.memory_info() { gpu_memory_total_mb = memory.total / (1024 * 1024); }
            }
        }

        let metadata = AgentMetadata {
            message_type: "handshake".to_string(),
            node_id: node_id.clone(),
            hostname: hostname.clone(),
            os_name: os_name.clone(),

            cpu_cores: System::physical_core_count().unwrap_or(0) as u32,
            cpu_threads: cpus.len() as u32,
            cpu_frequency_mhz: cpus.first().map(|c| c.frequency()).unwrap_or(0) as u32,
            cpu_brand: cpus.first().map(|c| c.vendor_id().to_string()).unwrap_or_default(),
            cpu_model: cpus.first().map(|c| c.brand().to_string()).unwrap_or_default(),

            gpu_name,
            gpu_memory_total_mb,

            ram_total_kb: system.total_memory() / 1024,
            swap_total_kb: system.total_swap() / 1024,
            disk_total_kb: disks.iter().map(|d| d.total_space() / 1024).sum(),
        };

        match serde_json::to_string(&metadata) {
            Ok(meta_json) => {
                if let Err(e) = write.send(Message::Text(meta_json)).await {
                    eprintln!("Error sending Handshake: {}", e);
                    continue;
                }
                println!("Waiting for server confirmation...");

                let ack_result = tokio::time::timeout(Duration::from_secs(10), read.next()).await;

                match ack_result {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        if let Ok(server_cmd) = serde_json::from_str::<ServerComand>(&text) {
                            if server_cmd.command == "HandshakeAccepted" {
                                println!("Server confirmed readiness! Starting metrics transmission.");
                            } else {
                                eprintln!("Error: Server sent unexpected command: {}", server_cmd.command);
                                continue;
                            }
                        } else {
                            eprintln!("Error parsing server response: {}", text);
                            continue;
                        }
                    }
                    Ok(Some(Ok(Message::Close(_)))) => {
                        eprintln!("Server closed connection during handshake.");
                        continue;
                    }
                    Ok(Some(Err(e))) => {
                        eprintln!("Error reading from socket: {}", e);
                        continue;
                    }
                    Ok(None) => {
                        eprintln!("WebSocket stream unexpectedly closed.");
                        continue;
                    }
                    Err(_) => {
                        eprintln!("Server did not respond within 10 seconds (Timeout). Reconnecting...");
                        continue;
                    }
                    _ => {
                        eprintln!("Received invalid data format instead of confirmation.");
                        continue;
                    }
                }
            }
            Err(e) => eprintln!("Error building JSON metadata: {}", e),
        }

        poll_interval.reset();

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    system.refresh_all();
                    networks.refresh(true);
                    disks.refresh(true);
                    components.refresh(true);

                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let cpu_usage_percent = system.global_cpu_usage();
                    let cpu_cores = System::physical_core_count().unwrap_or(0) as u32;
                    let cpus = system.cpus();
                    let cpu_threads = cpus.len() as u32;

                    let cpu_frequency_mhz = cpus.first().map(|c| c.frequency()).unwrap_or(0) as u32;
                    let cpu_brand = cpus.first().map(|c| c.vendor_id().to_string()).unwrap_or_default();
                    let cpu_model = cpus.first().map(|c| c.brand().to_string()).unwrap_or_default();

                    let mut cpu_temperature_celsius = components
                        .iter()
                        .filter(|c| {
                            let label = c.label().to_lowercase();
                            label.contains("cpu") || label.contains("core") ||
                            label.contains("package") || label.contains("tctl") ||
                            label.contains("tccd") || label.contains("k10temp") ||
                            label.contains("coretemp")
                        })
                        .filter_map(|c| c.temperature())
                        .fold(0.0_f32, f32::max);

                    if cpu_temperature_celsius == 0.0 {
                        cpu_temperature_celsius = get_windows_cpu_temp();
                    }

                    let mut gpu_name = "N/A".to_string();
                    let mut gpu_temperature_celsius = 0.0;
                    let mut gpu_usage_percent = 0.0;
                    let mut gpu_memory_used_mb = 0;
                    let mut gpu_memory_total_mb = 0;

                    if let Some(ref nvml_instance) = nvml {
                        if let Ok(device) = nvml_instance.device_by_index(0) {
                            if let Ok(name) = device.name() { gpu_name = name; }
                            if let Ok(temp) = device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu) {
                                gpu_temperature_celsius = temp as f32;
                            }
                            if let Ok(utilization) = device.utilization_rates() {
                                gpu_usage_percent = utilization.gpu as f32;
                            }
                            if let Ok(memory) = device.memory_info() {
                                gpu_memory_used_mb = memory.used / (1024 * 1024);
                                gpu_memory_total_mb = memory.total / (1024 * 1024);
                            }
                        }
                    }

                    let ram_used_kb = system.used_memory() / 1024;
                    let ram_total_kb = system.total_memory() / 1024;
                    let swap_used_kb = system.used_swap() / 1024;
                    let swap_total_kb = system.total_swap() / 1024;

                    let mut disk_used_kb = 0;
                    let mut disk_total_kb = 0;
                    for disk in disks.iter() {
                        disk_total_kb += disk.total_space() / 1024;
                        disk_used_kb += (disk.total_space() - disk.available_space()) / 1024;
                    }
                    let disk_used_percent = if disk_total_kb > 0 {
                        (disk_used_kb as f32 / disk_total_kb as f32) * 100.0
                    } else { 0.0 };

                    let mut disk_read_bytes_new = 0;
                    let mut disk_write_bytes_new = 0;

                    for process in system.processes().values() {
                        let usage = process.disk_usage();
                        disk_read_bytes_new += usage.read_bytes;
                        disk_write_bytes_new += usage.written_bytes;
                    }

                    let disk_read_kb_per_sec = (disk_read_bytes_new / 1024) / interval_secs;
                    let disk_write_kb_per_sec = (disk_write_bytes_new / 1024) / interval_secs;

                    let total_processes = system.processes().len();
                    let active_processes = system.processes().values()
                        .filter(|p| matches!(p.status(), sysinfo::ProcessStatus::Run))
                        .count();

                    let uptime_seconds = System::uptime();

                    let mut network_rx_bytes_total = 0;
                    let mut network_tx_bytes_total = 0;
                    let mut network_rx_bytes_new = 0;
                    let mut network_tx_bytes_new = 0;

                    for (_interface, data) in networks.iter() {
                        network_rx_bytes_total += data.total_received();
                        network_tx_bytes_total += data.total_transmitted();
                        network_rx_bytes_new += data.received();
                        network_tx_bytes_new += data.transmitted();
                    }

                    let network_rx_kb = network_rx_bytes_total / 1024;
                    let network_tx_kb = network_tx_bytes_total / 1024;

                    let network_rx_kb_per_sec = (network_rx_bytes_new / 1024) / interval_secs;
                    let network_tx_kb_per_sec = (network_tx_bytes_new / 1024) / interval_secs;

                    let metrics = SystemMetrics {
                        message_type: "metrics".to_string(),
                        node_id: node_id.clone(),
                        timestamp,

                        cpu_usage_percent,
                        cpu_temperature_celsius,

                        gpu_temperature_celsius,
                        gpu_usage_percent,
                        gpu_memory_used_mb,

                        ram_used_kb,
                        swap_used_kb,

                        disk_used_kb,
                        disk_used_percent,
                        disk_read_kb_per_sec,
                        disk_write_kb_per_sec,

                        active_processes,
                        total_processes,
                        uptime_seconds,

                        network_rx_kb,
                        network_tx_kb,
                        network_rx_kb_per_sec,
                        network_tx_kb_per_sec,
                    };

                    match serde_json::to_string(&metrics) {
                        Ok(json_string) => {
                            if let Err(e) = write.send(Message::Text(json_string)).await {
                                eprintln!("Error sending data (connection broken): {}", e);
                                break;
                            }
                            println!("Metrics sent ({} bytes)", serde_json::to_string(&metrics).unwrap().len());
                        }
                        Err(e) => eprintln!("Error building JSON: {}", e),
                    }
                }

                Some(msg) = read.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            println!("Received message from server: {}", text);

                            if let Ok(server_comand) = serde_json::from_str::<ServerComand>(&text) {
                                if server_comand.command == "SetInterval" {
                                    if let Some(value) = server_comand.value.and_then(|v| v.as_u64()) {
                                        interval_secs = std::cmp::max(1, value);
                                        println!("Interval set to: {} seconds", interval_secs);

                                        poll_interval = time::interval(Duration::from_secs(interval_secs));
                                        poll_interval.tick().await;
                                    }else{
                                     eprintln!("Command SETINTERVAL: Value is not u64")
                                    }
                                }else if server_comand.command == "GetInterval"{

                                }else if server_comand.command == "ForceMetricsUpdate"{

                                }else if server_comand.command == "RestartAgent"{

                                }else{
                                    println!("Command not found");
                                }



                            }
                        }
                        Ok(Message::Close(_)) => {
                            println!("Server closed connection normally.");
                            break;
                        }
                        Err(e) => {
                            eprintln!("Error reading from socket: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        println!("Initializing reconnection...");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}