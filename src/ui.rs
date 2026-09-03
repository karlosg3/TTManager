use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Cell, Chart, Clear, Dataset, Gauge, GraphType, Paragraph,
        Row, Sparkline, Table, Tabs, Wrap,
    },
};

use crate::app::{App, CpuMode, CurrentView, OverviewFocus};

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Main layout: Header (top), Tab content (middle), Footer (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header + Info banner
            Constraint::Length(3), // Navigation Tabs
            Constraint::Min(10),   // Content
            Constraint::Length(2), // Footer / Keybindings
        ])
        .split(size);

    draw_header(f, app, chunks[0]);
    draw_tabs(f, app, chunks[1]);

    match app.current_view {
        CurrentView::Overview => draw_overview(f, app, chunks[2]),
        CurrentView::Cpu => draw_cpu(f, app, chunks[2]),
        CurrentView::Ram => draw_ram(f, app, chunks[2]),
        CurrentView::Gpu => draw_gpu(f, app, chunks[2]),
    }

    draw_footer(f, app, chunks[3]);

    if app.show_help {
        draw_help_modal(f, size);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " 💻 System Monitor TUI (Task Manager) ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(block.inner(area));

    let left_info = vec![
        Line::from(vec![
            Span::styled("Dispositivo: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&app.system_info.host_name),
            Span::raw(" | "),
            Span::styled("SO: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&app.system_info.os_name),
            Span::raw(" ("),
            Span::raw(&app.system_info.kernel_version),
            Span::raw(")"),
        ]),
        Line::from(vec![
            Span::styled("Procesador:  ", Style::default().fg(Color::Cyan).bold()),
            Span::raw(&app.system_info.cpu_model),
            Span::raw(format!(" [{} hilos]", app.system_info.logical_cores)),
        ]),
    ];

    let right_info = vec![
        Line::from(vec![
            Span::styled("Gráfica:  ", Style::default().fg(Color::Magenta).bold()),
            Span::raw(&app.gpu_info().name),
            Span::raw(format!(" ({})", app.gpu_info().driver)),
        ]),
        Line::from(vec![
            Span::styled("Activo:   ", Style::default().fg(Color::Green).bold()),
            Span::raw(app.uptime_formatted()),
            Span::raw(" | "),
            Span::styled("Procesos: ", Style::default().fg(Color::Green).bold()),
            Span::raw(app.processes.len().to_string()),
            Span::raw(" | "),
            Span::styled("Puertos: ", Style::default().fg(Color::Green).bold()),
            Span::raw(app.ports.len().to_string()),
        ]),
    ];

    f.render_widget(block, area);
    f.render_widget(Paragraph::new(left_info), header_chunks[0]);
    f.render_widget(Paragraph::new(right_info), header_chunks[1]);
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles = vec!["[1] Resumen", "[2] CPU", "[3] Memoria (RAM)", "[4] GPU"];
    let selected_idx = match app.current_view {
        CurrentView::Overview => 0,
        CurrentView::Cpu => 1,
        CurrentView::Ram => 2,
        CurrentView::Gpu => 3,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .select(selected_idx)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::raw(" │ "));

    f.render_widget(tabs, area);
}

fn draw_overview(f: &mut Frame, app: &App, area: Rect) {
    // Top: 4 resource summary cards
    // Bottom: Split between active processes and listening ports
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(8)])
        .split(area);

    let cards_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(main_chunks[0]);

    // 1. CPU Card
    let cpu_pct = *app.cpu_history.back().unwrap_or(&0);
    let cpu_history_slice: Vec<u64> = app.cpu_history.iter().copied().collect();
    let cpu_card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" CPU ");
    let cpu_inner = cpu_card.inner(cards_chunks[0]);
    f.render_widget(cpu_card, cards_chunks[0]);

    let cpu_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(cpu_inner);

    let cpu_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(cpu_pct.min(100) as u16)
        .label(format!("{cpu_pct}%"));
    f.render_widget(cpu_gauge, cpu_layout[0]);

    let cpu_stats = Paragraph::new(format!(
        "{} GHz • {} núcleos",
        (app.system_info.frequency_mhz as f64) / 1000.0,
        app.system_info.logical_cores
    ))
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(cpu_stats, cpu_layout[1]);

    let cpu_spark = Sparkline::default()
        .data(&cpu_history_slice)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(cpu_spark, cpu_layout[2]);

    // 2. RAM Card
    let ram_used = app.sys.used_memory() as f64 / 1_073_741_824.0;
    let ram_total = app.sys.total_memory() as f64 / 1_073_741_824.0;
    let ram_pct = *app.ram_history.back().unwrap_or(&0);
    let ram_history_slice: Vec<u64> = app.ram_history.iter().copied().collect();
    let ram_card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Green))
        .title(" RAM ");
    let ram_inner = ram_card.inner(cards_chunks[1]);
    f.render_widget(ram_card, cards_chunks[1]);

    let ram_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(ram_inner);

    let ram_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green))
        .percent(ram_pct.min(100) as u16)
        .label(format!("{ram_pct}% ({ram_used:.1}/{ram_total:.1} GB)"));
    f.render_widget(ram_gauge, ram_layout[0]);

    let ram_stats = Paragraph::new(format!(
        "Disp: {:.1} GB • Total: {:.1} GB",
        (app.sys.available_memory() as f64 / 1_073_741_824.0),
        ram_total
    ))
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(ram_stats, ram_layout[1]);

    let ram_spark = Sparkline::default()
        .data(&ram_history_slice)
        .style(Style::default().fg(Color::Green));
    f.render_widget(ram_spark, ram_layout[2]);

    // 3. SWAP Card
    let swap_used = app.sys.used_swap() as f64 / 1_073_741_824.0;
    let swap_total = app.sys.total_swap() as f64 / 1_073_741_824.0;
    let swap_pct = *app.swap_history.back().unwrap_or(&0);
    let swap_history_slice: Vec<u64> = app.swap_history.iter().copied().collect();
    let swap_card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" SWAP ");
    let swap_inner = swap_card.inner(cards_chunks[2]);
    f.render_widget(swap_card, cards_chunks[2]);

    let swap_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(swap_inner);

    let swap_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Yellow))
        .percent(swap_pct.min(100) as u16)
        .label(format!("{swap_pct}% ({swap_used:.1}/{swap_total:.1} GB)"));
    f.render_widget(swap_gauge, swap_layout[0]);

    let swap_stats = Paragraph::new(format!("Libre: {:.1} GB", swap_total - swap_used))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(swap_stats, swap_layout[1]);

    let swap_spark = Sparkline::default()
        .data(&swap_history_slice)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(swap_spark, swap_layout[2]);

    // 4. GPU Card
    let gpu_info = app.gpu_info();
    let gpu_pct = *app.gpu_history.back().unwrap_or(&0);
    let gpu_history_slice: Vec<u64> = app.gpu_history.iter().copied().collect();
    let gpu_card = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" GPU ");
    let gpu_inner = gpu_card.inner(cards_chunks[3]);
    f.render_widget(gpu_card, cards_chunks[3]);

    let gpu_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(gpu_inner);

    let gpu_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Magenta))
        .percent(gpu_pct.min(100) as u16)
        .label(if let Some(temp) = gpu_info.temperature {
            format!("{gpu_pct}% • {temp:.0}°C")
        } else {
            format!("{gpu_pct}%")
        });
    f.render_widget(gpu_gauge, gpu_layout[0]);

    let vram_label = if let (Some(used), Some(tot)) = (gpu_info.vram_used, gpu_info.vram_total) {
        format!(
            "VRAM: {:.1}/{:.1} GB",
            used as f64 / 1_073_741_824.0,
            tot as f64 / 1_073_741_824.0
        )
    } else {
        format!("Driver: {}", gpu_info.driver)
    };
    let gpu_stats = Paragraph::new(vram_label).style(Style::default().fg(Color::DarkGray));
    f.render_widget(gpu_stats, gpu_layout[1]);

    let gpu_spark = Sparkline::default()
        .data(&gpu_history_slice)
        .style(Style::default().fg(Color::Magenta));
    f.render_widget(gpu_spark, gpu_layout[2]);

    // Tables split (Processes & Ports)
    let table_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(main_chunks[1]);

    draw_processes_table(f, app, table_chunks[0]);
    draw_ports_table(f, app, table_chunks[1]);
}

