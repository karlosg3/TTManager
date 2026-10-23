use std::collections::VecDeque;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Cell, Chart, Clear, Dataset, GraphType, Paragraph, Row,
        Table, Wrap,
    },
};

use crate::app::{App, CpuMode, CurrentView};
use crate::power::{Battery, ChargeState, PowerInfo};
use crate::updater::Version;

const GIB: f64 = 1_073_741_824.0;
const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const SPARKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
// Tux; las dos últimas filas (patas) van en amarillo
const LOGO: [&str; 7] = [
    "      .--.",
    "     |o_o |",
    "     |:_/ |",
    "    //   \\ \\",
    "   (|     | )",
    "  /'\\_   _/`\\",
    "  \\___)=(___/",
];
const LOGO_WIDTH: u16 = 24;

const LABEL_WIDTH: usize = 12;

pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(2), // Pestañas
        Constraint::Min(0),    // Contenido
        Constraint::Length(1), // Atajos
    ])
    .split(size);

    draw_header(f, app, chunks[0]);

    let body = chunks[1].inner(Margin::new(1, 1));
    match app.current_view {
        CurrentView::Home => draw_home(f, app, body),
        CurrentView::Cpu => draw_cpu(f, app, body),
        CurrentView::Ram => draw_ram(f, app, body),
        CurrentView::Gpu => draw_gpu(f, app, body),
        CurrentView::Disk => draw_disk(f, app, body),
        CurrentView::Procs => draw_procs(f, app, body),
        CurrentView::Ports => draw_ports(f, app, body),
        CurrentView::Power => draw_power(f, app, body),
    }

    draw_footer(f, app, chunks[2]);

    if app.show_help {
        draw_help_modal(f, size);
    }
    if app.show_update_prompt
        && let Some(version) = app.update_available
    {
        draw_update_modal(f, size, version);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn level_color(pct: u64) -> Color {
    if pct >= 85 {
        Color::Red
    } else if pct >= 60 {
        Color::Yellow
    } else {
        ACCENT
    }
}

fn label(text: &str) -> Span<'static> {
    Span::styled(
        format!("{text:<LABEL_WIDTH$}"),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn kv(key: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![label(key), Span::raw(value.into())])
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(DIM))
}

/// Barra delgada: tramo lleno con color por nivel, resto atenuado.
fn bar(pct: u64, width: usize) -> Vec<Span<'static>> {
    colored_bar(pct, width, level_color(pct))
}

fn colored_bar(pct: u64, width: usize, color: Color) -> Vec<Span<'static>> {
    let filled = (pct.min(100) as usize * width + 50) / 100;
    vec![
        Span::styled("━".repeat(filled), Style::default().fg(color)),
        dim("━".repeat(width - filled)),
    ]
}

/// Al revés que el uso: en batería y salud lo preocupante es un valor bajo
fn charge_color(pct: u64) -> Color {
    if pct <= 15 {
        Color::Red
    } else if pct <= 35 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn charge_metric(name: &str, pct: u64, detail: String, bar_width: usize) -> Line<'static> {
    let mut spans = vec![label(name)];
    spans.extend(colored_bar(pct, bar_width, charge_color(pct)));
    spans.push(Span::raw(format!(" {pct:>3}%")));
    if !detail.is_empty() {
        spans.push(dim(format!("  {detail}")));
    }
    Line::from(spans)
}

fn metric(name: &str, pct: u64, detail: String, bar_width: usize) -> Line<'static> {
    let mut spans = vec![label(name)];
    spans.extend(bar(pct, bar_width));
    spans.push(Span::raw(format!(" {pct:>3}%")));
    if !detail.is_empty() {
        spans.push(dim(format!("  {detail}")));
    }
    Line::from(spans)
}

/// Mini gráfica de texto con las últimas `width` muestras (0-100).
fn spark_line(history: &VecDeque<u64>, width: usize) -> Line<'static> {
    let skip = history.len().saturating_sub(width);
    let text: String = history
        .iter()
        .skip(skip)
        .map(|&v| SPARKS[((v.min(100) * 7 + 50) / 100) as usize])
        .collect();
    Line::from(vec![
        Span::raw(" ".repeat(LABEL_WIDTH)),
        Span::styled(text, Style::default().fg(ACCENT)),
    ])
}

