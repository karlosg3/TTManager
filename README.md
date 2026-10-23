# TTM (TuiTaskManager)

Un monitor de recursos del sistema para Linux de alto rendimiento, modular y con interfaz en terminal (TUI) inspirado en el Administrador de Tareas de Windows. Desarrollado en Rust con [`ratatui`](https://crates.io/crates/ratatui), [`crossterm`](https://crates.io/crates/crossterm), [`sysinfo`](https://crates.io/crates/sysinfo) y [`clap`](https://crates.io/crates/clap).

---

## Diseño

Minimalista y pensado para dejarlo siempre abierto: por defecto cada pestaña muestra solo un resumen compacto (estilo neofetch en **Inicio**) y el detalle se despliega bajo demanda con `d`.

| Pestaña | Vista compacta (por defecto) | Con detalle (`d`) |
| :--- | :--- | :--- |
| **1 Inicio** | Logo, sistema, CPU/GPU y barras de CPU, RAM, SWAP, GPU, disco y batería | Mini-gráficas de historial, núcleos, VRAM, procesos y puertos |
| **2 CPU** | Modelo, núcleos, barra de uso y mini-gráfica | Gráfica de 60 s y estadísticas; `c` alterna vista por núcleo |
| **3 Memoria** | Barras de RAM y SWAP con mini-gráficas | Gráficas duales y desglose (total, en uso, disponible, libre) |
| **4 GPU** | Modelo, driver, uso del motor, temperatura y VRAM | Gráfica de 60 s, VRAM, GTT y sensores |
| **5 Disco** | Una barra por disco con usado, libre y total | Tabla con montaje, dispositivo, sistema de archivos y tipo |
| **6 Procesos** | Los 6 primeros, con orden configurable (`s`) | Tabla completa a pantalla completa |
| **7 Puertos** | Los 6 primeros puertos en escucha | Tabla completa a pantalla completa |
| **8 Energía** | Fuente (batería, cargador o corriente directa), carga, salud, consumo y tiempo restante | Gráfica de 60 s, energía actual/máxima/de fábrica, ciclos, voltaje y modelo |

---

## Atajos de Teclado

| Atajo | Acción |
| :--- | :--- |
| `1`-`8` | Ir directamente a una pestaña |
| `Tab` / `→` | Pestaña siguiente |
| `Shift+Tab` / `←` | Pestaña anterior |
| `d` | Desplegar / ocultar el detalle de la pestaña actual |
| `c` | En CPU: alternar **Global** vs **Por Núcleo** (activa el detalle) |
| `s` | En Procesos: cambiar orden (CPU %, RAM MB, PID, Nombre) |
| `↑` / `↓` / `k` / `j` | Mover la fila seleccionada (Procesos y Puertos) |
| `PgUp` / `PgDown` | Mover 10 filas |
| `?` / `h` | Mostrar / ocultar ayuda |
| `q` / `Esc` / `Ctrl+C` | Salir |

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

# Iniciar directamente en otra pestaña: home, cpu, ram, gpu, disk, procs, ports, power
cargo run -- -v cpu
cargo run -- -v procs
```

### Descarga (Releases)

Cada versión publicada en [Releases](https://github.com/karlosg3/TTManager/releases) incluye un binario para Linux x86_64:

```bash
tar -xzf ttm-vX.Y.Z-linux-x86_64.tar.gz
./ttm
```

### Publicar una versión

Los releases son automáticos: al fusionar en `main` un cambio que suba `version` en `Cargo.toml` (por ejemplo a `0.2.0`), el workflow `Release` crea el tag `v0.2.0`, compila y publica el binario. Si la versión ya tiene tag, no hace nada.

### Compilación para Producción

```bash
cargo build --release
./target/release/ttm
```

---

## Estructura del Código

```text
ttm/
├── Cargo.toml      # Configuración del paquete y dependencias
├── Cargo.lock      # Versiones bloqueadas de dependencias
├── README.md       # Documentación completa del proyecto
└── src/
    ├── main.rs     # Orquestador del ciclo de vida, parser CLI y bucle de eventos
    ├── app.rs      # Estado de la aplicación, buffers históricos (VecDeque) y lógica
    ├── ui.rs       # Renderizado de componentes visuales, layouts y widgets Ratatui
    ├── disk.rs     # Detección de discos y particiones montadas
    ├── gpu.rs      # Detección y muestreo de GPU (AMD sysfs, NVIDIA SMI, Intel, lspci)
    ├── net.rs      # Mapeo de sockets y puertos locales a PIDs (/proc/net, /proc/*/fd)
    └── power.rs    # Batería, salud y adaptador de corriente (/sys/class/power_supply)
```

---

## Autor

- **K4** ([llamandoakalos@gmail.com](mailto:llamandoakalos@gmail.com))

---

## Licencia

Este proyecto es software libre bajo los términos de la [Licencia MIT](LICENSE).
