use super::top::{self, By};
use crate::charts::{self, PAIR};
use crate::graph::{self, Scale, Series, Tone};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

pub fn build(page: &Page) {
    let g = page.cards("Memory");

    // ----- In use, and its history -----
    let (ring_card, total, ring, lv) = charts::ring_card(
        "RAM",
        124,
        &[("In use", None), ("Available", None), ("Cache", None), ("Swap in use", None), ("Total", None)],
    );
    let gr = graph::graph(
        vec![Series::new("mem", "RAM", Tone::Accent), Series::new("swap", "Swap", Tone::Second)],
        Scale::Percent,
        fmt::pct,
        150,
    );
    let (hist, hist_sum) = charts::card("History", &gr.root);
    g.add(&charts::primary(&ring_card, &hist));

    // ----- Where it goes -----
    let parts = ["Apps", "Shared and tmpfs", "Kernel", "Cache"];
    let comp = charts::stack_bar(&parts, Some("Free"));
    let (comp_card, comp_sum) = charts::card("Where it goes", &comp.root);
    comp_card.set_tooltip_text(Some(
        "Apps: memory programs asked for (heaps, stacks).\nShared and tmpfs: shared memory and files in RAM disks like /tmp.\n\
         Kernel: memory the kernel keeps for itself.\nCache: copies of files, given back as soon as apps need it.",
    ));
    g.add(&comp_card);

    live::on_tick(&ring_card, move |s| {
        let m = &s.mem;
        if m.total == 0 {
            return;
        }
        let t = m.total as f64;
        let pct = m.used as f64 / t * 100.0;
        let warn = pct >= crate::prefs::get().warn_mem;
        ring.set(m.used as f64 / t, &fmt::pct(pct), "in use", warn);
        total.set_text(&format!("{} of {}", fmt::bytes(m.used as f64), fmt::bytes(t)));
        widgets::set_warn(&total, warn);
        for (l, v) in lv.iter().zip([m.used, m.available, m.cached + m.buffers, m.swap_used, m.total]) {
            l.set_text(&fmt::bytes(v as f64));
        }
        hist_sum.set_text(&format!("{} available", fmt::bytes(m.available as f64)));
        let cache = (m.cached + m.buffers) as f64;
        let values = [m.anon as f64, m.shared as f64, m.kernel as f64, cache];
        let free = (t - values.iter().sum::<f64>()).max(0.0);
        let texts: Vec<String> = values.iter().chain([free].iter()).map(|v| fmt::bytes(*v)).collect();
        comp.set(&values, t, &texts);
        comp_sum.set_text(&format!("{} could be freed for apps", fmt::bytes(m.available as f64)));
    });

    // ----- Who's using it -----
    let apps = charts::bar_list(10, "Reading apps…", true);
    let (apps_card, apps_sum) = charts::card("Apps using the most", &apps.root);
    apps_card.set_tooltip_text(Some("Each app with all its processes, as the kernel charges it (without file cache)"));
    let procs = charts::bar_list(10, "Reading processes…", true);
    let (procs_card, procs_sum) = charts::card("Processes using the most", &procs.root);
    procs_card.set_tooltip_text(Some("Resident memory of single processes; memory they share is counted in each"));
    let consumers = widgets::consumers("memory", ("Apps", &apps_card), ("Processes", &procs_card));
    live::on_tick(&g.list, move |s| {
        let total = Some(s.mem.total as f64).filter(|t| *t > 0.0);
        let top_apps = top::groups(s, By::Mem, 10, None);
        apps.set(&top_apps);
        procs.set(&top::procs(s, By::Mem, 10, None));
        let charged: u64 = s.groups.iter().map(|g| g.memory()).sum();
        apps_sum.set_text(&match total {
            Some(t) if charged > 0 => format!("{} of RAM", fmt::pct(charged as f64 / t * 100.0)),
            _ => String::new(),
        });
        apps_sum.set_tooltip_text(Some(&format!("All {} apps and services together", s.groups.len())));
        procs_sum.set_text(&format!("{} processes", s.process_count));
    });

    // ----- Swap, compression, commit, pressure -----
    let swap = charts::tile("Swap");
    let zram = charts::tile("Compression");
    let commit = charts::tile("Promised to apps");
    let psi = charts::tile("Waiting for memory");
    commit.root.set_tooltip_text(Some(
        "Memory apps have reserved, against what the system can back with RAM and swap. Past 100% a burst of use can run out.",
    ));
    g.add(&charts::row(&[swap.root.upcast_ref(), zram.root.upcast_ref(), commit.root.upcast_ref(), psi.root.upcast_ref()], 150));
    g.add(&consumers);
    live::on_tick(&swap.root, move |s| {
        let m = &s.mem;
        if m.swap_total > 0 {
            swap.value.set_text(&fmt::pct(m.swap_used as f64 / m.swap_total as f64 * 100.0));
            swap.sub.set_text(&format!("{} of {}", fmt::bytes(m.swap_used as f64), fmt::bytes(m.swap_total as f64)));
        } else {
            swap.value.set_text("None");
            swap.sub.set_text("when RAM runs out, apps are ended");
        }
        if m.zram_used > 0 {
            let ratio = m.zram_data as f64 / m.zram_used as f64;
            zram.value.set_text(&format!("{ratio:.1}×"));
            let saved = (1.0 - 1.0 / ratio.max(1.0)) * 100.0;
            zram.sub.set_text(&format!(
                "{} held in {} · {} saved",
                fmt::bytes(m.zram_data as f64),
                fmt::bytes(m.zram_used as f64),
                fmt::pct(saved)
            ));
        } else {
            zram.value.set_text("–");
            zram.sub.set_text("no zram in use");
        }
        if m.commit_limit > 0 {
            let f = m.committed as f64 / m.commit_limit as f64;
            commit.value.set_text(&fmt::bytes(m.committed as f64));
            commit.sub.set_text(&format!("{} of {}", fmt::pct(f * 100.0), fmt::bytes(m.commit_limit as f64)));
            widgets::set_warn(&commit.value, f > 1.0);
        }
        match m.pressure {
            Some(p) => {
                psi.value.set_text(&fmt::pct(p.some10));
                psi.sub.set_text(&format!("stalled · fully {}", fmt::pct(p.full10)));
                widgets::set_warn(&psi.value, p.some10 >= 10.0);
            }
            None => {
                psi.value.set_text("–");
                psi.sub.set_text("not reported by this kernel");
            }
        }
    });

    let g = page.diagnostics("memory.pressure", "Pressure and swapping");

    // ----- Pressure and swapping over time -----
    let gr = graph::graph(
        vec![Series::new("mem.psi", "Some waiting", Tone::Accent), Series::new("mem.psi.full", "All waiting", Tone::Second)],
        Scale::Auto { floor: 10.0 },
        fmt::pct,
        120,
    );
    let (psi_card, psi_sum) = charts::card("Pressure", &gr.root);
    psi_card.set_tooltip_text(Some("How much of the time programs were held up waiting for memory to be freed"));
    let gr = graph::graph(
        vec![Series::new("mem.swapout", "Out", Tone::Accent), Series::new("mem.swapin", "In", Tone::Second)],
        Scale::Auto { floor: 1024.0 * 1024.0 },
        fmt::rate,
        120,
    );
    let (sw_card, sw_sum) = charts::card("Swapping", &gr.root);
    g.add(&charts::row(&[psi_card.upcast_ref(), sw_card.upcast_ref()], PAIR));
    live::on_tick(&psi_card, move |s| {
        let m = &s.mem;
        psi_sum.set_text(&format!("{} major faults", fmt::per_sec(m.major_faults)));
        sw_sum.set_text(&if m.oom_kills > 0 {
            format!("{} ended for lack of memory since boot", m.oom_kills)
        } else {
            format!("{} still cached in RAM", fmt::bytes(m.swap_cached as f64))
        });
        widgets::set_warn(&sw_sum, m.oom_kills > 0);
    });

    let g = page.diagnostics("memory.details", "Memory accounting");

    // ----- Swap by app, and the details -----
    let swapped = charts::bar_list(8, "Nothing is swapped out.", true);
    let (swapped_card, _) = charts::card("Swapped out", &swapped.root);
    let details = widgets::vbox(0);
    details.add_css_class("spec-card");
    let (flow, v) = widgets::kv_flow(&[
        "Active",
        "Inactive",
        "Mapped files",
        "Dirty",
        "Writeback",
        "Locked",
        "Page tables",
        "Kernel stacks",
        "Slab (reclaimable)",
        "Slab (kernel)",
        "Huge pages",
        "Buffers",
    ]);
    details.append(&flow);
    let (details_card, _) = charts::card("Details", &details);
    g.add(&charts::row(&[swapped_card.upcast_ref(), details_card.upcast_ref()], PAIR));
    live::on_tick(&details_card, move |s| {
        swapped.set(&top::groups(s, By::Swap, 8, None));
        let m = &s.mem;
        let values = [
            m.active,
            m.inactive,
            m.mapped,
            m.dirty,
            m.writeback,
            m.locked,
            m.page_tables,
            m.kernel_stack,
            m.slab_reclaimable,
            m.slab_unreclaimable,
            m.hugepages,
            m.buffers,
        ];
        for (l, v) in v.iter().zip(values) {
            l.set_text(&fmt::bytes(v as f64));
        }
    });
}
