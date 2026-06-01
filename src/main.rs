mod metrics;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use serde_json::Value;
use sysinfo::{Components, Disks, Networks, System};
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use metrics::{collect_metrics, GpuHandle};

#[derive(Serialize, Debug)]
struct AgentMetadata {
    message_type: String,
    node_id: String,
    hostname: String,
    os_name: String,

    cpu_cores: u32,
    cpu_threads: u32,
    cpu_frequency_mhz: u32,
    cpu_brand: String,
    cpu_model: String,

    gpu_vendor: String,
    gpu_name: String,
    gpu_memory_total_mb: u64,

    ram_total_kb: u64,
    swap_total_kb: u64,
    disk_total_kb: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerComand {
    command: String,
    value: Option<Value>,
}

#[derive(Serialize, Debug)]
struct AgentResponse {
    message_type: String,
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
}

impl AgentResponse {
    fn ok(command: &str) -> Self {
        AgentResponse {
            message_type: "response".into(),
            command: command.into(),
            value: None,
        }
    }
    fn with_value(command: &str, v: Value) -> Self {
        AgentResponse {
            message_type: "response".into(),
            command: command.into(),
            value: Some(v),
        }
    }
}

#[tokio::main]
async fn main() {

    let server_url = "wss://darkened-paragraph-stump.ngrok-free.dev/ws/agent";

    let node_id = machine_uid::get().unwrap_or_else(|_| {
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

    let gpu = GpuHandle::init();

    loop {
        let ws_stream = loop {
            match connect_async(server_url).await {
                Ok((stream, _)) => {
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
        let (gpu_vendor, gpu_name, gpu_memory_total_mb) = gpu.handshake_info();

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

            gpu_vendor,
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

                let ack_result = tokio::time::timeout(Duration::from_secs(10), read.next()).await;

                match ack_result {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        if let Ok(server_cmd) = serde_json::from_str::<ServerComand>(&text) {
                            if server_cmd.command == "HandshakeAccepted" {} else {
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

                    let metrics = collect_metrics(
                        &node_id,
                        interval_secs,
                        &system,
                        &networks,
                        &disks,
                        &components,
                        &gpu,
                    );

                    match serde_json::to_string(&metrics) {
                        Ok(json_string) => {
                            if let Err(e) = write.send(Message::Text(json_string.clone())).await {
                                eprintln!("Error sending data (connection broken): {}", e);
                                break;
                            }
                        }
                        Err(e) => eprintln!("Error building JSON: {}", e),
                    }
                }

                Some(msg) = read.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if let Ok(server_comand) = serde_json::from_str::<ServerComand>(&text) {
                                if server_comand.command == "SetInterval" {
                                    if let Some(value) = server_comand.value.and_then(|v| v.as_u64()) {
                                        interval_secs = std::cmp::max(1, value);
                                        poll_interval = time::interval(Duration::from_secs(interval_secs));
                                        poll_interval.tick().await;
                                    } else {
                                        eprintln!("Command SETINTERVAL: Value is not u64")
                                    }
                                } else if server_comand.command == "GetInterval" {
                                    let resp = AgentResponse::with_value(
                                        "GetInterval",
                                        serde_json::json!({ "interval_secs": interval_secs }),
                                    );
                                    if let Ok(json) = serde_json::to_string(&resp) {
                                        if let Err(e) = write.send(Message::Text(json)).await {
                                            eprintln!("[GetInterval] send failed: {}", e);
                                            break;
                                        }
                                    }

                                } else if server_comand.command == "ForceMetricsUpdate" {
                                    system.refresh_all();
                                    networks.refresh(true);
                                    disks.refresh(true);
                                    components.refresh(true);

                                    let metrics = collect_metrics(
                                        &node_id,
                                        interval_secs,
                                        &system,
                                        &networks,
                                        &disks,
                                        &components,
                                        &gpu,
                                    );

                                    match serde_json::to_string(&metrics) {
                                        Ok(json) => {
                                            if let Err(e) = write.send(Message::Text(json.clone())).await {
                                                eprintln!("[ForceMetricsUpdate] send failed: {}", e);
                                                break;
                                            }

                                            if let Ok(ack) = serde_json::to_string(&AgentResponse::ok("ForceMetricsUpdate")) {
                                                let _ = write.send(Message::Text(ack)).await;
                                            }
                                        }
                                        Err(e) => eprintln!("[ForceMetricsUpdate] serialise error: {}", e),
                                    }

                                } else if server_comand.command == "RestartAgent" {
                                    if let Ok(ack) = serde_json::to_string(&AgentResponse::ok("RestartAgent")) {
                                        let _ = write.send(Message::Text(ack)).await;
                                    }
                                    let _ = write.send(Message::Close(None)).await;

                                    match std::env::current_exe() {
                                        Ok(exe) => {
                                            eprintln!("[RestartAgent] spawning: {}", exe.display());
                                            if let Err(e) = std::process::Command::new(&exe)
                                                .args(std::env::args_os().skip(1))
                                                .spawn()
                                            {
                                                eprintln!("[RestartAgent] spawn failed: {} — exiting anyway", e);
                                            }
                                        }
                                        Err(e) => eprintln!("[RestartAgent] could not resolve own path: {} — exiting", e),
                                    }
                                    std::process::exit(0);

                                } else if server_comand.command == "StopAgent" {
                                    if let Ok(ack) = serde_json::to_string(&AgentResponse::ok("StopAgent")) {
                                        let _ = write.send(Message::Text(ack)).await;
                                    }
                                    let _ = write.send(Message::Close(None)).await;
                                    std::process::exit(0);

                                } else {
                                    let resp = AgentResponse::with_value(
                                        "UnknownCommand",
                                        serde_json::json!({ "received": server_comand.command }),
                                    );
                                    if let Ok(json) = serde_json::to_string(&resp) {
                                        if let Err(e) = write.send(Message::Text(json)).await {
                                            eprintln!("[UnknownCommand] send failed: {}", e);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
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

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}