/// Tamaño legible: GB, o TB a partir de 1024 GB
fn size(bytes: u64) -> String {
    let gb = bytes as f64 / GIB;
    if gb >= 1024.0 {
        format!("{:.2} TB", gb / 1024.0)
    } else {
        format!("{gb:.1} GB")
    }
}

fn last(history: &VecDeque<u64>) -> u64 {
    *history.back().unwrap_or(&0)
}

fn pct_of(used: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (used as f64 / total as f64 * 100.0).round().min(100.0) as u64
    }
}

fn series(history: &VecDeque<u64>) -> Vec<(f64, f64)> {
    history
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as f64, v as f64))
        .collect()
}

fn history_chart<'a>(title: String, data: &'a [(f64, f64)], color: Color) -> Chart<'a> {
    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(color))
        .data(data);

    let axis_style = Style::default().fg(DIM);
    Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(axis_style)
                .title(Span::styled(
                    title,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
        )
        .x_axis(
            Axis::default()
                .style(axis_style)
                .bounds([0.0, 60.0])
                .labels(["60s", "30s", "0s"]),
        )
        .y_axis(
            Axis::default()
                .style(axis_style)
                .bounds([0.0, 100.0])
                .labels(["0%", "50%", "100%"]),
        )
}

fn render_columns(f: &mut Frame, area: Rect, columns: Vec<Vec<Line<'static>>>) {
    let n = columns.len() as u32;
    let areas = Layout::horizontal(vec![Constraint::Ratio(1, n); columns.len()]).split(area);
    for (col, rect) in columns.into_iter().zip(areas.iter()) {
        f.render_widget(Paragraph::new(col), *rect);
    }
}

fn bar_width(area: Rect) -> usize {
    // etiqueta + porcentaje + detalle (~18 columnas) deben caber junto a la barra
    (area.width as usize)
        .saturating_sub(LABEL_WIDTH + 5 + 18)
        .clamp(8, 70)
}

/// Duración legible: "1 h 05 min" o "12 min"
fn duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours} h {mins:02} min")
    } else {
        format!("{mins} min")
    }
}

fn hint(text: &str) -> Line<'static> {
    Line::from(dim(text.to_string()))
}

// ---------------------------------------------------------------------------
// Cabecera y pie
// ---------------------------------------------------------------------------

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(
            " ttm ",
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    for (i, view) in CurrentView::ALL.iter().enumerate() {
        let text = format!(" {} {} ", i + 1, view.title());
        spans.push(if *view == app.current_view {
            Span::styled(
                text,
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
        } else {
            dim(text)
        });
    }

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    f.render_widget(Paragraph::new(Line::from(spans)), rows[0]);
    f.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(DIM)),
        rows[1],
    );
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let mut hints = vec![];
    if let Some(version) = app.update_available {
        hints.push(format!("u actualizar a v{version}"));
    }
    hints.extend([
        "1-8 pestañas".to_string(),
        "←/→ navegar".to_string(),
        format!("d detalle [{}]", if app.expanded { "on" } else { "off" }),
    ]);
    match app.current_view {
        CurrentView::Cpu => hints.push("c núcleos".to_string()),
        CurrentView::Procs => {
            hints.push("s orden".to_string());
            hints.push("↑/↓ mover".to_string());
        }
        CurrentView::Ports => hints.push("↑/↓ mover".to_string()),
        _ => {}
    }
    hints.push("? ayuda".to_string());
    hints.push("q salir".to_string());

    f.render_widget(
        Paragraph::new(hints.join(" · "))
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center),
        area,
    );
}

// ---------------------------------------------------------------------------
// Inicio (estilo neofetch)
// ---------------------------------------------------------------------------

