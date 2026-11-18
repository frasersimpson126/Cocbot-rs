use serde::{Deserialize, Serialize};

/// Attack routines documented by the upstream BasePilot project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArmyStrategy {
    #[default]
    SneakyGoblin,
    Valkyrie,
    SuperMinion,
    ElectroDragon,
    SuperDragon,
    BuilderBaseBabyDragon,
}

impl ArmyStrategy {
    pub const ALL: [Self; 6] = [
        Self::SneakyGoblin,
        Self::Valkyrie,
        Self::SuperMinion,
        Self::ElectroDragon,
        Self::SuperDragon,
        Self::BuilderBaseBabyDragon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SneakyGoblin => "Sneaky Goblins",
            Self::Valkyrie => "Valkyries",
            Self::SuperMinion => "Super Minions",
            Self::ElectroDragon => "Electro Dragons",
            Self::SuperDragon => "Super Dragon",
            Self::BuilderBaseBabyDragon => "Builder Base — Baby Dragons",
        }
    }

    pub fn village_label(self) -> &'static str {
        match self {
            Self::BuilderBaseBabyDragon => "Builder Base",
            _ => "Home Village",
        }
    }

    pub fn deployment_plan(self) -> Vec<DeploymentStep> {
        match self {
            Self::ElectroDragon => vec![
                DeploymentStep::SelectMainTroop,
                DeploymentStep::DeployElectroDragons { count: 12, interval_ms: 200 },
                DeploymentStep::DeploySiegeMachine,
                DeploymentStep::DeployHeroes,
                DeploymentStep::ActivateHeroAbilities,
                DeploymentStep::DeploySpells,
            ],
            Self::BuilderBaseBabyDragon => vec![
                DeploymentStep::SelectMainTroop,
                DeploymentStep::SpreadMainTroops,
                DeploymentStep::WaitForBattleResolution,
            ],
            Self::SneakyGoblin | Self::Valkyrie | Self::SuperMinion | Self::SuperDragon => vec![
                DeploymentStep::SelectMainTroop,
                DeploymentStep::PerimeterDrag,
                DeploymentStep::DeploySuperDragonIfPresent,
                DeploymentStep::DeploySiegeMachine,
                DeploymentStep::DeployHeroes,
                DeploymentStep::ActivateHeroAbilities,
                DeploymentStep::DeployEarthquakeSpells { clicks: 11 },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentStep {
    SelectMainTroop,
    PerimeterDrag,
    SpreadMainTroops,
    DeployElectroDragons { count: u8, interval_ms: u16 },
    DeploySuperDragonIfPresent,
    DeploySiegeMachine,
    DeployHeroes,
    ActivateHeroAbilities,
    DeployEarthquakeSpells { clicks: u8 },
    DeploySpells,
    WaitForBattleResolution,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electro_dragon_plan_uses_twelve_individual_drops() {
        let plan = ArmyStrategy::ElectroDragon.deployment_plan();
        assert!(plan.contains(&DeploymentStep::DeployElectroDragons {
            count: 12,
            interval_ms: 200,
        }));
    }

    #[test]
    fn standard_perimeter_strategies_include_earthquake_pattern() {
        let plan = ArmyStrategy::SneakyGoblin.deployment_plan();
        assert!(plan.contains(&DeploymentStep::PerimeterDrag));
        assert!(plan.contains(&DeploymentStep::DeployEarthquakeSpells { clicks: 11 }));
    }

    #[test]
    fn builder_base_strategy_is_separate() {
        assert_eq!(
            ArmyStrategy::BuilderBaseBabyDragon.village_label(),
            "Builder Base"
        );
    }
}