fn draw_processes_table(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.overview_focus == OverviewFocus::Processes;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = format!(
        " Procesos Activos ({}) [s: {}] [Foco: Tab/p] ",
        app.processes.len(),
        app.process_sort.label()
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            if is_focused {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::Gray)
            },
        ));

    let header_cells = ["PID", "Nombre", "CPU %", "Memoria", "Estado"]
        .iter()
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        });
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows_count = area.height.saturating_sub(4) as usize;
    let start_idx = if app.process_table_state >= rows_count {
        app.process_table_state - rows_count + 1
    } else {
        0
    };
    let end_idx = (start_idx + rows_count).min(app.processes.len());

    let rows = app.processes[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(rel_idx, p)| {
            let actual_idx = start_idx + rel_idx;
            let is_selected = is_focused && actual_idx == app.process_table_state;

            let mem_mb = p.memory_bytes as f64 / 1_048_576.0;
            let mem_str = if mem_mb > 1024.0 {
                format!("{:.2} GB ({:.1}%)", mem_mb / 1024.0, p.memory_percent)
            } else {
                format!("{mem_mb:.1} MB ({:.1}%)", p.memory_percent)
            };

            let row_style = if is_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if actual_idx % 2 == 0 {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            Row::new(vec![
                Cell::from(p.pid.to_string()),
                Cell::from(p.name.clone()),
                Cell::from(format!("{:.1}%", p.cpu_usage)),
                Cell::from(mem_str),
                Cell::from(p.status.clone()),
            ])
            .style(row_style)
        });

    let widths = [
        Constraint::Length(8),
        Constraint::Percentage(40),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths).header(header).block(block);

    f.render_widget(table, area);
}

