//! TCP and UDP sockets from `/proc/net/{tcp,udp}{,6}`: connections by state,
//! what's listening, and (through the socket inodes the process scan found)
//! which process owns each.

use super::read;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, PartialEq)]
pub struct Socket {
    pub udp: bool,
    pub local: String,
    pub port: u16,
    pub remote: String,
    pub state: u8,
    pub inode: u64,
}

pub const ESTABLISHED: u8 = 0x01;
pub const TIME_WAIT: u8 = 0x06;
pub const CLOSE_WAIT: u8 = 0x08;
pub const LISTEN: u8 = 0x0A;
/// UDP sockets report "closed" when they're bound but not connected.
pub const UDP_BOUND: u8 = 0x07;

/// "0100007F:223D" (IPv4) or a 32-digit IPv6, little-endian words, then the port.
fn addr(text: &str) -> Option<(String, u16)> {
    let (a, p) = text.split_once(':')?;
    let port = u16::from_str_radix(p, 16).ok()?;
    let ip = match a.len() {
        8 => Ipv4Addr::from(u32::from_str_radix(a, 16).ok()?.swap_bytes()).to_string(),
        32 => {
            let mut bytes = [0u8; 16];
            for w in 0..4 {
                let word = u32::from_str_radix(&a[w * 8..w * 8 + 8], 16).ok()?.swap_bytes();
                bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }
            let v6 = Ipv6Addr::from(bytes);
            match v6.to_ipv4_mapped() {
                Some(v4) => v4.to_string(),
                None => v6.to_string(),
            }
        }
        _ => return None,
    };
    Some((ip, port))
}

pub fn parse(text: &str, udp: bool) -> Vec<Socket> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (local, port) = addr(f.get(1)?)?;
            let (remote, _) = addr(f.get(2)?)?;
            Some(Socket {
                udp,
                local,
                port,
                remote,
                state: u8::from_str_radix(f.get(3)?, 16).ok()?,
                inode: f.get(9)?.parse().ok()?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listener {
    pub udp: bool,
    pub addr: String,
    pub port: u16,
    pub pid: i32,
}

impl Listener {
    /// Reachable from other machines (not only this one).
    pub fn public(&self) -> bool {
        !(self.addr.starts_with("127.") || self.addr == "::1")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Conns {
    pub established: usize,
    pub listening: usize,
    pub time_wait: usize,
    pub close_wait: usize,
    pub other: usize,
    pub udp: usize,
    pub listeners: Vec<Listener>,
    /// Open TCP connections by process.
    pub by_pid: HashMap<i32, usize>,
}

pub fn sample(owners: &HashMap<u64, i32>) -> Conns {
    let mut c = Conns::default();
    let mut socks = Vec::new();
    for (file, udp) in [("tcp", false), ("tcp6", false), ("udp", true), ("udp6", true)] {
        socks.extend(parse(&read(&format!("/proc/net/{file}")), udp));
    }
    for s in &socks {
        let pid = owners.get(&s.inode).copied().unwrap_or(0);
        if s.udp {
            c.udp += 1;
            if s.state == UDP_BOUND && (s.remote == "0.0.0.0" || s.remote == "::") {
                c.listeners.push(Listener { udp: true, addr: s.local.clone(), port: s.port, pid });
            }
            continue;
        }
        match s.state {
            ESTABLISHED => {
                c.established += 1;
                if pid > 0 {
                    *c.by_pid.entry(pid).or_default() += 1;
                }
            }
            LISTEN => {
                c.listening += 1;
                c.listeners.push(Listener { udp: false, addr: s.local.clone(), port: s.port, pid });
            }
            TIME_WAIT => c.time_wait += 1,
            CLOSE_WAIT => c.close_wait += 1,
            _ => c.other += 1,
        }
    }
    c.listeners.sort_by(|a, b| (a.port, a.udp, &a.addr).cmp(&(b.port, b.udp, &b.addr)));
    // One port on several addresses: keep one entry, reachable from the network if any is.
    c.listeners.dedup_by(|a, b| {
        let same = a.port == b.port && a.udp == b.udp && a.pid == b.pid;
        if same && a.public() && !b.public() {
            b.addr = a.addr.clone();
        }
        same
    });
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sockets() {
        let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
            0: 0100007F:223D 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 78216739 1\n\
            1: C401A8C0:D2F0 22B2FA8E:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 555 1\n";
        let s = parse(text, false);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].local.as_str(), s[0].port, s[0].state, s[0].inode), ("127.0.0.1", 8765, LISTEN, 78216739));
        assert_eq!((s[1].local.as_str(), s[1].remote.as_str(), s[1].port), ("192.168.1.196", "142.250.178.34", 54000));
    }

    #[test]
    fn parses_ipv6() {
        assert_eq!(addr("00000000000000000000000001000000:0035"), Some(("::1".into(), 53)));
        assert_eq!(addr("0000000000000000FFFF00000100007F:0050"), Some(("127.0.0.1".into(), 80)));
    }
}
