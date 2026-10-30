use crate::charts::{self, Bar, PAIR};
use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::Snapshot;
use crate::sampler::sensors::Reading;
use crate::widgets::{self, Page};
use crate::{fmt, live, prefs};
use gtk::prelude::*;

/// The limit a reading is measured against: its own, or a sensible default.
fn limit(t: &Reading) -> f64 {
    t.high.unwrap_or(100.0)
}

pub fn build(page: &Page) {
    let holder = widgets::vbox(0);
    page.body.append(&holder);
    let page = Page { root: page.root.clone(), body: holder.clone() };
    live::ready(move |snap| {
        temperatures(&page, snap);
        if !snap.sensors.fans.is_empty() {
            fans(&page, snap);
        }
        for b in &snap.sensors.batteries {
            battery(&page, &b.name);
        }
        power(&page, snap);
    });
}

/// Each chip's hottest reading.
fn hottest_by_chip(s: &Snapshot) -> Vec<&Reading> {
    let mut out: Vec<&Reading> = Vec::new();
    for t in &s.sensors.temps {
        match out.iter_mut().find(|x| x.chip == t.chip) {
            Some(x) if t.value > x.value => *x = t,
            Some(_) => {}
            None => out.push(t),
        }
    }
    out
}

fn temperatures(page: &Page, snap: &Snapshot) {
    let g = page.cards("Temperatures");
    if snap.sensors.temps.is_empty() {
        g.add(&widgets::row("No temperature sensors", "The kernel exposes no hwmon temperature inputs here.", None));
        return;
    }

    // Compact temperatures leave the history charts as the main visualization.
    let chips: Vec<String> = hottest_by_chip(snap).iter().map(|t| t.chip.clone()).take(6).collect();
    let mut readings = Vec::new();
    let mut cards: Vec<gtk::Widget> = Vec::new();
    for chip in &chips {
        let reading = charts::tile(chip);
        cards.push(reading.root.clone().upcast());
        readings.push((chip.clone(), reading));
    }
    let refs: Vec<&gtk::Widget> = cards.iter().collect();
    g.add(&charts::row(&refs, 150));

    // ----- History: the CPU, and the two hottest other sensors -----
    let cpu_graph = graph::graph(vec![Series::new("temp.cpu", "CPU", Tone::Accent)], Scale::Auto { floor: 60.0 }, fmt::temp, 120);
    let (cpu_card, cpu_sum) = charts::card("Processor", &cpu_graph.root);
    let mut others: Vec<&Reading> = hottest_by_chip(snap).into_iter().filter(|t| t.chip != "CPU").collect();
    others.sort_by(|a, b| b.value.total_cmp(&a.value));
    let series: Vec<Series> = others
        .iter()
        .take(2)
        .enumerate()
        .map(|(i, t)| {
            Series::new(format!("temp.{}.{}", t.chip, t.label), &t.chip, if i == 0 { Tone::Accent } else { Tone::Second })
        })
        .collect();
    if series.is_empty() {
        g.add(&cpu_card);
    } else {
        let gr = graph::graph(series, Scale::Auto { floor: 60.0 }, fmt::temp, 120);
        let (other_card, _) = charts::card("Hottest elsewhere", &gr.root);
        g.add(&charts::row(&[cpu_card.upcast_ref(), other_card.upcast_ref()], PAIR));
    }

    // ----- Every sensor, in two columns -----
    let n = snap.sensors.temps.len();
    let half = n.div_ceil(2);
    let left = charts::bar_list(half, "", false);
    let right = charts::bar_list(half, "", false);
    let (lc, all_sum) = charts::card("All sensors", &left.root);
    let (rc, _) = charts::card("", &right.root);
    g.add(&widgets::disclosure(
        "sensors.temperatures",
        "All temperature sensors",
        &charts::row(&[lc.upcast_ref(), rc.upcast_ref()], PAIR),
    ));

    live::on_tick(&g.list, move |s| {
        let warn_at = prefs::get().warn_temp;
        for (chip, reading) in &readings {
            let Some(t) = s.sensors.temps.iter().filter(|t| &t.chip == chip).max_by(|a, b| a.value.total_cmp(&b.value)) else {
                continue;
            };
            reading.value.set_text(&fmt::temp(t.value));
            widgets::set_warn(&reading.value, t.value >= warn_at);
            reading.sub.set_text(&match t.high {
                Some(h) => format!("{} · limit {}", t.label, fmt::temp(h)),
                None => t.label.clone(),
            });
            reading.root.set_tooltip_text(Some(&format!("{chip}: hottest reading is {} ({})", fmt::temp(t.value), t.label)));
        }
        cpu_sum.set_text(&s.sensors.cpu_temp.map(fmt::temp).unwrap_or_default());
        widgets::set_warn(&cpu_sum, s.sensors.cpu_temp.is_some_and(|t| t >= warn_at));
        let bars: Vec<Bar> = s
            .sensors
            .temps
            .iter()
            .map(|t| Bar {
                name: format!("{} · {}", t.chip, t.label),
                frac: (t.value / limit(t)).clamp(0.0, 1.0),
                value: fmt::temp(t.value),
                tip: format!(
                    "{} · {}: {}{}",
                    t.chip,
                    t.label,
                    fmt::temp(t.value),
                    t.high.map(|h| format!(" (limit {})", fmt::temp(h))).unwrap_or_default()
                ),
                warn: t.value >= warn_at,
                ..Default::default()
            })
            .collect();
        let (a, b) = bars.split_at(bars.len().div_ceil(2));
        left.set(a);
        right.set(b);
        all_sum.set_text(&format!("{} readings", bars.len()));
    });
}

