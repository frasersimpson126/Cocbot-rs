use crate::{
    config::LootPriority,
    loot::ResourceSnapshot,
    ocr::{OcrEngine, OcrProfile},
    vision::SearchRegion,
};
use anyhow::{Result, bail};
use image::DynamicImage;

/// Minimum loot requirements used when deciding whether a searched base is worth attacking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPolicy {
    pub minimum_gold: u64,
    pub minimum_elixir: u64,
    pub minimum_dark_elixir: u64,
    pub minimum_score_percent: u8,
    pub minimum_confidence_percent: u8,
}

impl Default for TargetPolicy {
    fn default() -> Self {
        Self {
            minimum_gold: 300_000,
            minimum_elixir: 300_000,
            minimum_dark_elixir: 2_000,
            minimum_score_percent: 45,
            minimum_confidence_percent: 85,
        }
    }
}

/// Loot read from an opponent's base. Confidence is the weakest confidence among
/// the recognised resource values, not an average that could hide one bad OCR read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetCandidate {
    pub loot: ResourceSnapshot,
    pub confidence: f32,
}

impl TargetCandidate {
    /// Parses the three vertically ordered resource counters using the same tolerant
    /// digit parser as the live HUD reader. The caller supplies the OCR confidence.
    pub fn from_ocr_text(text: &str, confidence: f32) -> Option<Self> {
        let [gold, elixir, dark_elixir] = OcrEngine::parse_hud_triplet(text)?;
        Some(Self {
            loot: ResourceSnapshot { gold, elixir, dark_elixir },
            confidence,
        })
    }

    /// Reads the opponent's three resource counters from a caller-calibrated ROI.
    /// Returns None when the crop cannot be parsed into all three values.
    pub fn from_frame(
        frame: &DynamicImage,
        ocr: &OcrEngine,
        region: SearchRegion,
    ) -> Result<Option<Self>> {
        if region.width == 0 || region.height == 0
            || region.x.saturating_add(region.width) > frame.width()
            || region.y.saturating_add(region.height) > frame.height()
        {
            bail!("Opponent loot OCR region is outside the captured frame");
        }
        let crop = frame.crop_imm(region.x, region.y, region.width, region.height);
        let result = ocr.recognise(&crop, OcrProfile::HudNumbers)?;
        Ok(Self::from_ocr_text(&result.text, result.confidence))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetRejection {
    LowConfidence,
    GoldBelowMinimum,
    ElixirBelowMinimum,
    DarkElixirBelowMinimum,
    ScoreBelowMinimum,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetEvaluation {
    pub accepted: bool,
    pub score_percent: u8,
    pub rejection: Option<TargetRejection>,
}

/// Scores a candidate against the configured loot priority and hard minimums.
/// The score is deterministic and deliberately does not make deployment decisions.
#[derive(Debug, Clone, Copy)]
pub struct TargetEvaluator {
    policy: TargetPolicy,
    priority: LootPriority,
}

impl TargetEvaluator {
    pub fn new(policy: TargetPolicy, priority: LootPriority) -> Self {
        Self { policy, priority }
    }

    pub fn evaluate(&self, candidate: TargetCandidate) -> TargetEvaluation {
        let reject = |reason| TargetEvaluation {
            accepted: false,
            score_percent: 0,
            rejection: Some(reason),
        };

        if !candidate.confidence.is_finite()
            || candidate.confidence < self.policy.minimum_confidence_percent as f32 / 100.0
        {
            return reject(TargetRejection::LowConfidence);
        }
        if candidate.loot.gold < self.policy.minimum_gold {
            return reject(TargetRejection::GoldBelowMinimum);
        }
        if candidate.loot.elixir < self.policy.minimum_elixir {
            return reject(TargetRejection::ElixirBelowMinimum);
        }
        if candidate.loot.dark_elixir < self.policy.minimum_dark_elixir {
            return reject(TargetRejection::DarkElixirBelowMinimum);
        }

        let gold = ratio(candidate.loot.gold, self.policy.minimum_gold.saturating_mul(3));
        let elixir = ratio(candidate.loot.elixir, self.policy.minimum_elixir.saturating_mul(3));
        let dark = ratio(
            candidate.loot.dark_elixir,
            self.policy.minimum_dark_elixir.saturating_mul(3),
        );
        let (wg, we, wd) = match self.priority {
            LootPriority::Gold => (0.70, 0.20, 0.10),
            LootPriority::Elixir => (0.20, 0.70, 0.10),
            LootPriority::DarkElixir => (0.15, 0.15, 0.70),
            LootPriority::Balanced => (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
        };
        let score = ((gold * wg + elixir * we + dark * wd) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let accepted = score >= self.policy.minimum_score_percent;
        TargetEvaluation {
            accepted,
            score_percent: score,
            rejection: (!accepted).then_some(TargetRejection::ScoreBelowMinimum),
        }
    }
}

fn ratio(value: u64, cap: u64) -> f32 {
    if cap == 0 {
        return 1.0;
    }
    (value.min(cap) as f64 / cap as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(gold: u64, elixir: u64, dark_elixir: u64, confidence: f32) -> TargetCandidate {
        TargetCandidate {
            loot: ResourceSnapshot { gold, elixir, dark_elixir },
            confidence,
        }
    }

    #[test]
    fn parses_opponent_loot_from_three_ocr_lines() {
        let candidate = TargetCandidate::from_ocr_text("1,200,000\n850000\n12000", 0.97).unwrap();
        assert_eq!(candidate.loot, ResourceSnapshot {
            gold: 1_200_000, elixir: 850_000, dark_elixir: 12_000,
        });
        assert_eq!(candidate.confidence, 0.97);
    }

    #[test]
    fn malformed_ocr_does_not_produce_a_candidate() {
        assert!(TargetCandidate::from_ocr_text("1,200,000\n850000", 0.99).is_none());
    }

    #[test]
    fn rejects_low_confidence_even_when_loot_is_high() {
        let evaluator = TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced);
        let result = evaluator.evaluate(candidate(2_000_000, 2_000_000, 20_000, 0.40));
        assert_eq!(result.rejection, Some(TargetRejection::LowConfidence));
    }

    #[test]
    fn rejects_candidates_below_a_hard_resource_minimum() {
        let evaluator = TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced);
        let result = evaluator.evaluate(candidate(299_999, 2_000_000, 20_000, 0.99));
        assert_eq!(result.rejection, Some(TargetRejection::GoldBelowMinimum));
    }

    #[test]
    fn accepts_a_strong_balanced_candidate() {
        let evaluator = TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced);
        let result = evaluator.evaluate(candidate(900_000, 900_000, 6_000, 0.99));
        assert!(result.accepted);
        assert!(result.score_percent >= 90);
        assert_eq!(result.rejection, None);
    }

    #[test]
    fn priority_changes_the_score_without_changing_hard_minimums() {
        let policy = TargetPolicy::default();
        let loot = candidate(1_500_000, 400_000, 2_100, 0.99);
        let gold = TargetEvaluator::new(policy, LootPriority::Gold).evaluate(loot);
        let dark = TargetEvaluator::new(policy, LootPriority::DarkElixir).evaluate(loot);
        assert!(gold.score_percent > dark.score_percent);
    }

    #[test]
    fn non_finite_confidence_is_rejected() {
        let evaluator = TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced);
        assert_eq!(
            evaluator.evaluate(candidate(1_000_000, 1_000_000, 10_000, f32::NAN)).rejection,
            Some(TargetRejection::LowConfidence)
        );
    }
}
