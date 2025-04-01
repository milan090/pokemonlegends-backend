use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{combat::state::StatusCondition, monsters::monster::MonsterMove};

use super::monster::PokemonType;

/// Represents a move in the game

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct MoveData {
    pub id: u32,
    pub name: String,
    pub accuracy: Option<u8>, // Use Option for moves that don't check accuracy
    pub power: Option<u32>,   // Use Option for status moves
    pub pp: u8,
    pub priority: i8,
    #[serde(rename = "type")]
    pub move_type: PokemonType,
    pub damage_class: MoveCategory,
    pub target: TargetType,
    pub effect: EffectData,
    pub secondary_effect: Option<SecondaryEffectData>,
    pub description: String,
    // Add flags later if needed (e.g., is_contact, is_punch, ignores_substitute)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SecondaryEffectData {
    pub chance: u8, // Percentage chance
    pub effect: EffectData,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VolatileStatusType {
    Confusion,
    Flinch, // Typically lasts only one turn action
    Taunt,
    LeechSeed,
    Substitute, // Need HP value associated
    Bound, // e.g., Wrap, Fire Spin
    Infatuation,
    Disable,
    Encore,
    Torment,
    Imprison,
    Yawn,
    HealBlock,
    NoRetreat,
    Trapped, // e.g., Mean Look, Block
    Embargo,
    Ingrain,
    Aquaring,
    FocusEnergy,
    MagnetRise,
    Stockpile,
    Minimize,
    DefenseCurl,
    Rage,
    MudSport,
    WaterSport,
    LuckyChant,
    Recharge, // For moves like Hyper Beam
    TakeAim, // For moves that increase accuracy
    Curse // Ghost-type version
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "parameters", rename_all = "snake_case")]
pub enum EffectData {
    // === Standard Effects ===
    Damage {
        multi_hit: Option<MultiHitParams>,
        // Add flags/params for mechanics directly tied to damage calculation:
        crit_stage_bonus: Option<u8>, // Bonus stages to critical hit calculation
        drain_percent: Option<u8>,    // e.g., Giga Drain (percent of damage dealt)
        recoil_damage_percent: Option<u8>, // Percent of damage dealt taken as recoil (e.g., Flare Blitz)
        // recoil_fixed_amount: Option<u32>, // Less common
    },
    ApplyStatus {
        status: StatusCondition,
        target: EffectTarget,
    },
    StatChange {
        changes: Vec<StatChangeParam>,
        target: EffectTarget,
    },
    ApplyFieldEffect {
        effect_type: FieldEffectType,
        duration: Option<u8>,
        target_side: EffectTargetSide,
    },
    Heal {
        target: EffectTarget,
        percent: Option<u8>,       // Percent of Max HP
        fixed_amount: Option<u32>, // Fixed amount healing (e.g., specific items might use this)
        // Maybe add flags like: based_on_weather: bool, based_on_status: bool?
    },
    SwitchTarget {}, // Roar, Whirlwind, Dragon Tail, Circle Throw
    FixedDamage { // Seismic Toss, Night Shade, Psywave (needs custom calc), Sonic Boom
        damage_source: FixedDamageSource,
        // Add fixed_amount: Option<u32> here if needed instead of in FixedDamageSource
    },
    ApplyVolatileStatus { // Confusion Ray, Taunt, Encore, Disable, Leech Seed, Yawn, etc.
        status: VolatileStatusType,
        target: EffectTarget,
        // duration: Option<u8>, // Duration might be better tracked server-side in VolatileStatusData
    },
    FlinchTarget {}, // Needs to be checked *before* target acts, often secondary effect

