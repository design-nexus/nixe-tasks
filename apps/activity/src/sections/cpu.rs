use super::top::{self, By};
use crate::charts::{self, PAIR};
use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::Snapshot;
use crate::sampler::cpu::CoreKind;
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

/// The temperature coretemp reports for thread `i`'s core ("Core 8").
fn core_temp(s: &Snapshot, i: usize) -> Option<f64> {
    let id = s.cpu.layout.core_ids.get(i)?;
    let label = format!("Core {id}");
    s.sensors.temps.iter().find(|t| t.chip == "CPU" && t.label == label).map(|t| t.value)
}

pub fn build(page: &Page) {
    let g = page.cards("Usage");

    // ----- Usage ring and history -----
    let (ring_card, model, ring, lv) = charts::ring_card(
        "Processor",
        124,
        &[("User", Some(0)), ("Kernel", Some(1)), ("I/O wait", Some(2)), ("Interrupts", Some(3)), ("Idle", None)],
    );
    let gr = graph::graph(
        vec![Series::new("cpu", "Total", Tone::Accent), Series::new("cpu.system", "Kernel", Tone::Second)],
        Scale::Percent,
        fmt::pct,
        150,
    );
    let (hist, hist_sum) = charts::card("History", &gr.root);
    g.add(&charts::primary(&ring_card, &hist));

    // ----- Tiles -----
    let clock = charts::tile("Clock");
    let load = charts::tile("Load");
    let psi = charts::tile("Waiting for CPU");
    let temp = charts::tile("Temperature");
    g.add(&charts::row(&[clock.root.upcast_ref(), load.root.upcast_ref(), psi.root.upcast_ref(), temp.root.upcast_ref()], 150));

    live::on_tick(&ring_card, move |s| {
        let c = &s.cpu;
        model.set_text(&c.model);
        model.set_tooltip_text(Some(&c.model));
        ring.set_parts(&[c.user / 100.0, c.system / 100.0, c.iowait / 100.0, c.irq / 100.0], &fmt::pct(c.usage), "busy");
        let idle = (100.0 - c.usage).max(0.0);
        for (l, v) in lv.iter().zip([c.user, c.system, c.iowait, c.irq, idle]) {
            l.set_text(&fmt::pct(v));
        }
        hist_sum.set_text(&format!("{} threads", c.cores.len()));

        let avg = if c.freqs.is_empty() { 0.0 } else { c.freqs.iter().sum::<f64>() / c.freqs.len() as f64 };
        let peak = c.freqs.iter().copied().fold(0.0, f64::max);
        clock.value.set_text(&fmt::mhz(avg));
        clock.sub.set_text(&format!("fastest {} · max {}", fmt::mhz(peak), fmt::mhz(c.max_freq)));
        load.value.set_text(&format!("{:.2}", c.load[0]));
        load.sub.set_text(&format!("5 min {:.2} · 15 min {:.2}", c.load[1], c.load[2]));
        widgets::set_warn(&load.value, c.load[0] > c.cores.len() as f64);
        match c.pressure {
            Some(p) => {
                psi.value.set_text(&fmt::pct(p.some10));
                psi.sub.set_text(&format!("1 min {} · 5 min {}", fmt::pct(p.some60), fmt::pct(p.some300)));
            }
            None => {
                psi.value.set_text("–");
                psi.sub.set_text("not reported by this kernel");
            }
        }
        temp.value.set_text(&s.sensors.cpu_temp.map(fmt::temp).unwrap_or_else(|| "–".into()));
        let hottest =
            (0..c.cores.len()).filter_map(|i| core_temp(s, i)).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
        temp.sub.set_text(&hottest.map(|t| format!("hottest core {}", fmt::temp(t))).unwrap_or_else(|| "package".into()));
        widgets::set_warn(&temp.value, s.sensors.cpu_temp.is_some_and(|t| t >= crate::prefs::get().warn_temp));
    });

    // ----- Who's using it -----
    let apps = charts::bar_list(8, "Nothing busy right now.", true);
    let (apps_card, apps_sum) = charts::card("Busiest apps", &apps.root);
    let procs = charts::bar_list(8, "Nothing busy right now.", true);
    let (procs_card, procs_sum) = charts::card("Busiest processes", &procs.root);
    g.add(&widgets::consumers("cpu", ("Apps", &apps_card), ("Processes", &procs_card)));
    live::on_tick(&g.list, move |s| {
        apps.set(&top::groups(s, By::Cpu, 8, None));
        procs.set(&top::procs(s, By::Cpu, 8, None));
        apps_sum.set_text(&format!("{} running now", s.cpu.running));
        procs_sum.set_text(&format!("{} processes · {} threads", s.process_count, s.thread_count));
    });

    // ----- Clock and load -----
    let max = live::latest().map_or(5000.0, |s| s.cpu.max_freq.max(1000.0));
    let gr = graph::graph(
        vec![Series::new("cpu.freq.max", "Fastest core", Tone::Accent), Series::new("cpu.freq", "Average", Tone::Second)],
        Scale::Auto { floor: max },
        fmt::mhz,
        120,
    );
    let (freq_card, freq_sum) = charts::card("Clock speed", &gr.root);
    let gr =
        graph::graph(vec![Series::new("cpu.load", "Load", Tone::Accent)], Scale::Auto { floor: 4.0 }, |v| format!("{v:.2}"), 120);
    let (load_card, load_sum) = charts::card("Load average", &gr.root);
    let pressure =
        graph::graph(vec![Series::new("cpu.psi", "Waiting", Tone::Accent)], Scale::Auto { floor: 10.0 }, fmt::pct, 120);
    let (pressure, _) = charts::card("Waiting for CPU", &pressure.root);
    let trends = widgets::vbox(12);
    trends.append(&charts::row(&[freq_card.upcast_ref(), load_card.upcast_ref()], PAIR));
    trends.append(&pressure);
    g.add(&widgets::disclosure("cpu.trends", "Clock, load and pressure history", &trends));
    live::on_tick(&freq_card, move |s| {
        let c = &s.cpu;
        freq_sum.set_text(&if c.governor.is_empty() { String::new() } else { format!("{} governor", c.governor) });
        load_sum.set_text(&format!("{} running · {} waiting on disk", c.running, c.blocked));
    });

    threads(page);
    spec(page);
}