fn fans(page: &Page, snap: &Snapshot) {
    let g = page.cards("Fans");
    let series: Vec<Series> = snap
        .sensors
        .fans
        .iter()
        .take(2)
        .enumerate()
        .map(|(i, f)| {
            Series::new(format!("fan.{}.{}", f.chip, f.label), &f.label, if i == 0 { Tone::Accent } else { Tone::Second })
        })
        .collect();
    let gr = graph::graph(series, Scale::Auto { floor: 3000.0 }, |v| format!("{} rpm", v.round()), 110);
    let (graph_card, graph_sum) = charts::card("Speed", &gr.root);
    let list = charts::bar_list(snap.sensors.fans.len().max(1), "", false);
    let (list_card, _) = charts::card("Each fan", &list.root);
    g.add(&graph_card);
    g.add(&widgets::disclosure("sensors.fans", "Individual fans", &list_card));
    live::on_tick(&graph_card, move |s| {
        let max = s.sensors.fans.iter().map(|f| f.value).fold(6000.0, f64::max);
        let bars: Vec<Bar> = s
            .sensors
            .fans
            .iter()
            .map(|f| Bar {
                name: format!("{} · {}", f.chip, f.label),
                frac: f.value / max,
                value: format!("{} rpm", f.value.round()),
                ..Default::default()
            })
            .collect();
        list.set(&bars);
        let spinning = s.sensors.fans.iter().filter(|f| f.value > 0.0).count();
        graph_sum.set_text(&format!("{spinning} of {} spinning", s.sensors.fans.len()));
    });
}

