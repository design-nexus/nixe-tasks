//! Memory, swap and zram from `/proc/meminfo` and `/sys/block/zram*`, paging
//! activity from `/proc/vmstat`, and memory pressure.

use super::cpu::{Pressure, pressure};
use super::read;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Mem {
    pub total: u64,
    pub available: u64,
    pub used: u64,
    /// Page cache the kernel can drop (file pages, reclaimable slab, buffers), less shared memory.
    pub cached: u64,
    pub buffers: u64,
    pub shared: u64,
    pub dirty: u64,
    pub writeback: u64,
    /// Program memory (heaps and stacks): `AnonPages`.
    pub anon: u64,
    /// Memory the kernel holds for itself: unreclaimable slab, page tables, stacks, vmalloc.
    pub kernel: u64,
    pub slab_reclaimable: u64,
    pub slab_unreclaimable: u64,
    pub page_tables: u64,
    pub kernel_stack: u64,
    pub mapped: u64,
    pub locked: u64,
    pub hugepages: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub active: u64,
    pub inactive: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Swap that's also still in RAM (`SwapCached`).
    pub swap_cached: u64,
    /// Uncompressed data held in zram, and the RAM it actually takes.
    pub zram_data: u64,
    pub zram_used: u64,
    pub pressure: Option<Pressure>,
    /// Per second, since the last sample.
    pub major_faults: f64,
    pub swap_in: f64,
    pub swap_out: f64,
    /// Processes the kernel ended for want of memory, since boot.
    pub oom_kills: u64,
}

/// `/proc/meminfo` as bytes by key.
pub fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let mut parts = v.split_whitespace();
            let n: u64 = parts.next()?.parse().ok()?;
            let mult = if parts.next() == Some("kB") { 1024 } else { 1 };
            Some((k.to_string(), n * mult))
        })
        .collect()
}

pub fn from_meminfo(info: &HashMap<String, u64>) -> Mem {
    let g = |k: &str| info.get(k).copied().unwrap_or(0);
    let total = g("MemTotal");
    let available = if info.contains_key("MemAvailable") { g("MemAvailable") } else { g("MemFree") + g("Cached") };
    Mem {
        total,
        available,
        used: total.saturating_sub(available),
        cached: (g("Cached") + g("SReclaimable")).saturating_sub(g("Shmem")),
        buffers: g("Buffers"),
        shared: g("Shmem"),
        dirty: g("Dirty"),
        writeback: g("Writeback"),
        anon: g("AnonPages"),
        kernel: g("SUnreclaim") + g("PageTables") + g("KernelStack") + g("VmallocUsed") + g("Percpu"),
        slab_reclaimable: g("SReclaimable"),
        slab_unreclaimable: g("SUnreclaim"),
        page_tables: g("PageTables"),
        kernel_stack: g("KernelStack"),
        mapped: g("Mapped"),
        locked: g("Mlocked"),
        hugepages: g("HugePages_Total") * g("Hugepagesize"),
        committed: g("Committed_AS"),
        commit_limit: g("CommitLimit"),
        active: g("Active"),
        inactive: g("Inactive"),
        swap_total: g("SwapTotal"),
        swap_used: g("SwapTotal").saturating_sub(g("SwapFree")),
        swap_cached: g("SwapCached"),
        ..Default::default()
    }
}

/// The `/proc/vmstat` counters we turn into rates: major faults, pages swapped in and out, OOM kills.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VmStat {
    pub pgmajfault: u64,
    pub pswpin: u64,
    pub pswpout: u64,
    pub oom_kill: u64,
}

pub fn parse_vmstat(text: &str) -> VmStat {
    let mut v = VmStat::default();
    for line in text.lines() {
        let Some((k, n)) = line.split_once(' ') else { continue };
        let n = n.trim().parse().unwrap_or(0);
        match k {
            "pgmajfault" => v.pgmajfault = n,
            "pswpin" => v.pswpin = n,
            "pswpout" => v.pswpout = n,
            "oom_kill" => v.oom_kill = n,
            _ => {}
        }
    }
    v
}

#[derive(Default)]
pub struct MemSampler {
    prev: Option<VmStat>,
    page: u64,
}

impl MemSampler {
    pub fn new() -> Self {
        MemSampler { prev: None, page: unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64 }
    }

    pub fn sample(&mut self, elapsed: f64) -> Mem {
        let mut mem = from_meminfo(&parse_meminfo(&read("/proc/meminfo")));
        if let Ok(entries) = std::fs::read_dir("/sys/block") {
            for e in entries.flatten() {
                if !e.file_name().to_string_lossy().starts_with("zram") {
                    continue;
                }
                // orig_data_size compr_data_size mem_used_total …
                let stat = read(&e.path().join("mm_stat").to_string_lossy());
                let n: Vec<u64> = stat.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                if n.len() >= 3 {
                    mem.zram_data += n[0];
                    mem.zram_used += n[2];
                }
            }
        }
        mem.pressure = pressure("memory");
        let vm = parse_vmstat(&read("/proc/vmstat"));
        if let Some(prev) = self.prev
            && elapsed > 0.0
        {
            let rate = |a: u64, b: u64| a.saturating_sub(b) as f64 / elapsed;
            mem.major_faults = rate(vm.pgmajfault, prev.pgmajfault);
            mem.swap_in = rate(vm.pswpin, prev.pswpin) * self.page as f64;
            mem.swap_out = rate(vm.pswpout, prev.pswpout) * self.page as f64;
        }
        mem.oom_kills = vm.oom_kill;
        self.prev = Some(vm);
        mem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo() {
        let text = "MemTotal:       16000 kB\nMemFree:  1000 kB\nMemAvailable:   6000 kB\nBuffers: 100 kB\n\
                    Cached: 4000 kB\nSReclaimable: 500 kB\nSUnreclaim: 200 kB\nShmem: 300 kB\nAnonPages: 7000 kB\n\
                    PageTables: 50 kB\nKernelStack: 20 kB\nSwapTotal: 8000 kB\nSwapFree: 7000 kB\n\
                    HugePages_Total: 0\n";
        let m = from_meminfo(&parse_meminfo(text));
        assert_eq!(m.total, 16000 * 1024);
        assert_eq!(m.used, 10000 * 1024);
        assert_eq!(m.cached, 4200 * 1024);
        assert_eq!(m.anon, 7000 * 1024);
        assert_eq!(m.kernel, 270 * 1024);
        assert_eq!(m.swap_used, 1000 * 1024);
    }

    #[test]
    fn parses_vmstat() {
        let v = parse_vmstat("nr_free_pages 12\npgmajfault 99\npswpin 5\npswpout 7\noom_kill 1\n");
        assert_eq!(v, VmStat { pgmajfault: 99, pswpin: 5, pswpout: 7, oom_kill: 1 });
    }
}