/// Each logical processor, grouped by core type on hybrid chips.
fn threads(page: &Page) {
    let g = page.diagnostics("cpu.threads", "Each thread");
    let grid = crate::prefs::get().cpu_grid;
    g.note(if grid {
        "Busy time of every logical processor; darker is busier."
    } else {
        "Busy time of every logical processor, with its current clock."
    });
    let switch = widgets::hbox(4);
    for (label, as_grid) in [("Graphs", false), ("Grid", true)] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("chip");
        b.add_css_class("small");
        if as_grid == grid {
            b.add_css_class("selected");
        }
        b.connect_clicked(move |_| {
            if crate::prefs::get().cpu_grid != as_grid {
                crate::prefs::update(|p| p.cpu_grid = as_grid);
                crate::window::rebuild("cpu");
            }
        });
        switch.append(&b);
    }
    g.add(&switch);
    g.list.add_css_class("bare");
    g.list.set_spacing(8);
    live::ready(move |snap| thread_cells(&g, grid, snap));
}

fn thread_cells(g: &widgets::Group, grid: bool, snap: &Snapshot) {
    let threads = snap.cpu.cores.len().max(1);
    let layout = snap.cpu.layout.clone();
    let kind = |i: usize| layout.kinds.get(i).copied().unwrap_or_default();
    let mut kinds: Vec<CoreKind> = (0..threads).map(kind).collect();
    kinds.sort();
    kinds.dedup();

    let mut values = Vec::new();
    for k in kinds {
        let members: Vec<usize> = (0..threads).filter(|i| kind(*i) == k).collect();
        if k != CoreKind::Uniform {
            let max = members.iter().filter_map(|i| layout.max_freqs.get(*i)).copied().fold(0.0, f64::max);
            let head = widgets::label(&format!("{} · {} · up to {}", k.name(), members.len(), fmt::mhz(max)), "subgroup-title");
            g.add(&head);
        }
        let mut cells: Vec<gtk::Widget> = Vec::new();
        for i in members {
            let cell = widgets::vbox(2);
            cell.add_css_class(if grid { "core-tile" } else { "core-cell" });
            let name = widgets::label(&format!("CPU {i}"), "core-name");
            let value = widgets::label("", "core-value");
            value.add_css_class("tnum");
            if grid {
                cell.append(&name);
                cell.append(&value);
            } else {
                let head = widgets::hbox(6);
                name.set_hexpand(true);
                head.append(&name);
                head.append(&value);
                cell.append(&head);
                cell.append(&graph::sparkline(
                    vec![graph::Series::new(format!("cpu.{i}"), "", Tone::Accent)],
                    Scale::Percent,
                    34,
                    fmt::pct,
                ));
            }
            cells.push(cell.clone().upcast());
            values.push((i, cell, value));
        }
        let refs: Vec<&gtk::Widget> = cells.iter().collect();
        g.add(&charts::row(&refs, if grid { 58 } else { 104 }));
    }
    let anchor = g.list.clone();
    live::on_tick(&anchor, move |s| {
        for (i, cell, v) in &values {
            let i = *i;
            let busy = s.cpu.cores.get(i).copied().unwrap_or(0.0);
            let freq = s.cpu.freqs.get(i).copied().unwrap_or(0.0);
            if grid {
                v.set_text(&fmt::pct(busy));
                let level = [10.0, 30.0, 50.0, 70.0, 90.0].iter().filter(|t| busy >= **t).count();
                for (n, class) in ["heat-1", "heat-2", "heat-3", "heat-4", "heat-5"].iter().enumerate() {
                    if n + 1 == level {
                        cell.add_css_class(class);
                    } else {
                        cell.remove_css_class(class);
                    }
                }
            } else {
                v.set_text(&format!("{} · {:.1}", fmt::pct(busy), freq / 1000.0));
            }
            let split = s.cpu.core_split.get(i).copied().unwrap_or_default();
            let temp = core_temp(s, i).map(|t| format!(" · {}", fmt::temp(t))).unwrap_or_default();
            cell.set_tooltip_text(Some(&format!(
                "CPU {i}: {} busy at {}{temp}\nUser {} · kernel {} · I/O wait {}",
                fmt::pct(busy),
                fmt::mhz(freq),
                fmt::pct(split.user),
                fmt::pct(split.system + split.irq),
                fmt::pct(split.iowait),
            )));
        }
    });
}

