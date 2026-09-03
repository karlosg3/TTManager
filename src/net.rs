use std::collections::HashMap;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use sysinfo::{Pid, System};

#[derive(Debug, Clone)]
pub struct PortItem {
    pub port: u16,
    pub protocol: &'static str,
    pub address: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
}

pub fn collect_listening_ports(sys: &System) -> Vec<PortItem> {
    let inode_to_pid = build_inode_pid_map();

    let mut ports: Vec<PortItem> = Vec::new();

    parse_tcp_file("/proc/net/tcp", "TCP", &inode_to_pid, sys, &mut ports);
    parse_tcp_file("/proc/net/tcp6", "TCP6", &inode_to_pid, sys, &mut ports);
    parse_udp_file("/proc/net/udp", "UDP", &inode_to_pid, sys, &mut ports);
    parse_udp_file("/proc/net/udp6", "UDP6", &inode_to_pid, sys, &mut ports);

    // Deduplicate by (port, protocol, address)
    ports.sort_by(|a, b| a.port.cmp(&b.port).then_with(|| a.protocol.cmp(b.protocol)));
    ports.dedup_by(|a, b| a.port == b.port && a.protocol == b.protocol && a.address == b.address);

    ports
}

fn build_inode_pid_map() -> HashMap<u64, u32> {
    let mut map = HashMap::new();
    let proc_dir = Path::new("/proc");

    let Ok(entries) = fs::read_dir(proc_dir) else {
        return map;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let Ok(pid) = name_str.parse::<u32>() else {
            continue;
        };

        let fd_dir = entry.path().join("fd");
        let Ok(fd_entries) = fs::read_dir(fd_dir) else {
            continue;
        };

        for fd in fd_entries.filter_map(|e| e.ok()) {
            if let Ok(target) = fs::read_link(fd.path()) {
                let target_str = target.to_string_lossy();
                if target_str.starts_with("socket:[") && target_str.ends_with(']') {
                    let inode_str = &target_str[8..target_str.len() - 1];
                    if let Ok(inode) = inode_str.parse::<u64>() {
                        map.insert(inode, pid);
                    }
                }
            }
        }
    }

    map
}

fn parse_tcp_file(
    path: &str,
    proto: &'static str,
    inode_map: &HashMap<u64, u32>,
    sys: &System,
    results: &mut Vec<PortItem>,
) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    for line in content.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 10 {
            continue;
        }

        // State 0A is TCP_LISTEN
        if tokens[3] != "0A" {
            continue;
        }

        let local_addr = tokens[1];
        let inode_str = tokens[9];

        if let Some((addr_str, port)) = parse_ip_port(local_addr) {
            let inode = inode_str.parse::<u64>().unwrap_or(0);
            let pid = inode_map.get(&inode).copied();
            let process_name = pid.and_then(|p| {
                sys.process(Pid::from(p as usize))
                    .map(|proc| proc.name().to_string_lossy().to_string())
            });

            results.push(PortItem {
                port,
                protocol: proto,
                address: addr_str,
                pid,
                process_name,
            });
        }
    }
}

fn parse_udp_file(
    path: &str,
    proto: &'static str,
    inode_map: &HashMap<u64, u32>,
    sys: &System,
    results: &mut Vec<PortItem>,
) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    for line in content.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 10 {
            continue;
        }

        // State 07 is UDP active/listen
        if tokens[3] != "07" {
            continue;
        }

        let local_addr = tokens[1];
        let inode_str = tokens[9];

        if let Some((addr_str, port)) = parse_ip_port(local_addr) {
            if port == 0 {
                continue;
            }
            let inode = inode_str.parse::<u64>().unwrap_or(0);
            let pid = inode_map.get(&inode).copied();
            let process_name = pid.and_then(|p| {
                sys.process(Pid::from(p as usize))
                    .map(|proc| proc.name().to_string_lossy().to_string())
            });

            results.push(PortItem {
                port,
                protocol: proto,
                address: addr_str,
                pid,
                process_name,
            });
        }
    }
}

fn parse_ip_port(s: &str) -> Option<(String, u16)> {
    let mut parts = s.split(':');
    let ip_hex = parts.next()?;
    let port_hex = parts.next()?;

    let port = u16::from_str_radix(port_hex, 16).ok()?;

    if ip_hex.len() == 8 {
        // IPv4 in little endian
        let num = u32::from_str_radix(ip_hex, 16).ok()?;
        let ip = Ipv4Addr::from(num.to_le_bytes());
        Some((ip.to_string(), port))
    } else if ip_hex.len() == 32 {
        if ip_hex.chars().all(|c| c == '0') {
            Some(("::".to_string(), port))
        } else if ip_hex == "00000000000000000000000001000000" {
            Some(("::1".to_string(), port))
        } else {
            Some(("[IPv6]".to_string(), port))
        }
    } else {
        Some((ip_hex.to_string(), port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ip_port_ipv4() {
        let (ip, port) = parse_ip_port("0100007F:2CAA").unwrap();
        assert_eq!(ip, "127.0.0.1");
        assert_eq!(port, 11434);

        let (ip2, port2) = parse_ip_port("00000000:0050").unwrap();
        assert_eq!(ip2, "0.0.0.0");
        assert_eq!(port2, 80);
    }

    #[test]
    fn test_parse_ip_port_ipv6() {
        let (ip, port) = parse_ip_port("00000000000000000000000000000000:1F90").unwrap();
        assert_eq!(ip, "::");
        assert_eq!(port, 8080);
    }
}
