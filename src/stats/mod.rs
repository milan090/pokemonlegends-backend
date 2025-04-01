pub mod nature;


use nature::Nature;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatName {
    Attack,
    Defense,
    SpecialAttack,
    SpecialDefense,
    Speed,
    Accuracy,
    Evasion,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatSet<T> {
    pub hp: T,
    pub attack: T,
    pub defense: T,
    pub special_attack: T,
    pub special_defense: T,
    pub speed: T,
}

pub type BaseStats = StatSet<u32>;
pub type CalculatedStats = StatSet<u32>;

// StatStages needs special handling because it has additional fields
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BattleStatModifiers {
    // inherit the common stats
    pub battle_stats: StatSet<i8>,
    // battle-specific stats
    pub accuracy: i8,
    pub evasion: i8,
}
impl BattleStatModifiers {
    pub fn get_multiplier(&self, stat_name: StatName) -> f32 {
        let stage = match stat_name {
            StatName::Attack => self.battle_stats.attack,
            StatName::Defense => self.battle_stats.defense,
            StatName::SpecialAttack => self.battle_stats.special_attack,
            StatName::SpecialDefense => self.battle_stats.special_defense,
            StatName::Speed => self.battle_stats.speed,
            StatName::Accuracy => self.accuracy,
            StatName::Evasion => self.evasion,
        };
        
        match stat_name {
            // Accuracy and Evasion use 3/3 formula
            StatName::Accuracy | StatName::Evasion => {
                if stage >= 0 {
                    (3.0 + stage as f32) / 3.0
                } else {
                    3.0 / (3.0 - stage as f32)
                }
            },
            // Other stats use 2/2 formula
            _ => {
                if stage >= 0 {
                    (2.0 + stage as f32) / 2.0
                } else {
                    2.0 / (2.0 - stage as f32)
                }
            }
        }
    }
}


pub fn calculate_stats(
  base_stats: &BaseStats,
  level: u32,
  ivs: &StatSet<u8>,
  evs: &StatSet<u16>,
  nature: &Nature,
) -> CalculatedStats {
  // HP formula (different from other stats)
  let hp = ((2 * base_stats.hp + ivs.hp as u32 + (evs.hp as u32 / 4)) * level / 100) + level + 10;
  
  // Other stats use a common formula
  let attack = calculate_stat(
      base_stats.attack, 
      ivs.attack, 
      evs.attack, 
      level, 
      nature.get_multiplier(StatName::Attack)
  );
  
  let defense = calculate_stat(
      base_stats.defense, 
      ivs.defense, 
      evs.defense, 
      level, 
      nature.get_multiplier(StatName::Defense)
  );
  
  let special_attack = calculate_stat(
      base_stats.special_attack,
      ivs.special_attack,
      evs.special_attack,
      level,
      nature.get_multiplier(StatName::SpecialAttack)
  );

  let special_defense = calculate_stat(
      base_stats.special_defense,
      ivs.special_defense,
      evs.special_defense, 
      level,
      nature.get_multiplier(StatName::SpecialDefense)
  );

  let speed = calculate_stat(
      base_stats.speed,
      ivs.speed,
      evs.speed,
      level,
      nature.get_multiplier(StatName::Speed)
  );
  
  CalculatedStats {
      hp,
      attack,
      defense,
      special_attack,
      special_defense,
      speed,
  }
}

fn calculate_stat(base: u32, iv: u8, ev: u16, level: u32, nature_multiplier: f32) -> u32 {
  // Standard Pokémon stat formula
  let base_calc = (((2 * base + iv as u32 + (ev as u32 / 4)) * level) / 100) + 5;
  // Apply nature and round down
  (base_calc as f32 * nature_multiplier).floor() as u32
}