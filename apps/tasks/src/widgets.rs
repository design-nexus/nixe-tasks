//! Nyx-style building blocks: pages, groups and option rows.

pub use nyx_ui::widgets::*;

use gtk::pango;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

// ---------- Page / group ----------

pub struct Page {
    pub root: gtk::ScrolledWindow,
    pub body: gtk::Box,
}

/// A page. Its name shows in the top bar (or the settings dialog's header), so
/// the page itself starts straight with its content.
pub fn page(section: &str) -> Page {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("settings-page");
    body.add_css_class(&format!("page-{section}"));

    body.set_hexpand(true);

    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        // Scrolls with wheel, trackpad and keyboard; no visible scrollbar.
        .vscrollbar_policy(gtk::PolicyType::External)
        .child(&body)
        .vexpand(true)
        .build();
    Page { root, body }
}

impl Page {
    /// For pages whose content scrolls itself (tables): the page stops scrolling
    /// and hands its height to the last child.
    pub fn fill(&self) {
        self.root.set_vscrollbar_policy(gtk::PolicyType::Never);
        self.body.set_vexpand(true);
        self.body.add_css_class("fill");
    }

    pub fn group(&self, title: &str) -> Group {
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrapper.add_css_class("settings-group");
        // The settings dialog lists groups by this name.
        wrapper.set_widget_name(title);
        if !title.is_empty() {
            let l = gtk::Label::new(Some(&title.to_uppercase()));
            l.add_css_class("group-title");
            l.set_xalign(0.0);
            wrapper.append(&l);
        }
        // Rows join into one card, split by hairlines.
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.add_css_class("group-list");
        wrapper.append(&list);
        self.body.append(&wrapper);
        Group { wrapper, list }
    }
}

#[derive(Clone)]
pub struct Group {
    pub wrapper: gtk::Box,
    pub list: gtk::Box,
}

impl Group {
    pub fn add(&self, w: &impl IsA<gtk::Widget>) {
        self.list.append(w);
    }

    /// Put a small control at the right end of the group's title line.
    pub fn header_end(&self, w: &impl IsA<gtk::Widget>) {
        let Some(title) = self.wrapper.first_child() else { return };
        self.wrapper.remove(&title);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("group-head");
        title.set_hexpand(true);
        title.set_valign(gtk::Align::Center);
        row.append(&title);
        w.set_valign(gtk::Align::Center);
        row.append(w);
        self.wrapper.prepend(&row);
    }

    pub fn note(&self, text: &str) {
        let l = gtk::Label::new(None);
        l.set_markup(text);
        l.add_css_class("group-note");
        l.set_xalign(0.0);
        l.set_wrap(true);
        // Notes sit just under the group title.
        self.wrapper.insert_child_after(&l, self.wrapper.first_child().as_ref());
    }
}

// ---------- Config files and bar buttons ----------

// ---------- Rows ----------

/// A row with a number field. `on_change` gets each new value.
pub fn spin_row(
    title: &str,
    desc: &str,
    (min, max, step): (f64, f64, f64),
    value: f64,
    on_change: impl Fn(f64) + 'static,
) -> (gtk::Box, gtk::SpinButton) {
    let spin = gtk::SpinButton::with_range(min, max, step);
    spin.set_value(value);
    spin.set_valign(gtk::Align::Center);
    spin.connect_value_changed(move |s| on_change(s.value()));
    (row(title, desc, Some(spin.upcast_ref())), spin)
}

// ---------- Monitoring pieces ----------

/// A card holding a graph: title on the left, a live summary on the right.
pub fn graph_card(title: &str, graph: &impl IsA<gtk::Widget>) -> (gtk::Box, gtk::Label) {
    let card = vbox(10);
    card.add_css_class("graph-card");
    let head = hbox(10);
    let t = label(title, "graph-card-title");
    t.set_hexpand(true);
    t.set_ellipsize(pango::EllipsizeMode::End);
    let summary = label("", "dim");
    summary.add_css_class("mono");
    summary.set_xalign(1.0);
    head.append(&t);
    head.append(&summary);
    card.append(&head);
    card.append(graph);
    (card, summary)
}

/// Mark a value as past its warning threshold (or not).
pub fn set_warn(w: &impl IsA<gtk::Widget>, over: bool) {
    if over {
        w.add_css_class("warning-text");
    } else {
        w.remove_css_class("warning-text");
    }
}

/// A usage bar with its own threshold: `danger` when `over`.
pub fn warn_bar(bar: &gtk::LevelBar, over: bool) {
    if over {
        bar.add_css_class("warn");
    } else {
        bar.remove_css_class("warn");
    }
}

/// A spinner with a line of text, for content that's still loading.
pub fn loading(text: &str) -> gtk::Box {
    let b = hbox(10);
    b.add_css_class("loading");
    let spinner = gtk::Spinner::new();
    spinner.start();
    b.append(&spinner);
    b.append(&label(text, "dim"));
    b
}

/// A button that needs two clicks: the first arms it ("Click again to …"),
/// the second acts. It disarms itself after a few seconds.
pub fn two_click(label_text: &str, armed_text: &str, act: impl Fn() + 'static) -> gtk::Button {
    two_click_if(label_text, armed_text, || true, act)
}

/// Like `two_click`, but `confirm` is asked on every click; when it says no,
/// one click acts. While armed the button has the `armed` class, and its label
/// may be changed by the caller at any other time.
pub fn two_click_if(
    label_text: &str,
    armed_text: &str,
    confirm: impl Fn() -> bool + 'static,
    act: impl Fn() + 'static,
) -> gtk::Button {
    let b = gtk::Button::with_label(label_text);
    b.add_css_class("destructive-action");
    let idle = Rc::new(RefCell::new(String::new()));
    let armed_label = armed_text.to_string();
    // Each arming gets a number, so an old timer can't disarm a newer one.
    let generation = Rc::new(std::cell::Cell::new(0u32));
    b.connect_clicked(move |b| {
        if b.has_css_class("armed") {
            b.remove_css_class("armed");
            b.set_label(&idle.borrow());
            act();
            return;
        }
        if !confirm() {
            act();
            return;
        }
        *idle.borrow_mut() = b.label().map(|l| l.to_string()).unwrap_or_default();
        b.add_css_class("armed");
        b.set_label(&armed_label);
        generation.set(generation.get().wrapping_add(1));
        let (b2, idle2, gen2, mine) = (b.clone(), idle.clone(), generation.clone(), generation.get());
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || {
            if b2.has_css_class("armed") && gen2.get() == mine {
                b2.remove_css_class("armed");
                b2.set_label(&idle2.borrow());
            }
        });
    });
    b
}