fn draw_home(f: &mut Frame, app: &App, area: Rect) {
    let show_logo = area.width >= 64;
    let cols = Layout::horizontal([
        Constraint::Length(if show_logo { LOGO_WIDTH } else { 0 }),
        Constraint::Min(0),
    ])
    .split(area);

    if show_logo {
        let logo: Vec<Line> = LOGO
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let color = if i >= 5 { Color::Yellow } else { ACCENT };
                Line::from(Span::styled(*row, Style::default().fg(color)))
            })
            .collect();
        f.render_widget(Paragraph::new(logo), cols[0]);
    }

    f.render_widget(Paragraph::new(home_lines(app, cols[1])), cols[1]);
}

fn home_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    let info = &app.system_info;
    let gpu = app.gpu_info();
    let bw = bar_width(area);
    let spark_w = (area.width as usize).saturating_sub(LABEL_WIDTH).min(60);

    let title = format!("{} · ttm", info.host_name);
    let mut lines = vec![
        Line::from(Span::styled(
            title.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(dim("─".repeat(title.chars().count()))),
        kv("SO", format!("{} ({})", info.os_name, info.kernel_version)),
        kv("Activo", app.uptime_formatted()),
        kv(
            "CPU",
            format!("{} [{} hilos]", info.cpu_model, info.logical_cores),
        ),
        kv("GPU", format!("{} ({})", gpu.name, gpu.driver)),
        Line::from(""),
    ];

    let ram_used = app.sys.used_memory();
    let ram_total = app.sys.total_memory();
    let swap_used = app.sys.used_swap();
    let swap_total = app.sys.total_swap();

    lines.push(metric(
        "CPU",
        last(&app.cpu_history),
        format!("{:.1} GHz", info.frequency_mhz as f64 / 1000.0),
        bw,
    ));
    if app.expanded {
        lines.push(spark_line(&app.cpu_history, spark_w));
    }

    lines.push(metric(
        "RAM",
        last(&app.ram_history),
        format!(
            "{:.1}/{:.1} GB",
            ram_used as f64 / GIB,
            ram_total as f64 / GIB
        ),
        bw,
    ));
    if app.expanded {
        lines.push(spark_line(&app.ram_history, spark_w));
    }

    lines.push(metric(
        "SWAP",
        last(&app.swap_history),
        if swap_total == 0 {
            "sin swap".to_string()
        } else {
            format!(
                "{:.1}/{:.1} GB",
                swap_used as f64 / GIB,
                swap_total as f64 / GIB
            )
        },
        bw,
    ));
    if app.expanded {
        lines.push(spark_line(&app.swap_history, spark_w));
    }

    lines.push(metric(
        "GPU",
        last(&app.gpu_history),
        gpu.temperature
            .map(|t| format!("{t:.0} °C"))
            .unwrap_or_default(),
        bw,
    ));
    if app.expanded {
        lines.push(spark_line(&app.gpu_history, spark_w));
    }

    if let Some(disk) = app.primary_disk() {
        lines.push(metric(
            "DISCO",
            disk.used_percent(),
            format!("{}/{}", size(disk.used()), size(disk.total)),
            bw,
        ));
    }

    match app.power.battery() {
        Some(bat) => {
            let mut detail = app.power.summary();
            if let Some(w) = bat.power_w.filter(|w| *w > 0.0) {
                detail.push_str(&format!(" · {w:.1} W"));
            }
            lines.push(charge_metric(
                "BATERÍA",
                bat.capacity.unwrap_or(0),
                detail,
                bw,
            ));
            if app.expanded {
                lines.push(spark_line(&app.battery_history, spark_w));
            }
        }
        None => lines.push(kv("ENERGÍA", app.power.summary())),
    }

    if app.expanded {
        lines.push(Line::from(""));
        lines.push(kv(
            "Núcleos",
            format!(
                "{} físicos · {} lógicos",
                info.physical_cores, info.logical_cores
            ),
        ));
        lines.push(kv("Arq.", info.cpu_arch.clone()));
        if let (Some(u), Some(t)) = (gpu.vram_used, gpu.vram_total) {
            lines.push(kv(
                "VRAM",
                format!("{:.1}/{:.1} GB", u as f64 / GIB, t as f64 / GIB),
            ));
        }
        lines.push(kv(
            "Procesos",
            format!(
                "{}  ·  puertos en escucha: {}",
                app.processes.len(),
                app.ports.len()
            ),
        ));
    } else {
        lines.push(Line::from(""));
        lines.push(hint("d para ver gráficas y más detalle"));
    }

    lines
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

fn draw_cpu(f: &mut Frame, app: &App, area: Rect) {
    if !app.expanded {
        f.render_widget(Paragraph::new(cpu_summary(app, area)), area);
        return;
    }
    match app.cpu_mode {
        CpuMode::Global => draw_cpu_global(f, app, area),
        CpuMode::PerCore => draw_cpu_per_core(f, app, area),
    }
}

fn cpu_summary(app: &App, area: Rect) -> Vec<Line<'static>> {
    let info = &app.system_info;
    let spark_w = (area.width as usize).saturating_sub(LABEL_WIDTH).min(60);
    vec![
        kv("Modelo", info.cpu_model.clone()),
        kv(
            "Núcleos",
            format!(
                "{} físicos · {} hilos",
                info.physical_cores, info.logical_cores
            ),
        ),
        Line::from(""),
        metric(
            "Uso",
            last(&app.cpu_history),
            format!("{:.2} GHz", info.frequency_mhz as f64 / 1000.0),
            bar_width(area),
        ),
        spark_line(&app.cpu_history, spark_w),
        Line::from(""),
        hint("d gráfica y detalle · c por núcleo"),
    ]
}

fn draw_cpu_global(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area);
    let current = last(&app.cpu_history);
    let data = series(&app.cpu_history);
    f.render_widget(
        history_chart(format!(" CPU {current}% "), &data, ACCENT),
        chunks[0],
    );

    let info = &app.system_info;
    render_columns(
        f,
        chunks[1].inner(Margin::new(1, 0)),
        vec![
            vec![
                kv("Uso", format!("{current}%")),
                kv(
                    "Velocidad",
                    format!("{:.2} GHz", info.frequency_mhz as f64 / 1000.0),
                ),
                kv("Activo", app.uptime_formatted()),
            ],
            vec![
                kv("Físicos", info.physical_cores.to_string()),
                kv("Hilos", info.logical_cores.to_string()),
                kv("Arq.", info.cpu_arch.clone()),
                kv("Procesos", app.processes.len().to_string()),
            ],
        ],
    );
}

