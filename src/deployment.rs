use crate::{
    battle::{BattleState, TroopSlot},
    strategy::ArmyStrategy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPoint {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentPlan {
    pub strategy: ArmyStrategy,
    pub troop_slot_index: usize,
    pub troop_name: String,
    pub points: Vec<ScreenPoint>,
}

/// Builds a deployment *plan* only. This module deliberately does not send mouse
/// input; live deployment stays disabled until recognition is validated on real frames.
pub struct DeploymentPlanner {
    minimum_icon_confidence: f32,
}

impl Default for DeploymentPlanner {
    fn default() -> Self {
        Self { minimum_icon_confidence: 0.92 }
    }
}

impl DeploymentPlanner {
    pub fn with_minimum_icon_confidence(mut self, confidence: f32) -> Self {
        self.minimum_icon_confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn plan(
        &self,
        state: BattleState,
        strategy: ArmyStrategy,
        viewport: Viewport,
        slots: &[TroopSlot],
    ) -> Result<DeploymentPlan, DeploymentError> {
        if viewport.width < 640 || viewport.height < 480 {
            return Err(DeploymentError::InvalidViewport);
        }
        let expected_state = if strategy.is_builder_base() {
            BattleState::BuilderBattle
        } else {
            BattleState::HomeBattle
        };
        if state != expected_state {
            return Err(DeploymentError::WrongBattleState);
        }

        let expected_name = match strategy {
            ArmyStrategy::SneakyGoblin => "sneaky_goblin",
            ArmyStrategy::Valkyrie => "valkyrie",
            ArmyStrategy::SuperMinion => "super_minion",
            ArmyStrategy::ElectroDragon => "electro_dragon",
            ArmyStrategy::SuperDragon => "super_dragon",
            ArmyStrategy::BuilderBaseBabyDragon => "baby_dragon",
        };
        let found = slots.iter().enumerate().find(|(_, slot)| {
            slot.name.to_ascii_lowercase().contains(expected_name)
                && slot.bounds.confidence.is_finite()
                && slot.bounds.confidence >= self.minimum_icon_confidence
        }).ok_or(DeploymentError::MainTroopNotRecognised)?;

        // Deployment taps stay inside a conservative inset and are spaced to avoid
        // accidental overlap. Coordinates are relative to the game window.
        let x_positions = [0.16_f32, 0.30, 0.44, 0.58, 0.72, 0.86];
        let y = if strategy.is_builder_base() { 0.78 } else { 0.82 };
        let points = x_positions.iter().map(|x| ScreenPoint {
            x: (viewport.width as f32 * x).round() as u32,
            y: (viewport.height as f32 * y).round() as u32,
        }).collect();

        Ok(DeploymentPlan {
            strategy,
            troop_slot_index: found.0,
            troop_name: found.1.name.clone(),
            points,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentError {
    InvalidViewport,
    WrongBattleState,
    MainTroopNotRecognised,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::TemplateMatch;

    fn slot(name: &str, confidence: f32) -> TroopSlot {
        TroopSlot {
            name: name.to_owned(),
            bounds: TemplateMatch { x: 10, y: 10, width: 30, height: 30, confidence },
        }
    }

    #[test]
    fn refuses_to_plan_outside_the_matching_battle_state() {
        let result = DeploymentPlanner::default().plan(
            BattleState::Matchmaking,
            ArmyStrategy::SneakyGoblin,
            Viewport { width: 1280, height: 720 },
            &[slot("sneaky_goblin", 0.99)],
        );
        assert_eq!(result, Err(DeploymentError::WrongBattleState));
    }

    #[test]
    fn refuses_low_confidence_troop_matches() {
        let result = DeploymentPlanner::default().plan(
            BattleState::HomeBattle,
            ArmyStrategy::SneakyGoblin,
            Viewport { width: 1280, height: 720 },
            &[slot("sneaky_goblin", 0.70)],
        );
        assert_eq!(result, Err(DeploymentError::MainTroopNotRecognised));
    }

    #[test]
    fn creates_in_bounds_points_for_a_recognised_home_village_troop() {
        let plan = DeploymentPlanner::default().plan(
            BattleState::HomeBattle,
            ArmyStrategy::SneakyGoblin,
            Viewport { width: 1280, height: 720 },
            &[slot("sneaky_goblin", 0.99)],
        ).unwrap();
        assert_eq!(plan.points.len(), 6);
        assert!(plan.points.iter().all(|p| p.x < 1280 && p.y < 720));
        assert_eq!(plan.troop_slot_index, 0);
    }

    #[test]
    fn builder_base_strategy_requires_builder_battle_state() {
        let result = DeploymentPlanner::default().plan(
            BattleState::HomeBattle,
            ArmyStrategy::BuilderBaseBabyDragon,
            Viewport { width: 1280, height: 720 },
            &[slot("baby_dragon", 0.99)],
        );
        assert_eq!(result, Err(DeploymentError::WrongBattleState));
    }
}
