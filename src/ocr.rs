use anyhow::{Context, Result, bail};
use image::{DynamicImage, GrayImage, ImageFormat, imageops::{self, FilterType}};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Instant,
};

/// OCR profiles tuned for small game HUD text. Always crop to the smallest useful
/// region before calling Tesseract; reducing pixels is the largest speed improvement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrProfile {
    /// Three vertically stacked resource counters: gold, elixir, dark elixir.
    HudNumbers,
    /// Short labels and status text in the game UI.
    UiText,
    /// A single known text line.
    SingleLine,
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub elapsed_ms: u128,
    /// Conservative confidence: lowest recognised word confidence, normalised to 0..=1.
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct OcrEngine {
    executable: String,
    pub hud_scale: u32,
    pub ui_scale: u32,
    pub threshold: u8,
}

impl OcrEngine {
    pub fn new(executable: impl Into<String>) -> Self {
        Self {
            executable: executable.into(),
            hud_scale: 3,
            ui_scale: 1,
            threshold: 190,
        }
    }

    pub fn executable(&self) -> &str {
        &self.executable
    }

    /// Runs Tesseract over an in-memory image. The image is streamed through stdin,
    /// avoiding per-frame temporary files and filesystem contention.
    pub fn recognise(&self, image: &DynamicImage, profile: OcrProfile) -> Result<OcrResult> {
        if image.width() == 0 || image.height() == 0 {
            bail!("OCR received an empty image");
        }

        let started = Instant::now();
        let prepared = self.preprocess(image, profile);
        let mut png = std::io::Cursor::new(Vec::new());
        prepared.write_to(&mut png, ImageFormat::Png)
            .context("Could not encode OCR input image")?;

        let mut command = Command::new(&self.executable);
        command.env("OMP_THREAD_LIMIT", "1");
        command.arg("stdin").arg("stdout").arg("--oem").arg("1");
        match profile {
            OcrProfile::HudNumbers => {
                command.arg("--psm").arg("6")
                    .arg("-c").arg("tessedit_char_whitelist=0123456789l")
                    .arg("-c").arg("classify_bln_numeric_mode=1")
                    .arg("-c").arg("load_system_dawg=0")
                    .arg("-c").arg("load_freq_dawg=0")
                    .arg("-c").arg("preserve_interword_spaces=0");
            }
            OcrProfile::UiText => {
                command.arg("--psm").arg("11")
                    .arg("-c").arg("load_system_dawg=0")
                    .arg("-c").arg("load_freq_dawg=0");
            }
            OcrProfile::SingleLine => {
                command.arg("--psm").arg("7")
                    .arg("-c").arg("preserve_interword_spaces=0");
            }
        }
        // TSV provides word-level confidence without a second Tesseract invocation.
        command.arg("tsv");

        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::piped()).spawn()
            .with_context(|| format!("Could not start Tesseract at '{}'. Install Tesseract 5 or set its executable path in Settings.", self.executable))?;
        child.stdin.take().context("Tesseract stdin unavailable")?
            .write_all(png.get_ref()).context("Could not stream image to Tesseract")?;
        let output = child.wait_with_output().context("Tesseract process failed")?;
        if !output.status.success() {
            bail!("Tesseract exited with {}: {}", output.status,
                String::from_utf8_lossy(&output.stderr).trim());
        }

        let (text, confidence) = parse_tsv(&String::from_utf8_lossy(&output.stdout));
        Ok(OcrResult {
            text,
            elapsed_ms: started.elapsed().as_millis(),
            confidence,
        })
    }

    fn preprocess(&self, image: &DynamicImage, profile: OcrProfile) -> DynamicImage {
        match profile {
            OcrProfile::HudNumbers => {
                // HUD counters are light digits on dark translucent panels. Invert to
                // Tesseract's preferred dark-on-light polarity, then binarise.
                let gray = image.to_luma8();
                let mut inverted: GrayImage = gray.clone();
                imageops::invert(&mut inverted);
                for pixel in inverted.pixels_mut() {
                    pixel.0[0] = if pixel.0[0] >= self.threshold { 255 } else { 0 };
                }
                let scale = self.hud_scale.max(1);
                let enlarged = imageops::resize(
                    &inverted,
                    inverted.width().saturating_mul(scale),
                    inverted.height().saturating_mul(scale),
                    FilterType::CatmullRom,
                );
                DynamicImage::ImageLuma8(enlarged)
            }
            OcrProfile::UiText | OcrProfile::SingleLine => {
                let gray = image.to_luma8();
                let enhanced = imageops::contrast(&gray, 35.0);
                let scale = self.ui_scale.max(1);
                let resized = if scale > 1 {
                    imageops::resize(
                        &enhanced,
                        enhanced.width().saturating_mul(scale),
                        enhanced.height().saturating_mul(scale),
                        FilterType::Lanczos3,
                    )
                } else {
                    enhanced
                };
                DynamicImage::ImageLuma8(resized)
            }
        }
    }

    /// Parses digits only, tolerating thousands separators and common OCR whitespace.
    pub fn parse_amount(text: &str) -> Option<u64> {
        let digits: String = text.chars()
            .filter_map(|ch| match ch { 'l' | 'I' => Some('1'), c if c.is_ascii_digit() => Some(c), _ => None })
            .collect();
        if digits.is_empty() { None } else { digits.parse().ok() }
    }

    /// Resource HUD order is vertical: gold, elixir, dark elixir.
    pub fn parse_hud_triplet(text: &str) -> Option<[u64; 3]> {
        let values: Vec<u64> = text.lines()
            .filter_map(Self::parse_amount)
            .collect();
        (values.len() >= 3).then(|| [values[0], values[1], values[2]])
    }
}

