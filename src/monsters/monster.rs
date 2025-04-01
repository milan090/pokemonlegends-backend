use serde::{Deserialize, Serialize};
use uuid::Uuid;
use rand::seq::SliceRandom;
use rand::Rng;
use std::sync::Arc;

use crate::stats::{BaseStats, CalculatedStats, calculate_stats, StatSet};
use crate::stats::nature::Nature;

use crate::combat::state::StatusCondition;
/// Represents a monster's position in the game world using tile coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: u32,
    pub y: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum PokemonType {
    Normal, Fire, Water, Grass, Electric, Ice, Fighting, Poison, Ground,
    Flying, Psychic, Bug, Rock, Ghost, Dragon, Steel, Dark, Fairy,
}


/// Defines how monsters move around the game world
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MovementPattern {
    Random,           // Move randomly
    Linear,           // Move in a straight line
    Patrol { waypoints: Vec<Position> }, // Move between predefined waypoints
    Stationary,       // Don't move
}

/// Static template defining a monster type's base properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterTemplate {
    pub id: u32,
    pub name: String,
    pub types: Vec<PokemonType>,
    pub abilities: Vec<String>,
    pub base_experience: u32,
    pub min_level: u32,
    pub max_level: u32,
    pub base_stats: BaseStats,
    pub movement_pattern: Option<MovementPattern>,
    pub moves: Vec<(u32, u32)>, // (move_id, level_learned)
    pub spawn_rate: f32,
    pub growth_rate: GrowthRate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GrowthRate {
    #[serde(rename = "slow")]
    Slow,
    #[serde(rename = "medium")] 
    Medium,
    #[serde(rename = "medium-slow")]
    MediumSlow,
    #[serde(rename = "fast")]
    Fast
}

/// Lightweight monster representation for client display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayMonster {
    pub instance_id: String,
    pub template_id: u32,
    pub level: u32,
    pub position: Position,
    pub direction: String,
    pub in_combat: bool,
    pub max_hp: u32,
    pub current_hp: u32,
    pub name: String,
    pub types: Vec<PokemonType>,
}

/// Active monster instance in the game world
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monster {
    pub instance_id: String,
    pub template_id: u32,
    pub name: String,
    pub level: u32,
    pub position: Position,
    pub movement_pattern: MovementPattern,
    pub direction: String,
    pub spawn_time: u64,
    pub despawn_time: Option<u64>,
    pub current_hp: u32,
    pub status_condition: Option<StatusCondition>,
    pub types: Vec<PokemonType>,
    pub ability: String,
    pub moves: Vec<MonsterMove>,
    pub in_combat: bool,
    pub calculated_stats: CalculatedStats,
    pub ivs: StatSet<u8>,      // Adding IVs for wild monsters similar to Pokemon
    pub evs: StatSet<u16>,     // Adding EVs for wild monsters similar to Pokemon  
    pub nature: Nature,        // Adding nature for wild monsters similar to Pokemon
}

/// Represents a move that a monster can use
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterMove {
    pub id: u32,
    pub pp_remaining: u8,
}

impl Monster {
    /// Creates a new monster instance from a template at a specific position and level
    pub fn new(
        template: &MonsterTemplate, 
        position: Position, 
        level: u32, 
        move_repository: Option<&Arc<crate::monsters::move_manager::MoveRepository>>
    ) -> Self {
        // Generate random IVs (0-31 for each stat)
        let ivs = StatSet {
            hp: rand::thread_rng().gen_range(0..=31),
            attack: rand::thread_rng().gen_range(0..=31),
            defense: rand::thread_rng().gen_range(0..=31),
            special_attack: rand::thread_rng().gen_range(0..=31),
            special_defense: rand::thread_rng().gen_range(0..=31),
            speed: rand::thread_rng().gen_range(0..=31),
        };
        
        // Start with zero EVs for wild monsters
        let evs = StatSet {
            hp: 0,
            attack: 0,
            defense: 0,
            special_attack: 0,
            special_defense: 0,
            speed: 0,
        };
        
        // Generate a random nature
        let nature = Nature::random();
        
        // Calculate stats using the same formula as for Pokemon
        let calculated_stats = calculate_stats(&template.base_stats, level, &ivs, &evs, &nature);
        
        // Get monster moves
        let moves = if let Some(move_repo) = move_repository {
            // Use the move repository to get proper PP values
            move_repo.select_moves_for_monster(&template.moves, level)
        } else {
            // Fallback to default PP values
            template.moves
                .iter()
                .filter(|(_, level_learned)| *level_learned <= level)
                .map(|(move_id, _)| MonsterMove {
                    id: *move_id,
                    pp_remaining: 20, // Default PP value
                })
                .collect()
        };

        // Randomly select one ability from the template's abilities
        let ability = template.abilities
            .choose(&mut rand::thread_rng())
            .cloned()
            .unwrap_or_else(|| "None".to_string());
        
        Monster {
            instance_id: Uuid::new_v4().to_string(),
            template_id: template.id,
            name: template.name.clone(),
            level,
            position,
            movement_pattern: template.movement_pattern.clone().unwrap_or(MovementPattern::Random),
            direction: "down".to_string(),
            spawn_time: chrono::Utc::now().timestamp() as u64,
            despawn_time: None,
            current_hp: calculated_stats.hp, // Using the calculated HP
            status_condition: None,
            calculated_stats,
            types: template.types.clone(),
            ability,
            moves,
            in_combat: false,
            ivs,
            evs,
            nature,
        }
    }
    
    /// Convert a full Monster to a lightweight DisplayMonster for client display
    pub fn to_display(&self) -> DisplayMonster {
        DisplayMonster {
            instance_id: self.instance_id.clone(),
            template_id: self.template_id,
            level: self.level,
            position: self.position.clone(),
            direction: self.direction.clone(),
            in_combat: self.in_combat,
            max_hp: self.calculated_stats.hp,
            current_hp: self.current_hp,
            name: self.name.clone(),
            types: self.types.clone(),
        }
    }
} 