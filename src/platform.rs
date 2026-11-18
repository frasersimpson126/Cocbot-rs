use anyhow::{Context, Result, bail};
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use image::DynamicImage;
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

    /// Clicks relative to the captured game window. Coordinates are clamped to its bounds.
    pub fn click(&self, input: &mut Enigo, x: i32, y: i32) -> Result<()> {
        let local_x = x.clamp(0, self.width.saturating_sub(1) as i32);
        let local_y = y.clamp(0, self.height.saturating_sub(1) as i32);
        input.move_mouse(self.x + local_x, self.y + local_y, Coordinate::Abs)
            .context("Could not move mouse to game window")?;
        input.button(Button::Left, Direction::Click)
            .context("Could not click game window")?;
        Ok(())
    }
}

pub fn create_input() -> Result<Enigo> {
    Enigo::new(&Settings::default()).context("Could not initialise desktop input")
}