fn draw_cpu_per_core(f: &mut Frame, app: &App, area: Rect) {
    const CELL_WIDTH: usize = 26;
    const CORE_BAR: usize = 12;

    let cores = app.sys.cpus();
    let per_row = (area.width as usize / CELL_WIDTH).max(1);

    let mut lines: Vec<Line> = Vec::new();
    for (row, chunk) in cores.chunks(per_row).enumerate() {
        let mut spans: Vec<Span> = Vec::new();
        for (col, cpu) in chunk.iter().enumerate() {
            let idx = row * per_row + col;
            let pct = cpu.cpu_usage().round().min(100.0) as u64;
            spans.push(Span::styled(
                format!("#{idx:<2} "),
                Style::default().fg(ACCENT),
            ));
            spans.extend(bar(pct, CORE_BAR));
            spans.push(Span::raw(format!(" {pct:>3}%  ")));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(hint("c vuelve a la vista global"));

    f.render_widget(Paragraph::new(lines), area);
}

// ---------------------------------------------------------------------------
// Memoria
// ---------------------------------------------------------------------------

fn draw_ram(f: &mut Frame, app: &App, area: Rect) {
    let ram_used = app.sys.used_memory();
    let ram_total = app.sys.total_memory();
    let swap_used = app.sys.used_swap();
    let swap_total = app.sys.total_swap();
    let ram_pct = last(&app.ram_history);
    let swap_pct = last(&app.swap_history);

    let ram_detail = format!(
        "{:.2}/{:.2} GB",
        ram_used as f64 / GIB,
        ram_total as f64 / GIB
    );
    let swap_detail = if swap_total == 0 {
        "sin swap".to_string()
    } else {
        format!(
            "{:.2}/{:.2} GB",
            swap_used as f64 / GIB,
            swap_total as f64 / GIB
        )
    };

    if !app.expanded {
        let spark_w = (area.width as usize).saturating_sub(LABEL_WIDTH).min(60);
        let bw = bar_width(area);
        let lines = vec![
            metric("RAM", ram_pct, ram_detail, bw),
            spark_line(&app.ram_history, spark_w),
            Line::from(""),
            metric("SWAP", swap_pct, swap_detail, bw),
            spark_line(&app.swap_history, spark_w),
            Line::from(""),
            hint("d gráficas y desglose"),
        ];
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    let chunks = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area);
    let charts = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let ram_data = series(&app.ram_history);
    let swap_data = series(&app.swap_history);
    f.render_widget(
        history_chart(format!(" RAM {ram_pct}% "), &ram_data, ACCENT),
        charts[0],
    );
    f.render_widget(
        history_chart(format!(" SWAP {swap_pct}% "), &swap_data, Color::Yellow),
        charts[1],
    );

    render_columns(
        f,
        chunks[1].inner(Margin::new(1, 0)),
        vec![
            vec![
                kv("Total", format!("{:.2} GB", ram_total as f64 / GIB)),
                kv("En uso", format!("{:.2} GB", ram_used as f64 / GIB)),
                kv(
                    "Disponible",
                    format!("{:.2} GB", app.sys.available_memory() as f64 / GIB),
                ),
                kv(
                    "Libre",
                    format!("{:.2} GB", app.sys.free_memory() as f64 / GIB),
                ),
            ],
            vec![
                kv("Swap", format!("{:.2} GB", swap_total as f64 / GIB)),
                kv("Swap en uso", format!("{:.2} GB", swap_used as f64 / GIB)),
                kv(
                    "Swap libre",
                    format!(
                        "{:.2} GB",
                        swap_total.saturating_sub(swap_used) as f64 / GIB
                    ),
                ),
            ],
        ],
    );
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

fn draw_gpu(f: &mut Frame, app: &App, area: Rect) {
    let gpu = app.gpu_info();
    let current = last(&app.gpu_history);
    let vram = gpu.vram_used.zip(gpu.vram_total);

    if !app.expanded {
        let bw = bar_width(area);
        let spark_w = (area.width as usize).saturating_sub(LABEL_WIDTH).min(60);
        let mut lines = vec![
            kv("Modelo", gpu.name.clone()),
            kv("Driver", gpu.driver.clone()),
            Line::from(""),
            metric(
                "Motor",
                current,
                gpu.temperature
                    .map(|t| format!("{t:.0} °C"))
                    .unwrap_or_default(),
                bw,
            ),
            spark_line(&app.gpu_history, spark_w),
        ];
        if let Some((used, total)) = vram {
            lines.push(metric(
                "VRAM",
                pct_of(used, total),
                format!("{:.2}/{:.2} GB", used as f64 / GIB, total as f64 / GIB),
                bw,
            ));
        }
        lines.push(Line::from(""));
        lines.push(hint("d gráfica y detalle"));
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    let chunks = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).split(area);
    let data = series(&app.gpu_history);
    f.render_widget(
        history_chart(format!(" GPU {current}% "), &data, Color::Magenta),
        chunks[0],
    );

    let na = || "N/D".to_string();
    render_columns(
        f,
        chunks[1].inner(Margin::new(1, 0)),
        vec![
            vec![
                kv("Modelo", gpu.name.clone()),
                kv("Driver", gpu.driver.clone()),
                kv(
                    "Temp.",
                    gpu.temperature
                        .map(|t| format!("{t:.1} °C"))
                        .unwrap_or_else(na),
                ),
            ],
            vec![
                kv(
                    "VRAM",
                    vram.map(|(u, t)| format!("{:.2}/{:.2} GB", u as f64 / GIB, t as f64 / GIB))
                        .unwrap_or_else(|| "compartida".to_string()),
                ),
                kv(
                    "GTT",
                    gpu.gtt_used
                        .zip(gpu.gtt_total)
                        .map(|(u, t)| format!("{:.2}/{:.2} GB", u as f64 / GIB, t as f64 / GIB))
                        .unwrap_or_else(na),
                ),
                kv("Motor", format!("{current}%")),
            ],
        ],
    );
}

// ---------------------------------------------------------------------------
// Disco
// ---------------------------------------------------------------------------

fn draw_disk(f: &mut Frame, app: &App, area: Rect) {
    if app.disks.is_empty() {
        f.render_widget(Paragraph::new(hint("sin discos detectados")), area);
        return;
    }

    if app.expanded {
        let header = Row::new(
            [
                "Montaje",
                "Dispositivo",
                "FS",
                "Tipo",
                "Total",
                "Usado",
                "Libre",
                "Uso",
            ]
            .map(Cell::from),
        )
        .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD));

        let rows = app.disks.iter().map(|d| {
            Row::new(vec![
                Cell::from(d.mount.clone()),
                Cell::from(d.device.clone()),
                Cell::from(d.fs.clone()),
                Cell::from(d.kind.clone()),
                Cell::from(size(d.total)),
                Cell::from(size(d.used())),
                Cell::from(size(d.available)),
                Cell::from(format!("{}%", d.used_percent())),
            ])
        });

        let table = Table::new(
            rows,
            [
                Constraint::Min(10),
                Constraint::Min(12),
                Constraint::Length(8),
                Constraint::Length(6),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(5),
            ],
        )
        .header(header);
        f.render_widget(table, area);
        return;
    }

    // detalle "usado · libre · total" (~44 columnas) junto a la barra
    let bw = (area.width as usize)
        .saturating_sub(LABEL_WIDTH + 5 + 46)
        .clamp(8, 50);
    let mut lines: Vec<Line> = Vec::new();
    for d in &app.disks {
        let name: String = d.mount.chars().take(LABEL_WIDTH - 1).collect();
        lines.push(metric(
            &name,
            d.used_percent(),
            format!(
                "{} usado · {} libre · {}",
                size(d.used()),
                size(d.available),
                size(d.total)
            ),
            bw,
        ));
    }
    lines.push(Line::from(""));
    lines.push(hint("d dispositivo, sistema de archivos y tipo"));
    f.render_widget(Paragraph::new(lines), area);
}

// ---------------------------------------------------------------------------
// Tablas
// ---------------------------------------------------------------------------

fn visible_rows(area: Rect, expanded: bool) -> usize {
    // título + cabecera
    let avail = (area.height as usize).saturating_sub(2).max(1);
    if expanded { avail } else { avail.min(6) }
}

fn window(state: usize, rows: usize, len: usize) -> (usize, usize) {
    let start = if state >= rows { state + 1 - rows } else { 0 };
    (start.min(len), (start + rows).min(len))
}

fn row_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(Color::Black).bg(ACCENT)
    } else {
        Style::default()
    }
}

