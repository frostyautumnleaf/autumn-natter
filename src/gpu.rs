// Graphics devices of the machine.
//
// The best source is llama-server itself, because it lists the devices that
// llama.cpp can really use. When that is not possible, the code reads
// nvidia-smi or the /sys tree. A machine with no graphics device gets one CPU
// entry, so the device list in the user interface is never empty.

use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::util;

// One device. The device field is the short name llama.cpp expects in
// --device. The name field is what the user reads. A CPU row is not an
// accelerator, so it never appears in a device list.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub device: String,
    pub name: String,
    pub memory_mb: u64,
    pub enabled: bool,
    // True for a graphics device, false for the system memory row.
    pub is_offload: bool,
}

impl Gpu {
    // Text for the memory column of the user interface.
    pub fn memory_text(&self) -> String {
        if self.memory_mb == 0 {
            return String::from("unknown");
        }
        if self.memory_mb >= 1024 {
            let gib = self.memory_mb as f64 / 1024.0;
            return format!("{:.1} GiB", gib);
        }
        format!("{} MiB", self.memory_mb)
    }
}

// Look for the devices llama.cpp sees. The path is the llama-server binary.
pub fn from_llama(server_path: &Path) -> Vec<Gpu> {
    let capture = util::capture(
        &server_path.display().to_string(),
        &["--list-devices"],
        Duration::from_secs(30),
    );
    if !capture.ok && capture.out.is_empty() {
        return Vec::new();
    }
    parse_list_devices(&capture.out)
}

// Read a line such as "  CUDA0: NVIDIA GeForce RTX 4090 (24564 MiB, 23000 MiB free)".
pub fn parse_list_devices(text: &str) -> Vec<Gpu> {
    let mut out: Vec<Gpu> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.contains('(') || !line.contains("MiB") {
            continue;
        }
        let Some((head, tail)) = line.split_once(": ") else {
            continue;
        };
        let device = head.trim().to_string();
        if device.is_empty() || device == "(none)" {
            continue;
        }
        // Tail looks like "NVIDIA GeForce RTX 4090 (24564 MiB, 23000 MiB free)".
        let description = tail.split('(').next().unwrap_or("").trim().to_string();
        let memory_mb = tail
            .split('(')
            .nth(1)
            .and_then(|part| part.split_whitespace().next())
            .and_then(|num| num.parse::<u64>().ok())
            .unwrap_or(0);
        out.push(Gpu {
            name: if description.is_empty() {
                device.clone()
            } else {
                format!("{} - {}", device, description)
            },
            device,
            memory_mb,
            enabled: true,
            is_offload: true,
        });
    }
    out
}

// Ask nvidia-smi for the NVIDIA cards.
fn from_nvidia_smi() -> Vec<Gpu> {
    let capture = util::capture(
        "nvidia-smi",
        &[
            "--query-gpu=index,name,memory.total",
            "--format=csv,noheader,nounits",
        ],
        Duration::from_secs(10),
    );
    if !capture.ok {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in capture.out.lines() {
        let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
        if parts.len() < 3 {
            continue;
        }
        let index = parts[0];
        let memory_mb = parts[2].parse::<u64>().unwrap_or(0);
        out.push(Gpu {
            device: format!("CUDA{}", index),
            name: format!("CUDA{} - {}", index, parts[1]),
            memory_mb,
            enabled: true,
            is_offload: true,
        });
    }
    out
}

// Read the direct rendering manager tree on Linux.
fn from_sysfs() -> Vec<Gpu> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return out;
    };
    let mut cards: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().to_string().into())
        .filter(|name| {
            // Only a card entry, not card1-DP-1 and friends.
            let Some(rest) = name.strip_prefix("card") else {
                return false;
            };
            rest.len() == 1 && rest.chars().next().map(|c| c.is_ascii_digit()) == Some(true)
        })
        .collect();
    cards.sort();
    for card in cards {
        let device_dir = Path::new("/sys/class/drm").join(&card).join("device");
        let Ok(vendor_raw) = fs::read_to_string(device_dir.join("vendor")) else {
            continue;
        };
        let vendor = vendor_raw.trim().to_lowercase();
        // Skip the simple frame buffer and emulated display adapters.
        let driver = fs::read_link(device_dir.join("driver"))
            .ok()
            .and_then(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
            })
            .unwrap_or_default();
        if driver.contains("simple-framebuffer") || driver.contains("virtio") {
            continue;
        }
        let memory_mb = fs::read_to_string(device_dir.join("mem_info_vram_total"))
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
            .map(|bytes| bytes / (1024 * 1024))
            .unwrap_or(0);
        let maker = match vendor.as_str() {
            "0x10de" => "NVIDIA",
            "0x1002" => "AMD",
            "0x8086" => "Intel",
            "0x15ad" => "Microsoft",
            _ => "GPU",
        };
        let number = card.trim_start_matches("card").to_string();
        out.push(Gpu {
            // A sysfs card has no llama.cpp name, so the name is not usable in
            // --device. llama.cpp is asked for the names before it matters.
            device: format!("{}{}", if maker == "AMD" { "gfx" } else { "dev" }, number),
            name: format!("{} GPU {}", maker, number),
            memory_mb,
            enabled: true,
            is_offload: true,
        });
    }
    out
}

// A machine without a graphics device still needs one row in the list.
fn cpu_only() -> Vec<Gpu> {
    let memory_mb = read_mem_total_kb().map(|kb| kb / 1024).unwrap_or(0);
    vec![Gpu {
        device: "CPU".to_string(),
        name: "CPU (system memory)".to_string(),
        memory_mb,
        enabled: true,
        is_offload: false,
    }]
}

fn read_mem_total_kb() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            return rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok();
        }
    }
    None
}

// All devices of the machine, in a stable order.
// server_path is the llama-server binary, and may be missing.
pub fn detect(server_path: Option<&Path>) -> Vec<Gpu> {
    let mut devices = Vec::new();
    if let Some(path) = server_path {
        devices = from_llama(path);
    }
    if devices.is_empty() {
        devices = from_nvidia_smi();
    }
    if devices.is_empty() {
        devices = from_sysfs();
    }
    if devices.is_empty() {
        devices = cpu_only();
    }
    devices
}
