use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

const REPO: &str = "karlosg3/TTManager";
/// Como mucho una consulta a GitHub al día
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

// ---------------------------------------------------------------------------
// Versión
// ---------------------------------------------------------------------------

/// Versión semántica sencilla. El `Ord` derivado compara los campos en orden
/// de declaración (major, luego minor, luego patch): justo lo que necesitamos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("versión de Cargo.toml válida")
    }

    /// Acepta "1.2.3" o "v1.2.3". Las pre-release ("1.2.3-beta") se ignoran.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text
            .trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse::<u64>().ok());
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ---------------------------------------------------------------------------
// Caché en ~/.cache/ttm/update
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq)]
struct Cache {
    checked_at: u64,
    latest: Option<Version>,
    skipped: Option<Version>,
}

impl Cache {
    fn path() -> Option<PathBuf> {
        let base = env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
        Some(base.join("ttm").join("update"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|text| Self::from_text(&text))
            .unwrap_or_default()
    }

    /// Si no se puede guardar no pasa nada: solo se volverá a consultar antes
    fn save(&self) {
        if let Some(path) = Self::path() {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(path, self.to_text());
        }
    }

    fn from_text(text: &str) -> Self {
        let mut cache = Self::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("checked_at", v)) => cache.checked_at = v.parse().unwrap_or(0),
                Some(("latest", v)) => cache.latest = Version::parse(v),
                Some(("skipped", v)) => cache.skipped = Version::parse(v),
                _ => {}
            }
        }
        cache
    }

    fn to_text(&self) -> String {
        let mut text = format!("checked_at={}\n", self.checked_at);
        if let Some(v) = self.latest {
            text.push_str(&format!("latest={v}\n"));
        }
        if let Some(v) = self.skipped {
            text.push_str(&format!("skipped={v}\n"));
        }
        text
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Comprobación en segundo plano
// ---------------------------------------------------------------------------

/// Lanza la comprobación en otro hilo para no retrasar el arranque.
/// El receptor solo recibe algo si hay una versión más nueva que el usuario
/// no haya descartado.
pub fn spawn_check() -> Receiver<Version> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Some(version) = check_for_update() {
            // Si la app ya se cerró el receptor no existe; da igual
            let _ = tx.send(version);
        }
    });
    rx
}

fn check_for_update() -> Option<Version> {
    let mut cache = Cache::load();
    let latest = if now_secs().saturating_sub(cache.checked_at) < CHECK_INTERVAL_SECS {
        cache.latest
    } else {
        // Sin red: no se actualiza la fecha, así se reintenta en el próximo arranque
        let latest = fetch_latest(Duration::from_secs(5)).ok()?.version;
        cache.checked_at = now_secs();
        cache.latest = Some(latest);
        cache.save();
        Some(latest)
    }?;

    (latest > Version::current() && cache.skipped != Some(latest)).then_some(latest)
}

/// "No, gracias": no volver a avisar de esta versión (sí de las siguientes)
pub fn skip(version: Version) {
    let mut cache = Cache::load();
    cache.skipped = Some(version);
    cache.save();
}

// ---------------------------------------------------------------------------
// GitHub Releases
// ---------------------------------------------------------------------------

struct Asset {
    name: String,
    url: String,
}

struct Release {
    version: Version,
    assets: Vec<Asset>,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn get(url: &str, timeout: Duration) -> Result<Vec<u8>> {
    let user_agent = format!("ttm/{}", Version::current());
    let mut response = agent(timeout)
        .get(url)
        .header("User-Agent", &user_agent)
        .header("Accept", "application/vnd.github+json")
        .call()?;
    Ok(response.body_mut().read_to_vec()?)
}

fn fetch_latest(timeout: Duration) -> Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let json: serde_json::Value = serde_json::from_slice(&get(&url, timeout)?)?;

    let version = json["tag_name"]
        .as_str()
        .and_then(Version::parse)
        .ok_or("el release no tiene un tag de versión válido")?;

    let assets = json["assets"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a["name"].as_str()?.to_string(),
                        url: a["browser_download_url"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Release { version, assets })
}

// ---------------------------------------------------------------------------
// Instalación
// ---------------------------------------------------------------------------

/// Cómo se instaló ttm, que decide cómo debe actualizarse
#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallMethod {
    Deb,                 // /usr/bin/ttm gestionado por dpkg
    Cargo,               // ~/.cargo/bin/ttm
    Standalone(PathBuf), // binario del .tar.gz copiado a mano
    Dev,                 // target/debug o target/release de una compilación local
}

impl InstallMethod {
    fn detect() -> Result<Self> {
        let exe = env::current_exe()?;
        let owned_by_dpkg = || {
            Command::new("dpkg-query")
                .arg("-S")
                .arg(&exe)
                .output()
                .is_ok_and(|o| o.status.success())
        };

        Ok(if exe.starts_with("/usr") && owned_by_dpkg() {
            InstallMethod::Deb
        } else if exe.components().any(|c| c.as_os_str() == ".cargo") {
            InstallMethod::Cargo
        } else if exe.components().any(|c| c.as_os_str() == "target") {
            InstallMethod::Dev
        } else {
            InstallMethod::Standalone(exe)
        })
    }
}

/// Se ejecuta fuera de la TUI (con `--update` o tras aceptar el aviso), así
/// que puede escribir en la terminal y pedir la contraseña de sudo.
pub fn run_update() -> Result<()> {
    let current = Version::current();
    println!("Buscando la última versión de ttm…");
    let release = fetch_latest(Duration::from_secs(10))?;

    if release.version <= current {
        println!("ttm v{current} ya es la última versión.");
        return Ok(());
    }
    println!("Actualizando ttm v{current} → v{}", release.version);

    match InstallMethod::detect()? {
        InstallMethod::Deb => install_deb(&release)?,
        InstallMethod::Standalone(exe) => replace_binary(&release, &exe)?,
        InstallMethod::Cargo => install_with_cargo(&release)?,
        InstallMethod::Dev => {
            return Err(
                "es una compilación local; actualiza con `git pull && cargo build --release`"
                    .into(),
            );
        }
    }

    println!(
        "Listo: ttm v{} instalado. Vuelve a abrirlo.",
        release.version
    );
    Ok(())
}