fn header_row(cols: [&'static str; 5]) -> Row<'static> {
    Row::new(cols.map(Cell::from)).style(Style::default().fg(DIM).add_modifier(Modifier::BOLD))
}

fn table_title(name: &str, count: usize, extra: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            name.to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        dim(format!(" ({count}){extra}")),
    ])
}

fn draw_procs(f: &mut Frame, app: &App, area: Rect) {
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    f.render_widget(
        Paragraph::new(table_title(
            "Procesos",
            app.processes.len(),
            &format!(" · orden: {} [s]", app.process_sort.label()),
        )),
        parts[0],
    );

    let rows = visible_rows(area, app.expanded);
    let (start, end) = window(app.process_table_state, rows, app.processes.len());

    let body = app.processes[start..end].iter().enumerate().map(|(i, p)| {
        let mem_mb = p.memory_bytes as f64 / 1_048_576.0;
        let mem = if mem_mb > 1024.0 {
            format!("{:.2} GB ({:.1}%)", mem_mb / 1024.0, p.memory_percent)
        } else {
            format!("{mem_mb:.1} MB ({:.1}%)", p.memory_percent)
        };
        Row::new(vec![
            Cell::from(p.pid.to_string()),
            Cell::from(p.name.clone()),
            Cell::from(format!("{:.1}%", p.cpu_usage)),
            Cell::from(mem),
            Cell::from(p.status.clone()),
        ])
        .style(row_style(start + i == app.process_table_state))
    });

    let table = Table::new(
        body,
        [
            Constraint::Length(8),
            Constraint::Min(12),
            Constraint::Length(8),
            Constraint::Length(18),
            Constraint::Length(10),
        ],
    )
    .header(header_row(["PID", "Nombre", "CPU", "Memoria", "Estado"]));
    f.render_widget(table, parts[1]);
}

