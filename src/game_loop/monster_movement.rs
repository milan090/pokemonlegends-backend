use std::sync::Arc;
use rand::Rng;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use tokio::time::Duration;
use tracing::info;
use std::collections::{HashMap, HashSet};
use tokio::sync::Mutex;

use crate::monsters::{Monster, MovementPattern};
use crate::monsters::monster_manager::MonsterManager;
use crate::models::ServerMessage;
use crate::lobby::Lobby;

// Constants for monster movement
const DIRECTION_CHANGE_PROBABILITY: f32 = 0.15; // Chance to change direction randomly
const MOVEMENT_UPDATE_INTERVAL_MS: u64 = 2000; // Time between movement updates
const MONSTERS_MOVE_PERCENT: f32 = 0.7; // Percentage of spawn points that have a monster move per update
const ALL_DIRECTIONS: [&str; 4] = ["up", "down", "left", "right"]; // All possible directions

// Handles monster movement logic
pub async fn run_monster_movement(lobbies: Arc<dashmap::DashMap<String, Arc<Lobby>>>) {
    info!("Starting monster movement controller");
    
    loop {
        // Process each lobby independently
        for lobby_entry in lobbies.iter() {
            let lobby = lobby_entry.value().clone();
            
            // Skip lobbies without monster managers
            let monster_manager = lobby.monster_manager.clone();
            
            // Get all monsters in this lobby
            let lobby_monsters = MonsterManager::get_monsters_for_lobby(&lobby);
            
            // Skip if there are no monsters in this lobby
            if lobby_monsters.is_empty() {
                continue;
            }
            
            let mut rng = SmallRng::from_entropy();
            let mut updated_monsters: Vec<Monster> = Vec::new();
            
            // Group monsters by spawn point
            let mut monsters_by_spawn_point: HashMap<String, Vec<Monster>> = HashMap::new();
            
            for monster_mutex in lobby_monsters {
                // Get a copy of the monster by locking and cloning
                let monster = match monster_mutex.try_lock() {
                    Ok(guard) => guard.clone(),
                    Err(_) => continue, // Skip if mutex is locked
                };
                
                // Skip stationary monsters
                if let MovementPattern::Stationary = monster.movement_pattern {
                    continue;
                }
                
                // Skip monsters in combat
                if monster.in_combat {
                    continue;
                }
                
                // Find which spawn point this monster belongs to
                for spawn_point_entry in lobby.monsters_by_spawn_point.iter() {
                    let spawn_point_id = spawn_point_entry.key().clone();
                    let monster_ids = spawn_point_entry.value();
                    
                    if monster_ids.contains(&monster.instance_id) {
                        monsters_by_spawn_point
                            .entry(spawn_point_id)
                            .or_insert_with(Vec::new)
                            .push(monster);
                        break;
                    }
                }
            }
            
            // For each spawn point, determine if monsters should move
            for (spawn_point_id, spawn_monsters) in monsters_by_spawn_point {
                // Skip if no monsters in this spawn point
                if spawn_monsters.is_empty() {
                    continue;
                }
                
                // Apply movement chance to the whole spawn point
                if rng.gen::<f32>() <= MONSTERS_MOVE_PERCENT {
                    // Select one random monster from this spawn point to move
                    let monster_index = rng.gen_range(0..spawn_monsters.len());
                    let monster = &spawn_monsters[monster_index];
                    
                    // Move the monster
                    let updated_monster = move_monster(&monster_manager, &lobby, monster.clone(), &spawn_point_id).await;
                    updated_monsters.push(updated_monster);
                }
            }
            
            // Update all monsters in the lobby
            for updated_monster in &updated_monsters {
                if let Some(monster_entry) = lobby.active_monsters.get(&updated_monster.instance_id) {
                    let mutex = monster_entry.value();
                    if let Ok(mut monster) = mutex.try_lock() {
                        // Update the monster with the new data
                        *monster = updated_monster.clone();
                        
                        // Notify players about the monster movement
                        let monster_move_msg = ServerMessage::MonsterMoved { 
                            monster: updated_monster.to_display()
                        };
                        
                        if let Ok(msg_json) = serde_json::to_string(&monster_move_msg) {
                            let _ = lobby.tx.send(msg_json);
                        }
                    }
                }
            }
        }
        
        // Sleep before the next movement update
        tokio::time::sleep(Duration::from_millis(MOVEMENT_UPDATE_INTERVAL_MS)).await;
    }
}

