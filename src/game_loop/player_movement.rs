use std::sync::Arc;
use std::time::{Duration, Instant};
use chrono::Utc;
use tokio::time;
use dashmap::{DashMap, DashSet};
use tracing::{info, warn};
use serde_json::json;

use crate::models::{Lobby, PlayerState, ServerMessage};
use crate::app_state::AppState;

// Constants for player movement
const MOVEMENT_UPDATE_INTERVAL_MS: u64 = 50; // Send updates every 100ms
const MAX_PLAYER_SPEED: u32 = 3; // Maximum allowed movement in a single validation step
const MIN_MOVEMENT_VALIDATION_INTERVAL_MS: u64 = 50; // Minimum time between movement validations

pub struct PlayerMovementManager {
    // Track which players have moved since the last update
    moved_players: DashSet<String>,
    // Track last validated positions of players for validation
    last_validated_positions: DashMap<String, (PlayerState, Instant)>,
    // Track last broadcast state for each player
    last_broadcast_states: DashMap<String, PlayerState>,
}

impl PlayerMovementManager {
    pub fn new() -> Self {
        Self {
            moved_players: DashSet::new(),
            last_validated_positions: DashMap::new(),
            last_broadcast_states: DashMap::new(),
        }
    }

    // Validate a player movement request
    pub fn validate_movement(&self, player_id: &str, current_state: &PlayerState, new_x: u32, new_y: u32) -> bool {
        // Get the last validated position and timestamp
        if let Some(entry) = self.last_validated_positions.get(player_id) {
            let (last_state, last_update_time) = entry.value();
            let time_since_last_update = last_update_time.elapsed().as_millis() as u64;
            
            // Ensure minimum time between validations
            if time_since_last_update < MIN_MOVEMENT_VALIDATION_INTERVAL_MS {
                return false;
            }
            
            // Calculate distance moved
            let dx = if new_x > last_state.x { new_x - last_state.x } else { last_state.x - new_x };
            let dy = if new_y > last_state.y { new_y - last_state.y } else { last_state.y - new_y };
            
            // Check if movement is within allowed speed
            let allowed_distance = MAX_PLAYER_SPEED * (1 + (time_since_last_update as u32 / 100));
            if dx > allowed_distance || dy > allowed_distance {
                warn!("Player {} attempted to move too quickly: dx={}, dy={}, allowed={}", 
                    player_id, dx, dy, allowed_distance);
                return false;
            }
        }
        
        // Check map boundaries and collisions if needed
        // (implement collision detection here if needed)
        
        true
    }

    // Check if player state has meaningful changes compared to last broadcast
    fn has_meaningful_changes(&self, player_id: &str, current_state: &PlayerState) -> bool {
        if let Some(last_state) = self.last_broadcast_states.get(player_id) {
            // Check if position or direction has changed
            return current_state.x != last_state.x || 
                   current_state.y != last_state.y || 
                   current_state.direction != last_state.direction;
        }
        
        // If no previous state exists, always consider it a change
        true
    }

    // Register a validated movement
    pub fn register_movement(&self, player_id: String, state: PlayerState) {
        self.last_validated_positions.insert(player_id.clone(), (state.clone(), Instant::now()));
        
        // Only register for broadcast if meaningful changes occurred
        if self.has_meaningful_changes(&player_id, &state) {
            self.moved_players.insert(player_id);
        }
    }

    // Update the last broadcast state after sending updates
    pub fn update_broadcast_states(&self, players: &[PlayerState]) {
        for player in players {
            self.last_broadcast_states.insert(player.id.clone(), player.clone());
        }
    }

    // Clear the moved players list after sending updates
    pub fn clear_moved_players(&self) {
        self.moved_players.clear();
    }

    // Get list of players that have moved since last update
    pub fn get_moved_players(&self, lobby: &Lobby) -> Vec<PlayerState> {
        self.moved_players
            .iter()
            .filter_map(|player_id| {
                lobby.player_positions.get(player_id.key()).map(|entry| entry.value().clone())
            })
            .collect()
    }
}

// Main task to periodically send movement updates
pub async fn run_player_movement_controller(app_state: Arc<AppState>, movement_manager: Arc<PlayerMovementManager>) {
    info!("Starting player movement controller");
    let mut interval = time::interval(Duration::from_millis(MOVEMENT_UPDATE_INTERVAL_MS));
    
    loop {
        interval.tick().await;
        
        // Iterate through all lobbies
        for lobby_entry in app_state.lobbies.iter() {
            let lobby = lobby_entry.value();
            
            // Get players that have moved
            let moved_players = movement_manager.get_moved_players(lobby);
            
            // Only send updates if there are players that moved
            if !moved_players.is_empty() {
                // Create batch update message
                let batch_update = ServerMessage::PlayersMoved { 
                    players: moved_players.clone(),
                    timestamp: Utc::now().timestamp_millis() as u64
                };
                
                // Send the batch update to all players in the lobby
                let _ = lobby.tx.send(serde_json::to_string(&batch_update).unwrap());
                
                // Update the last broadcast states
                movement_manager.update_broadcast_states(&moved_players);
            }
        }
        
        // Clear the moved players list
        movement_manager.clear_moved_players();
    }
} 