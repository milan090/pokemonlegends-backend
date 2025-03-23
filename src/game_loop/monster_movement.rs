use std::sync::Arc;
use rand::Rng;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use tokio::time::Duration;
use tracing::info;

use crate::monsters::{Monster, MovementPattern, Position};
use crate::monsters::monster_manager::MonsterManager;
use crate::models::{Lobby, ServerMessage};

// Constants for monster movement
const MOVEMENT_SPEED_MULTIPLIER: f32 = 0.8;
const DIRECTION_CHANGE_PROBABILITY: f32 = 0.05; // 5% chance to change direction
const MOVEMENT_UPDATE_INTERVAL_MS: u64 = 5000; // Time between movement updates (changed from 200ms to 2000ms)
const MONSTERS_MOVE_PERCENT: f32 = 0.1; // Percentage of monsters that move during each update

// Handles monster movement logic
pub async fn run_monster_movement(monster_manager: Arc<MonsterManager>, lobbies: Arc<dashmap::DashMap<String, Arc<Lobby>>>) {
    info!("Starting monster movement controller");
    let mut rng = SmallRng::from_entropy();
    
    loop {
        // Get all active monsters
        let monsters = monster_manager.get_all_active_monsters().await;
        
        for monster in monsters {
            // Skip stationary monsters
            match &monster.movement_pattern {
                MovementPattern::Stationary => continue,
                _ => {}
            }
            
            // Only move a percentage of monsters each time
            if rng.gen::<f32>() > MONSTERS_MOVE_PERCENT {
                continue;
            }
            
            // Calculate movement based on pattern
            let updated_monster = move_monster(monster);
            
            // Update monster position in the manager
            if let Ok(mut active_monsters) = monster_manager.active_monsters.try_write() {
                if let Some(monster_ref) = active_monsters.get_mut(&updated_monster.instance_id) {
                    monster_ref.position = updated_monster.position.clone();
                    monster_ref.animation = updated_monster.animation.clone();
                    monster_ref.direction = updated_monster.direction.clone();
                }
                
                // Notify all lobbies about monster movement
                let monster_move_msg = ServerMessage::MonsterMoved { monster: updated_monster };
                if let Ok(msg_json) = serde_json::to_string(&monster_move_msg) {
                    for lobby_entry in lobbies.iter() {
                        let _ = lobby_entry.tx.send(msg_json.clone());
                    }
                }
            }
        }
        
        // Sleep before the next movement update
        tokio::time::sleep(Duration::from_millis(MOVEMENT_UPDATE_INTERVAL_MS)).await;
    }
}

// Apply movement logic to a monster
fn move_monster(mut monster: Monster) -> Monster {
    let mut rng = SmallRng::from_entropy();
    
    // Calculate the speed based on the monster's stats
    let speed = monster.stats.speed * MOVEMENT_SPEED_MULTIPLIER;
    
    match &monster.movement_pattern {
        MovementPattern::Random => {
            // Randomly change direction with a small probability
            if rng.gen::<f32>() < DIRECTION_CHANGE_PROBABILITY {
                let directions = ["up", "down", "left", "right"];
                monster.direction = directions[rng.gen_range(0..4)].to_string();
            }
            
            // Move in the current direction
            match monster.direction.as_str() {
                "up" => {
                    monster.position.y -= speed;
                    monster.animation = "upWalk".to_string();
                },
                "down" => {
                    monster.position.y += speed;
                    monster.animation = "downWalk".to_string();
                },
                "left" => {
                    monster.position.x -= speed;
                    monster.animation = "leftWalk".to_string();
                },
                "right" => {
                    monster.position.x += speed;
                    monster.animation = "rightWalk".to_string();
                },
                _ => {}
            }
            
            // Add boundary checks if needed here
            // Prevent monsters from walking off the map
            monster.position.x = monster.position.x.max(0.0).min(800.0);
            monster.position.y = monster.position.y.max(0.0).min(600.0);
        },
        MovementPattern::Linear => {
            // Simple linear movement (can be enhanced)
            match monster.direction.as_str() {
                "up" => {
                    monster.position.y -= speed;
                    monster.animation = "upWalk".to_string();
                    // If hit boundary, reverse direction
                    if monster.position.y <= 0.0 {
                        monster.direction = "down".to_string();
                    }
                },
                "down" => {
                    monster.position.y += speed;
                    monster.animation = "downWalk".to_string();
                    if monster.position.y >= 600.0 {
                        monster.direction = "up".to_string();
                    }
                },
                "left" => {
                    monster.position.x -= speed;
                    monster.animation = "leftWalk".to_string();
                    if monster.position.x <= 0.0 {
                        monster.direction = "right".to_string();
                    }
                },
                "right" => {
                    monster.position.x += speed;
                    monster.animation = "rightWalk".to_string();
                    if monster.position.x >= 800.0 {
                        monster.direction = "left".to_string();
                    }
                },
                _ => {}
            }
        },
        MovementPattern::Patrol { waypoints } => {
            // More complex patrol logic could be implemented here
            // For now, just use the linear movement as a placeholder
            if waypoints.is_empty() {
                return monster;
            }
            
            // Placeholder implementation - would need to track target waypoint
            match monster.direction.as_str() {
                "up" => {
                    monster.position.y -= speed;
                    monster.animation = "upWalk".to_string();
                    if monster.position.y <= 0.0 {
                        monster.direction = "down".to_string();
                    }
                },
                "down" => {
                    monster.position.y += speed;
                    monster.animation = "downWalk".to_string();
                    if monster.position.y >= 600.0 {
                        monster.direction = "up".to_string();
                    }
                },
                "left" => {
                    monster.position.x -= speed;
                    monster.animation = "leftWalk".to_string();
                    if monster.position.x <= 0.0 {
                        monster.direction = "right".to_string();
                    }
                },
                "right" => {
                    monster.position.x += speed;
                    monster.animation = "rightWalk".to_string();
                    if monster.position.x >= 800.0 {
                        monster.direction = "left".to_string();
                    }
                },
                _ => {}
            }
        },
        MovementPattern::Stationary => {
            // No movement needed
            monster.animation = match monster.direction.as_str() {
                "up" => "upIdle".to_string(),
                "down" => "downIdle".to_string(),
                "left" => "leftIdle".to_string(),
                "right" => "rightIdle".to_string(),
                _ => "downIdle".to_string(),
            };
        }
    }
    
    monster
} 