    // === Common Unique Patterns ===
    Rest {}, // Heals HP/status, applies Sleep for fixed turns
    Substitute { // User pays HP to create a substitute
        hp_cost_percent: u8,
    },
    ProtectDetect { // Protect, Detect, Spiky Shield, King's Shield, Baneful Bunker, Obstruct
        // Server-side checks consecutive use, specific move effects (e.g. stat drop)
    },
    BindTarget { // Wrap, Bind, Fire Spin, Clamp, Whirlpool, Sand Tomb, Magma Storm, Infestation
        min_turns: u8,
        max_turns: u8,
        // Server tracks turns, applies damage, prevents switching
    },
    MultiTurnCharge { // Fly, Dig, Dive, Bounce, Phantom Force, Shadow Force, Solar Beam, Sky Attack, Razor Wind
        // turn_one_effect: Option<Box<EffectData>>, // Effect on first turn (e.g., raise defense) - Might be too complex, handle server-side
        // Server tracks state, invulnerability, executes damage on turn 2
    },
    CrashDamageOnFail { // Hi Jump Kick, Jump Kick
        // Server applies damage to user if move misses, fails, or hits Protect
        // Need to specify damage type/amount (e.g., percent max HP)
        damage_percent_max_hp: Option<u8>, // e.g., 50%
    },
    RecoilPercentMaxHp { // Struggle, Head Smash (if not % of damage dealt)
        percent: u8, // Percent of User's Max HP taken as recoil
    },
    OneHitKO { // Fissure, Guillotine, Horn Drill, Sheer Cold
        // Accuracy calculation is special (Level based + base accuracy)
    },
    AffectPp { // Spite, Eerie Spell
        target: EffectTarget,
        amount: i8, // PP reduction amount
    },
    ForceSwitch { // Like SwitchTarget, but guaranteed if hit? Maybe merge with SwitchTarget? Needs review. Dragon Tail, Circle Throw fit here.
        // Differentiate from Roar/Whirlwind which just *request* a switch?
    },

    // === Generic Fallback ===
    #[serde(rename = "unknown_status")]
    UniqueLogic {}, // Examples: Transform, Mimic, Sketch, Counter, Mirror Coat, Bide,
                     //           Pain Split, Destiny Bond, Curse, Conversion, Endeavor,
                     //           Assist, Metronome, Sleep Talk, Skill Swap, Guard Swap, Power Swap,
                     //           Trick, Switcheroo, Pay Day, Present, Spit Up, Swallow, Stockpile,
                     //           Punishment, Stored Power, Hex, Acrobatics, Judgment, Natural Gift,
                     //           Magnitude, Metal Burst, Beat Up, Rage, etc.

