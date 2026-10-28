//! Well-known locations. Every path honours the XDG overrides so the whole app
//! can be pointed at a scratch copy of `~/.config` for testing.

pub use nyx_core::paths::*;

use std::path::PathBuf;

pub fn state_file() -> PathBuf {
    app_dir().join("state.json")
}

pub fn hypr_dir() -> PathBuf {
    config_home().join("hypr")
}

/// The single Hyprland file this app writes (startup items it manages).
pub fn managed_lua() -> PathBuf {
    hypr_dir().join("activity.lua")
}

pub fn hyprland_lua() -> PathBuf {
    hypr_dir().join("hyprland.lua")
}

pub fn user_autostart_lua() -> PathBuf {
    hypr_dir().join("autostart.lua")
}

pub fn omarchy_autostart_lua() -> PathBuf {
    let root = std::env::var_os("OMARCHY_PATH").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
    root.join("default/hypr/autostart.lua")
}

pub fn user_xdg_autostart() -> PathBuf {
    config_home().join("autostart")
}

pub fn system_xdg_autostart() -> PathBuf {
    PathBuf::from("/etc/xdg/autostart")
}