fn parse_tsv(tsv: &str) -> (String, f32) {
    use std::collections::BTreeMap;
    let mut lines: BTreeMap<(u32, u32, u32, u32), Vec<String>> = BTreeMap::new();
    let mut weakest_confidence = 100.0_f32;
    for row in tsv.lines().skip(1) {
        let fields: Vec<&str> = row.splitn(12, '\\t').collect();
        if fields.len() != 12 || fields[0] != "5" {
            continue;
        }
        let Ok(confidence) = fields[10].parse::<f32>() else { continue };
        let word = fields[11].trim();
        if confidence < 0.0 || word.is_empty() {
            continue;
        }
        let key = (
            fields[1].parse().unwrap_or(0),
            fields[2].parse().unwrap_or(0),
            fields[3].parse().unwrap_or(0),
            fields[4].parse().unwrap_or(0),
        );
        lines.entry(key).or_default().push(word.to_owned());
        weakest_confidence = weakest_confidence.min(confidence);
    }
    let text = lines.values()
        .map(|words| words.join(" "))
        .collect::<Vec<_>>()
        .join("\\n");
    let confidence = if weakest_confidence.is_finite() {
        (weakest_confidence / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (text, confidence)
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new("tesseract")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resource_amounts_with_separators() {
        assert_eq!(OcrEngine::parse_amount("1,234 567"), Some(1_234_567));
        assert_eq!(OcrEngine::parse_amount("O / l"), Some(1));
        assert_eq!(OcrEngine::parse_amount(""), None);
    }

    #[test]
    fn parses_hud_triplet_in_vertical_order() {
        assert_eq!(
            OcrEngine::parse_hud_triplet("1,200,000\n850000\n12000"),
            Some([1_200_000, 850_000, 12_000])
        );
    }

    #[test]
    fn parses_tesseract_tsv_into_lines_and_conservative_confidence() {
        let tsv = concat!(
            "level\\tpage_num\\tblock_num\\tpar_num\\tline_num\\tword_num\\tleft\\ttop\\twidth\\theight\\tconf\\ttext\\n",
            "5\\t1\\t1\\t1\\t1\\t1\\t0\\t0\\t20\\t10\\t96.5\\t1,200,000\\n",
            "5\\t1\\t1\\t1\\t2\\t1\\t0\\t20\\t20\\t10\\t92.0\\t850000\\n",
            "5\\t1\\t1\\t1\\t3\\t1\\t0\\t40\\t20\\t10\\t98.0\\t12000\\n",
        );
        let (text, confidence) = parse_tsv(tsv);
        assert_eq!(text, "1,200,000\\n850000\\n12000");
        assert!((confidence - 0.92).abs() < 0.001);
    }

    #[test]
    fn empty_tsv_has_zero_confidence() {
        let (text, confidence) = parse_tsv("level\\tpage_num\\tblock_num\\tpar_num\\tline_num\\tword_num\\tleft\\ttop\\twidth\\theight\\tconf\\ttext\\n");
        assert!(text.is_empty());
        assert_eq!(confidence, 0.0);
    }

    #[test]
    fn empty_input_is_rejected_before_starting_tesseract() {
        let engine = OcrEngine::default();
        let empty = DynamicImage::ImageLuma8(GrayImage::new(0, 0));
        assert!(engine.recognise(&empty, OcrProfile::HudNumbers).is_err());
    }
}
