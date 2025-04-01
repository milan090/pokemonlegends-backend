use super::StatName;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Nature {
    Hardy,
    Lonely,
    Brave,
    Adamant,
    Naughty,
    Bold,
    Docile,
    Relaxed,
    Impish,
    Lax,
    Timid,
    Hasty,
    Serious,
    Jolly,
    Naive,
    Modest,
    Mild,
    Quiet,
    Bashful,
    Rash,
    Calm,
    Gentle,
    Sassy,
    Careful,
    Quirky,
}


/// Represents which stats are increased/decreased by a Nature
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NatureEffect {
    pub increased_stat: Option<StatName>,
    pub decreased_stat: Option<StatName>,
}

impl Nature {
    const NEUTRAL_NATURES: [Nature; 5] = [
        Nature::Hardy,
        Nature::Docile,
        Nature::Serious,
        Nature::Bashful,
        Nature::Quirky,
    ];

    /// Returns which stats are increased/decreased by this nature
    pub fn get_effects(&self) -> NatureEffect {
        if Self::NEUTRAL_NATURES.contains(self) {
            return NatureEffect {
                increased_stat: None,
                decreased_stat: None,
            };
        }

        match self {
            Nature::Adamant => NatureEffect {
                increased_stat: Some(StatName::Attack),
                decreased_stat: Some(StatName::SpecialAttack),
            },
            Nature::Lonely => NatureEffect {
                increased_stat: Some(StatName::Attack),
                decreased_stat: Some(StatName::Defense),
            },
            Nature::Brave => NatureEffect {
                increased_stat: Some(StatName::Attack),
                decreased_stat: Some(StatName::Speed),
            },
            Nature::Naughty => NatureEffect {
                increased_stat: Some(StatName::Attack),
                decreased_stat: Some(StatName::SpecialDefense),
            },
            Nature::Bold => NatureEffect {
                increased_stat: Some(StatName::Defense),
                decreased_stat: Some(StatName::Attack),
            },
            Nature::Modest => NatureEffect {
                increased_stat: Some(StatName::SpecialAttack),
                decreased_stat: Some(StatName::Attack),
            },
            Nature::Calm => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::SpecialAttack),
            },
            Nature::Hasty => NatureEffect {
                increased_stat: Some(StatName::Speed),
                decreased_stat: Some(StatName::Attack),
            },
            Nature::Relaxed => NatureEffect {
                increased_stat: Some(StatName::Defense),
                decreased_stat: Some(StatName::SpecialDefense),
            },
            Nature::Impish => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::SpecialAttack),
            },
            Nature::Lax => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::Defense),
            },
            Nature::Timid => NatureEffect {
                increased_stat: Some(StatName::Speed),
                decreased_stat: Some(StatName::Attack),
            },
            Nature::Jolly => NatureEffect {
                increased_stat: Some(StatName::Speed),
                decreased_stat: Some(StatName::SpecialAttack),
            },
            Nature::Naive => NatureEffect {
                increased_stat: Some(StatName::Speed),
                decreased_stat: Some(StatName::SpecialDefense),
            },
            Nature::Mild => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::Attack),
            },
            Nature::Quiet => NatureEffect {
                increased_stat: Some(StatName::Speed),
                decreased_stat: Some(StatName::SpecialAttack),
            },
            Nature::Rash => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::SpecialAttack),
            },
            Nature::Gentle => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::Speed),
            },
            Nature::Sassy => NatureEffect {
                increased_stat: Some(StatName::Speed),
                decreased_stat: Some(StatName::SpecialDefense),
            },
            Nature::Careful => NatureEffect {
                increased_stat: Some(StatName::SpecialDefense),
                decreased_stat: Some(StatName::Speed),
            },
            _ => NatureEffect {
                increased_stat: None,
                decreased_stat: None,
            },
        }
    }

    /// Get the multiplier for a specific stat based on nature
    pub fn get_multiplier(&self, stat: StatName) -> f32 {
        let effect = self.get_effects();
        match (effect.increased_stat, effect.decreased_stat) {
            (Some(increased), _) if increased == stat => 1.1,
            (_, Some(decreased)) if decreased == stat => 0.9,
            _ => 1.0,
        }
    }

    /// Checks if this is a neutral nature (no stat changes)
    pub fn is_neutral(&self) -> bool {
        Self::NEUTRAL_NATURES.contains(self)
    }

    /// Get a random nature
    pub fn random() -> Self {
        let all_natures = [
            Nature::Hardy,
            Nature::Lonely,
            Nature::Brave,
            Nature::Adamant,
            Nature::Naughty,
            Nature::Bold,
            Nature::Docile,
            Nature::Relaxed,
            Nature::Impish,
            Nature::Lax,
            Nature::Timid,
            Nature::Hasty,
            Nature::Serious,
            Nature::Jolly,
            Nature::Naive,
            Nature::Modest,
            Nature::Mild,
            Nature::Quiet,
            Nature::Bashful,
            Nature::Rash,
            Nature::Calm,
            Nature::Gentle,
            Nature::Sassy,
            Nature::Careful,
            Nature::Quirky,
        ];
        let idx = rand::thread_rng().gen_range(0..all_natures.len());
        all_natures[idx]
    }
}