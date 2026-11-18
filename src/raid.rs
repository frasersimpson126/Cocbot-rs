use crate::{
    battle::BattleState,
    config::Village,
    deployment::{DeploymentError, DeploymentPlan, DeploymentPlanner, Viewport},
    strategy::ArmyStrategy,
    target::{TargetCandidate, TargetEvaluation, TargetEvaluator},
    battle::TroopSlot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaidPhase {
    Idle,
    Searching,
    EvaluatingOpponent,
    AwaitingDeployment,
    InBattle,
    AwaitingReturn,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaidCommand {
    None,
    WaitForStableFrame,
    FindOpponent,
    SkipOpponent,
    Deploy(DeploymentPlan),
    ReturnToVillage,
    RaidFinished,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaidError {
    WrongVillageStrategy,
    UnexpectedState,
    TargetRejected,
    Deployment(DeploymentError),
}

/// Coordinates one continuous farming loop. It produces explicit commands for the
/// backend but does not send input itself; the executor must validate each command
/// against a fresh, stabilised frame before acting.
pub struct RaidCoordinator {
    phase: RaidPhase,
    village: Village,
    strategy: ArmyStrategy,
    evaluator: TargetEvaluator,
    planner: DeploymentPlanner,
    last_evaluation: Option<TargetEvaluation>,
}

impl RaidCoordinator {
    pub fn new(
        village: Village,
        strategy: ArmyStrategy,
        evaluator: TargetEvaluator,
        planner: DeploymentPlanner,
    ) -> Result<Self, RaidError> {
        if strategy.is_builder_base() != (village == Village::BuilderBase) {
            return Err(RaidError::WrongVillageStrategy);
        }
        Ok(Self {
            phase: RaidPhase::Idle,
            village,
            strategy,
            evaluator,
            planner,
            last_evaluation: None,
        })
    }

    pub fn phase(&self) -> RaidPhase {
        self.phase
    }

    pub fn last_evaluation(&self) -> Option<TargetEvaluation> {
        self.last_evaluation
    }

    pub fn start(&mut self, state: BattleState) -> Result<RaidCommand, RaidError> {
        let expected_home = match self.village {
            Village::Home => BattleState::HomeVillage,
            Village::BuilderBase => BattleState::BuilderBase,
        };
        if state != expected_home {
            return Err(RaidError::UnexpectedState);
        }
        self.phase = RaidPhase::Searching;
        Ok(RaidCommand::FindOpponent)
    }

    /// Feed only stabilised screen states into this method.
    pub fn observe_state(&mut self, state: BattleState) -> RaidCommand {
        if state == BattleState::Unknown {
            return RaidCommand::WaitForStableFrame;
        }
        match (self.phase, state) {
            (RaidPhase::Searching, BattleState::Matchmaking) => RaidCommand::WaitForStableFrame,
            (RaidPhase::Searching, BattleState::OpponentFound) => {
                self.phase = RaidPhase::EvaluatingOpponent;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::AwaitingDeployment, BattleState::HomeBattle)
                if self.village == Village::Home =>
            {
                self.phase = RaidPhase::InBattle;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::AwaitingDeployment, BattleState::BuilderBattle)
                if self.village == Village::BuilderBase =>
            {
                self.phase = RaidPhase::InBattle;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::InBattle, BattleState::BattleWon | BattleState::BattleLost) => {
                self.phase = RaidPhase::AwaitingReturn;
                RaidCommand::ReturnToVillage
            }
            (RaidPhase::AwaitingReturn, BattleState::HomeVillage)
                if self.village == Village::Home =>
            {
                self.phase = RaidPhase::Searching;
                RaidCommand::FindOpponent
            }
            (RaidPhase::AwaitingReturn, BattleState::BuilderBase)
                if self.village == Village::BuilderBase =>
            {
                self.phase = RaidPhase::Searching;
                RaidCommand::FindOpponent
            }
            (RaidPhase::Paused, _) => RaidCommand::WaitForStableFrame,
            (_, BattleState::Unknown) => RaidCommand::WaitForStableFrame,
            _ => RaidCommand::None,
        }
    }

    pub fn evaluate_opponent(&mut self, candidate: TargetCandidate) -> RaidCommand {
        if self.phase != RaidPhase::EvaluatingOpponent {
            return RaidCommand::None;
        }
        let evaluation = self.evaluator.evaluate(candidate);
        self.last_evaluation = Some(evaluation);
        if evaluation.accepted {
            self.phase = RaidPhase::AwaitingDeployment;
            // The backend must call prepare_deployment once troop slots are recognised.
            RaidCommand::WaitForStableFrame
        } else {
            self.phase = RaidPhase::Searching;
            RaidCommand::SkipOpponent
        }
    }

    pub fn prepare_deployment(
        &mut self,
        state: BattleState,
        viewport: Viewport,
        slots: &[TroopSlot],
    ) -> Result<RaidCommand, RaidError> {
        if self.phase != RaidPhase::AwaitingDeployment {
            return Ok(RaidCommand::None);
        }
        let expected = if self.village == Village::BuilderBase {
            BattleState::BuilderBattle
        } else {
            BattleState::HomeBattle
        };
        // A deployment plan is generated only after the game has entered a verified
        // battle state and the configured main troop has been recognised.
        if state != expected {
            return Err(RaidError::UnexpectedState);
        }
        let plan = self.planner.plan(state, self.strategy, viewport, slots)
            .map_err(RaidError::Deployment)?;
        Ok(RaidCommand::Deploy(plan))
    }

    pub fn pause(&mut self) {
        self.phase = RaidPhase::Paused;
    }

    pub fn resume(&mut self, state: BattleState) -> RaidCommand {
        if self.phase != RaidPhase::Paused {
            return RaidCommand::None;
        }
        let expected_home = if self.village == Village::BuilderBase {
            BattleState::BuilderBase
        } else {
            BattleState::HomeVillage
        };
        if state == expected_home {
            self.phase = RaidPhase::Searching;
            RaidCommand::FindOpponent
        } else {
            RaidCommand::WaitForStableFrame
        }
    }

    pub fn stop(&mut self) -> RaidCommand {
        self.phase = RaidPhase::Idle;
        self.last_evaluation = None;
        RaidCommand::RaidFinished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::LootPriority,
        loot::ResourceSnapshot,
        target::TargetPolicy,
    };

    fn coordinator() -> RaidCoordinator {
        RaidCoordinator::new(
            Village::Home,
            ArmyStrategy::SneakyGoblin,
            TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced),
            DeploymentPlanner::default(),
        ).unwrap()
    }

    fn good_target() -> TargetCandidate {
        TargetCandidate {
            loot: ResourceSnapshot { gold: 900_000, elixir: 900_000, dark_elixir: 6_000 },
            confidence: 0.99,
        }
    }

    #[test]
    fn starts_only_from_the_correct_village() {
        let mut raid = coordinator();
        assert_eq!(raid.start(BattleState::HomeVillage), Ok(RaidCommand::FindOpponent));
        assert_eq!(raid.phase(), RaidPhase::Searching);
        assert_eq!(coordinator().start(BattleState::Matchmaking), Err(RaidError::UnexpectedState));
    }

    #[test]
    fn rejected_target_skips_and_returns_to_searching() {
        let mut raid = coordinator();
        raid.start(BattleState::HomeVillage).unwrap();
        raid.observe_state(BattleState::OpponentFound);
        let bad = TargetCandidate {
            loot: ResourceSnapshot { gold: 10, elixir: 10, dark_elixir: 0 },
            confidence: 0.99,
        };
        assert_eq!(raid.evaluate_opponent(bad), RaidCommand::SkipOpponent);
        assert_eq!(raid.phase(), RaidPhase::Searching);
    }

    #[test]
    fn accepted_target_waits_for_a_verified_battle_and_troop_bar() {
        let mut raid = coordinator();
        raid.start(BattleState::HomeVillage).unwrap();
        raid.observe_state(BattleState::OpponentFound);
        assert_eq!(raid.evaluate_opponent(good_target()), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.phase(), RaidPhase::AwaitingDeployment);
        assert_eq!(
            raid.prepare_deployment(BattleState::Matchmaking, Viewport { width: 1280, height: 720 }, &[]),
            Err(RaidError::UnexpectedState),
        );
    }

    #[test]
    fn unknown_detection_never_advances_the_raid() {
        let mut raid = coordinator();
        raid.start(BattleState::HomeVillage).unwrap();
        assert_eq!(raid.observe_state(BattleState::Unknown), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.phase(), RaidPhase::Searching);
    }

    #[test]
    fn battle_completion_requests_return_and_then_resumes_searching() {
        let mut raid = coordinator();
        raid.start(BattleState::HomeVillage).unwrap();
        raid.observe_state(BattleState::OpponentFound);
        raid.evaluate_opponent(good_target());
        raid.observe_state(BattleState::HomeBattle);
        assert_eq!(raid.phase(), RaidPhase::InBattle);
        assert_eq!(raid.observe_state(BattleState::BattleWon), RaidCommand::ReturnToVillage);
        assert_eq!(raid.observe_state(BattleState::HomeVillage), RaidCommand::FindOpponent);
    }

    #[test]
    fn wrong_village_strategy_is_rejected() {
        let result = RaidCoordinator::new(
            Village::BuilderBase,
            ArmyStrategy::SneakyGoblin,
            TargetEvaluator::new(TargetPolicy::default(), LootPriority::Balanced),
            DeploymentPlanner::default(),
        );
        assert!(matches!(result, Err(RaidError::WrongVillageStrategy)));
    }
}
