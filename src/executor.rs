use crate::{
    battle::BattleState,
    config::Village,
    deployment::{DeploymentPattern, DeploymentPlan},
    platform::GameWindow,
    vision::{SearchRegion, TemplateMatcher},
    raid::RaidCommand,
};
use anyhow::{Result, bail};
use enigo::Enigo;
use image::DynamicImage;
use std::{thread, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentReceipt {
    pub troop_name: String,
    pub actions_sent: usize,
}

/// Executes an already-recognised deployment plan. Callers must pass a freshly
/// stabilised battle state; the executor independently checks village, confidence,
/// and every coordinate before sending any mouse input.
pub struct DeploymentExecutor {
    minimum_troop_confidence: f32,
}

impl Default for DeploymentExecutor {
    fn default() -> Self {
        Self { minimum_troop_confidence: 0.92 }
    }
}

impl DeploymentExecutor {
    pub fn with_minimum_troop_confidence(mut self, confidence: f32) -> Self {
        self.minimum_troop_confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn execute(
        &self,
        window: &GameWindow,
        input: &mut Enigo,
        state: BattleState,
        plan: &DeploymentPlan,
    ) -> Result<DeploymentReceipt> {
        let (width, height) = window.dimensions();
        self.validate(state, width, height, plan)?;

        // Select the recognised troop slot before executing its strategy-specific
        // deployment gesture. A short pause lets the game register the selection.
        let troop_x = plan.troop_bounds.x + plan.troop_bounds.width / 2;
        let troop_y = plan.troop_bounds.y + plan.troop_bounds.height / 2;
        window.click(input, troop_x as i32, troop_y as i32)?;
        thread::sleep(Duration::from_millis(120));

        let actions_sent = match plan.pattern {
            DeploymentPattern::PerimeterDrag => {
                let points: Vec<_> = plan.points.iter()
                    .map(|p| (p.x as i32, p.y as i32))
                    .collect();
                window.drag_path(input, &points, Duration::from_millis(45))?;
                1
            }
            DeploymentPattern::IndividualDrops { interval_ms } => {
                click_points(window, input, &plan.points, Duration::from_millis(interval_ms as u64))?
            }
            DeploymentPattern::SpreadDrops => {
                click_points(window, input, &plan.points, Duration::from_millis(180))?
            }
        };

        Ok(DeploymentReceipt {
            troop_name: plan.troop_name.clone(),
            actions_sent,
        })
    }

    pub fn validate(
        &self,
        state: BattleState,
        width: u32,
        height: u32,
        plan: &DeploymentPlan,
    ) -> Result<()> {
        let expected_state = if plan.strategy.is_builder_base() {
            BattleState::BuilderBattle
        } else {
            BattleState::HomeBattle
        };
        if state != expected_state {
            bail!("Deployment refused: current battle state does not match the selected village");
        }
        let expected_troop = match plan.strategy {
            crate::strategy::ArmyStrategy::SneakyGoblin => "sneaky_goblin",
            crate::strategy::ArmyStrategy::Valkyrie => "valkyrie",
            crate::strategy::ArmyStrategy::SuperMinion => "super_minion",
            crate::strategy::ArmyStrategy::ElectroDragon => "electro_dragon",
            crate::strategy::ArmyStrategy::SuperDragon => "super_dragon",
            crate::strategy::ArmyStrategy::BuilderBaseBabyDragon => "baby_dragon",
        };
        if !plan.troop_name.to_ascii_lowercase().contains(expected_troop) {
            bail!("Deployment refused: recognised troop does not match the selected army strategy");
        }
        let pattern_matches = matches!(
            (plan.strategy, plan.pattern),
            (crate::strategy::ArmyStrategy::ElectroDragon, DeploymentPattern::IndividualDrops { .. })
                | (crate::strategy::ArmyStrategy::BuilderBaseBabyDragon, DeploymentPattern::SpreadDrops)
                | (crate::strategy::ArmyStrategy::SneakyGoblin, DeploymentPattern::PerimeterDrag)
                | (crate::strategy::ArmyStrategy::Valkyrie, DeploymentPattern::PerimeterDrag)
                | (crate::strategy::ArmyStrategy::SuperMinion, DeploymentPattern::PerimeterDrag)
                | (crate::strategy::ArmyStrategy::SuperDragon, DeploymentPattern::PerimeterDrag)
        );
        if !pattern_matches {
            bail!("Deployment refused: deployment pattern is incompatible with the selected strategy");
        }
        if width < 640 || height < 480 {
            bail!("Deployment refused: game viewport is too small");
        }
        if !plan.troop_bounds.confidence.is_finite()
            || plan.troop_bounds.confidence < self.minimum_troop_confidence
            || plan.troop_bounds.width == 0
            || plan.troop_bounds.height == 0
        {
            bail!("Deployment refused: troop slot recognition confidence is insufficient");
        }
        let right = plan.troop_bounds.x.saturating_add(plan.troop_bounds.width);
        let bottom = plan.troop_bounds.y.saturating_add(plan.troop_bounds.height);
        if right > width || bottom > height {
            bail!("Deployment refused: troop slot lies outside the game viewport");
        }
        let margin_x = width / 20;
        let margin_y = height / 10;
        if plan.points.is_empty()
            || plan.points.iter().any(|p| {
                p.x < margin_x || p.x >= width.saturating_sub(margin_x)
                    || p.y < margin_y || p.y >= height.saturating_sub(margin_y)
            })
        {
            bail!("Deployment refused: deployment path is empty or outside the safe play area");
        }
        if matches!(plan.pattern, DeploymentPattern::PerimeterDrag)
            && (plan.points.len() < 2 || plan.points.first() != plan.points.last())
        {
            bail!("Deployment refused: perimeter drag requires a closed path with at least two waypoints");
        }
        Ok(())
    }
}

fn click_points(
    window: &GameWindow,
    input: &mut Enigo,
    points: &[crate::deployment::ScreenPoint],
    interval: Duration,
) -> Result<usize> {
    for (index, point) in points.iter().enumerate() {
        window.click(input, point.x as i32, point.y as i32)?;
        if index + 1 < points.len() && !interval.is_zero() {
            thread::sleep(interval);
        }
    }
    Ok(points.len())
}

/// Recognises and clicks the result-screen return control. A result-state check and
/// a fresh template match are both required; the star/defeat marker itself is never
/// used as a click target.
pub struct ReturnHomeExecutor {
    matcher: TemplateMatcher,
    threshold: f32,
}

impl ReturnHomeExecutor {
    pub fn new(matcher: TemplateMatcher) -> Self {
        Self { matcher, threshold: 0.90 }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn execute(
        &self,
        window: &GameWindow,
        input: &mut Enigo,
        frame: &DynamicImage,
        village: Village,
        result_state: BattleState,
    ) -> Result<()> {
        if !matches!(result_state, BattleState::BattleWon | BattleState::BattleLost) {
            bail!("Return-to-village refused: battle result has not been recognised");
        }
        let names: &[&str] = if village == Village::BuilderBase {
            &["bbreturnhome.png", "returnhome.png"]
        } else {
            &["returnhome.png"]
        };
        let bottom = SearchRegion {
            x: 0,
            y: frame.height() / 2,
            width: frame.width(),
            height: frame.height() - frame.height() / 2,
        };
        let Some((_, button)) = self.matcher.find_any(frame, names, self.threshold, Some(bottom))? else {
            bail!("Return-to-village button was not recognised with sufficient confidence");
        };
        let (width, height) = window.dimensions();
        if button.x.saturating_add(button.width) > width
            || button.y.saturating_add(button.height) > height
        {
            bail!("Return-to-village button match lies outside the game window");
        }
        window.click(
            input,
            (button.x + button.width / 2) as i32,
            (button.y + button.height / 2) as i32,
        )
    }
}

/// Handles the menu-level transitions around matchmaking. Each action requires the
/// exact expected screen state and a fresh, high-confidence button template match.
pub struct RaidNavigationExecutor {
    matcher: TemplateMatcher,
    threshold: f32,
}

impl RaidNavigationExecutor {
    pub fn new(matcher: TemplateMatcher) -> Self {
        Self { matcher, threshold: 0.90 }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn execute(
        &self,
        window: &GameWindow,
        input: &mut Enigo,
        frame: &DynamicImage,
        village: Village,
        state: BattleState,
        command: &RaidCommand,
    ) -> Result<()> {
        let (names, expected_state, region) = match command {
            RaidCommand::OpenAttackMenu => {
                let names: &[&str] = if village == Village::BuilderBase {
                    &["bbattack.png"]
                } else {
                    &["attack.png", "farmbattle.png"]
                };
                (names, if village == Village::BuilderBase { BattleState::BuilderBase } else { BattleState::HomeVillage }, None)
            }
            RaidCommand::FindOpponent => {
                let names: &[&str] = if village == Village::BuilderBase {
                    &["bbfindnow.png"]
                } else {
                    &["findnow.png"]
                };
                (names, BattleState::AttackMenu, Some(bottom_region(frame)))
            }
            RaidCommand::SkipOpponent => {
                let names: &[&str] = if village == Village::BuilderBase {
                    &["bbnext.png", "next.png"]
                } else {
                    &["next.png"]
                };
                (names, BattleState::OpponentFound, Some(bottom_region(frame)))
            }
            _ => bail!("Command is not a matchmaking navigation action"),
        };
        if state != expected_state {
            bail!("Navigation refused: current screen does not match the requested action");
        }
        let Some((_, button)) = self.matcher.find_any(frame, names, self.threshold, region)? else {
            bail!("Navigation button was not recognised with sufficient confidence");
        };
        let (width, height) = window.dimensions();
        if button.x.saturating_add(button.width) > width
            || button.y.saturating_add(button.height) > height
        {
            bail!("Navigation button match lies outside the game window");
        }
        window.click(
            input,
            (button.x + button.width / 2) as i32,
            (button.y + button.height / 2) as i32,
        )
    }
}

fn bottom_region(frame: &DynamicImage) -> SearchRegion {
    SearchRegion {
        x: 0,
        y: frame.height() / 2,
        width: frame.width(),
        height: frame.height() - frame.height() / 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        deployment::{DeploymentPattern, DeploymentPlan, ScreenPoint},
        strategy::ArmyStrategy,
        vision::TemplateMatch,
    };

    fn plan() -> DeploymentPlan {
        DeploymentPlan {
            strategy: ArmyStrategy::SneakyGoblin,
            troop_slot_index: 0,
            troop_name: "sneaky_goblin".into(),
            troop_bounds: TemplateMatch {
                x: 10, y: 10, width: 30, height: 30, confidence: 0.99,
            },
            pattern: DeploymentPattern::PerimeterDrag,
            points: vec![ScreenPoint { x: 100, y: 100 }, ScreenPoint { x: 200, y: 100 }, ScreenPoint { x: 100, y: 100 }],
        }
    }

    #[test]
    fn refuses_wrong_battle_state() {
        assert!(DeploymentExecutor::default()
            .validate(BattleState::Matchmaking, 1280, 720, &plan()).is_err());
    }

    #[test]
    fn refuses_low_confidence_troop_recognition() {
        let mut p = plan();
        p.troop_bounds.confidence = 0.70;
        assert!(DeploymentExecutor::default()
            .validate(BattleState::HomeBattle, 1280, 720, &p).is_err());
    }

    #[test]
    fn refuses_out_of_bounds_deployment_points() {
        let mut p = plan();
        p.points.push(ScreenPoint { x: 1280, y: 200 });
        assert!(DeploymentExecutor::default()
            .validate(BattleState::HomeBattle, 1280, 720, &p).is_err());
    }

    #[test]
    fn accepts_valid_home_village_deployment() {
        assert!(DeploymentExecutor::default()
            .validate(BattleState::HomeBattle, 1280, 720, &plan()).is_ok());
    }

    #[test]
    fn refuses_home_village_strategy_in_builder_battle() {
        assert!(DeploymentExecutor::default()
            .validate(BattleState::BuilderBattle, 1280, 720, &plan()).is_err());
    }
}
