use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use sysinfo::{Pid, Signal, System};

/// Intérpretes y lanzadores genéricos: no identifican a ninguna aplicación
const GENERIC_COMMANDS: [&str; 14] = [
    "sh", "bash", "zsh", "fish", "env", "python", "python3", "perl", "java", "node", "electron",
    "sudo", "pkexec", "flatpak",
];

// ---------------------------------------------------------------------------
// Archivos .desktop
// ---------------------------------------------------------------------------

/// Una aplicación instalada, tal como la ve el lanzador del escritorio
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopEntry {
    pub name: String,
    /// Comando para abrirla, sin los códigos %u, %F…
    pub exec: Vec<String>,
    /// Nombres de proceso con los que se reconoce (en minúsculas)
    keys: Vec<String>,
}

impl DesktopEntry {
    /// Lee solo el grupo [Desktop Entry]; las acciones ([Desktop Action …])
    /// tienen su propio Name/Exec y no deben pisar los de la aplicación
    fn parse(id: &str, text: &str) -> Option<Self> {
        let mut fields: HashMap<&str, &str> = HashMap::new();
        let mut in_main = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_main = line == "[Desktop Entry]";
            } else if in_main && let Some((key, value)) = line.split_once('=') {
                fields.entry(key.trim()).or_insert(value.trim());
            }
        }

        let is_true = |key| fields.get(key).is_some_and(|v| *v == "true");
        if fields.get("Type") != Some(&"Application") || is_true("NoDisplay") || is_true("Hidden") {
            return None;
        }

        let exec = parse_exec(fields.get("Exec")?);
        let mut keys = Vec::new();
        keys.extend(command_key(&exec));
        if let Some(try_exec) = fields.get("TryExec") {
            keys.push(basename(try_exec));
        }
        if let Some(class) = fields.get("StartupWMClass") {
            keys.push(class.to_lowercase());
        }
        // "dev.zed.Zed" → "zed"
        keys.extend(id.rsplit('.').next().map(str::to_lowercase));
        keys.retain(|k| k.len() >= 2 && !GENERIC_COMMANDS.contains(&k.as_str()));
        keys.dedup();

        Some(Self {
            name: fields.get("Name")?.to_string(),
            exec,
            keys,
        })
    }
}

/// Separa la línea Exec respetando comillas y quita los códigos de campo
fn parse_exec(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => current.extend(chars.next()),
            ' ' if !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    // %u, %F, %i… los rellena el lanzador con archivos/URLs; al reiniciar no hay ninguno
    args.retain(|a| !(a.len() == 2 && a.starts_with('%')));
    args
}

/// El programa que lanza realmente la línea Exec, saltando `env VAR=x`
/// y entendiendo `flatpak run --command=x app.id`
fn command_key(exec: &[String]) -> Option<String> {
    let mut args = exec.iter().map(String::as_str);
    let mut program = args.next()?;
    if basename(program) == "env" {
        program = args.by_ref().find(|a| !a.contains('='))?;
    }
    if basename(program) == "flatpak" {
        let rest: Vec<&str> = args.collect();
        return rest
            .iter()
            .find_map(|a| a.strip_prefix("--command="))
            .or_else(|| rest.iter().rev().find(|a| a.contains('.')).copied())
            .map(|s| basename(s.rsplit('.').next().unwrap_or(s)));
    }
    Some(basename(program))
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn application_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());

    // Las del usuario primero: sus .desktop sustituyen a los del sistema
    let mut dirs = vec![data_home.join("applications")];
    dirs.extend(env::split_paths(&data_dirs).map(|d| d.join("applications")));
    dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));
    dirs.dedup();
    dirs
}

pub fn load_desktop_entries() -> Vec<DesktopEntry> {
    let mut seen_ids = Vec::new();
    let mut entries = Vec::new();
    for dir in application_dirs() {
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        for path in files.filter_map(|f| f.ok()).map(|f| f.path()) {
            let Some(id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".desktop"))
                .map(str::to_string)
            else {
                continue;
            };
            if seen_ids.contains(&id) {
                continue;
            }
            if let Some(entry) = fs::read_to_string(&path)
                .ok()
                .and_then(|text| DesktopEntry::parse(&id, &text))
            {
                entries.push(entry);
            }
            seen_ids.push(id);
        }
    }
    entries
}

// ---------------------------------------------------------------------------
// Agrupar procesos en aplicaciones
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AppItem {
    pub name: String,
    pub pids: Vec<u32>,
    /// Porcentaje del total del equipo (100% = todos los núcleos)
    pub cpu: f32,
    pub memory_bytes: u64,
    pub exec: Vec<String>,
    /// La aplicación contiene a este mismo ttm (p. ej. la terminal)
    pub contains_self: bool,
}

/// Lo mínimo de cada proceso que necesita el agrupado; separado de sysinfo
/// para poder probarlo con datos inventados
#[derive(Debug, Clone)]
struct ProcInfo {
    parent: Option<u32>,
    names: Vec<String>,
    uid: Option<u32>,
    cpu: f32,
    memory: u64,
}

