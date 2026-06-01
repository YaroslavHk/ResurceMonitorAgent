use sysinfo::{Components, System};

#[derive(serde::Serialize)]
pub struct CpuMetrics {
    pub cpu_usage_percent: f32,
    pub cpu_temperature_celsius: f32,
}

pub fn collect_cpu(system: &System, components: &Components) -> CpuMetrics {
    let cpu_usage_percent = system.global_cpu_usage();
    let cpu_temperature_celsius = read_cpu_temperature(components);

    CpuMetrics {
        cpu_usage_percent,
        cpu_temperature_celsius,
    }
}

fn read_cpu_temperature(components: &Components) -> f32 {
    let temp = components
        .iter()
        .filter(|c| {
            let label = c.label().to_lowercase();
            label.contains("cpu")
                || label.contains("core")
                || label.contains("package")
                || label.contains("tctl")
                || label.contains("tccd")
                || label.contains("k10temp")
                || label.contains("coretemp")
        })
        .filter_map(|c| c.temperature())
        .fold(0.0_f32, f32::max);

    if temp == 0.0 {
        fallback_cpu_temperature()
    } else {
        temp
    }
}

#[cfg(target_os = "windows")]
fn fallback_cpu_temperature() -> f32 {
    use wmi::{COMLibrary, WMIConnection};

    #[derive(serde::Deserialize)]
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

    let results: Result<Vec<ThermalZone>, _> =
        wmi_con.raw_query("SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature");

    if let Ok(zones) = results {
        let max_raw = zones
            .iter()
            .map(|z| z.current_temperature)
            .max()
            .unwrap_or(0);

        if max_raw > 0 {
            let celsius = (max_raw as f32 / 10.0) - 273.15;
            if (0.0..150.0).contains(&celsius) {
                return (celsius * 10.0).round() / 10.0;
            }
        }
    }
    0.0
}

#[cfg(not(target_os = "windows"))]
fn fallback_cpu_temperature() -> f32 {
    0.0
}