    // Could potentially add more specific patterns if they cover several moves cleanly:
    // - TypeChange { target: EffectTarget, new_type: PokemonType } // Soak, Magic Powder? Or UniqueLogic
    // - StatSwap { stat1: Stat, stat2: Stat } // Guard Swap, Power Swap? Or UniqueLogic
    // - IgnoreDefenses // Chip Away, Sacred Sword? Maybe a flag on Damage?
    // - ClearHazards { side: EffectTargetSide } // Rapid Spin, Defog? Or UniqueLogic (Defog has other effects)
    // - SetHp { target: EffectTarget, amount: Option<u32>, percent: Option<u8> } // Endeavor? Or UniqueLogic
}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MultiHitParams {
    pub min: u8,
    pub max: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatChangeParam {
    pub stat: Stat,
    pub stages: i8,
}

// --- Enums needed by MoveTemplate ---

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum MoveCategory {
    Physical, Special, Status,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TargetType {
    User,
    NormalOpponent, // Standard single target move
    AnyAdjacent,
    AllAdjacentOpponents,
    UserSide, // Light Screen, Tailwind
    OpponentSide, // Spikes, Stealth Rock
    WholeField, // Weather, Trick Room
    Ally, // Helping Hand, Aromatherapy
    AllPokemon, // Perish Song, Sunny Day
    RandomOpponent, // Outrage in doubles
    UserAndAllies, // Heal Bell, Safeguard
    AllOtherPokemon, // Earthquake, Magnitude - hits all except user
    AllOpponents, // Rock Slide, Surf
    UserOrAlly, // Acupressure, Helping Hand
    Adjacent, // Includes both allies and opponents
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum EffectTarget {
    User, Target,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum EffectTargetSide {
    User, Opponent, WholeField,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Stat { Hp, Attack, Defense, SpecialAttack, SpecialDefense, Speed, Accuracy, Evasion }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum FieldEffectType {
    // Player Side Effects
    Reflect, LightScreen, Safeguard, Tailwind, StealthRock, Spikes, ToxicSpikes, StickyWeb, // Add levels for Spikes/TSpikes later
    // Whole Field Effects
    TrickRoom, MagicRoom, WonderRoom, Gravity, Rain, HarshSunlight, Sandstorm, Hail, // Add more weather/terrain
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum FixedDamageSource {
    UserLevel,
    // FixedAmount(u32), // Example if needed
}

pub type TypeChart = HashMap<PokemonType, HashMap<PokemonType, f32>>;

/// Repository for move data
#[derive(Debug)]
pub struct MoveRepository {
    pub moves: HashMap<u32, MoveData>,
    pub type_chart: TypeChart,
}

impl MoveRepository {
    /// Create a new MoveRepository from the specified file path
    pub fn new(moves_path: &str, type_chart_path: &str) -> Arc<Self> {
        let moves = Self::load_moves(moves_path);
        info!("Loaded {} moves from {}", moves.len(), moves_path);
        let type_chart = Self::load_type_chart(type_chart_path);
        info!("Loaded {} type chart from {}", type_chart.len(), type_chart_path);
        Arc::new(MoveRepository { moves, type_chart })
    }
    
    /// Load move data from the JSON file
    fn load_moves(path: &str) -> HashMap<u32, MoveData> {
        match File::open(Path::new(path)) {
            Ok(file) => {
                let reader = BufReader::new(file);
                match serde_json::from_reader(reader) {
                    Ok(moves_map) => moves_map,
                    Err(e) => {
                        warn!("Failed to parse moves JSON: {}", e);
                        HashMap::new()
                    }
                }
            },
            Err(e) => {
                warn!("Failed to open moves file {}: {}", path, e);
                HashMap::new()
            }
        }
    }

    fn load_type_chart(path: &str) -> TypeChart {
        match File::open(Path::new(path)) {
            Ok(file) => {
                let reader = BufReader::new(file);
                match serde_json::from_reader(reader) {
                    Ok(type_chart) => type_chart,
                    Err(e) => {
                        warn!("Failed to parse type chart JSON: {}", e);
                        HashMap::new()
                    }
                }
            },
            Err(e) => {
                warn!("Failed to open type chart file {}: {}", path, e);
                HashMap::new()
            }
        }
    }
    
    /// Get move data by ID
    pub fn get_move(&self, move_id: u32) -> Option<&MoveData> {
        self.moves.get(&move_id)
    }
    
    /// Create a MonsterMove with proper PP from the move data
    pub fn create_monster_move(&self, move_id: u32) -> Option<MonsterMove> {
        self.get_move(move_id).map(|move_data| {
            MonsterMove {
                id: move_id,
                pp_remaining: move_data.pp,
            }
        })
    }
    
    /// Select moves for a Pokémon based on the available move IDs and the Pokémon's level
    pub fn select_moves_for_monster(
        &self, 
        available_moves: &[(u32, u32)], 
        level: u32
    ) -> Vec<MonsterMove> {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        use rand::rngs::SmallRng;
        
        let mut rng = SmallRng::from_entropy();
        
        // Filter moves that can be learned at or below the current level
        let level_filtered_moves: Vec<(u32, u32)> = available_moves
            .iter()
            .filter(|(_, level_learned)| *level_learned <= level)
            .map(|(move_id, level_learned)| (*move_id, *level_learned))
            .collect();
        
        if level_filtered_moves.is_empty() {
            return Vec::new();
        }
        
        // Sort moves by level learned (descending) to prioritize most recently learned moves
        let mut sorted_moves = level_filtered_moves.clone();
        sorted_moves.sort_by(|a, b| b.1.cmp(&a.1));
        
        // In official Pokémon games, Pokémon typically have up to 4 moves
        let max_moves = 4;
        
        let selected_move_ids = if sorted_moves.len() <= max_moves {
            // If we have 4 or fewer moves, use all of them
            sorted_moves.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
        } else {
            // Otherwise, use a strategy similar to official games:
            // 1. Include the most recently learned 2-3 moves
            let recent_moves = sorted_moves.iter().take(3).map(|(id, _)| *id).collect::<Vec<_>>();
            
            // 2. For remaining slots, randomly select from other available moves
            let mut selected = recent_moves;
            let remaining_moves: Vec<u32> = sorted_moves
                .into_iter()
                .skip(3)
                .map(|(id, _)| id)
                .collect();
            
            if !remaining_moves.is_empty() && selected.len() < max_moves {
                let slots_left = max_moves - selected.len();
                let mut additional_move_ids = remaining_moves;
                additional_move_ids.shuffle(&mut rng);
                
                for move_id in additional_move_ids.iter().take(slots_left) {
                    selected.push(*move_id);
                }
            }
            
            selected
        };
        
        // Create MonsterMove instances with proper PP values from the move data
        selected_move_ids
            .into_iter()
            .filter_map(|move_id| self.create_monster_move(move_id))
            .collect()
    }
} 