/// Nombres con los que buscar el proceso: el ejecutable real y, si tiene
/// guion, su prefijo ("zed-editor" → "zed", "firefox-bin" → "firefox")
fn process_names(name: &str, exe: Option<&Path>) -> Vec<String> {
    let main = exe
        .and_then(|e| e.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| name.to_lowercase());
    let mut names = vec![main.clone()];
    if let Some((prefix, _)) = main.split_once(['-', '_'])
        && prefix.len() >= 3
    {
        names.push(prefix.to_string());
    }
    names
}

pub fn collect_apps(sys: &System, entries: &[DesktopEntry]) -> Vec<AppItem> {
    let uid_of = |pid: u32| fs::metadata(format!("/proc/{pid}")).ok().map(|m| m.uid());
    let procs: HashMap<u32, ProcInfo> = sys
        .processes()
        .iter()
        // Los hilos aparecen como procesos y repetirían la memoria
        .filter(|(_, p)| p.thread_kind().is_none())
        .map(|(pid, p)| {
            let pid = pid.as_u32();
            let info = ProcInfo {
                parent: p.parent().map(|pp| pp.as_u32()),
                names: process_names(&p.name().to_string_lossy(), p.exe()),
                uid: uid_of(pid),
                cpu: p.cpu_usage(),
                memory: p.memory(),
            };
            (pid, info)
        })
        .collect();

    group_apps(
        &procs,
        entries,
        uid_of(std::process::id()),
        std::process::id(),
        sys.cpus().len().max(1),
    )
}

fn group_apps(
    procs: &HashMap<u32, ProcInfo>,
    entries: &[DesktopEntry],
    my_uid: Option<u32>,
    own_pid: u32,
    cores: usize,
) -> Vec<AppItem> {
    let mut by_key: HashMap<&str, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        for key in &entry.keys {
            by_key.entry(key.as_str()).or_insert(i);
        }
    }
    let entry_of = |info: &ProcInfo| {
        info.names
            .iter()
            .find_map(|n| by_key.get(n.as_str()).copied())
    };

    let mut apps: HashMap<usize, AppItem> = HashMap::new();
    for (&pid, info) in procs {
        if info.uid != my_uid {
            continue;
        }
        // Sube por los padres hasta encontrar una aplicación conocida. Se para
        // en init, en el systemd del usuario o al cambiar de usuario.
        let mut current = Some((pid, info));
        let mut found = None;
        while let Some((cur_pid, cur)) = current {
            if cur_pid == 1 || cur.uid != my_uid || cur.names.iter().any(|n| n == "systemd") {
                break;
            }
            if let Some(idx) = entry_of(cur) {
                found = Some(idx);
                break;
            }
            current = cur.parent.and_then(|pp| procs.get(&pp).map(|p| (pp, p)));
        }
        let Some(idx) = found else {
            continue;
        };

        let app = apps.entry(idx).or_insert_with(|| AppItem {
            name: entries[idx].name.clone(),
            pids: Vec::new(),
            cpu: 0.0,
            memory_bytes: 0,
            exec: entries[idx].exec.clone(),
            contains_self: false,
        });
        app.pids.push(pid);
        app.cpu += info.cpu / cores as f32;
        app.memory_bytes += info.memory;
        app.contains_self |= pid == own_pid;
    }
    apps.into_values().collect()
}

// ---------------------------------------------------------------------------
// Cerrar y reiniciar
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    Close,
    ForceClose,
    Restart,
}

/// Envía la señal ya (con los datos de sysinfo) y deja en otro hilo la
/// espera y el relanzamiento, para no congelar la interfaz. El resultado
/// llega como texto por `report`.
pub fn run_action(sys: &System, app: &AppItem, action: AppAction, report: Sender<String>) {
    let signal = if action == AppAction::ForceClose {
        Signal::Kill
    } else {
        Signal::Term
    };
    for pid in &app.pids {
        if let Some(process) = sys.process(Pid::from_u32(*pid)) {
            process.kill_with(signal);
        }
    }

    let app = app.clone();
    thread::spawn(move || {
        // Firefox y similares guardan la sesión al cerrar: se les da margen
        let timeout = Duration::from_secs(if action == AppAction::Restart { 10 } else { 5 });
        let closed = wait_for_exit(&app.pids, timeout);
        let message = match (action, closed) {
            (AppAction::Restart, true) => match launch(&app.exec) {
                Ok(()) => format!("{} reiniciada", app.name),
                Err(e) => format!("{} se cerró pero no se pudo abrir: {e}", app.name),
            },
            (AppAction::Restart, false) => {
                format!("{} no se cerró a tiempo; no se reinició", app.name)
            }
            (_, true) => format!("{} cerrada", app.name),
            (_, false) => format!("{} sigue abierta; prueba a forzar el cierre", app.name),
        };
        let _ = report.send(message);
    });
}

