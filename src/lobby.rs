use crate::app_state::AppState;
use crate::models::{Lobby, PlayerState, ServerMessage};
use std::sync::Arc;
use tokio::time::Instant;
use tokio::time::Duration;
use regex::Regex;

// Create a new lobby or get an existing one
pub fn get_or_create_lobby(state: &Arc<AppState>, lobby_id: &str) -> Arc<Lobby> {
    state.lobbies.entry(lobby_id.to_string()).or_insert_with(|| {
        let (tx, _) = tokio::sync::broadcast::channel(state.config.performance.broadcast_channel_size);
        Arc::new(Lobby {
            id: lobby_id.to_string(),
            player_positions: dashmap::DashMap::new(),
            player_last_active: dashmap::DashMap::new(),
            tx,
        })
    }).clone()
}

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
            lobby.player_positions.remove(&player_id);
            lobby.player_last_active.remove(&player_id);

            // Notify other players in the lobby
            let leave_msg = ServerMessage::PlayerLeft { id: player_id };
            let _ = lobby.tx.send(serde_json::to_string(&leave_msg).unwrap());
        }
    }
} 