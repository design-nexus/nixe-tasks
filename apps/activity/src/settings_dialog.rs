//! Settings as a card over the window: the groups listed on the left, the
//! chosen group's options on the right, and a search across all of them.

use crate::sections::settings;
use crate::{widgets, window};
use gtk::prelude::*;
use nyx_ui::settings_card::{self, Spec};

pub use nyx_ui::settings_card::{escape, fit, focus_search, is_open};

/// Each settings group's icon and one-line summary in the dialog's list.
const GROUPS: &[(&str, &str, &str)] = &[
    ("This window", "applications-graphics-symbolic", "Theme, glow, motion"),
    ("Monitoring", "utilities-system-monitor-symbolic", "Update rate, history, units"),
    ("Warnings", "dialog-warning-symbolic", "Temperature, memory, disk"),
    ("Alerts", "preferences-system-notifications-symbolic", "Hot CPU, low memory, failures"),
    ("Processes", "view-list-symbolic", "Ending tasks, kernel threads"),
    ("Keyboard", "input-keyboard-symbolic", "Shortcuts"),
];

const SPEC: Spec = Spec { groups: GROUPS, search_text: None };

pub fn open() {
    let (Some(overlay), Some(window)) = (window::overlay(), window::window()) else { return };
    settings_card::open(&overlay, &window, SPEC, || {
        let page = widgets::page("settings");
        settings::build(&page);
        (page.root.upcast(), page.body)
    });
}
