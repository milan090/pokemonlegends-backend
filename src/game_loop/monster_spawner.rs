use std::sync::Arc;
use tokio::time::{Duration, Instant};
use tracing::info;

use crate::monsters::monster_manager::MonsterManager;
use crate::models::{Lobby, ServerMessage};

// Handles monster spawning across all areas
pub async fn run_monster_spawner(monster_manager: Arc<MonsterManager>, lobbies: Arc<dashmap::DashMap<String, Arc<Lobby>>>) {
    info!("Starting monster spawner");
    
    // Track the last spawn time for each area
    let mut last_spawn_attempts = std::collections::HashMap::new();
    
    loop {
        // Process each spawn area
        for (area_id, area) in &monster_manager.spawn_areas {
            let now = Instant::now();
            let last_spawn = last_spawn_attempts.entry(area_id.clone()).or_insert(Instant::now() - Duration::from_secs(area.spawn_interval_sec));
            
            // Check if it's time to attempt a spawn
            if now.duration_since(*last_spawn).as_secs() >= area.spawn_interval_sec {
                // Update last spawn time
                *last_spawn = now;
                
                // Skip if already at max monsters
                let monsters_in_area = monster_manager.get_monsters_in_area(area_id).await;
                if monsters_in_area.len() >= area.max_monsters as usize {
                    continue;
                }
                
                // Get a random monster template for this area
                if let Some(template) = monster_manager.get_random_monster_for_area(area_id) {
                    // Attempt to spawn the monster
                    if let Some(new_monster) = monster_manager.spawn_monster(&template.id, area_id).await {
                        info!("Spawned monster: {} ({}), position: ({}, {})", 
                            new_monster.name, new_monster.instance_id, 
                            new_monster.position.x, new_monster.position.y);
                        
                        // Notify all lobbies about the new monster
                        let monster_spawn_msg = ServerMessage::MonsterSpawned { monster: new_monster.clone() };
                        let monster_json = serde_json::to_string(&monster_spawn_msg).unwrap();
                        
                        for lobby_entry in lobbies.iter() {
                            let _ = lobby_entry.tx.send(monster_json.clone());
                        }
                    }
                }
            }
        }
        
        // Sleep to avoid high CPU usage
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
} 