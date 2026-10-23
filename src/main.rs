mod app;
mod disk;
mod gpu;
mod net;
mod power;
mod ui;
mod updater;

use std::error::Error;
use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::{App, CurrentView};

/// TTM (TuiTaskManager) - Monitor de recursos del sistema para Linux
#[derive(Parser, Debug)]
#[command(name = "ttm", author, version, about, long_about = None)]
struct Cli {
    /// Intervalo de muestreo en milisegundos (por defecto: 1000 ms)
    #[arg(short, long, default_value_t = 1000)]
    interval: u64,

    /// Vista inicial: home, cpu, ram, gpu, disk, procs, ports, power
    #[arg(short, long, default_value = "home")]
    view: String,

    /// Buscar e instalar la última versión y salir
    #[arg(long)]
    update: bool,

    /// No buscar actualizaciones al arrancar (también: TTM_NO_UPDATE_CHECK=1)
    #[arg(long)]
    no_update_check: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

    // `ttm --update` no abre la TUI
    if args.update {
        update_and_exit();
    }

    // Hook to ensure terminal is cleanly restored on unexpected panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    if !args.no_update_check && std::env::var_os("TTM_NO_UPDATE_CHECK").is_none() {
        app.start_update_check();
    }

    // Set initial view from CLI arguments
    match args.view.to_lowercase().as_str() {
        "cpu" => app.set_view(CurrentView::Cpu),
        "ram" | "mem" => app.set_view(CurrentView::Ram),
        "gpu" => app.set_view(CurrentView::Gpu),
        "disk" | "disco" | "storage" => app.set_view(CurrentView::Disk),
        "procs" | "procesos" => app.set_view(CurrentView::Procs),
        "ports" | "puertos" => app.set_view(CurrentView::Ports),
        "power" | "energia" | "energía" | "battery" | "bateria" => {
            app.set_view(CurrentView::Power)
        }
        _ => app.set_view(CurrentView::Home),
    }

    let tick_rate = Duration::from_millis(args.interval.max(200));
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0))
            .min(Duration::from_millis(40));

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            // Ignore key release events if terminal generates them
            if key.kind != crossterm::event::KeyEventKind::Press {
                continue;
            }

            // Check Ctrl+C
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.quit();
                break;
            }

            // Con el aviso de actualización abierto, solo cuentan sus teclas
            if app.show_update_prompt {
                match key.code {
                    KeyCode::Char('s' | 'S' | 'y' | 'Y') | KeyCode::Enter => app.accept_update(),
                    KeyCode::Char('n' | 'N') => app.decline_update(),
                    KeyCode::Esc => app.postpone_update(),
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') => app.quit(),
                KeyCode::Esc => {
                    if app.show_help {
                        app.show_help = false;
                    } else {
                        app.quit();
                    }
                }
                KeyCode::Char('?') | KeyCode::Char('h') => app.toggle_help(),
                KeyCode::Char('u') | KeyCode::Char('U') => app.open_update_prompt(),
                KeyCode::Char('c') | KeyCode::Char('C') => app.toggle_cpu_mode(),
                KeyCode::Char('d') | KeyCode::Char('D') => app.toggle_expanded(),
                KeyCode::Char('s') | KeyCode::Char('S') => app.toggle_process_sort(),
                KeyCode::Tab => app.next_view(),
                KeyCode::BackTab => app.prev_view(),
                KeyCode::Right => app.next_view(),
                KeyCode::Left => app.prev_view(),
                KeyCode::Char('1') => app.set_view(CurrentView::Home),
                KeyCode::Char('2') => app.set_view(CurrentView::Cpu),
                KeyCode::Char('3') => app.set_view(CurrentView::Ram),
                KeyCode::Char('4') => app.set_view(CurrentView::Gpu),
                KeyCode::Char('5') => app.set_view(CurrentView::Disk),
                KeyCode::Char('6') => app.set_view(CurrentView::Procs),
                KeyCode::Char('7') => app.set_view(CurrentView::Ports),
                KeyCode::Char('8') => app.set_view(CurrentView::Power),
                KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
                KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
                KeyCode::PageUp => app.page_up(),
                KeyCode::PageDown => app.page_down(),
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // La terminal ya está restaurada: el instalador puede imprimir y pedir sudo
    if app.update_accepted {
        update_and_exit();
    }

    Ok(())
}

fn update_and_exit() -> ! {
    match updater::run_update() {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("No se pudo actualizar: {e}");
            std::process::exit(1);
        }
    }
}