fn draw_ports(f: &mut Frame, app: &App, area: Rect) {
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    f.render_widget(
        Paragraph::new(table_title("Puertos en escucha", app.ports.len(), "")),
        parts[0],
    );

    let rows = visible_rows(area, app.expanded);
    let (start, end) = window(app.port_table_state, rows, app.ports.len());

    let body = app.ports[start..end].iter().enumerate().map(|(i, p)| {
        Row::new(vec![
            Cell::from(p.port.to_string()),
            Cell::from(p.protocol),
            Cell::from(p.address.clone()),
            Cell::from(p.pid.map_or_else(|| "-".to_string(), |pid| pid.to_string())),
            Cell::from(p.process_name.clone().unwrap_or_else(|| "-".to_string())),
        ])
        .style(row_style(start + i == app.port_table_state))
    });

    let table = Table::new(
        body,
        [
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Min(16),
            Constraint::Length(8),
            Constraint::Min(12),
        ],
    )
    .header(header_row([
        "Puerto",
        "Proto",
        "Dirección",
        "PID",
        "Proceso",
    ]));
    f.render_widget(table, parts[1]);
}

// ---------------------------------------------------------------------------
// Energía
// ---------------------------------------------------------------------------

fn draw_power(f: &mut Frame, app: &App, area: Rect) {
    let power = &app.power;
    let Some(bat) = power.battery() else {
        let lines = vec![
            kv("Fuente", power.source().label()),
            Line::from(""),
            hint("no se detectó batería: equipo de escritorio siempre conectado"),
        ];
        f.render_widget(Paragraph::new(lines), area);
        return;
    };

    if !app.expanded {
        let spark_w = (area.width as usize).saturating_sub(LABEL_WIDTH).min(60);
        let mut lines = power_summary(power, bat, bar_width(area));
        lines.insert(2, spark_line(&app.battery_history, spark_w));
        lines.push(Line::from(""));
        lines.push(hint("d gráfica y detalle de la batería"));
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    let chunks = Layout::vertical([Constraint::Min(6), Constraint::Length(5)]).split(area);
    let capacity = bat.capacity.unwrap_or(0);
    let data = series(&app.battery_history);
    f.render_widget(
        history_chart(
            format!(" Batería {capacity}% · {} ", power.summary()),
            &data,
            charge_color(capacity),
        ),
        chunks[0],
    );

    let na = || "N/D".to_string();
    let wh = |v: Option<f64>| v.map(|v| format!("{v:.1} Wh")).unwrap_or_else(na);
    let or_na = |s: &str| if s.is_empty() { na() } else { s.to_string() };
    render_columns(
        f,
        chunks[1].inner(Margin::new(1, 0)),
        vec![
            vec![
                kv("Fuente", power.source().label()),
                kv("Estado", bat.state().label()),
                kv(
                    "Consumo",
                    bat.power_w.map(|w| format!("{w:.2} W")).unwrap_or_else(na),
                ),
                kv("Restante", time_left(bat).unwrap_or_else(na)),
                kv(
                    "Salud",
                    bat.health().map(|h| format!("{h}%")).unwrap_or_else(na),
                ),
            ],
            vec![
                kv("Energía", wh(bat.energy_now_wh)),
                kv("Máxima", wh(bat.energy_full_wh)),
                kv("De fábrica", wh(bat.energy_design_wh)),
                kv(
                    "Ciclos",
                    bat.cycles.map(|c| c.to_string()).unwrap_or_else(na),
                ),
                kv(
                    "Voltaje",
                    bat.voltage_v
                        .map(|v| format!("{v:.2} V"))
                        .unwrap_or_else(na),
                ),
            ],
            vec![
                kv("Batería", bat.name.clone()),
                kv("Modelo", or_na(&bat.model)),
                kv("Fabricante", or_na(&bat.manufacturer)),
                kv("Tecnología", or_na(&bat.technology)),
            ],
        ],
    );
}

fn power_summary(power: &PowerInfo, bat: &Battery, bw: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        kv("Fuente", power.source().label()),
        charge_metric("Carga", bat.capacity.unwrap_or(0), power.summary(), bw),
    ];
    if let Some(health) = bat.health() {
        lines.push(charge_metric(
            "Salud",
            health,
            bat.cycles
                .map(|c| format!("{c} ciclos"))
                .unwrap_or_default(),
            bw,
        ));
    }
    if let Some(w) = bat.power_w.filter(|w| *w > 0.0) {
        lines.push(kv("Consumo", format!("{w:.1} W")));
    }
    if let Some(left) = time_left(bat) {
        lines.push(kv("Restante", left));
    }
    lines
}

