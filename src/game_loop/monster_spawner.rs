use std::sync::Arc;
use tokio::time::{Duration, Instant};
use tracing::info;
use rand::{seq::SliceRandom, rngs::SmallRng, SeedableRng, Rng};

use crate::models::ServerMessage;
use crate::lobby::Lobby;

// Configuration for monster spawner behavior
pub struct SpawnerConfig {
    // Percentage of spawn points to process each cycle (0.0 to 1.0)
    pub spawn_percentage: f32,
    // Minimum number of spawn points to process each cycle
    pub min_spawn_points: usize,
    // Sleep time between spawn cycles in milliseconds
    pub cycle_interval_ms: u64,
}

impl Default for SpawnerConfig {
    fn default() -> Self {
        Self {
            spawn_percentage: 0.3,  // Process 30% of spawn points per cycle
            min_spawn_points: 1,    // At least one spawn point per cycle
            cycle_interval_ms: 10000, // 10s between cycles
        }
    }
}

// Handles monster spawning across all areas
pub async fn run_monster_spawner(
    lobbies: Arc<dashmap::DashMap<String, Arc<Lobby>>>,
    config: SpawnerConfig
) {
    info!("Starting monster spawner");
    
    // Track the last spawn time for each spawn point
    let mut last_spawn_attempts = std::collections::HashMap::new();
    
    loop {
        info!("Running monster spawner");
        // Process each lobby
        for lobby_entry in lobbies.iter() {
            let lobby = lobby_entry.value().clone();
            
            // Skip lobbies without monster managers
            let monster_manager = lobby.monster_manager.clone();
            
            // Get all spawn points for this lobby's map
            let all_spawn_points: Vec<_> = monster_manager.map_data.spawn_points.iter().collect();
            
            // If we have no spawn points, skip this lobby
            if all_spawn_points.is_empty() {
                continue;
            }
            
            let mut rng = SmallRng::from_entropy();
            
            // Filter spawn points based on their individual spawn density
            let spawn_points_to_process = all_spawn_points.iter()
                .filter(|(_, spawn_point)| {
                    // If spawn_density is set, use it as probability to include this point
                    if let Some(density) = spawn_point.spawn_density {
                        let random_value: f32 = rng.gen();
                        random_value <= density
                    } else {
                        // Otherwise use global percentage
                        true
                    }
                })
                .collect::<Vec<_>>();
            
            // If using global percentage, select a subset based on config
            let spawn_points_to_process = if spawn_points_to_process.len() > all_spawn_points.len() / 2 {
                // Determine how many spawn points to process this cycle
                let num_to_process = (all_spawn_points.len() as f32 * config.spawn_percentage).max(config.min_spawn_points as f32) as usize;
                let num_to_process = num_to_process.min(all_spawn_points.len());
                
                spawn_points_to_process
                    .choose_multiple(&mut rng, num_to_process)
                    .copied()
                    .collect::<Vec<_>>()
            } else {
                // If individual spawn densities filtered enough, use those
                spawn_points_to_process
            };
            
            // Process selected spawn points for this lobby
            for &(spawn_point_id, spawn_point) in &spawn_points_to_process {
                let now = Instant::now();
                let key = format!("{}:{}", lobby.id, spawn_point_id);
                let last_spawn = last_spawn_attempts.entry(key).or_insert(Instant::now() - Duration::from_secs(spawn_point.spawn_interval_sec));
                
                // Check if it's time to attempt a spawn
                if now.duration_since(*last_spawn).as_secs() >= spawn_point.spawn_interval_sec {
                    // Update last spawn time
                    *last_spawn = now;
                    
                    // Skip if already at max monsters for this spawn point in this lobby
                    let monsters_in_point = crate::monsters::monster_manager::MonsterManager::get_monsters_in_spawn_point(&lobby, spawn_point_id);
                    if monsters_in_point.len() >= spawn_point.max_monsters as usize {
                        continue;
                    }
                    
                    // Calculate how many more monsters we can spawn in this point
                    let slots_available = spawn_point.max_monsters as usize - monsters_in_point.len();
                    
                    // Lower the probability to favor spawning multiple monsters
                    // Determine how many to spawn this cycle (1 to slots_available)
                    let spawn_count = if slots_available > 1 {
                        // Either spawn 1 or a random number up to slots_available
                        if rng.gen_bool(0.3) { 1 } else { rng.gen_range(1..=slots_available) }
                    } else {
                        1
                    };
                    
                    info!("Attempting to spawn {} monsters at spawn point {} in lobby {}", spawn_count, spawn_point_id, lobby.id);
                    
                    // Track how many we actually spawned
                    let mut spawned_count = 0;
                    
                    for _ in 0..spawn_count {
                        // Get a random monster template for this spawn point
                        if let Some(template) = monster_manager.get_random_monster_for_spawn_point(spawn_point_id) {
                            // Use the numeric ID directly
                            if let Some(new_monster) = monster_manager.spawn_monster(template.id, spawn_point_id, &lobby).await {
                                spawned_count += 1;
                                info!("Spawned monster: {} (level {}) at spawn point {}, position: ({}, {}) in lobby {} [{}/{}]", 
                                    new_monster.name, new_monster.level, spawn_point_id,
                                    new_monster.position.x, new_monster.position.y, lobby.id,
                                    spawned_count, spawn_count);
                                
                                // Notify only this lobby about the new monster
                                let monster_spawn_msg = ServerMessage::MonsterSpawned { monster: new_monster.to_display() };
                                let monster_json = serde_json::to_string(&monster_spawn_msg).unwrap();
                                
                                let _ = lobby.tx.send(monster_json);
                            }
                        }
                    }
                }
            }
        }
        
        // Sleep to avoid high CPU usage
        tokio::time::sleep(Duration::from_millis(config.cycle_interval_ms)).await;
    }
} 