use anyhow::{Context, Result, bail};
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use image::DynamicImage;
use std::{thread, time::Duration};
use xcap::Window;

/// Captures the Google Play Games window and sends physical mouse input to its screen
/// coordinates. The game must be visible and not minimised.
pub struct GameWindow {
    window: Window,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl GameWindow {
    pub fn find() -> Result<Self> {
        let windows = Window::all().context("Could not enumerate desktop windows")?;
        for window in windows {
            let title = window.title().unwrap_or_default().to_lowercase();
            if !title.contains("clash of clans") || window.is_minimized().unwrap_or(true) {
                continue;
            }
            let x = window.x().context("Could not read game window X position")?;
            let y = window.y().context("Could not read game window Y position")?;
            let width = window.width().context("Could not read game window width")?;
            let height = window.height().context("Could not read game window height")?;
            if width < 640 || height < 480 {
                continue;
            }
            return Ok(Self { window, x, y, width, height });
        }
        bail!("Clash of Clans window not found. Open the game in Google Play Games and keep it visible.")
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn capture(&self) -> Result<DynamicImage> {
        let frame = self.window.capture_image().context("Game window capture failed")?;
        if frame.width() == 0 || frame.height() == 0 {
            bail!("Game window capture returned an empty frame");
        }
        Ok(DynamicImage::ImageRgba8(frame))
    }

    fn absolute(&self, x: i32, y: i32) -> (i32, i32) {
        let local_x = x.clamp(0, self.width.saturating_sub(1) as i32);
        let local_y = y.clamp(0, self.height.saturating_sub(1) as i32);
        (self.x + local_x, self.y + local_y)
    }

    pub fn move_to(&self, input: &mut Enigo, x: i32, y: i32) -> Result<()> {
        let (x, y) = self.absolute(x, y);
        input.move_mouse(x, y, Coordinate::Abs)
            .context("Could not move mouse to game window")?;
        Ok(())
    }

    /// Clicks relative to the captured game window. Coordinates are clamped to its bounds.
    pub fn click(&self, input: &mut Enigo, x: i32, y: i32) -> Result<()> {
        self.move_to(input, x, y)?;
        input.button(Button::Left, Direction::Click)
            .context("Could not click game window")?;
        Ok(())
    }

    /// Performs one continuous held-button drag through the supplied local coordinates.
    /// The left button is released even if an intermediate mouse move fails.
    pub fn drag_path(
        &self,
        input: &mut Enigo,
        points: &[(i32, i32)],
        waypoint_delay: Duration,
    ) -> Result<()> {
        if points.len() < 2 {
            bail!("A drag path requires at least two points");
        }
        self.move_to(input, points[0].0, points[0].1)?;
        input.button(Button::Left, Direction::Press)
            .context("Could not press mouse button for drag")?;

        let mut movement_error = None;
        for &(x, y) in &points[1..] {
            if let Err(error) = self.move_to(input, x, y) {
                movement_error = Some(error);
                break;
            }
            if !waypoint_delay.is_zero() {
                thread::sleep(waypoint_delay);
            }
        }

        let release_result = input.button(Button::Left, Direction::Release)
            .context("Could not release mouse button after drag");
        if let Some(error) = movement_error {
            let _ = release_result;
            return Err(error);
        }
        release_result?;
        Ok(())
    }
}

pub fn create_input() -> Result<Enigo> {
    Enigo::new(&Settings::default()).context("Could not initialise desktop input")
}
