use crate::{
    battle::BattleState,
    deployment::Viewport,
    pipeline::{BattlePipeline, FrameAnalysis},
    raid::{RaidCommand, RaidCoordinator, RaidError, RaidPhase},
};
use anyhow::Result;
use image::DynamicImage;

#[derive(Debug, Clone)]
pub struct AutomationFrame {
    pub perception: FrameAnalysis,
    pub command: RaidCommand,
}

/// Joins perception and raid decisions into a single per-frame orchestration API.
/// The caller remains responsible for executing returned commands and acknowledging
/// successful mouse actions; this prevents a planned click being mistaken for a
/// completed action.
pub struct AutomationEngine {
    pipeline: BattlePipeline,
    raid: RaidCoordinator,
}

impl AutomationEngine {
    pub fn new(pipeline: BattlePipeline, raid: RaidCoordinator) -> Self {
        Self { pipeline, raid }
    }

    pub fn start(&mut self, initial_state: BattleState) -> Result<RaidCommand, RaidError> {
        self.raid.start(initial_state)
    }

    pub fn phase(&self) -> RaidPhase {
        self.raid.phase()
    }

    pub fn process_frame(
        &mut self,
        frame: &DynamicImage,
        viewport: Viewport,
    ) -> Result<AutomationFrame> {
        let perception = self.pipeline.process_frame(frame)?;
        // A stable connection-error overlay pauses the coordinator. Recovery never
        // resumes farming automatically; an explicit resume is required.
        if perception.stable_state == BattleState::ConnectionError {
            self.raid.pause();
            return Ok(AutomationFrame {
                perception,
                command: RaidCommand::WaitForStableFrame,
            });
        }
        let command = match self.raid.phase() {
            // Monitoring and recognition must not implicitly start a farming session.
            // The UI must explicitly start a raid before commands can be planned.
            RaidPhase::Idle => RaidCommand::None,
            RaidPhase::EvaluatingOpponent => {
                if let Some(candidate) = perception.opponent_loot {
                    self.raid.evaluate_opponent(candidate)
                } else {
                    RaidCommand::WaitForStableFrame
                }
            }
            RaidPhase::AwaitingDeployment => {
                if matches!(
                    perception.stable_state,
                    BattleState::HomeBattle | BattleState::BuilderBattle
                ) {
                    self.raid.prepare_deployment(
                        perception.stable_state,
                        viewport,
                        &perception.troop_slots,
                    ).unwrap_or_else(|error| {
                        tracing::warn!("Deployment plan not ready: {error:?}");
                        RaidCommand::WaitForStableFrame
                    })
                } else {
                    RaidCommand::WaitForStableFrame
                }
            }
            _ => self.raid.observe_state(perception.stable_state),
        };
        Ok(AutomationFrame { perception, command })
    }

    pub fn confirm_deployment_started(&mut self, state: BattleState) -> Result<(), RaidError> {
        self.raid.confirm_deployment_started(state)
    }

    pub fn retry_deployment(&mut self) -> Result<(), RaidError> {
        self.raid.retry_deployment()
    }

    pub fn confirm_opponent_skipped(&mut self) -> Result<(), RaidError> {
        self.raid.confirm_opponent_skipped()
    }

    pub fn retry_find_now(&mut self) -> Result<(), RaidError> {
        self.raid.retry_find_now()
    }

    pub fn pause(&mut self) {
        self.raid.pause();
        self.pipeline.reset();
    }

    pub fn resume(&mut self, state: BattleState) -> RaidCommand {
        self.pipeline.reset();
        self.raid.resume(state)
    }

    pub fn stop(&mut self) -> RaidCommand {
        self.pipeline.reset();
        self.raid.stop()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        battle::BattleState,
        config::{LootPriority, Village},
        deployment::DeploymentPlanner,
        ocr::OcrEngine,
        raid::RaidCoordinator,
        strategy::ArmyStrategy,
        target::{TargetEvaluator, TargetPolicy},
        vision::SearchRegion,
    };

    #[test]
    fn orchestration_waits_when_the_screen_cannot_be_recognised() {
        let pipeline = BattlePipeline::new(
            "missing-template-directory",
            OcrEngine::default(),
            SearchRegion { x: 0, y: 0, width: 10, height: 10 },
            Vec::new(),
        );
        let raid = RaidCoordinator::new(
            Village::Home,
            ArmyStrategy::SneakyGoblin,
            TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced),
            DeploymentPlanner::default(),
        ).unwrap();
        let mut engine = AutomationEngine::new(pipeline, raid);
        assert_eq!(engine.start(BattleState::HomeVillage), Ok(RaidCommand::OpenAttackMenu));
        let frame = DynamicImage::new_rgba8(1280, 720);
        let result = engine.process_frame(&frame, Viewport { width: 1280, height: 720 }).unwrap();
        assert_eq!(result.perception.stable_state, BattleState::Unknown);
        assert_eq!(result.command, RaidCommand::WaitForStableFrame);
    }
}