fn time_left(bat: &Battery) -> Option<String> {
    let secs = bat.time_left_secs()?;
    let goal = if bat.state() == ChargeState::Charging {
        "para cargarse"
    } else {
        "de batería"
    };
    Some(format!("{} {goal}", duration(secs)))
}

// ---------------------------------------------------------------------------
// Ayuda
// ---------------------------------------------------------------------------

fn draw_help_modal(f: &mut Frame, area: Rect) {
    let width = 58.min(area.width.saturating_sub(4));
    let height = 15.min(area.height.saturating_sub(2));
    let modal = Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, modal);

    let key = |k: &str, desc: &str| {
        Line::from(vec![
            Span::styled(
                format!("  {k:<16}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(desc.to_string()),
        ])
    };

    let text = vec![
        key("1-8", "Ir a una pestaña"),
        key("Tab / ← / →", "Pestaña siguiente / anterior"),
        key("d", "Desplegar / ocultar detalle de la pestaña"),
        key("c", "CPU: alternar global / por núcleo"),
        key("s", "Procesos: cambiar orden"),
        key("↑ ↓ j k", "Mover fila (Procesos y Puertos)"),
        key("PgUp / PgDn", "Mover 10 filas"),
        key("u", "Actualizar si hay una versión nueva"),
        key("? / h", "Mostrar u ocultar esta ayuda"),
        key("q / Esc", "Salir"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " Ayuda ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));

    f.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: true }),
        modal,
    );
}

