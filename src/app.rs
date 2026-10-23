use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use sysinfo::{ProcessesToUpdate, System};

use crate::disk::{DiskItem, collect_disks};
use crate::gpu::{GpuCollector, GpuInfo};
use crate::net::{PortItem, collect_listening_ports};
use crate::power::{PowerInfo, collect_power};
use crate::updater::{self, Version};

const HISTORY_CAPACITY: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentView {
    Home,  // Resumen estilo neofetch
    Cpu,   // CPU (Global vs Por Núcleo)
    Ram,   // Memoria física y swap
    Gpu,   // Cómputo y VRAM
    Disk,  // Almacenamiento
    Procs, // Tabla de procesos
    Ports, // Tabla de puertos en escucha
    Power, // Batería y fuente de energía
}

impl CurrentView {
    pub const ALL: [CurrentView; 8] = [
        CurrentView::Home,
        CurrentView::Cpu,
        CurrentView::Ram,
        CurrentView::Gpu,
        CurrentView::Disk,
        CurrentView::Procs,
        CurrentView::Ports,
        CurrentView::Power,
    ];

    pub fn title(self) -> &'static str {
        match self {
            CurrentView::Home => "Inicio",
            CurrentView::Cpu => "CPU",
            CurrentView::Ram => "Memoria",
            CurrentView::Gpu => "GPU",
            CurrentView::Disk => "Disco",
            CurrentView::Procs => "Procesos",
            CurrentView::Ports => "Puertos",
            CurrentView::Power => "Energía",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    Global,
    PerCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSort {
    CpuDesc,
    MemDesc,
    Pid,
    Name,
}

impl ProcessSort {
    pub fn label(&self) -> &'static str {
        match self {
            ProcessSort::CpuDesc => "CPU % ↓",
            ProcessSort::MemDesc => "RAM MB ↓",
            ProcessSort::Pid => "PID ↑",
            ProcessSort::Name => "Nombre A-Z",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessItem {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub memory_percent: f32,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub host_name: String,
    pub os_name: String,
    pub kernel_version: String,
    pub cpu_model: String,
    pub cpu_arch: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub frequency_mhz: u64,
}

pub struct App {
    pub current_view: CurrentView,
    pub cpu_mode: CpuMode,
    pub should_quit: bool,
    pub show_help: bool,
    pub expanded: bool,

    // Buffers circulares para gráficas de historial (máximo 60 muestras)
    pub cpu_history: VecDeque<u64>,
    pub ram_history: VecDeque<u64>,
    pub gpu_history: VecDeque<u64>,
    pub swap_history: VecDeque<u64>,
    pub per_core_history: Vec<VecDeque<u64>>,
    pub battery_history: VecDeque<u64>,

    pub sys: System,
    pub gpu_collector: GpuCollector,
    pub system_info: SystemInfo,
    pub processes: Vec<ProcessItem>,
    pub ports: Vec<PortItem>,
    pub disks: Vec<DiskItem>,
    pub power: PowerInfo,

    pub process_sort: ProcessSort,
    pub process_table_state: usize,
    pub port_table_state: usize,

    // Actualizaciones: versión nueva encontrada, aviso abierto y respuesta
    pub update_available: Option<Version>,
    pub show_update_prompt: bool,
    pub update_accepted: bool,
    update_rx: Option<Receiver<Version>>,

    last_proc_update: Instant,
    last_net_update: Instant,
    last_disk_update: Instant,
    last_power_update: Instant,
}

impl App {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        // Give sysinfo initial sampling
        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let gpu_collector = GpuCollector::new();

        // System information detection
        let host_name = System::host_name().unwrap_or_else(|| "Linux-PC".to_string());
        let os_name = System::name()
            .map(|n| {
                if let Some(v) = System::os_version() {
                    format!("{n} {v}")
                } else {
                    n
                }
            })
            .unwrap_or_else(|| "Linux".to_string());
        let kernel_version = System::kernel_version().unwrap_or_else(|| "-".to_string());
        let cpu_arch = System::cpu_arch();

        let (cpu_model, frequency_mhz) = if let Some(cpu) = sys.cpus().first() {
            (cpu.brand().trim().to_string(), cpu.frequency())
        } else {
            ("CPU Genérico".to_string(), 0)
        };

        let logical_cores = sys.cpus().len();
        let physical_cores = System::physical_core_count().unwrap_or(logical_cores);

        let system_info = SystemInfo {
            host_name,
            os_name,
            kernel_version,
            cpu_model,
            cpu_arch,
            physical_cores,
            logical_cores,
            frequency_mhz,
        };

        let mut per_core_history = Vec::with_capacity(logical_cores);
        for _ in 0..logical_cores {
            let mut q = VecDeque::with_capacity(HISTORY_CAPACITY);
            q.push_back(0);
            per_core_history.push(q);
        }

        let mut cpu_history = VecDeque::with_capacity(HISTORY_CAPACITY);
        let mut ram_history = VecDeque::with_capacity(HISTORY_CAPACITY);
        let mut gpu_history = VecDeque::with_capacity(HISTORY_CAPACITY);
        let mut swap_history = VecDeque::with_capacity(HISTORY_CAPACITY);

        let initial_cpu = sys.global_cpu_usage().round() as u64;
        let initial_ram = if sys.total_memory() > 0 {
            ((sys.used_memory() as f64 / sys.total_memory() as f64) * 100.0).round() as u64
        } else {
            0
        };
        let initial_swap = if sys.total_swap() > 0 {
            ((sys.used_swap() as f64 / sys.total_swap() as f64) * 100.0).round() as u64
        } else {
            0
        };
        let initial_gpu = gpu_collector.info.utilization.unwrap_or(0.0).round() as u64;

        cpu_history.push_back(initial_cpu);
        ram_history.push_back(initial_ram);
        swap_history.push_back(initial_swap);
        gpu_history.push_back(initial_gpu);

        let ports = collect_listening_ports(&sys);
        let power = collect_power();
        let mut battery_history = VecDeque::with_capacity(HISTORY_CAPACITY);
        if let Some(capacity) = power.battery().and_then(|b| b.capacity) {
            battery_history.push_back(capacity);
        }

        let mut app = Self {
            current_view: CurrentView::Home,
            cpu_mode: CpuMode::Global,
            should_quit: false,
            show_help: false,
            expanded: false,
            cpu_history,
            ram_history,
            gpu_history,
            swap_history,
            per_core_history,
            battery_history,
            sys,
            gpu_collector,
            system_info,
            processes: Vec::new(),
            ports,
            disks: collect_disks(),
            power,
            process_sort: ProcessSort::CpuDesc,
            process_table_state: 0,
            port_table_state: 0,
            update_available: None,
            show_update_prompt: false,
            update_accepted: false,
            update_rx: None,
            last_proc_update: Instant::now(),
            last_net_update: Instant::now(),
            last_disk_update: Instant::now(),
            last_power_update: Instant::now(),
        };

        app.update_processes();
        app
    }

    /// Fuera de `new()` para que los tests no hagan peticiones de red
    pub fn start_update_check(&mut self) {
        self.update_rx = Some(updater::spawn_check());
    }

    pub fn tick(&mut self) {
        // try_recv no bloquea: si el hilo aún no respondió, seguimos
        if let Some(version) = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.update_available = Some(version);
            self.show_update_prompt = true;
            self.update_rx = None;
        }

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.gpu_collector.update();

        let cpu_usage = self.sys.global_cpu_usage().round().min(100.0) as u64;
        push_history(&mut self.cpu_history, cpu_usage);

        let ram_pct = if self.sys.total_memory() > 0 {
            ((self.sys.used_memory() as f64 / self.sys.total_memory() as f64) * 100.0)
                .round()
                .min(100.0) as u64
        } else {
            0
        };
        push_history(&mut self.ram_history, ram_pct);

        let swap_pct = if self.sys.total_swap() > 0 {
            ((self.sys.used_swap() as f64 / self.sys.total_swap() as f64) * 100.0)
                .round()
                .min(100.0) as u64
        } else {
            0
        };
        push_history(&mut self.swap_history, swap_pct);

        let gpu_usage = self
            .gpu_collector
            .info
            .utilization
            .unwrap_or(0.0)
            .round()
            .min(100.0) as u64;
        push_history(&mut self.gpu_history, gpu_usage);

        for (i, cpu) in self.sys.cpus().iter().enumerate() {
            if i < self.per_core_history.len() {
                let usage = cpu.cpu_usage().round().min(100.0) as u64;
                push_history(&mut self.per_core_history[i], usage);
            }
        }

        // Update processes every ~1.5s
        if self.last_proc_update.elapsed().as_millis() >= 1500 {
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
            self.update_processes();
            self.last_proc_update = Instant::now();
        }

        // Update listening ports every ~4.0s
        if self.last_net_update.elapsed().as_millis() >= 4000 {
            self.ports = collect_listening_ports(&self.sys);
            self.last_net_update = Instant::now();
        }

        // El almacenamiento cambia despacio: ~5 s
        if self.last_disk_update.elapsed().as_secs() >= 5 {
            self.disks = collect_disks();
            self.last_disk_update = Instant::now();
        }

        // Batería y adaptador: ~2 s, para que el consumo en vatios se vea vivo
        if self.last_power_update.elapsed().as_millis() >= 2000 {
            self.power = collect_power();
            self.last_power_update = Instant::now();
        }
        if let Some(capacity) = self.power.battery().and_then(|b| b.capacity) {
            push_history(&mut self.battery_history, capacity);
        }
    }

    pub fn update_processes(&mut self) {
        let total_mem = self.sys.total_memory() as f32;
        let mut list: Vec<ProcessItem> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, proc)| {
                let mem_bytes = proc.memory();
                let mem_pct = if total_mem > 0.0 {
                    (mem_bytes as f32 / total_mem) * 100.0
                } else {
                    0.0
                };
                let status_str = format!("{:?}", proc.status());

                ProcessItem {
                    pid: pid.as_u32(),
                    name: proc.name().to_string_lossy().to_string(),
                    cpu_usage: proc.cpu_usage(),
                    memory_bytes: mem_bytes,
                    memory_percent: mem_pct,
                    status: status_str,
                }
            })
            .collect();

        match self.process_sort {
            ProcessSort::CpuDesc => {
                list.sort_by(|a, b| {
                    b.cpu_usage
                        .partial_cmp(&a.cpu_usage)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            ProcessSort::MemDesc => {
                list.sort_by_key(|a| std::cmp::Reverse(a.memory_bytes));
            }
            ProcessSort::Pid => {
                list.sort_by_key(|a| a.pid);
            }
            ProcessSort::Name => {
                list.sort_by_key(|a| a.name.to_lowercase());
            }
        }

        self.processes = list;
        if self.process_table_state >= self.processes.len() && !self.processes.is_empty() {
            self.process_table_state = self.processes.len() - 1;
        }
    }

    pub fn next_view(&mut self) {
        let all = CurrentView::ALL;
        self.current_view = all[(self.current_view.index() + 1) % all.len()];
    }

    pub fn prev_view(&mut self) {
        let all = CurrentView::ALL;
        self.current_view = all[(self.current_view.index() + all.len() - 1) % all.len()];
    }

    pub fn set_view(&mut self, view: CurrentView) {
        self.current_view = view;
    }

    pub fn toggle_cpu_mode(&mut self) {
        self.cpu_mode = match self.cpu_mode {
            CpuMode::Global => CpuMode::PerCore,
            CpuMode::PerCore => CpuMode::Global,
        };
        // La vista por núcleo solo existe en modo detallado
        if self.cpu_mode == CpuMode::PerCore {
            self.expanded = true;
        }
    }

    pub fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }

    pub fn toggle_process_sort(&mut self) {
        self.process_sort = match self.process_sort {
            ProcessSort::CpuDesc => ProcessSort::MemDesc,
            ProcessSort::MemDesc => ProcessSort::Pid,
            ProcessSort::Pid => ProcessSort::Name,
            ProcessSort::Name => ProcessSort::CpuDesc,
        };
        self.update_processes();
    }

    fn active_table(&mut self) -> Option<(&mut usize, usize)> {
        match self.current_view {
            CurrentView::Procs => Some((&mut self.process_table_state, self.processes.len())),
            CurrentView::Ports => Some((&mut self.port_table_state, self.ports.len())),
            _ => None,
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if let Some((state, len)) = self.active_table()
            && len > 0
        {
            *state = state.saturating_add_signed(delta).min(len - 1);
        }
    }

    pub fn scroll_up(&mut self) {
        self.move_cursor(-1);
    }

    pub fn scroll_down(&mut self) {
        self.move_cursor(1);
    }

    pub fn page_up(&mut self) {
        self.move_cursor(-10);
    }

    pub fn page_down(&mut self) {
        self.move_cursor(10);
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// "Sí": se cierra la TUI y main.rs instala la actualización
    pub fn accept_update(&mut self) {
        self.update_accepted = true;
        self.quit();
    }

    /// "No": no volver a avisar de esta versión
    pub fn decline_update(&mut self) {
        if let Some(version) = self.update_available.take() {
            updater::skip(version);
        }
        self.show_update_prompt = false;
    }

    /// "Más tarde": se cierra el aviso pero sigue disponible con `u`
    pub fn postpone_update(&mut self) {
        self.show_update_prompt = false;
    }

    pub fn open_update_prompt(&mut self) {
        self.show_update_prompt = self.update_available.is_some();
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn uptime_formatted(&self) -> String {
        let secs = System::uptime();
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        if days > 0 {
            format!("{days}d {hours:02}:{mins:02}:{s:02}")
        } else {
            format!("{hours:02}:{mins:02}:{s:02}")
        }
    }

    /// Disco principal para el resumen: la raíz, o el primero disponible
    pub fn primary_disk(&self) -> Option<&DiskItem> {
        self.disks
            .iter()
            .find(|d| d.mount == "/")
            .or_else(|| self.disks.first())
    }

    pub fn gpu_info(&self) -> &GpuInfo {
        &self.gpu_collector.info
    }
}

fn push_history(queue: &mut VecDeque<u64>, val: u64) {
    if queue.len() >= HISTORY_CAPACITY {
        queue.pop_front();
    }
    queue.push_back(val);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_metrics() {
        let app = App::new();
        println!("Host: {}", app.system_info.host_name);
        println!("OS: {}", app.system_info.os_name);
        println!("Kernel: {}", app.system_info.kernel_version);
        println!("CPU: {}", app.system_info.cpu_model);
        println!("Logical cores: {}", app.system_info.logical_cores);
        println!("GPU Name: {}", app.gpu_info().name);
        println!("GPU Driver: {}", app.gpu_info().driver);
        println!("GPU Util: {:?}", app.gpu_info().utilization);
        println!("GPU VRAM used: {:?}", app.gpu_info().vram_used);
        println!("GPU Temp: {:?}", app.gpu_info().temperature);
        println!("Processes count: {}", app.processes.len());
        println!("Ports count: {}", app.ports.len());
        assert!(!app.system_info.cpu_model.is_empty());
        assert!(!app.processes.is_empty());
    }
}

#[test]
fn test_navigation_and_modes() {
    let mut app = App::new();
    assert_eq!(app.current_view, CurrentView::Home);

    for expected in [
        CurrentView::Cpu,
        CurrentView::Ram,
        CurrentView::Gpu,
        CurrentView::Disk,
        CurrentView::Procs,
        CurrentView::Ports,
        CurrentView::Power,
        CurrentView::Home,
    ] {
        app.next_view();
        assert_eq!(app.current_view, expected);
    }

    app.prev_view();
    assert_eq!(app.current_view, CurrentView::Power);

    assert!(!app.expanded);
    app.toggle_expanded();
    assert!(app.expanded);
    app.toggle_expanded();

    assert_eq!(app.cpu_mode, CpuMode::Global);
    app.toggle_cpu_mode();
    assert_eq!(app.cpu_mode, CpuMode::PerCore);
    assert!(app.expanded);
    app.toggle_cpu_mode();
    assert_eq!(app.cpu_mode, CpuMode::Global);

    assert_eq!(app.process_sort, ProcessSort::CpuDesc);
    app.toggle_process_sort();
    assert_eq!(app.process_sort, ProcessSort::MemDesc);
}

#[test]
fn test_scroll_only_in_tables() {
    let mut app = App::new();
    app.scroll_down();
    assert_eq!(app.process_table_state, 0);

    app.set_view(CurrentView::Procs);
    app.scroll_down();
    assert_eq!(app.process_table_state, 1);
    app.scroll_up();
    assert_eq!(app.process_table_state, 0);
    app.page_down();
    assert_eq!(app.process_table_state, 10.min(app.processes.len() - 1));
}

#[test]
fn test_history_capacity() {
    let mut q = VecDeque::new();
    for i in 0..100 {
        push_history(&mut q, i);
    }
    assert_eq!(q.len(), HISTORY_CAPACITY);
    assert_eq!(*q.front().unwrap(), 40);
    assert_eq!(*q.back().unwrap(), 99);
}
