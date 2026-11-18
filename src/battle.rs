use crate::vision::{SearchRegion, TemplateMatch, TemplateMatcher};
use anyhow::Result;
use image::DynamicImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleState {
    HomeVillage,
    BuilderBase,
    Matchmaking,
    OpponentFound,
    HomeBattle,
    BuilderBattle,
    BattleWon,
    BattleLost,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateDetection {
    pub state: BattleState,
    pub confidence: f32,
    pub anchor: Option<TemplateMatch>,
}

/// Screen-state recogniser. The names and broad screen regions follow BasePilot's
/// VisionService conventions; matching assets are supplied in assets/templates.
pub struct BattleStateDetector {
    matcher: TemplateMatcher,
    threshold: f32,
}

impl BattleStateDetector {
    pub fn new(matcher: TemplateMatcher) -> Self {
        Self { matcher, threshold: 0.86 }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn detect(&self, frame: &DynamicImage) -> Result<StateDetection> {
        let bottom = Some(SearchRegion {
            x: 0,
            y: frame.height() / 2,
            width: frame.width(),
            height: frame.height() - frame.height() / 2,
        });

        // Check battle-end and in-battle anchors before home-screen controls: some
        // interface elements remain visible behind overlays.
        let candidates = [
            (&["bstar.png", "bbstar.png"][..], BattleState::BattleWon, bottom),
            (&["endbattle.png"][..], BattleState::HomeBattle, bottom),
            (&["findnow.png"][..], BattleState::Matchmaking, bottom),
            (&["attack.png", "farmbattle.png"][..], BattleState::HomeVillage, bottom),
        ];

        for (templates, state, region) in candidates {
            if let Some((_, found)) = self.matcher.find_any(frame, templates, self.threshold, region)? {
                return Ok(StateDetection { state, confidence: found.confidence, anchor: Some(found) });
            }
        }
        Ok(StateDetection { state: BattleState::Unknown, confidence: 0.0, anchor: None })
    }
}

/// Requires repeated identical detections before publishing a state. A mismatch or
/// Unknown observation immediately returns Unknown, preventing stale battle states
/// from authorising an action while the screen is transitioning.
#[derive(Debug, Clone)]
pub struct StateStabilizer {
    required_samples: u8,
    candidate: BattleState,
    consecutive: u8,
}

impl Default for StateStabilizer {
    fn default() -> Self {
        Self::new(2)
    }
}

impl StateStabilizer {
    pub fn new(required_samples: u8) -> Self {
        Self {
            required_samples: required_samples.max(1),
            candidate: BattleState::Unknown,
            consecutive: 0,
        }
    }

    pub fn reset(&mut self) {
        self.candidate = BattleState::Unknown;
        self.consecutive = 0;
    }

    pub fn observe(&mut self, detection: BattleState) -> BattleState {
        if detection == BattleState::Unknown {
            self.reset();
            return BattleState::Unknown;
        }
        if detection == self.candidate {
            self.consecutive = self.consecutive.saturating_add(1);
        } else {
            self.candidate = detection;
            self.consecutive = 1;
        }
        if self.consecutive >= self.required_samples {
            detection
        } else {
            BattleState::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TroopSlot {
    pub name: String,
    pub bounds: TemplateMatch,
}

/// Recognises configured troop/hero icons in the deployment bar. Detection is kept
/// separate from deployment so uncertain icon matches cannot trigger a click.
pub struct DeploymentBarRecognizer {
    matcher: TemplateMatcher,
    threshold: f32,
}

impl DeploymentBarRecognizer {
    pub fn new(matcher: TemplateMatcher) -> Self {
        Self { matcher, threshold: 0.90 }
    }

    pub fn recognise(
        &self,
        frame: &DynamicImage,
        troop_templates: &[&str],
    ) -> Result<Vec<TroopSlot>> {
        let bar = SearchRegion {
            x: 0,
            y: frame.height().saturating_mul(2) / 3,
            width: frame.width(),
            height: frame.height() - frame.height().saturating_mul(2) / 3,
        };
        let mut slots = Vec::new();
        for template in troop_templates {
            if let Some(found) = self.matcher.find(frame, template, self.threshold, Some(bar))? {
                slots.push(TroopSlot {
                    name: template.trim_end_matches(".png").to_owned(),
                    bounds: found,
                });
            }
        }
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_the_safe_default_for_unrecognised_screens() {
        let detection = StateDetection { state: BattleState::Unknown, confidence: 0.0, anchor: None };
        assert_eq!(detection.state, BattleState::Unknown);
        assert!(detection.anchor.is_none());
    }

    #[test]
    fn stabilizer_requires_consecutive_matching_frames() {
        let mut stable = StateStabilizer::new(3);
        assert_eq!(stable.observe(BattleState::HomeBattle), BattleState::Unknown);
        assert_eq!(stable.observe(BattleState::HomeBattle), BattleState::Unknown);
        assert_eq!(stable.observe(BattleState::HomeBattle), BattleState::HomeBattle);
    }

    #[test]
    fn unknown_frame_clears_pending_state() {
        let mut stable = StateStabilizer::new(2);
        stable.observe(BattleState::HomeBattle);
        assert_eq!(stable.observe(BattleState::Unknown), BattleState::Unknown);
        assert_eq!(stable.observe(BattleState::HomeBattle), BattleState::Unknown);
    }

    #[test]
    fn a_state_change_must_stabilise_again() {
        let mut stable = StateStabilizer::new(2);
        stable.observe(BattleState::HomeBattle);
        assert_eq!(stable.observe(BattleState::HomeBattle), BattleState::HomeBattle);
        assert_eq!(stable.observe(BattleState::Matchmaking), BattleState::Unknown);
        assert_eq!(stable.observe(BattleState::Matchmaking), BattleState::Matchmaking);
    }
}
