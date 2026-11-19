use crate::strategy::ArmyStrategy;
use image::GenericImageView;
use std::{fs, path::{Path, PathBuf}};

/// A logical recognition requirement. Alternative filenames are accepted because
/// Home Village and Builder Base use different HUD artwork.
#[derive(Debug, Clone)]
pub struct TemplateGroup {
    pub name: &'static str,
    pub alternatives: Vec<String>,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct TemplateAssetDiagnostic {
    pub file: String,
    pub valid: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TemplateGroupDiagnostic {
    pub name: &'static str,
    pub required: bool,
    pub selected: Option<String>,
    pub assets: Vec<TemplateAssetDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct TemplatePackDiagnostics {
    pub directory: PathBuf,
    pub groups: Vec<TemplateGroupDiagnostic>,
}

impl TemplatePackDiagnostics {
    pub fn inspect(directory: impl Into<PathBuf>, strategy: ArmyStrategy) -> Self {
        let directory = directory.into();
        let groups = required_groups(strategy).into_iter().map(|group| {
            let assets = group.alternatives.iter().map(|file| {
                validate_asset(&directory, file)
            }).collect::<Vec<_>>();
            let selected = assets.iter().find(|asset| asset.valid).map(|asset| asset.file.clone());
            TemplateGroupDiagnostic {
                name: group.name,
                required: group.required,
                selected,
                assets,
            }
        }).collect();
        Self { directory, groups }
    }

    pub fn ready_groups(&self) -> usize {
        self.groups.iter().filter(|group| group.selected.is_some()).count()
    }

    pub fn required_groups(&self) -> usize {
        self.groups.iter().filter(|group| group.required).count()
    }

    pub fn missing_required(&self) -> Vec<&TemplateGroupDiagnostic> {
        self.groups.iter().filter(|group| group.required && group.selected.is_none()).collect()
    }

    pub fn is_ready(&self) -> bool {
        self.missing_required().is_empty()
    }

    /// Human-readable startup diagnostics suitable for the application log.
    pub fn messages(&self) -> Vec<String> {
        let mut messages = vec![format!(
            "Template pack: {} ({} of {} required groups ready)",
            self.directory.display(),
            self.groups.iter().filter(|g| g.required && g.selected.is_some()).count(),
            self.required_groups(),
        )];
        for group in &self.groups {
            match &group.selected {
                Some(file) => messages.push(format!("Template OK: {} -> {}", group.name, file)),
                None if group.required => messages.push(format!(
                    "Template missing: {} (expected one of: {})",
                    group.name, group.assets.iter().map(|a| a.file.as_str()).collect::<Vec<_>>().join(", ")
                )),
                None => messages.push(format!("Optional template group unavailable: {}", group.name)),
            }
            for asset in group.assets.iter().filter(|asset| !asset.valid && asset.message != "not present") {
                messages.push(format!("Invalid template {}: {}", asset.file, asset.message));
            }
        }
        messages
    }
}

pub fn required_groups(strategy: ArmyStrategy) -> Vec<TemplateGroup> {
    let group = |name, alternatives: &[&str], required| TemplateGroup {
        name,
        alternatives: alternatives.iter().map(|name| (*name).to_owned()).collect(),
        required,
    };
    let troop = match strategy {
        ArmyStrategy::SneakyGoblin => "sneaky_goblin.png",
        ArmyStrategy::Valkyrie => "valkyrie.png",
        ArmyStrategy::SuperMinion => "super_minion.png",
        ArmyStrategy::ElectroDragon => "electro_dragon.png",
        ArmyStrategy::SuperDragon => "super_dragon.png",
        ArmyStrategy::BuilderBaseBabyDragon => "baby_dragon.png",
    };
    let builder = strategy.is_builder_base();
    vec![
        if builder {
            group("village entry", &["bbattack.png"], true)
        } else {
            group("village entry", &["attack.png", "farmbattle.png"], true)
        },
        if builder {
            group("find opponent", &["bbfindnow.png"], true)
        } else {
            group("find opponent", &["findnow.png"], true)
        },
        if builder {
            group("matchmaking", &["bbsearching.png"], true)
        } else {
            group("matchmaking", &["searching.png"], true)
        },
        group("opponent found", &["opponentfound.png"], true),
        if builder {
            group("battle HUD", &["bbendbattle.png", "endbattle.png"], true)
        } else {
            group("battle HUD", &["endbattle.png"], true)
        },
        if builder {
            group("battle won", &["bbstar.png", "bstar.png"], true)
        } else {
            group("battle won", &["bstar.png"], true)
        },
        if builder {
            group("battle lost", &["bbdefeat.png", "defeat.png"], true)
        } else {
            group("battle lost", &["defeat.png"], true)
        },
        if builder {
            group("return home", &["bbreturnhome.png", "returnhome.png"], true)
        } else {
            group("return home", &["returnhome.png"], true)
        },
        group("selected army troop", &[troop], true),
        group("next opponent", &["next.png", "bbnext.png"], false),
        group("hero ability", &["king_ability.png", "queen_ability.png", "warden_ability.png", "champion_ability.png"], false),
        group("siege machine", &["siege_machine.png"], false),
        group("spell icons", &["spell.png", "earthquake_spell.png"], false),
    ]
}

fn validate_asset(directory: &Path, file: &str) -> TemplateAssetDiagnostic {
    let path = directory.join(file);
    if !path.exists() {
        return TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: "not present".into() };
    }
    if !path.is_file() {
        return TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: "path is not a regular file".into() };
    }
    match fs::metadata(&path) {
        Ok(metadata) if metadata.len() == 0 => {
            return TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: "file is empty".into() };
        }
        Ok(metadata) if metadata.len() > 16 * 1024 * 1024 => {
            return TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: "file exceeds the 16 MiB template limit".into() };
        }
        Err(error) => {
            return TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: format!("cannot read file metadata: {error}") };
        }
        _ => {}
    }
    match image::open(&path) {
        Ok(decoded) => {
            let (width, height) = decoded.dimensions();
            if width < 8 || height < 8 {
                TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: format!("image is too small ({width}x{height}; minimum 8x8)") }
            } else if width > 2048 || height > 2048 {
                TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: format!("image is too large ({width}x{height}; maximum 2048x2048)") }
            } else {
                TemplateAssetDiagnostic { file: file.to_owned(), valid: true, message: format!("valid PNG image ({width}x{height})") }
            }
        }
        Err(error) => TemplateAssetDiagnostic { file: file.to_owned(), valid: false, message: format!("image decode failed: {error}") },
    }
}
