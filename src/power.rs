use std::fs;
use std::path::Path;

const POWER_SUPPLY: &str = "/sys/class/power_supply";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeState {
    Charging,
    Discharging,
    Full,
    NotCharging, // Conectado pero sin cargar (umbral de carga o batería llena)
    Unknown,
}

impl ChargeState {
    fn parse(status: &str) -> Self {
        match status {
            "Charging" => ChargeState::Charging,
            "Discharging" => ChargeState::Discharging,
            "Full" => ChargeState::Full,
            "Not charging" => ChargeState::NotCharging,
            _ => ChargeState::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChargeState::Charging => "Cargando",
            ChargeState::Discharging => "Descargando",
            ChargeState::Full => "Carga completa",
            ChargeState::NotCharging => "Sin cargar",
            ChargeState::Unknown => "Desconocido",
        }
    }
}

/// De dónde obtiene el equipo la energía en este momento
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    Desktop, // Sin batería: siempre conectado a la corriente
    Battery,
    Charger,
    Unknown,
}

impl PowerSource {
    pub fn label(self) -> &'static str {
        match self {
            PowerSource::Desktop => "Corriente directa (sin batería)",
            PowerSource::Battery => "Batería (desconectado)",
            PowerSource::Charger => "Cargador",
            PowerSource::Unknown => "Desconocida",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Battery {
    pub name: String,
    pub model: String,
    pub manufacturer: String,
    pub technology: String,
    pub capacity: Option<u64>,
    pub status: Option<ChargeState>,
    pub cycles: Option<u64>,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_design_wh: Option<f64>,
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
}

impl Battery {
    pub fn state(&self) -> ChargeState {
        self.status.unwrap_or(ChargeState::Unknown)
    }

    /// Salud: capacidad máxima actual frente a la de fábrica
    pub fn health(&self) -> Option<u64> {
        let (full, design) = self.energy_full_wh.zip(self.energy_design_wh)?;
        (design > 0.0).then(|| (full / design * 100.0).round().min(100.0) as u64)
    }

    /// Segundos restantes hasta vaciarse (descargando) o llenarse (cargando)
    pub fn time_left_secs(&self) -> Option<u64> {
        let power = self.power_w.filter(|p| *p > 0.0)?;
        let now = self.energy_now_wh?;
        let wh = match self.state() {
            ChargeState::Discharging => now,
            ChargeState::Charging => (self.energy_full_wh? - now).max(0.0),
            _ => return None,
        };
        Some((wh / power * 3600.0) as u64)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PowerInfo {
    pub batteries: Vec<Battery>,
    /// None si el sistema no expone ningún adaptador de corriente
    pub ac_online: Option<bool>,
}

impl PowerInfo {
    pub fn battery(&self) -> Option<&Battery> {
        self.batteries.first()
    }

    pub fn source(&self) -> PowerSource {
        let Some(bat) = self.battery() else {
            return PowerSource::Desktop;
        };
        match (self.ac_online, bat.state()) {
            (Some(true), _) => PowerSource::Charger,
            (Some(false), _) | (None, ChargeState::Discharging) => PowerSource::Battery,
            (None, ChargeState::Charging | ChargeState::Full | ChargeState::NotCharging) => {
                PowerSource::Charger
            }
            (None, ChargeState::Unknown) => PowerSource::Unknown,
        }
    }

    /// Resumen corto del estado, p. ej. "Cargando · cargador"
    pub fn summary(&self) -> String {
        let Some(bat) = self.battery() else {
            return PowerSource::Desktop.label().to_string();
        };
        match (bat.state(), self.source()) {
            (ChargeState::NotCharging | ChargeState::Full, PowerSource::Charger) => {
                format!("{} · usando el cargador", bat.state().label())
            }
            (state, _) => state.label().to_string(),
        }
    }
}

pub fn collect_power() -> PowerInfo {
    collect_power_from(Path::new(POWER_SUPPLY))
}

fn collect_power_from(root: &Path) -> PowerInfo {
    let mut info = PowerInfo::default();
    let Ok(entries) = fs::read_dir(root) else {
        return info;
    };

    let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        let read = |file: &str| {
            fs::read_to_string(path.join(file))
                .ok()
                .map(|s| s.trim().to_string())
        };
        // Ratones, teclados, etc. reportan scope=Device; no alimentan al equipo
        if read("scope").as_deref() == Some("Device") {
            continue;
        }
        match read("type").as_deref() {
            Some("Battery") => {
                if read("present").as_deref() == Some("0") {
                    continue;
                }
                info.batteries.push(read_battery(&path));
            }
            Some("Mains" | "USB") => {
                let online = read("online").as_deref() == Some("1");
                info.ac_online = Some(info.ac_online.unwrap_or(false) || online);
            }
            _ => {}
        }
    }
    info
}

fn read_battery(path: &Path) -> Battery {
    let text = |file: &str| {
        fs::read_to_string(path.join(file))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let num = |file: &str| text(file).and_then(|s| s.parse::<f64>().ok());

    // sysfs usa micro-unidades: µWh, µW, µAh, µA, µV
    let voltage = num("voltage_now").map(|v| v / 1e6);
    let design_voltage = num("voltage_min_design").map(|v| v / 1e6).or(voltage);

    // Algunas baterías solo exponen carga (µAh) y corriente (µA); se convierten con el voltaje
    let energy = |e: &str, c: &str| {
        num(e)
            .map(|v| v / 1e6)
            .or_else(|| Some(num(c)? / 1e6 * design_voltage?))
    };
    let power_w = num("power_now")
        .map(|p| p / 1e6)
        .or_else(|| Some(num("current_now")? / 1e6 * voltage?))
        .map(f64::abs);

    let capacity = num("capacity").map(|c| c.clamp(0.0, 100.0) as u64);
    let energy_now_wh = energy("energy_now", "charge_now");
    let energy_full_wh = energy("energy_full", "charge_full");

    Battery {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        model: text("model_name").unwrap_or_default(),
        manufacturer: text("manufacturer").unwrap_or_default(),
        technology: text("technology").unwrap_or_default(),
        capacity: capacity.or_else(|| {
            let (now, full) = energy_now_wh.zip(energy_full_wh)?;
            (full > 0.0).then(|| (now / full * 100.0).round().min(100.0) as u64)
        }),
        status: text("status").map(|s| ChargeState::parse(&s)),
        cycles: num("cycle_count").map(|c| c as u64).filter(|c| *c > 0),
        energy_now_wh,
        energy_full_wh,
        energy_design_wh: energy("energy_full_design", "charge_full_design"),
        power_w,
        voltage_v: voltage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str, supplies: &[(&str, &[(&str, &str)])]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ttm-power-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (dev, files) in supplies {
            let dir = root.join(dev);
            fs::create_dir_all(&dir).unwrap();
            for (file, value) in *files {
                fs::write(dir.join(file), format!("{value}\n")).unwrap();
            }
        }
        root
    }

    #[test]
    fn desktop_without_battery() {
        let root = fixture("desktop", &[]);
        let info = collect_power_from(&root);
        assert!(info.battery().is_none());
        assert_eq!(info.source(), PowerSource::Desktop);
        assert_eq!(info.summary(), PowerSource::Desktop.label());
    }

    #[test]
    fn laptop_charging_on_ac() {
        let root = fixture(
            "charging",
            &[
                ("ADP0", &[("type", "Mains"), ("online", "1")]),
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("status", "Charging"),
                        ("capacity", "92"),
                        ("cycle_count", "347"),
                        ("energy_now", "31620000"),
                        ("energy_full", "34250000"),
                        ("energy_full_design", "38000000"),
                        ("power_now", "7878000"),
                        ("voltage_now", "8667000"),
                    ],
                ),
                // Batería de un ratón: debe ignorarse
                (
                    "hidpp_battery_0",
                    &[("type", "Battery"), ("scope", "Device")],
                ),
            ],
        );
        let info = collect_power_from(&root);
        assert_eq!(info.batteries.len(), 1);
        let bat = info.battery().unwrap();
        assert_eq!(bat.capacity, Some(92));
        assert_eq!(bat.health(), Some(90));
        assert_eq!(bat.cycles, Some(347));
        assert_eq!(bat.state(), ChargeState::Charging);
        assert_eq!(info.source(), PowerSource::Charger);
        // (34.25 - 31.62) Wh / 7.878 W ≈ 0.334 h
        assert_eq!(bat.time_left_secs(), Some(1201));
    }

    #[test]
    fn charge_based_battery_discharging() {
        let root = fixture(
            "discharging",
            &[
                ("AC", &[("type", "Mains"), ("online", "0")]),
                (
                    "BAT1",
                    &[
                        ("type", "Battery"),
                        ("status", "Discharging"),
                        ("charge_now", "2000000"),
                        ("charge_full", "4000000"),
                        ("charge_full_design", "5000000"),
                        ("current_now", "1000000"),
                        ("voltage_now", "10000000"),
                        ("voltage_min_design", "10000000"),
                    ],
                ),
            ],
        );
        let info = collect_power_from(&root);
        let bat = info.battery().unwrap();
        assert_eq!(bat.capacity, Some(50));
        assert_eq!(bat.health(), Some(80));
        assert_eq!(bat.power_w, Some(10.0));
        assert_eq!(bat.time_left_secs(), Some(7200));
        assert_eq!(info.source(), PowerSource::Battery);
    }

    #[test]
    fn plugged_in_but_not_charging_runs_on_charger() {
        let root = fixture(
            "threshold",
            &[
                ("AC", &[("type", "Mains"), ("online", "1")]),
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("status", "Not charging"),
                        ("capacity", "80"),
                    ],
                ),
            ],
        );
        let info = collect_power_from(&root);
        assert_eq!(info.source(), PowerSource::Charger);
        assert_eq!(info.summary(), "Sin cargar · usando el cargador");
        assert_eq!(info.battery().unwrap().time_left_secs(), None);
    }
}
