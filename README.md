# sys_monitor

Un monitor de recursos del sistema para Linux de alto rendimiento, modular y con interfaz en terminal (TUI) inspirado en el Administrador de Tareas de Windows. Desarrollado en Rust con [`ratatui`](https://crates.io/crates/ratatui), [`crossterm`](https://crates.io/crates/crossterm), [`sysinfo`](https://crates.io/crates/sysinfo) y [`clap`](https://crates.io/crates/clap).

---

## Características Principales

- **Panel Superior de Hardware y Sistema**:
  - Detección precisa del modelo de procesador (e.g. AMD Ryzen / Intel Core) y recuento de hilos.
  - Detección del modelo de tarjeta gráfica y driver activo (AMD DRM/sysfs, NVIDIA SMI, Intel).
  - Nombre del dispositivo / host, distribución Linux, versión de Kernel y tiempo activo (uptime).
- **Vista 1: Resumen General (Overview)**:
  - 4 tarjetas superiores con medidores porcentuales (Gauge), métricas clave y mini-gráficas en tiempo real (Sparkline) para CPU, RAM, SWAP y GPU.
  - Tabla de procesos activos en tiempo real con PID, nombre, consumo de CPU %, memoria utilizada (MB/GB y %) y estado.
  - Tabla de puertos locales en escucha cruzados con su respectivo PID y nombre de proceso asociado vía `/proc/net` y sockets.
  - Alternancia de foco entre tablas y ordenamiento configurable por CPU, RAM, PID o Nombre.
- **Vista 2: Detalle de CPU**:
  - **Modo Global**: Gráfica histórica continua de alta resolución (Braille dots, escala 0%-100%, 60 segundos) y panel de estadísticas completas (velocidad en GHz, núcleos físicos, lógicos, arquitectura, procesos).
  - **Modo Por Núcleo (Per-Core)**: Visualización en cuadrícula con métricas, medidores y gráficas individuales por cada núcleo/hilo del procesador. Alternable con la tecla `[c]`.
- **Vista 3: Detalle de Memoria (RAM y SWAP)**:
  - Gráficas históricas duales en tiempo real para memoria física y archivo de paginación (SWAP).
  - Desglose detallado: Memoria total, en uso, disponible, libre, SWAP en uso y libre.
- **Vista 4: Detalle de GPU**:
  - Gráfica histórica de utilización del motor de cómputo en tiempo real (0%-100%).
  - Panel de memoria de video (VRAM usada / total), memoria compartida dinámica (GTT), temperatura del chip (°C) y controlador.
- **Navegación Intuitiva y Ayuda**:
  - Navegación rápida por pestañas numeradas `[1-4]`, flechas o `[Tab]`.
  - Ventana modal de ayuda integrada pulsando `[?]` o `[h]`.
  - Salida limpia y restauración garantizada de la terminal incluso en caso de interrupción (`Ctrl+C`, `Esc`, `q`).

---

## Atajos de Teclado

| Atajo | Acción |
| :--- | :--- |
| `1`, `2`, `3`, `4` | Cambiar directamente a la pestaña (1: Resumen, 2: CPU, 3: Memoria, 4: GPU) |
| `Tab` / `→` | Pestaña siguiente |
| `Shift+Tab` / `←` | Pestaña anterior |
| `c` / `C` | En pestaña CPU: Alternar vista **Global** vs **Por Núcleo (PerCore)** |
| `p` / `P` | En Resumen: Alternar foco entre tabla de Procesos y tabla de Puertos |
| `s` / `S` | En Resumen: Cambiar orden de procesos (CPU %, RAM MB, PID, Nombre) |
| `↑` / `↓` / `k` / `j` | Desplazar fila seleccionada en la tabla activa |
| `PgUp` / `PgDown` | Desplazar 10 filas en la tabla |
| `?` / `h` | Mostrar / ocultar ventana flotante de ayuda |
| `q` / `Esc` / `Ctrl+C` | Salir de la aplicación |

---

## Instalación y Ejecución

### Requisitos Previos

- **Rust & Cargo**: Toolchain de Rust (Edición 2024 / Rust 1.85+)
- **Sistema Operativo**: Linux

### Ejecución en Desarrollo

```bash
cargo run
```

### Opciones de Línea de Comandos

```bash
# Cambiar intervalo de actualización a 500 ms
cargo run -- -i 500

# Iniciar directamente en la vista de CPU o GPU
cargo run -- -v cpu
cargo run -- -v gpu
```

### Compilación para Producción

```bash
cargo build --release
./target/release/sys_monitor
```

---

## Estructura del Código

```text
sys_monitor/
├── Cargo.toml      # Configuración del paquete y dependencias
├── Cargo.lock      # Versiones bloqueadas de dependencias
├── README.md       # Documentación completa del proyecto
└── src/
    ├── main.rs     # Orquestador del ciclo de vida, parser CLI y bucle de eventos
    ├── app.rs      # Estado de la aplicación, buffers históricos (VecDeque) y lógica
    ├── ui.rs       # Renderizado de componentes visuales, layouts y widgets Ratatui
    ├── gpu.rs      # Detección y muestreo de GPU (AMD sysfs, NVIDIA SMI, Intel, lspci)
    └── net.rs      # Mapeo de sockets y puertos locales a PIDs (/proc/net, /proc/*/fd)
```

---

## Autor

- **K4** ([llamandoakalos@gmail.com](mailto:llamandoakalos@gmail.com))

---

## Licencia

Este proyecto es software libre bajo los términos de la [Licencia MIT](LICENSE).
