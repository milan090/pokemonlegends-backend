use crate::app_state::AppState;
use crate::models::{PlayerState, ServerMessage};
use crate::monsters::monster_manager::MonsterManager;
use crate::monsters::Monster;
use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::{broadcast, Mutex};
use tokio::time::Instant;
use tokio::time::Duration;
use regex::Regex;
use axum::extract::ws::Message;
use futures_util::{SinkExt, stream::SplitSink};
use axum::extract::ws::{Utf8Bytes, WebSocket};

// Lobby struct representing a game lobby
pub struct Lobby {
    pub id: String,
    pub player_positions: DashMap<String, PlayerState>,
    pub player_last_active: DashMap<String, Instant>,
    pub tx: broadcast::Sender<String>,
    pub map_id: String,  // Map ID for this lobby
    pub active_monsters: DashMap<String, Arc<Mutex<Monster>>>, // Monster instance ID → Monster
    pub monsters_by_spawn_point: DashMap<String, Vec<String>>, // Spawn point ID → Monster IDs
    pub monster_manager: Arc<MonsterManager>, // Lobby-specific monster manager
    pub player_connections: DashMap<String, Arc<tokio::sync::Mutex<SplitSink<WebSocket, Message>>>>, // Player ID → WebSocket sender
} 

impl Lobby {
    // Send a message to a specific player in the lobby
    pub async fn send_to_player(&self, player_id: &str, message: &ServerMessage) -> Result<(), String> {
        if let Some(sender) = self.player_connections.get(player_id) {
            let message_json = serde_json::to_string(message)
                .map_err(|e| format!("Failed to serialize message: {}", e))?;
            
            let mut sender_lock = sender.lock().await;
            sender_lock.send(Message::Text(Utf8Bytes::from(message_json)))
                .await
                .map_err(|e| format!("Failed to send message to player {}: {}", player_id, e))?;
            Ok(())
        } else {
            Err(format!("Player {} not found in lobby", player_id))
        }
    }
    
    // Broadcast a message to all players in the lobby except for specific players
    pub async fn broadcast_except(&self, message: &ServerMessage, exclude_player_ids: &[&str]) -> Result<(), String> {
        let message_json = serde_json::to_string(message)
            .map_err(|e| format!("Failed to serialize message: {}", e))?;
            
        let _ = self.tx.send(message_json);
        Ok(())
    }
}

// Create a new lobby or get an existing one
// pub async fn get_or_create_lobby(state: &Arc<AppState>, lobby_id: &str, map_id: Option<&str>) -> Arc<Lobby> {
//     let map_id_str = map_id.unwrap_or("map1").to_string();
    
//     // Check if the lobby already exists
//     if let Some(lobby_ref) = state.lobbies.get(lobby_id) {
//         return lobby_ref.clone();
//     }
    
//     // Create a new lobby
//     let (tx, _) = tokio::sync::broadcast::channel(state.config.performance.broadcast_channel_size);
    
//     // Create lobby without monster manager first
//     let new_lobby = Arc::new(Lobby {
//         id: lobby_id.to_string(),
//         player_positions: dashmap::DashMap::new(),
//         player_last_active: dashmap::DashMap::new(),
//         tx: tx.clone(),
//         map_id: map_id_str.clone(),
//         active_monsters: dashmap::DashMap::new(),
//         monsters_by_spawn_point: dashmap::DashMap::new(),
//         monster_manager: None,
//     });
    
//     // Create monster manager for the lobby if factory is available
//     if let Some(factory) = &state.monster_manager_factory {
//         let monster_manager = factory.create_monster_manager(&map_id_str).await;
        
//         if let Ok(manager) = monster_manager {
//             // Create a new lobby with the monster manager
//             let lobby_with_manager = Arc::new(Lobby {
//                 id: lobby_id.to_string(),
//                 player_positions: dashmap::DashMap::new(),
//                 player_last_active: dashmap::DashMap::new(),
//                 tx,
//                 map_id: map_id_str,
//                 active_monsters: dashmap::DashMap::new(),
//                 monsters_by_spawn_point: dashmap::DashMap::new(),
//                 monster_manager: Some(manager),
//             });
            
//             // Store the lobby in the app state
//             state.lobbies.insert(lobby_id.to_string(), lobby_with_manager.clone());
//             return lobby_with_manager;
//         } else {
//             tracing::warn!("Failed to create monster manager for lobby {}: {:?}", lobby_id, monster_manager.err());
//         }
//     }
    
//     // Store the lobby without monster manager as fallback
//     state.lobbies.insert(lobby_id.to_string(), new_lobby.clone());
//     new_lobby
// }

// Get an existing lobby
pub fn get_lobby(state: &Arc<AppState>, lobby_id: &str) -> Option<Arc<Lobby>> {
    state.lobbies.get(lobby_id).map(|lobby_ref| lobby_ref.clone())
}

// Validate lobby ID format
pub fn validate_lobby_id(lobby_id: &str) -> bool {
    let re = Regex::new(r"^[A-Z0-9]{4}-[A-Z0-9]{4}$").unwrap();
    re.is_match(lobby_id)
}

// Clean up inactive players from lobbies
pub async fn cleanup_inactive_lobbies(state: &Arc<AppState>) {
    let now = Instant::now();
    let timeout = Duration::from_secs(state.config.game.inactive_timeout_sec);

    // Go through each lobby
    for lobby_ref in state.lobbies.iter() {
        let lobby = lobby_ref.value();
        
        // Find inactive players in this lobby
        let inactive_players: Vec<String> = lobby
            .player_last_active
            .iter()
            .filter_map(|entry| {
                let id = entry.key().clone();
                let last_active = *entry.value();
                if now.duration_since(last_active) > timeout {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        

        // Remove inactive players from this lobby
        for player_id in inactive_players {
            tracing::info!("Removing inactive player from lobby {}: {}", lobby.id, player_id);
            
            // Before removing the player from memory, make sure their state is persisted
            if let Some(player_state) = lobby.player_positions.get(&player_id) {
                // Connect to Redis to save player state
                if let Ok(mut redis_conn) = state.redis.get_async_connection().await {
                    let state_json = serde_json::to_string(player_state.value()).unwrap_or_default();
                    let _ = crate::redis_manager::store_player_state(
                        &mut redis_conn,
                        &lobby.id,
                        &player_id,
                        &state_json,
                    ).await.unwrap_or_else(|e| {
                        tracing::error!("Failed to persist player state during cleanup: {}", e);
                    });
                }
            }
            
            lobby.player_positions.remove(&player_id);
            lobby.player_last_active.remove(&player_id);
            lobby.player_connections.remove(&player_id);

            // Notify other players in the lobby
            let leave_msg = ServerMessage::PlayerLeft { id: player_id };
            let _ = lobby.tx.send(serde_json::to_string(&leave_msg).unwrap());
        }
    }
} 