fn battery(page: &Page, name: &str) {
    let g = page.cards(&format!("Battery · {name}"));
    let name = name.to_string();

    let (charge, charge_sum, ring, lv) = charts::ring_card(
        "Charge",
        120,
        &[("State", None), ("Time left", None), ("Power", None), ("Charger", None), ("Stops charging at", None)],
    );
    let gr = graph::graph(vec![Series::new(format!("bat.{name}"), "Charge", Tone::Accent)], Scale::Percent, fmt::pct, 130);
    let (hist, _) = charts::card("Charge over time", &gr.root);
    g.add(&charts::primary(&charge, &hist));

    let gr = graph::graph(
        vec![Series::new(format!("bat.{name}.watts"), "Power", Tone::Accent)],
        Scale::Auto { floor: 10.0 },
        fmt::watts,
        120,
    );
    let (draw, draw_sum) = charts::card("Power draw", &gr.root);
    let health_body = widgets::vbox(12);
    let health_value = widgets::label("–", "stat-value");
    health_body.append(&health_value);
    let (legend, hv) =
        charts::legend(&[("Holds now", None), ("When new", None), ("Cycles", None), ("Voltage", None), ("Model", None)]);
    health_body.append(&legend);
    let (health, health_sum) = charts::card("Health", &health_body);
    let diagnostics = charts::row(&[draw.upcast_ref(), health.upcast_ref()], PAIR);
    g.add(&widgets::disclosure(&format!("sensors.battery.{name}"), "Battery health and power history", &diagnostics));

    live::on_tick(&charge, move |s| {
        let Some(b) = s.sensors.batteries.iter().find(|x| x.name == name) else { return };
        let low = b.percent <= 15.0 && b.status == "Discharging";
        ring.set(b.percent / 100.0, &fmt::pct(b.percent), &b.status.to_lowercase(), low);
        charge_sum.set_text(&format!("{:.1} of {:.1} Wh", b.energy_now, b.energy_full));
        lv[0].set_text(&b.status);
        lv[1].set_text(&b.hours_left.map(fmt::hours).unwrap_or_else(|| "–".into()));
        lv[2].set_text(&fmt::watts(b.watts));
        lv[3].set_text(match s.sensors.ac {
            Some(true) => "Plugged in",
            Some(false) => "Unplugged",
            None => "–",
        });
        lv[4].set_text(&b.limit.map(fmt::pct).unwrap_or_else(|| "100%".into()));
        draw_sum.set_text(&match b.status.as_str() {
            "Discharging" => format!("using {}", fmt::watts(b.watts)),
            "Charging" => format!("charging at {}", fmt::watts(b.watts)),
            _ => "not charging".into(),
        });
        match b.health() {
            Some(h) => {
                health_value.set_text(&format!("{} of original capacity", fmt::pct(h)));
                widgets::set_warn(&health_value, h < 70.0);
                health_sum.set_text(if h >= 80.0 {
                    "Good"
                } else if h >= 60.0 {
                    "Worn"
                } else {
                    "Replace soon"
                });
            }
            None => {
                health_value.set_text("–");
                health_sum.set_text("Not available");
            }
        }
        hv[0].set_text(&format!("{:.1} Wh", b.energy_full));
        hv[1].set_text(&if b.energy_design > 0.0 { format!("{:.1} Wh", b.energy_design) } else { "–".into() });
        hv[2].set_text(&b.cycles.map(|c| c.to_string()).unwrap_or_else(|| "–".into()));
        hv[3].set_text(&format!("{:.2} V", b.volts));
        let model =
            [b.model.as_str(), b.technology.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        hv[4].set_text(if model.is_empty() { "–" } else { &model });
    });
}

fn power(page: &Page, snap: &Snapshot) {
    let g = page.cards("Power");
    let charger = charts::tile("Charger");
    let profile = charts::tile("Power profile");
    let mut tiles: Vec<gtk::Widget> = vec![charger.root.clone().upcast(), profile.root.clone().upcast()];
    let ports: Vec<(String, charts::Tile)> = snap
        .sensors
        .sources
        .iter()
        .map(|p| {
            let t = charts::tile(&p.name);
            tiles.push(t.root.clone().upcast());
            (p.name.clone(), t)
        })
        .collect();
    let refs: Vec<&gtk::Widget> = tiles.iter().collect();
    g.add(&charts::row(&refs, 150));
    live::on_tick(&charger.root, move |s| {
        charger.value.set_text(match s.sensors.ac {
            Some(true) => "Plugged in",
            Some(false) => "On battery",
            None => "–",
        });
        let draw = s.sensors.batteries.iter().map(|b| b.watts).sum::<f64>();
        charger.sub.set_text(&if draw > 0.1 { format!("battery {}", fmt::watts(draw)) } else { "battery idle".into() });
        let c = &s.cpu;
        profile.value.set_text(&if c.platform_profile.is_empty() { "–".to_string() } else { capitalise(&c.platform_profile) });
        profile.sub.set_text(&if c.epp.is_empty() { c.governor.clone() } else { format!("CPU prefers {}", c.epp) });
        for (name, t) in &ports {
            let Some(p) = s.sensors.sources.iter().find(|p| &p.name == name) else { continue };
            t.value.set_text(if p.online { "Charging" } else { "Not in use" });
            t.sub.set_text(&match (p.online, p.mode.as_str()) {
                (true, m) if p.watts > 0.1 => format!("{} · {m}", fmt::watts(p.watts)),
                (true, m) => m.to_string(),
                _ => "USB Power Delivery".into(),
            });
        }
    });
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
