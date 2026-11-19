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
    StartingMatchmaking,
    EvaluatingOpponent,
    SkippingOpponent,
    AwaitingNextOpponent,
    AwaitingDeployment,
    DeploymentPrepared,
    InBattle,
    AwaitingReturn,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaidCommand {
    None,
    WaitForStableFrame,
    OpenAttackMenu,
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
    paused_from: Option<RaidPhase>,
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
            paused_from: None,
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
        if state != expected_home || self.phase != RaidPhase::Idle {
            return Err(RaidError::UnexpectedState);
        }
        self.paused_from = None;
        self.phase = RaidPhase::Searching;
        Ok(RaidCommand::OpenAttackMenu)
    }

    /// Feed only stabilised screen states into this method.
    pub fn observe_state(&mut self, state: BattleState) -> RaidCommand {
        if state == BattleState::Unknown {
            return RaidCommand::WaitForStableFrame;
        }
        match (self.phase, state) {
            (RaidPhase::Searching, BattleState::AttackMenu) => {
                self.phase = RaidPhase::StartingMatchmaking;
                RaidCommand::FindOpponent
            }
            (RaidPhase::StartingMatchmaking, BattleState::Matchmaking) => {
                self.phase = RaidPhase::Searching;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::StartingMatchmaking, BattleState::OpponentFound) => {
                self.phase = RaidPhase::EvaluatingOpponent;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::StartingMatchmaking, _) => RaidCommand::WaitForStableFrame,
            (RaidPhase::Searching, BattleState::Matchmaking) => RaidCommand::WaitForStableFrame,
            (RaidPhase::SkippingOpponent, BattleState::OpponentFound) => RaidCommand::SkipOpponent,
            (RaidPhase::SkippingOpponent, _) => RaidCommand::WaitForStableFrame,
            (RaidPhase::AwaitingNextOpponent, BattleState::Matchmaking) => {
                self.phase = RaidPhase::Searching;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::AwaitingNextOpponent, _) => RaidCommand::WaitForStableFrame,
            (RaidPhase::DeploymentPrepared, _) => RaidCommand::WaitForStableFrame,
            (RaidPhase::Searching, BattleState::OpponentFound) => {
                self.phase = RaidPhase::EvaluatingOpponent;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::AwaitingDeployment, BattleState::HomeBattle)
                if self.village == Village::Home => RaidCommand::WaitForStableFrame,
            (RaidPhase::AwaitingDeployment, BattleState::BuilderBattle)
                if self.village == Village::BuilderBase => RaidCommand::WaitForStableFrame,
            (RaidPhase::InBattle, BattleState::BattleWon | BattleState::BattleLost) => {
                self.phase = RaidPhase::AwaitingReturn;
                RaidCommand::ReturnToVillage
            }
            (RaidPhase::AwaitingReturn, BattleState::HomeVillage)
                if self.village == Village::Home =>
            {
                self.phase = RaidPhase::Searching;
                RaidCommand::OpenAttackMenu
            }
            (RaidPhase::AwaitingReturn, BattleState::BuilderBase)
                if self.village == Village::BuilderBase =>
            {
                self.phase = RaidPhase::Searching;
                RaidCommand::OpenAttackMenu
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
            self.phase = RaidPhase::SkippingOpponent;
            RaidCommand::SkipOpponent
        }
    }

    /// Reset a pending search transition if the Find Now click failed.
    pub fn retry_find_now(&mut self) -> Result<(), RaidError> {
        if self.phase != RaidPhase::StartingMatchmaking {
            return Err(RaidError::UnexpectedState);
        }
        self.phase = RaidPhase::Searching;
        Ok(())
    }

    /// Call after the next-opponent button click has been confirmed by the input layer.
    pub fn confirm_opponent_skipped(&mut self) -> Result<(), RaidError> {
        if self.phase != RaidPhase::SkippingOpponent {
            return Err(RaidError::UnexpectedState);
        }
        self.phase = RaidPhase::AwaitingNextOpponent;
        Ok(())
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
        self.phase = RaidPhase::DeploymentPrepared;
        Ok(RaidCommand::Deploy(plan))
    }

    /// Call only after the input executor confirms the deployment gesture was sent.
    pub fn confirm_deployment_started(&mut self, state: BattleState) -> Result<(), RaidError> {
        let expected = if self.village == Village::BuilderBase {
            BattleState::BuilderBattle
        } else {
            BattleState::HomeBattle
        };
        if self.phase != RaidPhase::DeploymentPrepared || state != expected {
            return Err(RaidError::UnexpectedState);
        }
        self.phase = RaidPhase::InBattle;
        Ok(())
    }

    /// Return to plan generation if the deployment executor failed before sending input.
    pub fn retry_deployment(&mut self) -> Result<(), RaidError> {
        if self.phase != RaidPhase::DeploymentPrepared {
            return Err(RaidError::UnexpectedState);
        }
        self.phase = RaidPhase::AwaitingDeployment;
        Ok(())
    }

    pub fn pause(&mut self) {
        if !matches!(self.phase, RaidPhase::Idle | RaidPhase::Paused) {
            self.paused_from = Some(self.phase);
            self.phase = RaidPhase::Paused;
        }
    }

    pub fn resume(&mut self, state: BattleState) -> RaidCommand {
        if self.phase != RaidPhase::Paused {
            return RaidCommand::None;
        }
        let previous = self.paused_from.take().unwrap_or(RaidPhase::Idle);
        let home_state = if self.village == Village::BuilderBase {
            BattleState::BuilderBase
        } else {
            BattleState::HomeVillage
        };
        let battle_state = if self.village == Village::BuilderBase {
            BattleState::BuilderBattle
        } else {
            BattleState::HomeBattle
        };
        match (previous, state) {
            (RaidPhase::InBattle, current) if current == battle_state => {
                self.phase = RaidPhase::InBattle;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::AwaitingReturn, BattleState::BattleWon | BattleState::BattleLost) => {
                self.phase = RaidPhase::AwaitingReturn;
                RaidCommand::ReturnToVillage
            }
            (RaidPhase::AwaitingDeployment | RaidPhase::DeploymentPrepared, current)
                if current == battle_state =>
            {
                self.phase = RaidPhase::AwaitingDeployment;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::SkippingOpponent, BattleState::OpponentFound) => {
                self.phase = RaidPhase::SkippingOpponent;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::AwaitingNextOpponent, BattleState::OpponentFound | BattleState::Matchmaking) => {
                self.phase = RaidPhase::AwaitingNextOpponent;
                RaidCommand::WaitForStableFrame
            }
            (RaidPhase::StartingMatchmaking, BattleState::AttackMenu | BattleState::Matchmaking) => {
                self.phase = RaidPhase::StartingMatchmaking;
                RaidCommand::WaitForStableFrame
            }
            (_, current) if current == home_state => {
                self.phase = RaidPhase::Searching;
                RaidCommand::OpenAttackMenu
            }
            (_, BattleState::AttackMenu) => {
                self.phase = RaidPhase::Searching;
                RaidCommand::FindOpponent
            }
            (_, BattleState::Matchmaking) => {
                self.phase = RaidPhase::Searching;
                RaidCommand::WaitForStableFrame
            }
            (_, BattleState::OpponentFound) => {
                self.phase = RaidPhase::EvaluatingOpponent;
                RaidCommand::WaitForStableFrame
            }
            _ => {
                self.paused_from = Some(previous);
                RaidCommand::WaitForStableFrame
            }
        }
    }

    pub fn stop(&mut self) -> RaidCommand {
        self.phase = RaidPhase::Idle;
        self.paused_from = None;
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
        vision::TemplateMatch,
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
        assert_eq!(raid.start(BattleState::HomeVillage), Ok(RaidCommand::OpenAttackMenu));
        assert_eq!(raid.phase(), RaidPhase::Searching);
        assert_eq!(coordinator().start(BattleState::Matchmaking), Err(RaidError::UnexpectedState));
    }

    #[test]
    fn find_now_is_not_repeated_while_the_search_screen_is_transitioning() {
        let mut raid = coordinator();
        raid.start(BattleState::HomeVillage).unwrap();
        assert_eq!(raid.observe_state(BattleState::AttackMenu), RaidCommand::FindOpponent);
        assert_eq!(raid.phase(), RaidPhase::StartingMatchmaking);
        assert_eq!(raid.observe_state(BattleState::AttackMenu), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.observe_state(BattleState::Matchmaking), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.phase(), RaidPhase::Searching);
        assert_eq!(raid.observe_state(BattleState::OpponentFound), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.phase(), RaidPhase::EvaluatingOpponent);
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
        assert_eq!(raid.phase(), RaidPhase::SkippingOpponent);
        assert_eq!(raid.observe_state(BattleState::OpponentFound), RaidCommand::WaitForStableFrame);
        raid.confirm_opponent_skipped().unwrap();
        assert_eq!(raid.phase(), RaidPhase::AwaitingNextOpponent);
        assert_eq!(raid.observe_state(BattleState::OpponentFound), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.observe_state(BattleState::Matchmaking), RaidCommand::WaitForStableFrame);
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
        let slot = TroopSlot {
            name: "sneaky_goblin".into(),
            bounds: TemplateMatch { x: 10, y: 10, width: 30, height: 30, confidence: 0.99 },
        };
        assert!(matches!(
            raid.prepare_deployment(
                BattleState::HomeBattle,
                Viewport { width: 1280, height: 720 },
                &[slot],
            ),
            Ok(RaidCommand::Deploy(_))
        ));
        assert_eq!(raid.phase(), RaidPhase::DeploymentPrepared);
        raid.confirm_deployment_started(BattleState::HomeBattle).unwrap();
        assert_eq!(raid.phase(), RaidPhase::InBattle);
        assert_eq!(raid.observe_state(BattleState::BattleWon), RaidCommand::ReturnToVillage);
        assert_eq!(raid.observe_state(BattleState::HomeVillage), RaidCommand::OpenAttackMenu);
    }

    #[test]
    fn pausing_and_resuming_during_a_battle_preserves_the_raid_phase() {
        let mut raid = coordinator();
        raid.start(BattleState::HomeVillage).unwrap();
        raid.observe_state(BattleState::OpponentFound);
        raid.evaluate_opponent(good_target());
        let slot = TroopSlot {
            name: "sneaky_goblin".into(),
            bounds: TemplateMatch { x: 10, y: 10, width: 30, height: 30, confidence: 0.99 },
        };
        raid.prepare_deployment(
            BattleState::HomeBattle,
            Viewport { width: 1280, height: 720 },
            &[slot],
        ).unwrap();
        raid.confirm_deployment_started(BattleState::HomeBattle).unwrap();
        raid.pause();
        assert_eq!(raid.phase(), RaidPhase::Paused);
        assert_eq!(raid.resume(BattleState::HomeBattle), RaidCommand::WaitForStableFrame);
        assert_eq!(raid.phase(), RaidPhase::InBattle);
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
