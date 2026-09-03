use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct GpuInfo {
    pub name: String,
    pub driver: String,
    pub utilization: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub gtt_used: Option<u64>,
    pub gtt_total: Option<u64>,
    pub temperature: Option<f32>,
}

pub struct GpuCollector {
    card_path: Option<PathBuf>,
    hwmon_temp_path: Option<PathBuf>,
    pub info: GpuInfo,
    is_nvidia: bool,
}

impl GpuCollector {
    pub fn new() -> Self {
        let (card_path, driver) = Self::find_drm_card();
        let hwmon_temp_path = card_path
            .as_ref()
            .and_then(|p| Self::find_hwmon_temp(&p.join("device")));

        let name = Self::detect_gpu_name(&card_path, &driver);
        let is_nvidia = driver.to_lowercase().contains("nvidia");

        let mut collector = Self {
            card_path,
            hwmon_temp_path,
            info: GpuInfo {
                name,
                driver,
                utilization: None,
                vram_used: None,
                vram_total: None,
                gtt_used: None,
                gtt_total: None,
                temperature: None,
            },
            is_nvidia,
        };

        collector.update();
        collector
    }

    fn find_drm_card() -> (Option<PathBuf>, String) {
        let drm_path = Path::new("/sys/class/drm");
        if let Ok(entries) = fs::read_dir(drm_path) {
            let mut cards: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        name.starts_with("card")
                            && name[4..].chars().all(|c| c.is_ascii_digit())
                            && p.join("device").exists()
                    } else {
                        false
                    }
                })
                .collect();

            cards.sort();

            if let Some(card) = cards.into_iter().next() {
                let driver = Self::detect_driver(&card.join("device"));
                return (Some(card), driver);
            }
        }
        (None, "Desconocido".to_string())
    }

    fn detect_driver(device_path: &Path) -> String {
        let driver_link = device_path.join("driver");
        if let Ok(target) = fs::read_link(driver_link)
            && let Some(name) = target.file_name().and_then(|n| n.to_str())
        {
            return name.to_string();
        }
        "Desconocido".to_string()
    }

    fn find_hwmon_temp(device_path: &Path) -> Option<PathBuf> {
        let hwmon_dir = device_path.join("hwmon");
        if let Ok(entries) = fs::read_dir(hwmon_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let p = entry.path();
                let temp_file = p.join("temp1_input");
                if temp_file.exists() {
                    return Some(temp_file);
                }
            }
        }
        None
    }

    fn detect_gpu_name(card_path: &Option<PathBuf>, driver: &str) -> String {
        // 1. Try lspci output
        if let Ok(output) = Command::new("lspci").output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let lower = line.to_lowercase();
                if (lower.contains("vga compatible controller")
                    || lower.contains("3d controller")
                    || lower.contains("display controller"))
                    && let Some(pos) = line.find(':')
                {
                    let candidate = line[pos + 1..].trim();
                    let name = if let Some(sub_pos) = candidate.find(':') {
                        candidate[sub_pos + 1..].trim()
                    } else {
                        candidate
                    };
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
        }

        // 2. Try glxinfo renderer
        if let Ok(output) = Command::new("glxinfo").arg("-B").output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("OpenGL renderer string:") {
                    let renderer = trimmed.trim_start_matches("OpenGL renderer string:").trim();
                    let short_name = if let Some(idx) = renderer.find('(') {
                        renderer[..idx].trim()
                    } else {
                        renderer
                    };
                    if !short_name.is_empty() {
                        return short_name.to_string();
                    }
                }
            }
        }

        // 3. Try reading sysfs product/label
        if let Some(card) = card_path {
            let label = card.join("device").join("label");
            if let Ok(s) = fs::read_to_string(label) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }

        if driver != "Desconocido" {
            format!("GPU ({driver})")
        } else {
            "GPU Integrada / Genérica".to_string()
        }
    }

    pub fn update(&mut self) {
        if self.is_nvidia {
            self.update_nvidia();
            return;
        }

        if let Some(ref card) = self.card_path {
            let dev = card.join("device");

            // AMD / Linux sysfs GPU busy percent
            let busy_path = dev.join("gpu_busy_percent");
            if let Ok(s) = fs::read_to_string(busy_path)
                && let Ok(val) = s.trim().parse::<f32>()
            {
                self.info.utilization = Some(val.clamp(0.0, 100.0));
            }

            // VRAM Used
            let vram_used_path = dev.join("mem_info_vram_used");
            if let Ok(s) = fs::read_to_string(vram_used_path)
                && let Ok(val) = s.trim().parse::<u64>()
            {
                self.info.vram_used = Some(val);
            }

            // VRAM Total
            let vram_total_path = dev.join("mem_info_vram_total");
            if let Ok(s) = fs::read_to_string(vram_total_path)
                && let Ok(val) = s.trim().parse::<u64>()
            {
                self.info.vram_total = Some(val);
            }

            // GTT Used
            let gtt_used_path = dev.join("mem_info_gtt_used");
            if let Ok(s) = fs::read_to_string(gtt_used_path)
                && let Ok(val) = s.trim().parse::<u64>()
            {
                self.info.gtt_used = Some(val);
            }

            // GTT Total
            let gtt_total_path = dev.join("mem_info_gtt_total");
            if let Ok(s) = fs::read_to_string(gtt_total_path)
                && let Ok(val) = s.trim().parse::<u64>()
            {
                self.info.gtt_total = Some(val);
            }

            // Temperature
            if let Some(ref temp_path) = self.hwmon_temp_path
                && let Ok(s) = fs::read_to_string(temp_path)
                && let Ok(millidegrees) = s.trim().parse::<f32>()
            {
                self.info.temperature = Some(millidegrees / 1000.0);
            }
        }
    }

    fn update_nvidia(&mut self) {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu",
                "--format=csv,noheader,nounits",
            ])
            .output();

        if let Ok(out) = output
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(first_line) = text.lines().next() {
                let parts: Vec<&str> = first_line.split(',').map(|s| s.trim()).collect();
                if parts.len() >= 4 {
                    if let Ok(u) = parts[0].parse::<f32>() {
                        self.info.utilization = Some(u);
                    }
                    if let Ok(m_used) = parts[1].parse::<u64>() {
                        self.info.vram_used = Some(m_used * 1024 * 1024);
                    }
                    if let Ok(m_tot) = parts[2].parse::<u64>() {
                        self.info.vram_total = Some(m_tot * 1024 * 1024);
                    }
                    if let Ok(t) = parts[3].parse::<f32>() {
                        self.info.temperature = Some(t);
                    }
                }
            }
        }
    }
}