// ---------------------------------------------------------------------------
// Aviso de actualización
// ---------------------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn draw_update_modal(f: &mut Frame, area: Rect, version: Version) {
    let modal = centered(area, 62, 9);
    f.render_widget(Clear, modal);

    let bold = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let text = vec![
        Line::from(vec![
            Span::raw("Hay una nueva versión de ttm: "),
            dim(format!("v{}", Version::current())),
            Span::raw(" → "),
            Span::styled(format!("v{version}"), bold),
        ]),
        Line::from(""),
        Line::from("¿Quieres actualizar ahora? ttm se cerrará para instalarla."),
        Line::from(""),
        Line::from(vec![
            Span::styled("[s]", bold),
            Span::raw(" Sí   "),
            Span::styled("[n]", bold),
            Span::raw(" No, omitir esta versión   "),
            Span::styled("[Esc]", bold),
            Span::raw(" Luego"),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(" Actualización disponible ", bold));

    f.render_widget(
        Paragraph::new(text)
            .block(block.padding(ratatui::widgets::Padding::horizontal(1)))
            .wrap(Wrap { trim: true }),
        modal,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_view_renders_at_any_size() {
        let mut app = App::new();
        for expanded in [false, true] {
            app.expanded = expanded;
            for view in CurrentView::ALL {
                app.set_view(view);
                for (w, h) in [(100, 30), (80, 24), (40, 10), (10, 3)] {
                    render(&app, w, h);
                }
            }
        }
        app.show_help = true;
        render(&app, 80, 24);

        app.show_help = false;
        app.update_available = Version::parse("9.9.9");
        app.show_update_prompt = true;
        for (w, h) in [(100, 30), (40, 10), (10, 3)] {
            render(&app, w, h);
        }
    }
}
