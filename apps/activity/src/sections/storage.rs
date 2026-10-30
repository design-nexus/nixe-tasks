use super::top::{self, By};
use crate::charts::{self, PAIR};
use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::disk::{Disk, Mount};
use crate::widgets::{self, Page};
use crate::{cmd, fmt, live};
use gtk::pango;
use gtk::prelude::*;

fn kind(d: &Disk) -> &'static str {
    if d.name.starts_with("nvme") {
        "NVMe SSD"
    } else if d.removable {
        "Removable"
    } else if d.rotational {
        "Hard disk"
    } else {
        "SSD"
    }
}

/// A mounted filesystem: how full it is as a ring, with its path and free space.
fn mount_card(m: &Mount) -> (gtk::Box, charts::Ring, gtk::Label) {
    let card = widgets::hbox(14);
    card.add_css_class("graph-card");
    let ring = charts::ring(72);
    card.append(&ring.root);
    let text = widgets::vbox(2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let path = widgets::label(&m.path, "graph-card-title");
    path.set_ellipsize(pango::EllipsizeMode::Middle);
    text.append(&path);
    let detail = widgets::label("", "dim");
    detail.add_css_class("tnum");
    text.append(&detail);
    text.append(&widgets::label(&format!("{} · {}", m.fs, m.device.rsplit('/').next().unwrap_or(&m.device)), "stat-sub"));
    card.append(&text);
    let open = gtk::Button::from_icon_name("folder-open-symbolic");
    open.add_css_class("flat");
    open.set_valign(gtk::Align::Center);
    open.set_tooltip_text(Some("Open in the file manager"));
    let target = m.path.clone();
    open.connect_clicked(move |_| cmd::spawn(&["xdg-open", &target]));
    card.append(&open);
    card.set_tooltip_text(Some(&format!("{} · {}", m.device, m.fs)));
    (card, ring, detail)
}

pub fn build(page: &Page) {
    let holder = widgets::vbox(0);
    page.body.append(&holder);
    let page = Page { root: page.root.clone(), body: holder.clone() };
    live::ready(move |snap| {
        if snap.disks.is_empty() {
            holder.append(&widgets::banner("No disks were found in <tt>/sys/block</tt>.", true));
            return;
        }
        overall(&page);
        for d in &snap.disks {
            disk_section(&page, d);
        }
    });
}

/// Every disk together, and who's reading and writing.
fn overall(page: &Page) {
    let g = page.cards("All disks");
    let read = charts::tile("Reading", Some(charts::spark("disk.read", Scale::Auto { floor: 1024.0 * 1024.0 }, fmt::rate)));
    let write = charts::tile("Writing", Some(charts::spark("disk.write", Scale::Auto { floor: 1024.0 * 1024.0 }, fmt::rate)));
    let busy = charts::tile("Busiest disk", Some(charts::spark("disk.busy", Scale::Percent, fmt::pct)));
    let psi = charts::tile("Waiting on disk", Some(charts::spark("io.psi", Scale::Auto { floor: 10.0 }, fmt::pct)));
    psi.root.set_tooltip_text(Some("How much of the time programs were held up waiting for the disks"));
    g.add(&charts::row(&[read.root.upcast_ref(), write.root.upcast_ref(), busy.root.upcast_ref(), psi.root.upcast_ref()], 150));

    let procs = charts::bar_list(8, "Nothing is reading or writing.", true);
    let (procs_card, _) = charts::card("Busiest processes", &procs.root);
    let apps = charts::bar_list(8, "Nothing is reading or writing.", true);
    let (apps_card, _) = charts::card("Busiest apps and services", &apps.root);
    g.add(&charts::row(&[procs_card.upcast_ref(), apps_card.upcast_ref()], PAIR));

    live::on_tick(&read.root, move |s| {
        read.value.set_text(&fmt::rate(s.disk_read()));
        read.sub.set_text(&format!("{} read since boot", fmt::bytes(s.disks.iter().map(|d| d.read_total as f64).sum())));
        write.value.set_text(&fmt::rate(s.disk_write()));
        write.sub.set_text(&format!("{} written since boot", fmt::bytes(s.disks.iter().map(|d| d.written_total as f64).sum())));
        busy.value.set_text(&fmt::pct(s.disk_busy()));
        let top = s.disks.iter().max_by(|a, b| a.busy.total_cmp(&b.busy));
        busy.sub.set_text(&top.map(|d| d.name.clone()).unwrap_or_default());
        match s.io_pressure {
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
    live::on_tick(&procs_card, move |s| {
        procs.set(&top::procs(s, By::Io, 8, None));
        apps.set(&top::groups(s, By::Io, 8, None));
    });
}

fn disk_section(page: &Page, d: &Disk) {
    let g = page.cards(&format!("{} · {}", d.model, d.name));
    let name = d.name.clone();

    let gr = graph::graph(
        vec![
            Series::new(format!("disk.{name}.read"), "Read", Tone::Accent),
            Series::new(format!("disk.{name}.write"), "Write", Tone::Second),
        ],
        Scale::Auto { floor: 1024.0 * 1024.0 },
        fmt::rate,
        120,
    );
    let (tp_card, tp_sum) = charts::card("Throughput", &gr.root);
    let gr = graph::graph(
        vec![
            Series::new(format!("disk.{name}.riops"), "Reads", Tone::Accent),
            Series::new(format!("disk.{name}.wiops"), "Writes", Tone::Second),
        ],
        Scale::Auto { floor: 100.0 },
        fmt::per_sec,
        120,
    );
    let (iops_card, iops_sum) = charts::card("Requests", &gr.root);
    g.add(&charts::row(&[tp_card.upcast_ref(), iops_card.upcast_ref()], PAIR));

    let active = charts::tile("Active time", Some(charts::spark(format!("disk.{name}.busy"), Scale::Percent, fmt::pct)));
    let latency =
        charts::tile("Response time", Some(charts::spark(format!("disk.{name}.lat"), Scale::Auto { floor: 1.0 }, fmt::ms)));
    let space = charts::tile("Space used", None);
    let temp = charts::tile("Temperature", None);
    let temp_ring = charts::ring(56);
    temp_ring.root.set_halign(gtk::Align::Start);
    temp_ring.root.set_margin_top(6);
    temp.root.append(&temp_ring.root);
    let size_ring = charts::ring(56);
    size_ring.root.set_halign(gtk::Align::Start);
    size_ring.root.set_margin_top(6);
    space.root.append(&size_ring.root);
    g.add(&charts::row(
        &[active.root.upcast_ref(), latency.root.upcast_ref(), space.root.upcast_ref(), temp.root.upcast_ref()],
        150,
    ));

    // ----- Filesystems on it -----
    let mut mounts = Vec::new();
    let mut cards: Vec<gtk::Widget> = Vec::new();
    for m in &d.mounts {
        let (card, ring, detail) = mount_card(m);
        cards.push(card.upcast());
        mounts.push((m.path.clone(), ring, detail));
    }
    if !cards.is_empty() {
        let refs: Vec<&gtk::Widget> = cards.iter().collect();
        g.add(&charts::row(&refs, 260));
    }

    // ----- The drive -----
    let spec = widgets::vbox(0);
    spec.add_css_class("graph-card");
    spec.add_css_class("spec-card");
    let (flow, v) = widgets::kv_flow(&["Type", "Size", "Interface", "Firmware", "Read since boot", "Written since boot"]);
    spec.append(&flow);
    g.add(&spec);
    let static_info = [
        kind(d).to_string(),
        fmt::bytes(d.size as f64),
        if d.transport.is_empty() { "–".into() } else { d.transport.clone() },
        if d.firmware.is_empty() { "–".into() } else { d.firmware.clone() },
    ];
    for (l, t) in v.iter().zip(&static_info) {
        l.set_text(t);
    }

    live::on_tick(&tp_card, move |s| {
        let Some(d) = s.disks.iter().find(|x| x.name == name) else { return };
        tp_sum.set_text(&format!("{} active", fmt::pct(d.busy)));
        iops_sum.set_text(&format!("{} total", fmt::per_sec(d.read_iops + d.write_iops)));
        active.value.set_text(&fmt::pct(d.busy));
        active.sub.set_text(&format!("↓ {} · ↑ {}", fmt::rate(d.read_bps), fmt::rate(d.write_bps)));
        latency.value.set_text(&if d.read_iops + d.write_iops > 0.0 { fmt::ms(d.latency_ms) } else { "–".into() });
        latency.sub.set_text(&format!("per request · {:.2} queued", d.queue));
        // How full the drive's filesystems are, together.
        let (used, total) = d.mounts.iter().fold((0u64, 0u64), |(u, t), m| (u + m.used(), t + m.total));
        if total > 0 {
            let f = used as f64 / total as f64;
            space.value.set_text(&fmt::bytes(used as f64));
            space.sub.set_text(&format!("of {} mounted", fmt::bytes(total as f64)));
            size_ring.set(f, &fmt::pct(f * 100.0), "", f * 100.0 >= crate::prefs::get().warn_disk);
        } else {
            space.value.set_text("–");
            space.sub.set_text("nothing mounted");
        }
        size_ring.root.set_visible(total > 0);
        match d.temp {
            Some(t) => {
                temp.value.set_text(&fmt::temp(t));
                temp.sub.set_text("controller");
                let hot = t >= crate::prefs::get().warn_temp;
                temp_ring.set((t / 85.0).clamp(0.0, 1.0), "", "", hot);
                widgets::set_warn(&temp.value, hot);
                temp_ring.root.set_visible(true);
            }
            None => {
                temp.value.set_text("–");
                temp.sub.set_text("not reported");
                temp_ring.root.set_visible(false);
            }
        }
        v[4].set_text(&fmt::bytes(d.read_total as f64));
        v[5].set_text(&fmt::bytes(d.written_total as f64));
        for (path, ring, detail) in &mounts {
            if let Some(m) = d.mounts.iter().find(|m| &m.path == path)
                && m.total > 0
            {
                let full = m.used() as f64 / m.total as f64;
                ring.set(full, &fmt::pct(full * 100.0), "", full * 100.0 >= crate::prefs::get().warn_disk);
                detail.set_text(&format!("{} free of {}", fmt::bytes(m.free as f64), fmt::bytes(m.total as f64)));
            }
        }
    });
}
