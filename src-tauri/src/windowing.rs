use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use tauri::{utils::config::Color, App, LogicalSize, Manager, WebviewWindow, WindowEvent};

use crate::errors::{AppError, AppResult};

const DEFAULT_WIDTH: f64 = 1280.0;
const DEFAULT_HEIGHT: f64 = 744.0;
const MIN_WIDTH: f64 = 1152.0;
const MIN_HEIGHT: f64 = 670.0;
const RATIO_EPSILON: f64 = 0.008;
const WINDOW_BG: Color = Color(232, 240, 247, 255);

pub struct WindowSizingState {
    ratio: Mutex<f64>,
    last_size: Mutex<(f64, f64)>,
    adjusting: AtomicBool,
}

impl WindowSizingState {
    pub fn new() -> Self {
        Self {
            ratio: Mutex::new(DEFAULT_WIDTH / DEFAULT_HEIGHT),
            last_size: Mutex::new((DEFAULT_WIDTH, DEFAULT_HEIGHT)),
            adjusting: AtomicBool::new(false),
        }
    }

    fn set_baseline(&self, width: f64, height: f64) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }

        if let Ok(mut ratio) = self.ratio.lock() {
            *ratio = width / height;
        }

        if let Ok(mut last_size) = self.last_size.lock() {
            *last_size = (width, height);
        }
    }

    fn ratio(&self) -> f64 {
        self.ratio
            .lock()
            .map(|ratio| *ratio)
            .unwrap_or(DEFAULT_WIDTH / DEFAULT_HEIGHT)
    }

    fn last_size(&self) -> (f64, f64) {
        self.last_size
            .lock()
            .map(|size| *size)
            .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT))
    }
}

fn logical_inner_size(window: &WebviewWindow) -> Option<(f64, f64)> {
    let scale_factor = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?.to_logical::<f64>(scale_factor);
    Some((size.width, size.height))
}

fn apply_window_size(window: &WebviewWindow, width: f64, height: f64) -> AppResult<()> {
    let width = width.max(MIN_WIDTH);
    let height = height.max(MIN_HEIGHT);
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|error| {
            AppError::new(
                "WINDOW_SIZE_FAILED",
                format!("Failed to resize window: {error}"),
            )
        })
}

fn enforce_aspect_ratio(window: &WebviewWindow, state: &WindowSizingState) {
    if state.adjusting.swap(false, Ordering::SeqCst) {
        if let Some((width, height)) = logical_inner_size(window) {
            if let Ok(mut last_size) = state.last_size.lock() {
                *last_size = (width, height);
            }
        }
        return;
    }

    let Some((width, height)) = logical_inner_size(window) else {
        return;
    };
    let ratio = state.ratio();
    let (last_width, last_height) = state.last_size();

    if height <= 0.0 || ratio <= 0.0 {
        state.set_baseline(width, height);
        return;
    }

    let current_ratio = width / height;
    if (current_ratio - ratio).abs() <= RATIO_EPSILON {
        state.set_baseline(width, height);
        return;
    }

    let width_delta = (width - last_width).abs();
    let height_delta = (height - last_height).abs();
    let (target_width, target_height) = if width_delta >= height_delta {
        (
            width.max(MIN_WIDTH),
            (width / ratio).round().max(MIN_HEIGHT),
        )
    } else {
        (
            (height * ratio).round().max(MIN_WIDTH),
            height.max(MIN_HEIGHT),
        )
    };

    if (target_width - width).abs() < 1.0 && (target_height - height).abs() < 1.0 {
        state.set_baseline(width, height);
        return;
    }

    state.adjusting.store(true, Ordering::SeqCst);
    if apply_window_size(window, target_width, target_height).is_err() {
        state.adjusting.store(false, Ordering::SeqCst);
        state.set_baseline(width, height);
    }
}

pub fn install(app: &mut App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let _ = window.set_background_color(Some(WINDOW_BG));
    let app_handle = app.handle().clone();
    if let Some((width, height)) = logical_inner_size(&window) {
        let state = app_handle.state::<WindowSizingState>();
        state.set_baseline(width, height);
    }

    let window_for_events = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) => {
            let state = app_handle.state::<WindowSizingState>();
            enforce_aspect_ratio(&window_for_events, &state);
        }
        WindowEvent::CloseRequested { .. } => {
            let _ = crate::windows::bootstrap::sync_root_state_to_current_profile(None);
        }
        _ => {}
    });

    Ok(())
}