// Helper function to get direction vectors
fn get_direction_vector(direction: &str) -> (i32, i32) {
    match direction {
        "up" => (0, -1),
        "down" => (0, 1),
        "left" => (-1, 0),
        "right" => (1, 0),
        _ => (0, 0),
    }
}

// Helper function to get the opposite direction
fn get_opposite_direction(direction: &str) -> &'static str {
    match direction {
        "up" => "down",
        "down" => "up",
        "left" => "right",
        "right" => "left",
        _ => "down",
    }
}

// Helper function to get a random direction
fn get_random_direction(exclude_direction: Option<&str>, rng: &mut SmallRng) -> String {
    let mut valid_directions: Vec<&str> = ALL_DIRECTIONS.to_vec();
    
    // Remove the excluded direction if specified
    if let Some(exclude) = exclude_direction {
        valid_directions.retain(|&d| d != exclude);
    }
    
    valid_directions[rng.gen_range(0..valid_directions.len())].to_string()
}

// Helper function to update direction based on movement pattern
fn update_direction_for_movement_pattern(monster: &mut Monster, rng: &mut SmallRng) {
    match &monster.movement_pattern {
        MovementPattern::Random => {
            // Random pattern has a chance to change direction
            if rng.gen::<f32>() < DIRECTION_CHANGE_PROBABILITY {
                monster.direction = get_random_direction(None, rng);
            }
        },
        MovementPattern::Linear | MovementPattern::Patrol { .. } => {
            // These patterns only change direction when needed, not randomly
        },
        MovementPattern::Stationary => {
            // No movement for stationary monsters
        }
    }
}

// Apply movement logic to a monster with boundary and obstacle checks
async fn move_monster(monster_manager: &Arc<MonsterManager>, lobby: &Arc<Lobby>, mut monster: Monster, spawn_point_id: &str) -> Monster {
    let mut rng = SmallRng::from_entropy();
    
    let tile_movement = 2;
    
    // Update direction based on movement pattern before movement
    update_direction_for_movement_pattern(&mut monster, &mut rng);
    
    // Get the spawn point
    let spawn_point = match monster_manager.map_data.spawn_points.get(spawn_point_id) {
        Some(spawn_point) => spawn_point,
        None => return monster,
    };
    
    // Set up boundaries
    let min_x = spawn_point.tile_x;
    let min_y = spawn_point.tile_y;
    let max_x = spawn_point.tile_x + spawn_point.width;
    let max_y = spawn_point.tile_y + spawn_point.height;
    
    // Get valid positions from the monster manager
    let valid_positions = if let Some(valid_pos_map) = monster_manager.map_data.valid_positions.get(spawn_point_id) {
        // Start with the pre-computed positions
        let mut positions = valid_pos_map.valid_positions.clone();
        
        // Remove positions occupied by players
        for player_entry in lobby.player_positions.iter() {
            let player = player_entry.value();
            positions.remove(&(player.x, player.y));
        }
        
        // Remove positions occupied by other monsters
        for monster_entry in lobby.active_monsters.iter() {
            if let Ok(other_monster) = monster_entry.value().try_lock() {
                if other_monster.instance_id != monster.instance_id {
                    positions.remove(&(other_monster.position.x, other_monster.position.y));
                }
            }
        }
        
        positions
    } else {
        HashSet::new()
    };
    
    // Apply movement based on the pattern
    match &monster.movement_pattern {
        MovementPattern::Random => {
            // First try to move in the current direction
            let moves_in_direction = get_moves_in_direction(&monster, tile_movement, &valid_positions);
            
            if !moves_in_direction.is_empty() {
                // Choose the furthest move in current direction
                let (new_x, new_y) = moves_in_direction.last().unwrap();
                monster.position.x = *new_x;
                monster.position.y = *new_y;
            } else {
                // If we couldn't move in current direction, try other directions
                let possible_moves = get_possible_moves(&monster, 1, &valid_positions);
                
                if !possible_moves.is_empty() {
                    // Choose a random valid move
                    let random_index = rng.gen_range(0..possible_moves.len());
                    let (new_x, new_y) = possible_moves[random_index];
                    
                    // Update direction based on the chosen move
                    update_direction_from_move(&mut monster, new_x, new_y);
                    
                    // Move monster to new position
                    monster.position.x = new_x;
                    monster.position.y = new_y;
                }
            }
        },
        MovementPattern::Linear | MovementPattern::Patrol { .. } => {
            // Get possible moves in current direction
            let moves_in_direction = get_moves_in_direction(&monster, tile_movement, &valid_positions);
            
            if !moves_in_direction.is_empty() {
                // Choose the furthest move in current direction
                let (new_x, new_y) = moves_in_direction.last().unwrap();
                monster.position.x = *new_x;
                monster.position.y = *new_y;
            } else {
                // If no valid moves in current direction, reverse direction
                monster.direction = get_opposite_direction(&monster.direction).to_string();
                
                // Try to move one step in the new direction
                let new_moves = get_moves_in_direction(&monster, 1, &valid_positions);
                if !new_moves.is_empty() {
                    let (new_x, new_y) = new_moves.last().unwrap();
                    monster.position.x = *new_x;
                    monster.position.y = *new_y;
                }
            }
        },
        MovementPattern::Stationary => {
            // No movement needed for stationary monsters
        }
    }
    
    // Ensure monster stays within the spawn area boundaries
    monster.position.x = monster.position.x.clamp(min_x, max_x - 1);
    monster.position.y = monster.position.y.clamp(min_y, max_y - 1);
    
    monster
}

