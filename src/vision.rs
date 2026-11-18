use anyhow::{Context, Result, bail};
use image::{imageops, DynamicImage, GrayImage};
use std::path::{Path, PathBuf};

/// Coordinates are relative to the captured game window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemplateMatch {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Similarity in the range 0.0..=1.0.
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Lightweight, resolution-independent template matcher.
///
/// Templates are authored against a 2560px-wide reference image and stored as PNGs
/// in assets/templates. Both the frame and template are reduced before matching so
/// matching cost stays bounded on high-resolution desktop captures.
pub struct TemplateMatcher {
    directory: PathBuf,
    reference_width: u32,
}

impl TemplateMatcher {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self { directory: directory.into(), reference_width: 2560 }
    }

    pub fn with_reference_width(mut self, width: u32) -> Self {
        self.reference_width = width.max(1);
        self
    }

    pub fn has_template(&self, name: &str) -> bool {
        self.template_path(name).is_file()
    }

    pub fn find(
        &self,
        frame: &DynamicImage,
        name: &str,
        threshold: f32,
        region: Option<SearchRegion>,
    ) -> Result<Option<TemplateMatch>> {
        if !(0.0..=1.0).contains(&threshold) {
            bail!("Template threshold must be between 0 and 1");
        }
        let path = self.template_path(name);
        let template = image::open(&path)
            .with_context(|| format!("Could not load template {}", path.display()))?
            .to_luma8();
        let (fw, fh) = (frame.width(), frame.height());
        let region = region.unwrap_or(SearchRegion { x: 0, y: 0, width: fw, height: fh });
        if region.width == 0 || region.height == 0
            || region.x.saturating_add(region.width) > fw
            || region.y.saturating_add(region.height) > fh
        {
            bail!("Template search region is outside the captured frame");
        }

        let sx = fw as f32 / self.reference_width as f32;
        let scaled_w = ((template.width() as f32 * sx).round() as u32).max(1);
        let scaled_h = ((template.height() as f32 * sx).round() as u32).max(1);
        let scale = 4_u32;
        let tw = (scaled_w / scale).max(1);
        let th = (scaled_h / scale).max(1);
        let small_template = imageops::resize(&template, tw, th, imageops::FilterType::Triangle);

        let gray = frame.to_luma8();
        let crop = imageops::crop_imm(&gray, region.x, region.y, region.width, region.height).to_image();
        let search = imageops::resize(
            &crop,
            (region.width / scale).max(1),
            (region.height / scale).max(1),
            imageops::FilterType::Triangle,
        );
        if tw > search.width() || th > search.height() {
            return Ok(None);
        }

        let mut best_error = f32::MAX;
        let mut best_xy = None;
        let step = 1_u32;
        for y in (0..=search.height() - th).step_by(step as usize) {
            for x in (0..=search.width() - tw).step_by(step as usize) {
                let mut error = 0_u64;
                for ty in 0..th {
                    for tx in 0..tw {
                        let a = search.get_pixel(x + tx, y + ty)[0] as i16;
                        let b = small_template.get_pixel(tx, ty)[0] as i16;
                        error += (a - b).unsigned_abs() as u64;
                    }
                }
                let mean_error = error as f32 / (tw * th) as f32 / 255.0;
                if mean_error < best_error {
                    best_error = mean_error;
                    best_xy = Some((x, y));
                }
            }
        }

        let Some((x, y)) = best_xy else { return Ok(None) };
        let confidence = (1.0 - best_error).clamp(0.0, 1.0);
        if confidence < threshold {
            return Ok(None);
        }
        Ok(Some(TemplateMatch {
            x: region.x + x * scale,
            y: region.y + y * scale,
            width: scaled_w,
            height: scaled_h,
            confidence,
        }))
    }

    pub fn find_any(
        &self,
        frame: &DynamicImage,
        names: &[&str],
        threshold: f32,
        region: Option<SearchRegion>,
    ) -> Result<Option<(String, TemplateMatch)>> {
        let mut best: Option<(String, TemplateMatch)> = None;
        for name in names {
            if !self.template_path(name).exists() { continue; }
            if let Some(found) = self.find(frame, name, threshold, region)? {
                if best.as_ref().is_none_or(|(_, current)| found.confidence > current.confidence) {
                    best = Some(((*name).to_owned(), found));
                }
            }
        }
        Ok(best)
    }

    fn template_path(&self, name: &str) -> PathBuf {
        // Prevent template names from escaping the configured template directory.
        let file = Path::new(name).file_name().unwrap_or_else(|| std::ffi::OsStr::new(""));
        self.directory.join(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    #[test]
    fn detects_exact_template_in_frame() {
        let mut frame = GrayImage::from_pixel(80, 60, Luma([0]));
        let template = GrayImage::from_fn(8, 8, |x, y| {
            if (x + y) % 2 == 0 { Luma([255]) } else { Luma([40]) }
        });
        imageops::replace(&mut frame, &template, 20, 16);
        let dir = std::env::temp_dir().join(format!("cocbot-template-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        template.save(dir.join("sample.png")).unwrap();
        let matcher = TemplateMatcher::new(&dir).with_reference_width(80);
        let result = matcher.find(
            &DynamicImage::ImageLuma8(frame),
            "sample.png",
            0.95,
            None,
        ).unwrap();
        assert!(result.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }
}
