# Bitácora de Proyecto: System Monitor TUI en Rust

Documento de contexto, arquitectura técnica, dependencias y plan de desarrollo iterativo para la construcción de una herramienta TUI (Terminal User Interface) de monitoreo de hardware en Rust.

---

## 1. Fundamentos y Entorno de Desarrollo

### 1.1. Herramientas Base
* **`rustup`**: Gestor de versiones y toolchains de Rust.
* **`rustc`**: Compilador de Rust.
* **`cargo`**: Gestor oficial de paquetes, dependencias, compilación y pruebas.

### 1.2. Comandos Esenciales de Cargo
* `cargo new <nombre> --bin`: Inicializa un proyecto ejecutable.
* `cargo add <crate>`: Registra e instala una dependencia en `Cargo.toml`.
* `cargo check`: Verificación rápida de tipos y compilación sin generar binario.
* `cargo run`: Compila en modo desarrollo y ejecuta la aplicación.
* `cargo build --release`: Compila el binario final optimizado en `target/release/`.

### 1.3. Dependencias del Proyecto (`Cargo.toml`)
```toml
[dependencies]
sysinfo = "0.33"      # Extracción de métricas de CPU, RAM y procesos
ratatui = "0.29"      # Framework para interfaces en terminal (TUI)
crossterm = "0.28"    # Control de terminal (raw mode, eventos de teclado)
clap = { version = "4.5", features = ["derive"] } # Parser de argumentos CLI
```
---

## 2. Arquitectura y Modularización del Código

El archivo `main.rs` deja de concentrar toda la lógica y pasa a actuar como orquestador del ciclo de vida. El proyecto se estructura en tres capas:

```
sys_tui/
├── Cargo.toml
└── src/
    ├── main.rs   # Orquestador: ciclo de vida de terminal, bucle de eventos e I/O
    ├── app.rs    # Estado de la aplicación: structs, enums, métricas y lógica interna
    └── ui.rs     # Renderizado visual: layouts, widgets y tabs con Ratatui
```

### 2.1. Conexión de Módulos

En Rust, la visibilidad entre archivos se declara en el punto de entrada:

```Rust
// src/main.rs
mod app;
mod ui;

use app::App;
```

## 3. Especificación del Estado (`src/app.rs`)

```Rust
use std::collections::VecDeque;

pub enum CurrentView {
    Overview, // Vista 1: Resumen global, procesos y puertos
    Cpu,      // Vista 2: Detalle de CPU (Global vs Por Núcleo)
    Ram,      // Vista 3: Detalle de memoria física y swap
    Gpu,      // Vista 4: Detalle de cómputo y VRAM
}

pub enum CpuMode {
    Global,
    PerCore,
}

pub struct App {
    pub current_view: CurrentView,
    pub cpu_mode: CpuMode,
    pub should_quit: bool,
    
    // Buffers circulares para gráficas de historial (máximo 60 muestras)
    pub cpu_history: VecDeque<u64>,
    pub ram_history: VecDeque<u64>,
    pub gpu_history: VecDeque<u64>,
}
```

## 4. Requerimientos de Vistas

    Vista 1: Resumen General (Overview)

        Base: Porcentajes actuales de CPU, RAM y GPU.

        Intermedio: Tabla ordenada de procesos activos con su consumo respectivo.

        Avanzado: Mapeo de puertos locales en escucha (backend, frontend, contenedores) cruzados con su PID.

    Vista 2: Detalle de CPU

        Alternancia mediante atajo entre vista de uso general y vista de todos los núcleos individuales.

        Gráfica histórica de uso continuo (estilo Administrador de Tareas de Windows).

    Vista 3: Detalle de RAM

        Historial continuo de uso temporal de memoria física y swap.

    Vista 4: Detalle de GPU

        Porcentaje de cómputo del chip, consumo de VRAM y gráfica histórica en tiempo real.

## 5. Plan de Iteraciones de Desarrollo

Fase,Objetivo Principal,Conceptos Clave de Rust,Componentes Ratatui
Fase 1,Arquitectura base y navegación por pestañas,"enum, match, modularidad (mod/pub), structs","Tabs, Block, Layout"
Fase 2,Vistas detalladas con historial de CPU y RAM,"Colecciones (VecDeque), slices, conversiones numéricas","Sparkline, Chart, BarChart"
Fase 3,Integración de métricas de GPU,Manejo defensivo con Option<T> y Result,"Gauge, Chart"
Fase 4,Procesos y puertos en uso,"Iteradores (filter, map, sort_by), parsing de sockets de red","Table, Scrollbar"
