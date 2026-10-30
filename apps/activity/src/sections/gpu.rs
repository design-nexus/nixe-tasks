use super::top::{self, By};
use crate::charts::{self, Bar, PAIR};
use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::gpu::{Gpu, Power, Vendor};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

pub fn build(page: &Page) {
    let holder = widgets::vbox(0);
    page.body.append(&holder);
    let page = Page { root: page.root.clone(), body: holder.clone() };
    live::ready(move |snap| {
        if snap.gpus.is_empty() {
            holder.append(&widgets::banner(
                "No GPU was found under <tt>/sys/class/drm</tt>, or sampling is paused while the window is hidden.",
                false,
            ));
            return;
        }
        for g in &snap.gpus {
            gpu_section(&page, g);
        }
    });
}

/// "video-enhance" -> "Video enhance".
fn engine_name(e: &str) -> String {
    let e = e.replace(['-', '_'], " ");
    let mut c = e.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn gpu_section(page: &Page, g: &Gpu) {
    let group = page.cards(&g.name);
    let pci = g.pci.clone();
    let vendor = g.vendor;

    // Honest state card for a GPU we deliberately don't poll.
    let asleep_desc = if vendor == Vendor::Nvidia {
        "It's powered down to save battery. Asking it for statistics would wake it, so Activity waits until an app starts using it."
    } else {
        "It's powered down to save energy."
    };
    let idle_desc = "Awake but unused. Activity doesn't poll it now, so it can go back to sleep.";
    let sleep_row = widgets::row("Asleep", asleep_desc, None);
    sleep_row.add_css_class("graph-card");
    group.add(&sleep_row);

    // ----- Usage -----
    let (usage, usage_sum, ring, lv) = charts::ring_card(
        "Usage",
        120,
        &[("Clock", None), ("Memory", None), ("Temperature", None), ("Power", None), ("Apps using it", None)],
    );
    let gr = graph::graph(vec![Series::new(format!("gpu.{pci}.util"), "Busy", Tone::Accent)], Scale::Percent, fmt::pct, 140);
    let (hist, hist_sum) = charts::card("History", &gr.root);
    let row1 = charts::row(&[usage.upcast_ref(), hist.upcast_ref()], PAIR);
    group.add(&row1);

    // ----- Engines and clock -----
    let engines = charts::bar_list(5, "No app is using it.", false);
    let (eng_card, eng_sum) = charts::card("Engines", &engines.root);
    eng_card
        .set_tooltip_text(Some("How busy each part of the GPU is: 3D (render), video decoding and encoding, copying, compute"));
    let floor = g.max_clock.unwrap_or(1000.0).max(100.0);
    let gr =
        graph::graph(vec![Series::new(format!("gpu.{pci}.freq"), "Clock", Tone::Accent)], Scale::Auto { floor }, fmt::mhz, 110);
    let (freq_card, freq_sum) = charts::card("Clock speed", &gr.root);
    let gr = graph::graph(vec![Series::new(format!("gpu.{pci}.vram"), "Used", Tone::Accent)], Scale::Percent, fmt::pct, 110);
    let (vram_card, vram_sum) = charts::card("Video memory", &gr.root);
    let row2 = charts::row(&[eng_card.upcast_ref(), freq_card.upcast_ref(), vram_card.upcast_ref()], PAIR);
    group.add(&row2);

    // ----- Who's using it -----
    let busy = charts::bar_list(8, "No app is using it.", true);
    let (busy_card, _) =
        charts::card(if vendor == Vendor::Nvidia { "Apps using it, by video memory" } else { "Busiest apps" }, &busy.root);
    let mem = charts::bar_list(8, "No app holds GPU memory.", true);
    let (mem_card, mem_sum) = charts::card("GPU memory by app", &mem.root);
    let row3 = charts::row(&[busy_card.upcast_ref(), mem_card.upcast_ref()], PAIR);
    group.add(&row3);

    // ----- The card itself -----
    let spec = widgets::vbox(0);
    spec.add_css_class("graph-card");
    spec.add_css_class("spec-card");
    let (flow, v) = widgets::kv_flow(&["Driver", "PCI address", "Clock range", "Power limit", "Power saving", "Vendor"]);
    spec.append(&flow);
    group.add(&spec);
    v[0].set_text(if g.driver.is_empty() { "–" } else { &g.driver });
    v[1].set_text(&g.pci);
    v[5].set_text(match vendor {
        Vendor::Intel => "Intel",
        Vendor::Nvidia => "NVIDIA",
        Vendor::Amd => "AMD",
        Vendor::Other => "–",
    });

    live::on_tick(&group.list, move |s| {
        let Some(g) = s.gpus.iter().find(|x| x.pci == pci) else { return };
        let asleep = g.power_state == Power::Asleep;
        let idle = g.power_state == Power::Idle;
        sleep_row.set_visible(asleep || idle);
        // The row's text column holds the title, then the description.
        let text = sleep_row.first_child();
        if let Some(t) = text.as_ref().and_then(|c| c.first_child()).and_downcast::<gtk::Label>() {
            t.set_text(if asleep { "Asleep" } else { "Idle" });
        }
        if let Some(d) = text.as_ref().and_then(|c| c.last_child()).and_downcast::<gtk::Label>() {
            d.set_text(if asleep { asleep_desc } else { idle_desc });
        }
        // Nothing to chart for a GPU we aren't asking.
        let polled = !asleep && !(vendor == Vendor::Nvidia && idle);
        row1.set_visible(polled);
        row2.set_visible(polled);
        let dash = || "–".to_string();

        let util = g.util.unwrap_or(0.0);
        ring.set(util / 100.0, &fmt::pct(util), "busy", util >= 90.0);
        usage_sum.set_text(&match g.power_state {
            Power::Asleep => "asleep".into(),
            Power::Idle => "idle".into(),
            Power::Active => g.temp.map(fmt::temp).unwrap_or_default(),
        });
        hist_sum.set_text(&g.util.map(fmt::pct).unwrap_or_default());
        lv[0].set_text(&match (g.act_clock.or(g.clock), g.max_clock) {
            (Some(c), Some(m)) if m > 0.0 => format!("{} / {}", fmt::mhz(c), fmt::mhz(m)),
            (Some(c), _) => fmt::mhz(c),
            _ => dash(),
        });
        lv[1].set_text(&match (g.vram_used, g.vram_total, g.client_mem) {
            (Some(u), Some(t), _) => format!("{} / {}", fmt::bytes(u as f64), fmt::bytes(t as f64)),
            (_, _, Some(m)) => format!("{} shared", fmt::bytes(m as f64)),
            _ if vendor == Vendor::Intel => "Shared with RAM".into(),
            _ => dash(),
        });
        lv[2].set_text(&g.temp.map(fmt::temp).unwrap_or_else(dash));
        lv[3].set_text(&g.watts.map(fmt::watts).unwrap_or_else(dash));
        lv[4].set_text(&g.clients.to_string());

        // Engines (from DRM fdinfo; NVIDIA doesn't report them there).
        eng_card.set_visible(vendor != Vendor::Nvidia);
        let bars: Vec<Bar> = g
            .engines
            .iter()
            .map(|(name, pct)| Bar { name: engine_name(name), frac: pct / 100.0, value: fmt::pct(*pct), ..Default::default() })
            .collect();
        engines.set(&bars);
        eng_sum.set_text(&g.rc6.map(|r| format!("{} power saving", fmt::pct(r))).unwrap_or_default());
        freq_card.set_visible(g.act_clock.is_some() || g.clock.is_some());
        freq_sum.set_text(&g.max_clock.map(|m| format!("max {}", fmt::mhz(m))).unwrap_or_default());
        vram_card.set_visible(g.vram_total.is_some());
        if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
            vram_sum.set_text(&format!("{} of {}", fmt::bytes(u as f64), fmt::bytes(t as f64)));
        }

        // Who's using it.
        let users: Vec<Bar> = if vendor == Vendor::Nvidia {
            // Compute apps report their video memory; graphics apps only that they hold it open.
            let procs = s.procs.as_deref().unwrap_or(&[]);
            let mem_of = |pid: i32| g.proc_mem.iter().find(|(p, _)| *p == pid).map(|(_, m)| *m);
            let mut list: Vec<_> = procs.iter().filter(|p| p.nvidia || mem_of(p.pid).is_some()).collect();
            list.sort_by_key(|p| std::cmp::Reverse(mem_of(p.pid).unwrap_or(0)));
            let max = list.first().and_then(|p| mem_of(p.pid)).unwrap_or(1).max(1) as f64;
            list.into_iter()
                .take(8)
                .map(|p| {
                    let (_, icon) = top::app_identity(&p.name, &[p.pid], s);
                    let m = mem_of(p.pid);
                    Bar {
                        name: p.name.clone(),
                        icon,
                        frac: m.map_or(0.0, |m| m as f64 / max),
                        value: m.map_or("open".into(), |m| fmt::bytes(m as f64)),
                        pid: p.pid,
                        ..Default::default()
                    }
                })
                .collect()
        } else {
            top::procs(s, By::Gpu, 8, Some(100.0))
        };
        busy.set(&users);
        let held = top::procs(s, By::GpuMem, 8, None);
        mem_card.set_visible(vendor != Vendor::Nvidia);
        mem_sum.set_text(&g.client_mem.map(|m| format!("{} in all", fmt::bytes(m as f64))).unwrap_or_default());
        mem.set(&held);

        v[2].set_text(&match g.max_clock {
            Some(m) => format!("up to {}", fmt::mhz(m)),
            None => dash(),
        });
        v[3].set_text(&g.watts_limit.map(fmt::watts).unwrap_or_else(dash));
        v[4].set_text(&match (g.rc6, g.power_state) {
            (Some(r), _) => format!("{} of the time", fmt::pct(r)),
            (None, Power::Asleep) => "Asleep".into(),
            _ => dash(),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_engines() {
        assert_eq!(engine_name("video-enhance"), "Video enhance");
        assert_eq!(engine_name("render"), "Render");
    }
}