fn draw_ports_table(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.overview_focus == OverviewFocus::Ports;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = format!(" Puertos en Escucha ({}) ", app.ports.len());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            if is_focused {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::Gray)
            },
        ));

    let header_cells = ["Puerto", "Proto", "Dirección", "PID", "Proceso"]
        .iter()
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        });
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows_count = area.height.saturating_sub(4) as usize;
    let start_idx = if app.port_table_state >= rows_count {
        app.port_table_state - rows_count + 1
    } else {
        0
    };
    let end_idx = (start_idx + rows_count).min(app.ports.len());

    let rows = app.ports[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(rel_idx, p)| {
            let actual_idx = start_idx + rel_idx;
            let is_selected = is_focused && actual_idx == app.port_table_state;

            let pid_str = p
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string());
            let name_str = p.process_name.clone().unwrap_or_else(|| "-".to_string());

            let row_style = if is_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if actual_idx % 2 == 0 {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            Row::new(vec![
                Cell::from(p.port.to_string()),
                Cell::from(p.protocol),
                Cell::from(p.address.clone()),
                Cell::from(pid_str),
                Cell::from(name_str),
            ])
            .style(row_style)
        });

    let widths = [
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Percentage(32),
        Constraint::Length(8),
        Constraint::Percentage(35),
    ];

    let table = Table::new(rows, widths).header(header).block(block);

    f.render_widget(table, area);
}

fn draw_cpu(f: &mut Frame, app: &App, area: Rect) {
    let mode_title = match app.cpu_mode {
        CpuMode::Global => "Modo Global (Atajo: [c] para Por Núcleo)",
        CpuMode::PerCore => "Modo Por Núcleo (Atajo: [c] para Global)",
    };

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            format!(" CPU: {} • {} ", app.system_info.cpu_model, mode_title),
            Style::default().fg(Color::Cyan).bold(),
        ));

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    match app.cpu_mode {
        CpuMode::Global => draw_cpu_global(f, app, inner),
        CpuMode::PerCore => draw_cpu_per_core(f, app, inner),
    }
}

