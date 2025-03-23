use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Represents a monster's position in the game world
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

// Monster movement patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MovementPattern {
    Random,           // Move randomly
    Linear,           // Move in a straight line
    Patrol { waypoints: Vec<Position> }, // Move between predefined waypoints
    Stationary,       // Don't move
}

// Monster stats (can be expanded)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterStats {
    pub health: u32,
    pub damage: u32,
    pub defense: u32,
    pub speed: f32,
    pub detection_range: f32,  // How far the monster can detect players
    pub attack_range: f32,     // How far the monster can attack players
}

// Monster metadata loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterTemplate {
    pub id: String,
    pub name: String,
    pub level: u32,
    pub stats: MonsterStats,
    pub movement_pattern: MovementPattern,
    pub sprite_name: String,
    pub spawn_rate: f32,       // Probability of spawning (0.0 - 1.0)
    pub spawn_limit: u32,      // Maximum number that can exist at once
}

// Represents an active monster instance in the game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monster {
    pub instance_id: String,   // Unique ID for this specific monster instance
    pub template_id: String,   // ID linking to the monster template
    pub name: String,
    pub level: u32,
    pub stats: MonsterStats,
    pub position: Position,
    pub animation: String,     // Current animation state
    pub movement_pattern: MovementPattern,
    pub direction: String,     // Current facing direction
    pub spawn_time: u64,       // When this monster was spawned
    pub despawn_time: Option<u64>, // When this monster will despawn (if applicable)
}

impl Monster {
    pub fn new(template: &MonsterTemplate, position: Position) -> Self {
        Monster {
            instance_id: Uuid::new_v4().to_string(),
            template_id: template.id.clone(),
            name: template.name.clone(),
            level: template.level,
            stats: template.stats.clone(),
            position,
            animation: "idle".to_string(),
            movement_pattern: template.movement_pattern.clone(),
            direction: "down".to_string(),
            spawn_time: chrono::Utc::now().timestamp() as u64,
            despawn_time: None,
        }
    }
} 