/// The processor itself: what it is and how it's set up.
fn spec(page: &Page) {
    let g = page.diagnostics("cpu.hardware", "Processor details");
    let card = widgets::vbox(0);
    card.add_css_class("graph-card");
    card.add_css_class("spec-card");
    let (flow, v) = widgets::kv_flow(&[
        "Model",
        "Cores",
        "Clock range",
        "L1 cache",
        "L2 cache",
        "L3 cache",
        "Governor",
        "Energy preference",
        "Power profile",
        "Turbo",
        "Virtualization",
        "Context switches",
        "Interrupts",
        "New processes",
        "Uptime",
    ]);
    card.append(&flow);
    g.add(&card);
    live::on_tick(&card, move |s| {
        let c = &s.cpu;
        let l = &c.layout;
        let dash = || "–".to_string();
        v[0].set_text(&c.model);
        v[0].set_tooltip_text(Some(&c.model));
        let mut kinds: Vec<(CoreKind, usize)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (i, k) in l.kinds.iter().enumerate() {
            let core = (l.core_ids.get(i).copied().unwrap_or(i as u32), *k);
            if !seen.insert(core) {
                continue;
            }
            match kinds.iter_mut().find(|(x, _)| x == k) {
                Some((_, n)) => *n += 1,
                None => kinds.push((*k, 1)),
            }
        }
        kinds.sort();
        let cores = if kinds.iter().all(|(k, _)| *k == CoreKind::Uniform) {
            format!("{} cores", c.physical.max(1))
        } else {
            kinds.iter().map(|(k, n)| format!("{n} {}", k.short())).collect::<Vec<_>>().join(" + ")
        };
        v[1].set_text(&format!("{cores} · {} threads", c.cores.len()));
        v[2].set_text(&if c.max_freq > 0.0 { format!("{} – {}", fmt::mhz(l.min_freq), fmt::mhz(c.max_freq)) } else { dash() });
        let cache = |prefix: &str| {
            let parts: Vec<String> = l
                .caches
                .iter()
                .filter(|x| x.name.starts_with(prefix))
                .map(|x| {
                    let kind = x.name.trim_start_matches(prefix);
                    let total = fmt::bytes(x.size as f64);
                    if kind.is_empty() { total } else { format!("{total} {}", if kind == "d" { "data" } else { "instr." }) }
                })
                .collect();
            if parts.is_empty() { dash() } else { parts.join(" · ") }
        };
        v[3].set_text(&cache("L1"));
        v[4].set_text(&cache("L2"));
        v[5].set_text(&cache("L3"));
        let or_dash = |t: &str| if t.is_empty() { dash() } else { t.to_string() };
        v[6].set_text(&or_dash(&c.governor));
        v[7].set_text(&or_dash(&c.epp));
        v[8].set_text(&or_dash(&c.platform_profile));
        v[9].set_text(match c.turbo {
            Some(true) => "On",
            Some(false) => "Off",
            None => "–",
        });
        v[10].set_text(l.virtualization.unwrap_or("–"));
        v[11].set_text(&fmt::per_sec(c.ctxt_per_sec));
        v[12].set_text(&fmt::per_sec(c.intr_per_sec));
        v[13].set_text(&fmt::per_sec(c.forks_per_sec));
        v[14].set_text(&fmt::duration(c.uptime));
    });
}
