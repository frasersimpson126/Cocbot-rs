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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentPattern {
    /// One continuous drag around the outer perimeter, not a sequence of taps.
    PerimeterDrag,
    /// Individual drops with a fixed delay between them.
    IndividualDrops { interval_ms: u16 },
    /// Separate drops distributed across the Builder Base perimeter.
    SpreadDrops,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentPlan {
    pub strategy: ArmyStrategy,
    pub troop_slot_index: usize,
    pub troop_name: String,
    pub pattern: DeploymentPattern,
    /// For PerimeterDrag these are ordered drag waypoints. For the other patterns
    /// each point is an individual troop drop.
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

        let (pattern, points) = match strategy {
            ArmyStrategy::ElectroDragon => (
                DeploymentPattern::IndividualDrops { interval_ms: 200 },
                diamond_points(viewport, 12),
            ),
            ArmyStrategy::BuilderBaseBabyDragon => (
                DeploymentPattern::SpreadDrops,
                diamond_points(viewport, 6),
            ),
            ArmyStrategy::SneakyGoblin
            | ArmyStrategy::Valkyrie
            | ArmyStrategy::SuperMinion
            | ArmyStrategy::SuperDragon => (
                DeploymentPattern::PerimeterDrag,
                perimeter_drag_path(viewport),
            ),
        };

        Ok(DeploymentPlan {
            strategy,
            troop_slot_index: found.0,
            troop_name: found.1.name.clone(),
            pattern,
            points,
        })
    }
}

fn point(viewport: Viewport, x: f32, y: f32) -> ScreenPoint {
    ScreenPoint {
        x: (viewport.width as f32 * x).round() as u32,
        y: (viewport.height as f32 * y).round() as u32,
    }
}

/// A closed rectangular perimeter route with extra waypoints along each edge.
/// The executor must treat this as one held-button drag gesture.
fn perimeter_drag_path(viewport: Viewport) -> Vec<ScreenPoint> {
    [
        (0.12, 0.22), (0.12, 0.50), (0.12, 0.78),
        (0.30, 0.78), (0.50, 0.78), (0.70, 0.78),
        (0.88, 0.78), (0.88, 0.50), (0.88, 0.22),
        (0.70, 0.22), (0.50, 0.22), (0.30, 0.22),
        (0.12, 0.22),
    ]
    .into_iter()
    .map(|(x, y)| point(viewport, x, y))
    .collect()
}

/// Generates evenly spaced drops around a diamond-shaped deployment perimeter.
fn diamond_points(viewport: Viewport, count: usize) -> Vec<ScreenPoint> {
    if count == 0 {
        return Vec::new();
    }
    (0..count)
        .map(|index| {
            let t = index as f32 / count as f32;
            let (x, y) = if t < 0.25 {
                let p = t * 4.0;
                (0.50 + 0.36 * p, 0.50 - 0.28 * p)
            } else if t < 0.50 {
                let p = (t - 0.25) * 4.0;
                (0.86 - 0.36 * p, 0.22 + 0.56 * p)
            } else if t < 0.75 {
                let p = (t - 0.50) * 4.0;
                (0.50 - 0.36 * p, 0.78 - 0.56 * p)
            } else {
                let p = (t - 0.75) * 4.0;
                (0.14 + 0.36 * p, 0.22 - 0.56 * p)
            };
            point(viewport, x, y)
        })
        .collect()
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

    fn viewport() -> Viewport {
        Viewport { width: 1280, height: 720 }
    }

    #[test]
    fn refuses_to_plan_outside_the_matching_battle_state() {
        let result = DeploymentPlanner::default().plan(
            BattleState::Matchmaking, ArmyStrategy::SneakyGoblin, viewport(),
            &[slot("sneaky_goblin", 0.99)],
        );
        assert_eq!(result, Err(DeploymentError::WrongBattleState));
    }

    #[test]
    fn refuses_low_confidence_troop_matches() {
        let result = DeploymentPlanner::default().plan(
            BattleState::HomeBattle, ArmyStrategy::SneakyGoblin, viewport(),
            &[slot("sneaky_goblin", 0.70)],
        );
        assert_eq!(result, Err(DeploymentError::MainTroopNotRecognised));
    }

    #[test]
    fn standard_armies_use_one_perimeter_drag_path() {
        let plan = DeploymentPlanner::default().plan(
            BattleState::HomeBattle, ArmyStrategy::SneakyGoblin, viewport(),
            &[slot("sneaky_goblin", 0.99)],
        ).unwrap();
        assert_eq!(plan.pattern, DeploymentPattern::PerimeterDrag);
        assert_eq!(plan.points.len(), 13);
        assert_eq!(plan.points.first(), plan.points.last());
        assert!(plan.points.iter().all(|p| p.x < 1280 && p.y < 720));
    }

    #[test]
    fn electro_dragons_are_individual_drops_with_two_hundred_ms_spacing() {
        let plan = DeploymentPlanner::default().plan(
            BattleState::HomeBattle, ArmyStrategy::ElectroDragon, viewport(),
            &[slot("electro_dragon", 0.99)],
        ).unwrap();
        assert_eq!(plan.pattern, DeploymentPattern::IndividualDrops { interval_ms: 200 });
        assert_eq!(plan.points.len(), 12);
    }

    #[test]
    fn builder_base_strategy_requires_builder_battle_state() {
        let result = DeploymentPlanner::default().plan(
            BattleState::HomeBattle, ArmyStrategy::BuilderBaseBabyDragon, viewport(),
            &[slot("baby_dragon", 0.99)],
        );
        assert_eq!(result, Err(DeploymentError::WrongBattleState));
    }

    #[test]
    fn invalid_viewports_are_rejected() {
        let result = DeploymentPlanner::default().plan(
            BattleState::HomeBattle, ArmyStrategy::SneakyGoblin,
            Viewport { width: 320, height: 240 },
            &[slot("sneaky_goblin", 0.99)],
        );
        assert_eq!(result, Err(DeploymentError::InvalidViewport));
    }
}
