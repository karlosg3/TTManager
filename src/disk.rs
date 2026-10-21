use sysinfo::Disks;

/// Sistemas de archivos virtuales que no representan almacenamiento real
const VIRTUAL_FS: [&str; 7] = [
    "tmpfs",
    "devtmpfs",
    "squashfs",
    "overlay",
    "ramfs",
    "efivarfs",
    "fuse.portal",
];

#[derive(Debug, Clone)]
pub struct DiskItem {
    pub mount: String,
    pub device: String,
    pub fs: String,
    pub kind: String,
    pub total: u64,
    pub available: u64,
}

impl DiskItem {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    pub fn used_percent(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            (self.used() as f64 / self.total as f64 * 100.0)
                .round()
                .min(100.0) as u64
        }
    }
}

pub fn collect_disks() -> Vec<DiskItem> {
    let disks = Disks::new_with_refreshed_list();
    let mut items: Vec<DiskItem> = Vec::new();

    for disk in disks.list() {
        let fs = disk.file_system().to_string_lossy().to_string();
        let device = disk.name().to_string_lossy().to_string();
        if disk.total_space() == 0 || VIRTUAL_FS.contains(&fs.as_str()) {
            continue;
        }
        // Un mismo dispositivo puede montarse varias veces (bind mounts, subvolúmenes)
        if items
            .iter()
            .any(|d| d.device == device && d.total == disk.total_space())
        {
            continue;
        }
        items.push(DiskItem {
            mount: disk.mount_point().to_string_lossy().to_string(),
            device,
            fs,
            kind: disk.kind().to_string(),
            total: disk.total_space(),
            available: disk.available_space(),
        });
    }

    // La raíz primero, luego por punto de montaje
    items.sort_by(|a, b| {
        (a.mount != "/")
            .cmp(&(b.mount != "/"))
            .then_with(|| a.mount.cmp(&b.mount))
    });
    items
}
