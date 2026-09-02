use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{
    LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent,
};

use crate::portable;

pub const MIN_WIDTH: u32 = 800;
pub const MIN_HEIGHT: u32 = 520;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

static LAST: Mutex<Option<WindowState>> = Mutex::new(None);
static SAVE_GEN: AtomicU64 = AtomicU64::new(0);

pub fn attach(window: &WebviewWindow) {
    if let Some(saved) = load() {
        apply(window, &saved);
        set_last(saved);
    }
    let win = window.clone();
    let _ = window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            schedule_save(&win);
        }
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
            save_now(&win);
        }
        _ => {}
    });
}

fn load() -> Option<WindowState> {
    let path = portable::window_file().ok()?;
    let bytes = fs::read(path).ok()?;
    let mut state: WindowState = serde_json::from_slice(&bytes).ok()?;
    let (w, h) = clamp_size(state.width, state.height);
    state.width = w;
    state.height = h;
    Some(state)
}

fn save(state: &WindowState) {
    let Ok(path) = portable::window_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let Ok(data) = serde_json::to_vec_pretty(state) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, data).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn apply(window: &WebviewWindow, state: &WindowState) {
    let (width, height) = clamp_size(state.width, state.height);
    let _ = window.set_size(tauri::Size::Logical(LogicalSize::new(
        f64::from(width),
        f64::from(height),
    )));
    if position_on_a_monitor(window, state.x, state.y, width, height) {
        let _ = window.set_position(tauri::Position::Logical(LogicalPosition::new(
            f64::from(state.x),
            f64::from(state.y),
        )));
    }
    if state.maximized {
        let _ = window.maximize();
    }
}

fn capture(window: &WebviewWindow) -> Option<WindowState> {
    if window.is_minimized().ok()? || window.is_fullscreen().ok()? {
        return last();
    }
    let previous = last();
    if window.is_maximized().ok()? {
        let mut state = previous.unwrap_or_else(default_state);
        state.maximized = true;
        return Some(state);
    }
    let scale = window.scale_factor().ok()?.max(0.1);
    let size: PhysicalSize<u32> = window.inner_size().ok()?;
    let pos: PhysicalPosition<i32> = window.outer_position().ok()?;
    let (width, height) = clamp_size(
        (f64::from(size.width) / scale).round() as u32,
        (f64::from(size.height) / scale).round() as u32,
    );
    Some(WindowState {
        x: (f64::from(pos.x) / scale).round() as i32,
        y: (f64::from(pos.y) / scale).round() as i32,
        width,
        height,
        maximized: false,
    })
}

fn schedule_save(window: &WebviewWindow) {
    let gen = SAVE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let win = window.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        if SAVE_GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        save_now(&win);
    });
}

fn save_now(window: &WebviewWindow) {
    let Some(state) = capture(window) else {
        return;
    };
    set_last(state.clone());
    save(&state);
}

fn last() -> Option<WindowState> {
    LAST.lock().ok().and_then(|g| g.clone())
}

fn set_last(state: WindowState) {
    if let Ok(mut g) = LAST.lock() {
        *g = Some(state);
    }
}

fn default_state() -> WindowState {
    WindowState {
        x: 80,
        y: 80,
        width: 1120,
        height: 780,
        maximized: false,
    }
}

pub fn clamp_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(MIN_WIDTH), height.max(MIN_HEIGHT))
}

fn position_on_a_monitor(window: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
    let px = (f64::from(x) * scale).round() as i32;
    let py = (f64::from(y) * scale).round() as i32;
    let pw = (f64::from(width) * scale).round() as i32;
    let ph = (f64::from(height) * scale).round() as i32;
    monitors.iter().any(|m| {
        rects_overlap(
            px,
            py,
            pw,
            ph,
            m.position().x,
            m.position().y,
            m.size().width as i32,
            m.size().height as i32,
        )
    })
}

fn rects_overlap(ax: i32, ay: i32, aw: i32, ah: i32, bx: i32, by: i32, bw: i32, bh: i32) -> bool {
    ax < bx.saturating_add(bw)
        && ax.saturating_add(aw) > bx
        && ay < by.saturating_add(bh)
        && ay.saturating_add(ah) > by
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_enforces_minimum() {
        assert_eq!(clamp_size(100, 100), (800, 520));
        assert_eq!(clamp_size(1200, 900), (1200, 900));
    }

    #[test]
    fn overlap_detects_shared_area() {
        assert!(rects_overlap(0, 0, 800, 600, 100, 100, 800, 600));
        assert!(!rects_overlap(0, 0, 800, 600, 2000, 0, 800, 600));
    }

    #[test]
    fn roundtrip_json() {
        let state = WindowState {
            x: 40,
            y: -8,
            width: 1280,
            height: 720,
            maximized: true,
        };
        let bytes = serde_json::to_vec(&state).unwrap();
        let back: WindowState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(state, back);
    }
}
