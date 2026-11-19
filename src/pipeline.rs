use crate::{
    battle::{
        BattleState, BattleStateDetector, DeploymentBarRecognizer, StateDetection,
        StateStabilizer, TroopSlot,
    },
    ocr::OcrEngine,
    target::TargetCandidate,
    vision::{SearchRegion, TemplateMatcher},
};
use anyhow::Result;
use image::DynamicImage;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FrameAnalysis {
    pub detection: StateDetection,
    pub stable_state: BattleState,
    pub opponent_loot: Option<TargetCandidate>,
    pub troop_slots: Vec<TroopSlot>,
}

/// Connects frame capture, state recognition, opponent OCR and troop-bar matching.
/// Expensive OCR and troop matching run only on the screen states where they are useful.
pub struct BattlePipeline {
    state_detector: BattleStateDetector,
    deployment_recognizer: DeploymentBarRecognizer,
    stabilizer: StateStabilizer,
    ocr: OcrEngine,
    opponent_loot_region: Option<SearchRegion>,
    troop_templates: Vec<String>,
    troop_bar_region: Option<SearchRegion>,
}

impl BattlePipeline {
    pub fn new(
        template_directory: impl Into<PathBuf>,
        ocr: OcrEngine,
        opponent_loot_region: SearchRegion,
        troop_templates: Vec<String>,
    ) -> Self {
        let directory = template_directory.into();
        Self {
            state_detector: BattleStateDetector::new(TemplateMatcher::new(directory.clone())),
            deployment_recognizer: DeploymentBarRecognizer::new(TemplateMatcher::new(directory)),
            stabilizer: StateStabilizer::default(),
            ocr,
            opponent_loot_region: Some(opponent_loot_region),
            troop_templates,
            troop_bar_region: None,
        }
    }

    /// Disables opponent-loot OCR until the crop has been calibrated against real screenshots.
    pub fn without_opponent_loot_ocr(mut self) -> Self {
        self.opponent_loot_region = None;
        self
    }

    pub fn with_troop_bar_region(mut self, region: Option<SearchRegion>) -> Self {
        self.troop_bar_region = region;
        self
    }

    pub fn with_stable_samples(mut self, samples: u8) -> Self {
        self.stabilizer = StateStabilizer::new(samples);
        self
    }

    pub fn process_frame(&mut self, frame: &DynamicImage) -> Result<FrameAnalysis> {
        let detection = self.state_detector.detect(frame)?;
        let stable_state = self.stabilizer.observe(detection.state);

        let opponent_loot = if stable_state == BattleState::OpponentFound {
            match self.opponent_loot_region {
                Some(region) => TargetCandidate::from_frame(frame, &self.ocr, region)?,
                None => None,
            }
        } else {
            None
        };

        let troop_slots = if matches!(
            stable_state,
            BattleState::HomeBattle | BattleState::BuilderBattle
        ) {
            let names: Vec<&str> = self.troop_templates.iter().map(String::as_str).collect();
            self.deployment_recognizer.recognise_in_region(frame, &names, self.troop_bar_region)?
        } else {
            Vec::new()
        };

        Ok(FrameAnalysis {
            detection,
            stable_state,
            opponent_loot,
            troop_slots,
        })
    }

    pub fn reset(&mut self) {
        self.stabilizer.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_requires_multiple_stable_frames_before_publishing_state() {
        let mut pipeline = BattlePipeline::new(
            "missing-template-directory",
            OcrEngine::default(),
            SearchRegion { x: 0, y: 0, width: 10, height: 10 },
            Vec::new(),
        ).with_stable_samples(2);
        pipeline.reset();
        // Recognition has no templates, so the first frame must remain Unknown.
        let frame = DynamicImage::new_rgba8(1280, 720);
        let analysis = pipeline.process_frame(&frame).unwrap();
        assert_eq!(analysis.stable_state, BattleState::Unknown);
        assert!(analysis.opponent_loot.is_none());
        assert!(analysis.troop_slots.is_empty());
    }
}