// Helper function to update direction based on the move made
fn update_direction_from_move(monster: &mut Monster, new_x: u32, new_y: u32) {
    if new_x < monster.position.x {
        monster.direction = "left".to_string();
    } else if new_x > monster.position.x {
        monster.direction = "right".to_string();
    } else if new_y < monster.position.y {
        monster.direction = "up".to_string();
    } else if new_y > monster.position.y {
        monster.direction = "down".to_string();
    }
}

// Get possible moves in all directions
fn get_possible_moves(monster: &Monster, tile_movement: u32, valid_positions: &HashSet<(u32, u32)>) -> Vec<(u32, u32)> {
    let mut possible_moves = Vec::new();
    let current_x = monster.position.x;
    let current_y = monster.position.y;
    
    // First check moves in current direction
    let current_direction_moves = get_moves_in_direction(monster, tile_movement, valid_positions);
    if !current_direction_moves.is_empty() {
        return current_direction_moves;
    }
    
    // If no moves available in current direction, check all other directions
    // Define all four cardinal directions
    let all_directions = [
        "right",
        "down", 
        "left", 
        "up",
    ];
    
    // Try each direction except the current one
    for &dir in &all_directions {
        if dir == monster.direction {
            continue;
        }
        
        // Create a temporary monster with the new direction
        let mut temp_monster = monster.clone();
        temp_monster.direction = dir.to_string();
        
        // Try to find valid moves in this direction (just one step)
        let moves = get_moves_in_direction(&temp_monster, 1, valid_positions);
        possible_moves.extend(moves);
    }
    
    possible_moves
}

// Get moves in a specific direction (for linear movement)
fn get_moves_in_direction(monster: &Monster, tile_movement: u32, valid_positions: &HashSet<(u32, u32)>) -> Vec<(u32, u32)> {
    let mut moves = Vec::new();
    let current_x = monster.position.x;
    let current_y = monster.position.y;
    
    let (dx, dy) = get_direction_vector(&monster.direction);
    
    for step in 1..=tile_movement {
        // Calculate the new position, handling potential underflow
        let new_x = if dx < 0 && current_x < step as u32 {
            continue; // Would go below 0, skip
        } else {
            (current_x as i32 + dx * step as i32) as u32
        };
        
        let new_y = if dy < 0 && current_y < step as u32 {
            continue; // Would go below 0, skip
        } else {
            (current_y as i32 + dy * step as i32) as u32
        };
        
        if valid_positions.contains(&(new_x, new_y)) {
            moves.push((new_x, new_y));
        } else {
            break; // Stop at first obstacle
        }
    }
    
    moves
} 