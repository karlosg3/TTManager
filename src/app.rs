use std::collections::VecDeque;
use std::time::Instant;
use sysinfo::{ProcessesToUpdate, System};

use crate::gpu::{GpuCollector, GpuInfo};
use crate::net::{PortItem, collect_listening_ports};

const HISTORY_CAPACITY: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentView {
    Overview, // Vista 1: Resumen global, procesos y puertos
    Cpu,      // Vista 2: Detalle de CPU (Global vs Por Núcleo)
    Ram,      // Vista 3: Detalle de memoria física y swap
    Gpu,      // Vista 4: Detalle de cómputo y VRAM
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewFocus {
    Processes,
    Ports,
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

    // Buffers circulares para gráficas de historial (máximo 60 muestras)
    pub cpu_history: VecDeque<u64>,
    pub ram_history: VecDeque<u64>,
    pub gpu_history: VecDeque<u64>,
    pub swap_history: VecDeque<u64>,
    pub per_core_history: Vec<VecDeque<u64>>,

    pub sys: System,
    pub gpu_collector: GpuCollector,
    pub system_info: SystemInfo,
    pub processes: Vec<ProcessItem>,
    pub ports: Vec<PortItem>,

    pub process_sort: ProcessSort,
    pub overview_focus: OverviewFocus,
    pub process_table_state: usize,
    pub port_table_state: usize,

    last_proc_update: Instant,
    last_net_update: Instant,
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

        let mut app = Self {
            current_view: CurrentView::Overview,
            cpu_mode: CpuMode::Global,
            should_quit: false,
            show_help: false,
            cpu_history,
            ram_history,
            gpu_history,
            swap_history,
            per_core_history,
            sys,
            gpu_collector,
            system_info,
            processes: Vec::new(),
            ports,
            process_sort: ProcessSort::CpuDesc,
            overview_focus: OverviewFocus::Processes,
            process_table_state: 0,
            port_table_state: 0,
            last_proc_update: Instant::now(),
            last_net_update: Instant::now(),
        };

        app.update_processes();
        app
    }

    pub fn tick(&mut self) {
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
        self.current_view = match self.current_view {
            CurrentView::Overview => CurrentView::Cpu,
            CurrentView::Cpu => CurrentView::Ram,
            CurrentView::Ram => CurrentView::Gpu,
            CurrentView::Gpu => CurrentView::Overview,
        };
    }

    pub fn prev_view(&mut self) {
        self.current_view = match self.current_view {
            CurrentView::Overview => CurrentView::Gpu,
            CurrentView::Cpu => CurrentView::Overview,
            CurrentView::Ram => CurrentView::Cpu,
            CurrentView::Gpu => CurrentView::Ram,
        };
    }

    pub fn set_view(&mut self, view: CurrentView) {
        self.current_view = view;
    }

    pub fn toggle_cpu_mode(&mut self) {
        self.cpu_mode = match self.cpu_mode {
            CpuMode::Global => CpuMode::PerCore,
            CpuMode::PerCore => CpuMode::Global,
        };
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

    pub fn toggle_overview_focus(&mut self) {
        self.overview_focus = match self.overview_focus {
            OverviewFocus::Processes => OverviewFocus::Ports,
            OverviewFocus::Ports => OverviewFocus::Processes,
        };
    }

    pub fn scroll_up(&mut self) {
        match self.overview_focus {
            OverviewFocus::Processes => {
                if self.process_table_state > 0 {
                    self.process_table_state -= 1;
                }
            }
            OverviewFocus::Ports => {
                if self.port_table_state > 0 {
                    self.port_table_state -= 1;
                }
            }
        }
    }

    pub fn scroll_down(&mut self) {
        match self.overview_focus {
            OverviewFocus::Processes => {
                if !self.processes.is_empty() && self.process_table_state < self.processes.len() - 1
                {
                    self.process_table_state += 1;
                }
            }
            OverviewFocus::Ports => {
                if !self.ports.is_empty() && self.port_table_state < self.ports.len() - 1 {
                    self.port_table_state += 1;
                }
            }
        }
    }

    pub fn page_up(&mut self) {
        match self.overview_focus {
            OverviewFocus::Processes => {
                self.process_table_state = self.process_table_state.saturating_sub(10);
            }
            OverviewFocus::Ports => {
                self.port_table_state = self.port_table_state.saturating_sub(10);
            }
        }
    }

    pub fn page_down(&mut self) {
        match self.overview_focus {
            OverviewFocus::Processes => {
                if !self.processes.is_empty() {
                    self.process_table_state =
                        (self.process_table_state + 10).min(self.processes.len() - 1);
                }
            }
            OverviewFocus::Ports => {
                if !self.ports.is_empty() {
                    self.port_table_state = (self.port_table_state + 10).min(self.ports.len() - 1);
                }
            }
        }
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
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
    assert_eq!(app.current_view, CurrentView::Overview);

    app.next_view();
    assert_eq!(app.current_view, CurrentView::Cpu);
    app.next_view();
    assert_eq!(app.current_view, CurrentView::Ram);
    app.next_view();
    assert_eq!(app.current_view, CurrentView::Gpu);
    app.next_view();
    assert_eq!(app.current_view, CurrentView::Overview);

    app.prev_view();
    assert_eq!(app.current_view, CurrentView::Gpu);

    assert_eq!(app.cpu_mode, CpuMode::Global);
    app.toggle_cpu_mode();
    assert_eq!(app.cpu_mode, CpuMode::PerCore);
    app.toggle_cpu_mode();
    assert_eq!(app.cpu_mode, CpuMode::Global);

    assert_eq!(app.overview_focus, OverviewFocus::Processes);
    app.toggle_overview_focus();
    assert_eq!(app.overview_focus, OverviewFocus::Ports);

    assert_eq!(app.process_sort, ProcessSort::CpuDesc);
    app.toggle_process_sort();
    assert_eq!(app.process_sort, ProcessSort::MemDesc);
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