fn require_x86_64() -> Result<()> {
    if env::consts::ARCH == "x86_64" {
        Ok(())
    } else {
        Err(format!(
            "solo se publican binarios x86_64 (este equipo: {})",
            env::consts::ARCH
        )
        .into())
    }
}

fn install_deb(release: &Release) -> Result<()> {
    require_x86_64()?;
    let data = download_verified(release, |name| name.ends_with("_amd64.deb"))?;

    let path = env::temp_dir().join(format!("ttm_{}_amd64.deb", release.version));
    fs::write(&path, data)?;

    println!("Instalando con apt (puede pedir tu contraseña)…");
    let status = Command::new("sudo")
        .args(["apt", "install", "-y"])
        .arg(&path)
        .status();
    let _ = fs::remove_file(&path);

    if status?.success() {
        Ok(())
    } else {
        Err("apt no pudo instalar el paquete".into())
    }
}

fn replace_binary(release: &Release, exe: &Path) -> Result<()> {
    require_x86_64()?;
    let tarball = download_verified(release, |name| name.ends_with("-linux-x86_64.tar.gz"))?;
    let binary = extract_binary(&tarball)?;

    // El temporal va en la misma carpeta: rename() solo es atómico dentro
    // del mismo sistema de archivos
    let dir = exe.parent().ok_or("ruta del ejecutable inválida")?;
    let tmp = dir.join(".ttm.new");
    let write = || -> io::Result<()> {
        fs::write(&tmp, &binary)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        // Linux permite sustituir un ejecutable en uso: el proceso actual
        // conserva el archivo viejo abierto hasta que termina
        fs::rename(&tmp, exe)
    };

    write().map_err(|e| -> Box<dyn Error> {
        let _ = fs::remove_file(&tmp);
        if e.kind() == io::ErrorKind::PermissionDenied {
            format!(
                "sin permiso para escribir en {}; prueba con `sudo ttm --update`",
                dir.display()
            )
            .into()
        } else {
            e.into()
        }
    })
}

fn install_with_cargo(release: &Release) -> Result<()> {
    let repo = format!("https://github.com/{REPO}");
    let tag = format!("v{}", release.version);
    println!("Compilando con cargo (puede tardar unos minutos)…");
    let status = Command::new("cargo")
        .args([
            "install", "--locked", "--force", "--git", &repo, "--tag", &tag,
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("cargo install falló".into())
    }
}

/// Descarga el archivo que cumpla `matches` y lo compara con su `.sha256`
fn download_verified(release: &Release, matches: impl Fn(&str) -> bool) -> Result<Vec<u8>> {
    let asset = release
        .assets
        .iter()
        .find(|a| matches(&a.name))
        .ok_or("esta versión no incluye un paquete para este sistema")?;
    let checksum_name = format!("{}.sha256", asset.name);
    let checksum = release
        .assets
        .iter()
        .find(|a| a.name == checksum_name)
        .ok_or("falta el archivo .sha256 del paquete")?;

    println!("Descargando {}…", asset.name);
    let timeout = Duration::from_secs(120);
    let data = get(&asset.url, timeout)?;

    // Formato de sha256sum: "<hash>  <nombre>"
    let expected = String::from_utf8(get(&checksum.url, timeout)?)?;
    let expected = expected.split_whitespace().next().ok_or("checksum vacío")?;
    let actual = sha256_hex(&data);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("el checksum no coincide ({actual} ≠ {expected}); se cancela").into());
    }
    println!("Checksum SHA-256 verificado.");
    Ok(data)
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Busca el ejecutable `ttm` dentro del .tar.gz (está en una subcarpeta)
fn extract_binary(tarball: &[u8]) -> Result<Vec<u8>> {
    let mut archive = tar::Archive::new(GzDecoder::new(tarball));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path()?.file_name() == Some("ttm".as_ref()) {
            let mut binary = Vec::new();
            entry.read_to_end(&mut binary)?;
            return Ok(binary);
        }
    }
    Err("el paquete no contiene el binario ttm".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_versions() {
        let v = |s| Version::parse(s).unwrap();
        assert_eq!(v("v0.2.0").to_string(), "0.2.0");
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.2.1") > v("0.2.0"));
        assert_eq!(Version::parse("0.3.0-beta"), None);
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
    }

    #[test]
    fn cache_round_trip() {
        let cache = Cache {
            checked_at: 1_700_000_000,
            latest: Version::parse("0.3.0"),
            skipped: Version::parse("0.2.5"),
        };
        assert_eq!(Cache::from_text(&cache.to_text()), cache);
        assert_eq!(Cache::from_text("basura\n"), Cache::default());
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn extracts_binary_from_release_tarball() {
        // Mismo layout que el workflow: ttm-vX-linux-x86_64/{ttm,README.md}
        let mut builder = tar::Builder::new(Vec::new());
        for (path, content) in [
            ("ttm-v9.9.9-linux-x86_64/README.md", &b"readme"[..]),
            ("ttm-v9.9.9-linux-x86_64/ttm", &b"\x7fELF binario"[..]),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, content).unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        io::Write::write_all(&mut gz, &builder.into_inner().unwrap()).unwrap();

        let binary = extract_binary(&gz.finish().unwrap()).unwrap();
        assert_eq!(binary, b"\x7fELF binario");
    }
}