fn is_alive(pid: u32) -> bool {
    // Un proceso zombi ya terminó: solo espera a que su padre lo recoja.
    // En /proc/<pid>/stat el estado va justo después del ")" del nombre.
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.trim_start().chars().next())
        })
        .is_some_and(|state| state != 'Z')
}

fn wait_for_exit(pids: &[u32], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !pids.iter().any(|&pid| is_alive(pid)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// `setsid -f` lanza la app en su propia sesión: no es hija de ttm, no
/// escribe en su terminal y sigue abierta aunque ttm se cierre
fn launch(exec: &[String]) -> io::Result<()> {
    if exec.is_empty() {
        return Err(io::Error::other("el .desktop no tiene comando"));
    }
    let home = env::var_os("HOME").unwrap_or_else(|| "/".into());
    let status = Command::new("setsid")
        .arg("-f")
        .args(exec)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("setsid falló"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "[Desktop Entry]
Type=Application
Name=Firefox
Exec=firefox %u
StartupWMClass=firefox

[Desktop Action new-window]
Name=New Window
Exec=firefox --new-window %u
";

    fn entry(id: &str, name: &str, exec: &str) -> DesktopEntry {
        let text = format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n");
        DesktopEntry::parse(id, &text).unwrap()
    }

    #[test]
    fn parses_main_group_only() {
        let e = DesktopEntry::parse("firefox", FIREFOX).unwrap();
        assert_eq!(e.name, "Firefox");
        assert_eq!(e.exec, ["firefox"]);
        assert_eq!(e.keys, ["firefox"]);

        let hidden = FIREFOX.replace("Type=Application", "Type=Application\nNoDisplay=true");
        assert_eq!(DesktopEntry::parse("firefox", &hidden), None);
    }

    #[test]
    fn parses_exec_lines() {
        assert_eq!(
            parse_exec(r#""/opt/My App/app" --flag "a \"b\"" %F"#),
            ["/opt/My App/app", "--flag", "a \"b\""]
        );
        let key = |exec: &str| command_key(&parse_exec(exec));
        assert_eq!(
            key("env GDK_BACKEND=x11 /usr/bin/slack %U").as_deref(),
            Some("slack")
        );
        assert_eq!(
            key("/usr/bin/flatpak run --branch=stable org.gimp.GIMP %U").as_deref(),
            Some("gimp")
        );
        // Un .desktop que es un script de bash no debe adueñarse de las shells
        assert!(entry("tool", "Tool", "bash -c run").keys == ["tool"]);
    }

    #[test]
    fn process_name_prefixes() {
        let exe = Path::new("/home/u/.local/zed.app/libexec/zed-editor");
        assert_eq!(
            process_names("zed-editor", Some(exe)),
            ["zed-editor", "zed"]
        );
        // Sin ejecutable legible se usa el nombre del proceso
        assert_eq!(process_names("slack", None), ["slack"]);
    }

    #[test]
    fn groups_processes_by_nearest_app() {
        let entries = vec![
            entry("firefox", "Firefox", "firefox %u"),
            entry("com.system76.CosmicTerm", "COSMIC Terminal", "cosmic-term"),
        ];
        let p = |parent: u32, name: &str, uid: u32, mem: u64| ProcInfo {
            parent: Some(parent),
            names: process_names(name, None),
            uid: Some(uid),
            cpu: 8.0,
            memory: mem,
        };
        let procs = HashMap::from([
            (1, p(0, "systemd", 0, 0)),
            (100, p(1, "systemd", 1000, 0)),
            (200, p(1, "firefox-bin", 1000, 300)),
            (201, p(200, "Isolated Web Co", 1000, 100)),
            (300, p(100, "cosmic-term", 1000, 50)),
            (301, p(300, "bash", 1000, 5)),
            (302, p(301, "ttm", 1000, 5)),
            // Firefox abierto desde la terminal sigue contando como Firefox
            (303, p(301, "firefox", 1000, 40)),
            (400, p(100, "pipewire", 1000, 10)),
            (500, p(1, "firefox", 0, 999)), // de otro usuario
        ]);

        let mut apps = group_apps(&procs, &entries, Some(1000), 302, 4);
        apps.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(apps.len(), 2);
        let (term, firefox) = (&apps[0], &apps[1]);
        assert_eq!(term.name, "COSMIC Terminal");
        assert_eq!(term.pids.len(), 3);
        assert!(term.contains_self);
        assert_eq!(firefox.memory_bytes, 440);
        assert_eq!(firefox.cpu, 6.0); // 3 procesos × 8% / 4 núcleos
        assert!(!firefox.contains_self);
    }

    #[test]
    fn zombie_and_missing_processes_are_not_alive() {
        assert!(is_alive(std::process::id()));
        assert!(!is_alive(u32::MAX));
        assert!(wait_for_exit(&[u32::MAX], Duration::from_millis(10)));
    }
}
