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
        // StateDetection is deliberately explicit: absence of a known anchor must
        // never be interpreted as permission to deploy.
        let detection = StateDetection { state: BattleState::Unknown, confidence: 0.0, anchor: None };
        assert_eq!(detection.state, BattleState::Unknown);
        assert!(detection.anchor.is_none());
    }
}