fn draw_cpu_global(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(7)])
        .split(area);

    // High precision Windows Task Manager style Chart
    let data: Vec<(f64, f64)> = app
        .cpu_history
        .iter()
        .enumerate()
        .map(|(i, &val)| (i as f64, val as f64))
        .collect();

    let dataset = Dataset::default()
        .name("Uso de CPU (%)")
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan).bold())
        .data(&data);

    let current_val = *app.cpu_history.back().unwrap_or(&0);
    let x_labels = vec![
        Span::raw("60s"),
        Span::raw("45s"),
        Span::raw("30s"),
        Span::raw("15s"),
        Span::raw("0s (actual)"),
    ];

    let y_labels = vec![
        Span::raw("0%"),
        Span::raw("25%"),
        Span::raw("50%"),
        Span::raw("75%"),
        Span::raw("100%"),
    ];

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" Historial de uso continuo (%): {current_val}% actual "),
                    Style::default().fg(Color::Cyan).bold(),
                )),
        )
        .x_axis(
            Axis::default()
                .title("Tiempo")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 60.0])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title("%")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 100.0])
                .labels(y_labels),
        );

    f.render_widget(chart, chunks[0]);

    // Bottom CPU stats panel (Windows Task Manager style)
    let stats_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Detalles del Procesador ");

    let stats_inner = stats_block.inner(chunks[1]);
    f.render_widget(stats_block, chunks[1]);

    let stat_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(stats_inner);

    let col1 = vec![
        Line::from(vec![
            Span::styled("Utilización: ", Style::default().fg(Color::Yellow).bold()),
            Span::styled(
                format!("{current_val}%"),
                Style::default().fg(Color::Cyan).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Velocidad:   ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(format!(
                "{:.2} GHz",
                (app.system_info.frequency_mhz as f64) / 1000.0
            )),
        ]),
    ];

    let col2 = vec![
        Line::from(vec![
            Span::styled(
                "Núcleos Físicos: ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(app.system_info.physical_cores.to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                "Hilos / Lógicos: ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(app.system_info.logical_cores.to_string()),
        ]),
    ];

    let col3 = vec![
        Line::from(vec![
            Span::styled("Arquitectura: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&app.system_info.cpu_arch),
        ]),
        Line::from(vec![
            Span::styled("Procesos:     ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(app.processes.len().to_string()),
        ]),
    ];

    let col4 = vec![
        Line::from(vec![
            Span::styled("Tiempo Activo: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(app.uptime_formatted()),
        ]),
        Line::from(vec![
            Span::styled("Modo de Vista: ", Style::default().fg(Color::Yellow).bold()),
            Span::styled("[c] PerCore", Style::default().fg(Color::Green)),
        ]),
    ];

    f.render_widget(Paragraph::new(col1), stat_cols[0]);
    f.render_widget(Paragraph::new(col2), stat_cols[1]);
    f.render_widget(Paragraph::new(col3), stat_cols[2]);
    f.render_widget(Paragraph::new(col4), stat_cols[3]);
}

fn draw_cpu_per_core(f: &mut Frame, app: &App, area: Rect) {
    let cores = app.sys.cpus();
    if cores.is_empty() {
        return;
    }

    // Determine grid layout: 4 columns or 2 columns based on core count
    let cols_count = if cores.len() > 8 { 4 } else { 2 };
    let rows_count = cores.len().div_ceil(cols_count);

    let row_constraints = vec![Constraint::Ratio(1, rows_count as u32); rows_count];
    let row_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for (idx, cpu) in cores.iter().enumerate() {
        let r = idx / cols_count;
        let c = idx % cols_count;

        let col_constraints = vec![Constraint::Ratio(1, cols_count as u32); cols_count];
        let col_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(row_chunks[r]);

        let core_area = col_chunks[c];
        let usage = cpu.cpu_usage().round().min(100.0) as u16;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" Núcleo #{} ", idx));
        let inner = block.inner(core_area);
        f.render_widget(block, core_area);

        let inner_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        let gauge_color = if usage > 85 {
            Color::Red
        } else if usage > 50 {
            Color::Yellow
        } else {
            Color::Cyan
        };

        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(gauge_color))
            .percent(usage)
            .label(format!("{usage}%"));
        f.render_widget(gauge, inner_layout[0]);

        if idx < app.per_core_history.len() && inner_layout[1].height > 0 {
            let hist: Vec<u64> = app.per_core_history[idx].iter().copied().collect();
            let spark = Sparkline::default()
                .data(&hist)
                .style(Style::default().fg(gauge_color));
            f.render_widget(spark, inner_layout[1]);
        }
    }
}

fn draw_ram(f: &mut Frame, app: &App, area: Rect) {
    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " Memoria Física (RAM) y Archivo de Paginación (SWAP) ",
            Style::default().fg(Color::Green).bold(),
        ));

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(8)])
        .split(inner);

    // Split top into 2 charts: RAM chart and Swap chart
    let chart_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    // RAM Chart
    let ram_data: Vec<(f64, f64)> = app
        .ram_history
        .iter()
        .enumerate()
        .map(|(i, &val)| (i as f64, val as f64))
        .collect();

    let ram_dataset = Dataset::default()
        .name("RAM (%)")
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Green).bold())
        .data(&ram_data);

    let ram_pct = *app.ram_history.back().unwrap_or(&0);
    let ram_used_gb = app.sys.used_memory() as f64 / 1_073_741_824.0;
    let ram_total_gb = app.sys.total_memory() as f64 / 1_073_741_824.0;

    let x_labels = vec![Span::raw("60s"), Span::raw("30s"), Span::raw("0s")];

    let y_labels = vec![Span::raw("0%"), Span::raw("50%"), Span::raw("100%")];

    let ram_chart = Chart::new(vec![ram_dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" Historial de Memoria Física: {ram_used_gb:.2} / {ram_total_gb:.2} GB ({ram_pct}%) "),
                    Style::default().fg(Color::Green).bold(),
                )),
        )
        .x_axis(
            Axis::default()
                .title("Tiempo")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 60.0])
                .labels(x_labels.clone()),
        )
        .y_axis(
            Axis::default()
                .title("%")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 100.0])
                .labels(y_labels.clone()),
        );

    f.render_widget(ram_chart, chart_chunks[0]);

    // Swap Chart
    let swap_data: Vec<(f64, f64)> = app
        .swap_history
        .iter()
        .enumerate()
        .map(|(i, &val)| (i as f64, val as f64))
        .collect();

    let swap_dataset = Dataset::default()
        .name("SWAP (%)")
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Yellow).bold())
        .data(&swap_data);

    let swap_pct = *app.swap_history.back().unwrap_or(&0);
    let swap_used_gb = app.sys.used_swap() as f64 / 1_073_741_824.0;
    let swap_total_gb = app.sys.total_swap() as f64 / 1_073_741_824.0;

    let swap_chart = Chart::new(vec![swap_dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" Historial de Swap: {swap_used_gb:.2} / {swap_total_gb:.2} GB ({swap_pct}%) "),
                    Style::default().fg(Color::Yellow).bold(),
                )),
        )
        .x_axis(
            Axis::default()
                .title("Tiempo")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 60.0])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title("%")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 100.0])
                .labels(y_labels),
        );

    f.render_widget(swap_chart, chart_chunks[1]);

    // Memory Breakdown Panel
    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Green))
        .title(" Desglose de Memoria y Paginación ");

    let details_inner = details_block.inner(chunks[1]);
    f.render_widget(details_block, chunks[1]);

    let detail_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(details_inner);

    let free_ram_gb = app.sys.free_memory() as f64 / 1_073_741_824.0;
    let avail_ram_gb = app.sys.available_memory() as f64 / 1_073_741_824.0;

    let ram_col1 = vec![
        Line::from(vec![
            Span::styled(
                "RAM Total:       ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(format!("{ram_total_gb:.2} GB")),
        ]),
        Line::from(vec![
            Span::styled(
                "En Uso:          ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::styled(
                format!("{ram_used_gb:.2} GB ({ram_pct}%)"),
                Style::default().fg(Color::Green).bold(),
            ),
        ]),
    ];

    let ram_col2 = vec![
        Line::from(vec![
            Span::styled(
                "Disponible:      ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(format!("{avail_ram_gb:.2} GB")),
        ]),
        Line::from(vec![
            Span::styled(
                "Libre:           ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(format!("{free_ram_gb:.2} GB")),
        ]),
    ];

    let free_swap_gb = swap_total_gb - swap_used_gb;
    let ram_col3 = vec![
        Line::from(vec![
            Span::styled(
                "SWAP Total:      ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(format!("{swap_total_gb:.2} GB")),
        ]),
        Line::from(vec![
            Span::styled(
                "SWAP Usado:      ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::styled(
                format!("{swap_used_gb:.2} GB ({swap_pct}%)"),
                Style::default().fg(Color::Yellow).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "SWAP Libre:      ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw(format!("{free_swap_gb:.2} GB")),
        ]),
    ];

    f.render_widget(Paragraph::new(ram_col1), detail_cols[0]);
    f.render_widget(Paragraph::new(ram_col2), detail_cols[1]);
    f.render_widget(Paragraph::new(ram_col3), detail_cols[2]);
}

fn draw_gpu(f: &mut Frame, app: &App, area: Rect) {
    let gpu_info = app.gpu_info();

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Span::styled(
            format!(" GPU: {} ({}) ", gpu_info.name, gpu_info.driver),
            Style::default().fg(Color::Magenta).bold(),
        ));

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(9)])
        .split(inner);

    // GPU Compute Utilization Chart
    let data: Vec<(f64, f64)> = app
        .gpu_history
        .iter()
        .enumerate()
        .map(|(i, &val)| (i as f64, val as f64))
        .collect();

    let dataset = Dataset::default()
        .name("Cómputo GPU (%)")
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Magenta).bold())
        .data(&data);

    let current_gpu = *app.gpu_history.back().unwrap_or(&0);
    let x_labels = vec![
        Span::raw("60s"),
        Span::raw("45s"),
        Span::raw("30s"),
        Span::raw("15s"),
        Span::raw("0s (actual)"),
    ];

    let y_labels = vec![
        Span::raw("0%"),
        Span::raw("25%"),
        Span::raw("50%"),
        Span::raw("75%"),
        Span::raw("100%"),
    ];

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" Historial de Cómputo GPU en Tiempo Real: {current_gpu}% actual "),
                    Style::default().fg(Color::Magenta).bold(),
                )),
        )
        .x_axis(
            Axis::default()
                .title("Tiempo")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 60.0])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title("%")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, 100.0])
                .labels(y_labels),
        );

    f.render_widget(chart, chunks[0]);

    // Bottom GPU Details Panel
    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" Memoria de Video (VRAM) y Sensores ");

    let details_inner = details_block.inner(chunks[1]);
    f.render_widget(details_block, chunks[1]);

    let detail_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(details_inner);

    let vram_used_gb = gpu_info.vram_used.map(|v| v as f64 / 1_073_741_824.0);
    let vram_total_gb = gpu_info.vram_total.map(|v| v as f64 / 1_073_741_824.0);
    let vram_pct = if let (Some(u), Some(t)) = (vram_used_gb, vram_total_gb) {
        if t > 0.0 {
            (u / t * 100.0).round() as u16
        } else {
            0
        }
    } else {
        0
    };

    let col1_lines = vec![
        Line::from(vec![
            Span::styled("Dispositivo:  ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&gpu_info.name),
        ]),
        Line::from(vec![
            Span::styled("Controlador:  ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&gpu_info.driver),
        ]),
        Line::from(vec![
            Span::styled("Temperatura:  ", Style::default().fg(Color::Yellow).bold()),
            if let Some(t) = gpu_info.temperature {
                Span::styled(format!("{t:.1} °C"), Style::default().fg(Color::Red).bold())
            } else {
                Span::raw("N/D")
            },
        ]),
    ];

    let col2_lines = vec![
        Line::from(vec![
            Span::styled("VRAM Total:   ", Style::default().fg(Color::Yellow).bold()),
            if let Some(tot) = vram_total_gb {
                Span::raw(format!("{tot:.2} GB"))
            } else {
                Span::raw("Memoria Compartida")
            },
        ]),
        Line::from(vec![
            Span::styled("VRAM Usada:   ", Style::default().fg(Color::Yellow).bold()),
            if let Some(used) = vram_used_gb {
                Span::styled(
                    format!("{used:.2} GB ({vram_pct}%)"),
                    Style::default().fg(Color::Magenta).bold(),
                )
            } else {
                Span::raw("N/D")
            },
        ]),
    ];

    let gtt_used_gb = gpu_info.gtt_used.map(|v| v as f64 / 1_073_741_824.0);
    let gtt_total_gb = gpu_info.gtt_total.map(|v| v as f64 / 1_073_741_824.0);

    let col3_lines = vec![
        Line::from(vec![
            Span::styled(
                "GTT / Dinámica: ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            if let (Some(u), Some(t)) = (gtt_used_gb, gtt_total_gb) {
                Span::raw(format!("{u:.2} / {t:.2} GB"))
            } else {
                Span::raw("N/D")
            },
        ]),
        Line::from(vec![
            Span::styled(
                "Uso de Motor:   ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::styled(
                format!("{current_gpu}%"),
                Style::default().fg(Color::Magenta).bold(),
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(col1_lines), detail_cols[0]);
    f.render_widget(Paragraph::new(col2_lines), detail_cols[1]);
    f.render_widget(Paragraph::new(col3_lines), detail_cols[2]);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let focus_hint = if app.current_view == CurrentView::Overview {
        "[p] Cambiar foco Proc/Puertos │ [s] Ordenar │ "
    } else if app.current_view == CurrentView::Cpu {
        "[c] Global / Por Núcleo │ "
    } else {
        ""
    };

    let text = format!(
        " [1-4] Pestañas │ [Tab/←/→] Navegar │ {focus_hint}[↑/↓] Desplazar │ [?] Ayuda │ [q] Salir"
    );

    let footer = Paragraph::new(text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}

fn draw_help_modal(f: &mut Frame, area: Rect) {
    let modal_width = 62.min(area.width.saturating_sub(4));
    let modal_height = 18.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(modal_width)) / 2;
    let y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Atajos de Teclado y Ayuda",
            Style::default().fg(Color::Cyan).bold(),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  1, 2, 3, 4      ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Ir a pestaña (Resumen, CPU, RAM, GPU)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Tab / ← / →     ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Pestaña siguiente / anterior"),
        ]),
        Line::from(vec![
            Span::styled(
                "  c / C           ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("En pestaña CPU: Alternar Global / Por Núcleo"),
        ]),
        Line::from(vec![
            Span::styled(
                "  p / P           ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("En Resumen: Cambiar foco entre Procesos y Puertos"),
        ]),
        Line::from(vec![
            Span::styled(
                "  s / S           ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("En Resumen: Cambiar criterio de orden de procesos"),
        ]),
        Line::from(vec![
            Span::styled(
                "  ↑ / ↓ / j / k   ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Desplazar fila en la tabla activa"),
        ]),
        Line::from(vec![
            Span::styled(
                "  PgUp / PgDown   ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Desplazar 10 elementos en la tabla"),
        ]),
        Line::from(vec![
            Span::styled(
                "  ? / h           ",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Abrir o cerrar esta ventana de ayuda"),
        ]),
        Line::from(vec![
            Span::styled(
                "  q / Esc / Ctrl+C",
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::raw("Salir de la aplicación"),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Ayuda ",
            Style::default().fg(Color::Cyan).bold(),
        ));

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, modal_area);
}
