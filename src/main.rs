mod app;
mod disk;
mod gpu;
mod net;
mod ui;

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

    /// Vista inicial: home, cpu, ram, gpu, disk, procs, ports
    #[arg(short, long, default_value = "home")]
    view: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

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

    // Set initial view from CLI arguments
    match args.view.to_lowercase().as_str() {
        "cpu" => app.set_view(CurrentView::Cpu),
        "ram" | "mem" => app.set_view(CurrentView::Ram),
        "gpu" => app.set_view(CurrentView::Gpu),
        "disk" | "disco" | "storage" => app.set_view(CurrentView::Disk),
        "procs" | "procesos" => app.set_view(CurrentView::Procs),
        "ports" | "puertos" => app.set_view(CurrentView::Ports),
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

    Ok(())
}
