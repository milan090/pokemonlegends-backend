use crate::app_state::AppState;
use crate::models::{ClientMessage, PlayerState, ServerMessage, DisplayPokemon};
use crate::lobby::{Lobby, validate_lobby_id, get_lobby};
use crate::redis_manager;
use crate::game_loop;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Path, Query,
    },
    response::IntoResponse,
    Json,
};
use axum::extract::ws::Utf8Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;
use tracing::{info, error, warn};
use uuid::Uuid;
use std::sync::Arc;
use crate::monsters::monster_manager::MonsterManager;
use crate::monsters::Monster;
use crate::monsters::monster::DisplayMonster;
use tokio::sync::Mutex;

// Public lobbies endpoint to fetch list of active lobbies
pub async fn public_lobbies_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let lobbies = state.lobbies.iter().map(|entry| {
        let lobby = entry.value();
        let players_count = lobby.player_positions.len();
        serde_json::json!({
            "lobby_id": lobby.id,
            "players": players_count
        })
    }).collect::<Vec<_>>();
    Json(lobbies)
}

// Handler for lobby websocket connections
pub async fn ws_lobby_handler(
    Path(lobby_id): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !validate_lobby_id(&lobby_id) {
        return (axum::http::StatusCode::BAD_REQUEST, "Invalid lobby id").into_response();
    }
    
    // Extract and validate username
    let username = match params.get("username") {
        Some(username) => {
            if !validate_username(username) {
                return (axum::http::StatusCode::BAD_REQUEST, "Invalid username format. Only alphanumeric characters and underscores are allowed.").into_response();
            }
            username.clone()
        },
        None => {
            return (axum::http::StatusCode::BAD_REQUEST, "Username is required").into_response();
        }
    };
    
    // Get the lobby, but don't create if it doesn't exist
    match get_lobby(&state, &lobby_id) {
        Some(lobby) => ws.on_upgrade(move |socket| handle_lobby_socket(socket, state, lobby, username)),
        None => (axum::http::StatusCode::NOT_FOUND, "Lobby not found").into_response()
    }
}

// Validate username format: only alphanumeric and underscores allowed
fn validate_username(username: &str) -> bool {
    if username.is_empty() || username.len() > 20 {
        return false;
    }
    
    username.chars().all(|c| c.is_alphanumeric() || c == '_')
}

// Health check endpoint
pub async fn health_handler() -> impl IntoResponse {
    "OK"
}

// Handle WebSocket connection for a lobby
pub async fn handle_lobby_socket(socket: WebSocket, state: Arc<AppState>, lobby: Arc<Lobby>, username: String) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(tokio::sync::Mutex::new(sender));
    
    // Clone state for usage throughout this function
    let state_for_tasks = state.clone();
    let state_for_disconnect = state.clone();

    // Wait for join message with session token
    let session_token = if let Some(Ok(Message::Text(text))) = receiver.next().await {
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Join { session_token }) => session_token,
            _ => {
                tracing::error!("First message must be a join message with session token");
                return;
            }
        }
    } else {
        tracing::error!("Failed to receive join message");
        return;
    };

    // Connect to Redis
    let mut redis_conn = state_for_tasks.redis.get_async_connection().await.expect("Failed to connect to Redis");
    
    // Retrieve or create player id via Redis
    let player_id = match redis_manager::get_player_id(&mut redis_conn, &session_token).await {
        Ok(id) => {
            tracing::info!("Found existing player ID {} for session {}", id, session_token);
            id
        },
        Err(_) => {
            // Create a new player ID
            let new_id = Uuid::new_v4().to_string();
            tracing::info!("Created new player ID {} for session {}", new_id, session_token);
            new_id
        }
    };
    tracing::info!("Player connected to lobby {}: {} (session: {})", lobby.id, player_id, session_token);

    // Store session-token mapping in Redis with permanent persistence
    // We use a constant 10-year timeout (function will handle this)
    redis_manager::store_session(
        &mut redis_conn, 
        &session_token, 
        &player_id,
        0 // The actual value doesn't matter, the function uses a long-term constant
    ).await.expect("Failed to store session in Redis");

    // Create a broadcast receiver for lobby events
    let mut rx = lobby.tx.subscribe();

    // Retrieve or initialize player state from Redis
    let player_state = match redis_manager::get_player_state(&mut redis_conn, &lobby.id, &player_id).await {
        Ok(state_json) => match serde_json::from_str::<PlayerState>(&state_json) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!("Error deserializing player state: {}", e);
                return;
            }
        },
        Err(_) => {
            // Create a new player state with random position
            let new_state = PlayerState {
                id: player_id.clone(),
                username: username.clone(),
                x: 5 + rand::random::<u32>() % 16,
                y: 5,
                direction: "down".to_string(),
                in_combat: false,
            };
            
            // Store the new state in Redis
            let state_json = serde_json::to_string(&new_state).unwrap();
            if let Err(e) = redis_manager::store_player_state(
                &mut redis_conn,
                &lobby.id,
                &player_id,
                &state_json,
            ).await {
                tracing::error!("Failed to store new player state in Redis: {}", e);
            }
            
            new_state
        }
    };
    tracing::info!("Player state in lobby {}: {:?}", lobby.id, player_state);

    // Add player to lobby state
    lobby.player_positions.insert(player_id.clone(), player_state.clone());
    lobby.player_last_active.insert(player_id.clone(), Instant::now());
    
    // Store the WebSocket sender in the lobby's player_connections map
    lobby.player_connections.insert(player_id.clone(), sender.clone());

    // Send welcome message
    let welcome_msg = ServerMessage::Welcome { 
        id: player_id.clone(), 
        username: player_state.username.clone(),
        x: player_state.x, 
        y: player_state.y 
    };
    if let Err(e) = sender.lock().await.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&welcome_msg).unwrap()))).await {
        tracing::error!("Failed to send welcome message: {}", e);
        return;
    }

    // Send current players in lobby
    let players = lobby.player_positions.iter().map(|entry| entry.value().clone()).collect::<Vec<_>>();
    let players_msg = ServerMessage::Players { players };
    if let Err(e) = sender.lock().await.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&players_msg).unwrap()))).await {
        tracing::error!("Failed to send players message: {}", e);
        return;
    }

    // Send current monsters to the player if monster manager exists
    let monsters = MonsterManager::get_monsters_for_lobby(&lobby);
    let display_monsters = monsters.iter()
        .filter_map(|m| {
            if let Ok(monster) = m.try_lock() {
                Some(monster.to_display())
            } else {
                None
            }
        })
        .collect();
    let monsters_msg = ServerMessage::Monsters { monsters: display_monsters };
    if let Err(e) = sender.lock().await.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&monsters_msg).unwrap()))).await {
        tracing::error!("Failed to send monsters message: {}", e);
        return;
    }

    // Send player's Pokémon collection
    if let Some(pokemon_collection_manager) = &state_for_tasks.pokemon_collection_manager {
        match pokemon_collection_manager.get_active_pokemons(&player_id).await {
            Ok(pokemons) => { 
                // Convert Vec<Pokemon> to Vec<DisplayPokemon>
                let display_pokemons: Vec<DisplayPokemon> = pokemons
                    .iter()
                    .map(|p| pokemon_collection_manager.pokemon_to_display_pokemon(p))
                    .collect();

                // Use the converted Vec<DisplayPokemon>
                let active_pokemons_msg: ServerMessage = ServerMessage::ActivePokemons { pokemons: display_pokemons };
                    
                info!("Sending pokemon collection message to player {}: {:?}", player_id, active_pokemons_msg);
                if let Err(e) = sender.lock().await.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&active_pokemons_msg).unwrap()))).await {
                    tracing::error!("Failed to send pokemon collection message: {}", e);
                }
            },
            Err(e) => {
                tracing::error!("Failed to fetch pokemon collection for player {}: {}", player_id, e);
            }
        }
    }

    // Notify others in lobby about the new player
    let new_player_msg = ServerMessage::PlayerJoined { player: player_state };
    let _ = lobby.tx.send(serde_json::to_string(&new_player_msg).unwrap());

    // Clone references for tasks
    let player_id_for_receiver = player_id.clone();
    let lobby_for_receiver = lobby.clone();
    let sender_for_receiver = sender.clone();
    let player_id_for_forward = player_id.clone();
    let lobby_for_forward = lobby.clone();

    // Handle incoming messages from the player
    let mut player_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                // Update last active timestamp in lobby
                lobby_for_receiver.player_last_active.insert(player_id_for_receiver.clone(), Instant::now());
                info!("Received message: {}", text);
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Join { session_token }) => {
                    },
                    Ok(ClientMessage::Move { x, y, direction }) => {
                        // Get current player state
                        let current_state = match lobby_for_receiver.player_positions.get(&player_id_for_receiver) {
                            Some(state) => state.value().clone(),
                            None => continue,
                        };

                        // Validate movement with the movement manager
                        let movement_valid = state_for_tasks.player_movement_manager.as_ref().unwrap()
                            .validate_movement(&player_id_for_receiver, &current_state, x, y);

                        // Only process movement if it's valid
                        if movement_valid {
                            let updated_player = PlayerState {
                                id: player_id_for_receiver.clone(),
                                username: current_state.username.clone(),
                                x,
                                y,
                                direction,
                                in_combat: current_state.in_combat,
                            };
                            
                            // Update player state in Redis
                            if let Ok(mut redis_conn) = state_for_tasks.redis.get_async_connection().await {
                                let player_json = serde_json::to_string(&updated_player).unwrap();
                                let _ = redis_manager::store_player_state(
                                    &mut redis_conn,
                                    &lobby_for_receiver.id,
                                    &player_id_for_receiver,
                                    &player_json,
                                ).await.unwrap_or_else(|e| {
                                    tracing::error!("Failed to persist player state: {}", e);
                                });
                            }
                            
                            // Update player state in lobby
                            lobby_for_receiver.player_positions.insert(player_id_for_receiver.clone(), updated_player.clone());
                            
                            // Register the validated movement in the movement manager
                            state_for_tasks.player_movement_manager.as_ref().unwrap()
                                .register_movement(player_id_for_receiver.clone(), updated_player.clone());
                        } else {
                            // Send correction message to the client who tried invalid movement
                            let correction_msg = ServerMessage::PlayersMoved { 
                                players: vec![current_state],
                                timestamp: Utc::now().timestamp_millis() as u64
                            };
                            
                            if let Err(e) = sender_for_receiver.lock().await.send(
                                Message::Text(Utf8Bytes::from(serde_json::to_string(&correction_msg).unwrap()))
                            ).await {
                                tracing::error!("Failed to send correction message: {}", e);
                            }
                        }                    },
                    Ok(ClientMessage::Ping) => {
                        let pong_msg = ServerMessage::Pong;
                        let mut sender_lock = sender_for_receiver.lock().await;
                        if let Err(e) = sender_lock.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&pong_msg).unwrap()))).await {
                            tracing::error!("Failed to send pong message: {}", e);
                            break;
                        }
                    },
                    Ok(ClientMessage::Interact { monster_id }) => {
                        handle_player_interaction(&state_for_tasks, &lobby_for_receiver.id, &player_id_for_receiver, &monster_id).await;
                    },
                    Ok(ClientMessage::ChooseStarter { starter_id }) => {
                        let pokemon_collection_manager = state_for_tasks.pokemon_collection_manager.as_ref().unwrap();
                        let display_pokemon = match pokemon_collection_manager.choose_starting_pokemons(&player_id_for_receiver, starter_id).await {
                            Ok(pokemon) => pokemon,
                            Err(e) => {
                                tracing::error!("Failed to choose starter pokemon: {}", e);
                                let error_msg = ServerMessage::Error { message: format!("Failed to choose starter: {}", e) };
                                if let Err(send_err) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &error_msg).await {
                                    tracing::error!("Failed to send error message to player {}: {}", player_id_for_receiver, send_err);
                                }
                                continue;
                            }
                        };
                        let new_pokemon_msg = ServerMessage::NewPokemon {
                            pokemon: display_pokemon, 
                            active_index: Some(0),
                        };
                        if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &new_pokemon_msg).await {
                            tracing::error!("Failed to send new pokemon message: {}", e);
                        }
                    },
                    Ok(ClientMessage::CombatAction { battle_id, action }) => {
                        // Get the battle manager
                        if let Some(battle_manager) = state_for_tasks.battle_manager.as_ref() {
                            // Handle the action, passing the lobby for message sending
                            match battle_manager.handle_player_action(&player_id_for_receiver, battle_id, action, &lobby_for_receiver, state_for_tasks.pokemon_collection_manager.as_ref().unwrap()).await {
                                Ok(_) => {
                                    info!("Player {} submitted action for battle {} and turn was processed", player_id_for_receiver, battle_id);
                                },
                                Err(e) => {
                                    error!("Failed to handle player action for battle {}: {}", battle_id, e);
                                    // Send error back to the player
                                    let error_msg = ServerMessage::Error { message: e };
                                    if let Err(send_err) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &error_msg).await {
                                        error!("Failed to send error message to player {}: {}", player_id_for_receiver, send_err);
                                    }
                                }
                            }
                        } else {
                            error!("Battle manager not found when handling combat action");
                        }
                    },
                    Ok(ClientMessage::ChallengePlayer { target_player_id }) => {
                        info!("Player {} is challenging player {}", player_id_for_receiver, target_player_id);
                        
                        // Verify challenger is not in combat
                        if let Some(challenger_state) = lobby_for_receiver.player_positions.get(&player_id_for_receiver) {
                            if challenger_state.value().in_combat {
                                let challenge_failed_msg = ServerMessage::ChallengeFailed { 
                                    reason: "You are already in combat".to_string() 
                                };
                                if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &challenge_failed_msg).await {
                                    error!("Failed to send challenge failed message: {}", e);
                                }
                                continue;
                            }
                            
                            // Check if player is trying to challenge themselves
                            if player_id_for_receiver == target_player_id {
                                let challenge_failed_msg = ServerMessage::ChallengeFailed { 
                                    reason: "You cannot challenge yourself".to_string() 
                                };
                                if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &challenge_failed_msg).await {
                                    error!("Failed to send challenge failed message: {}", e);
                                }
                                continue;
                            }
                            
                            // Verify target player exists and is online
                            if let Some(target_player_state) = lobby_for_receiver.player_positions.get(&target_player_id) {
                                // Verify target player is not in combat
                                if target_player_state.value().in_combat {
                                    let challenge_failed_msg = ServerMessage::ChallengeFailed { 
                                        reason: "Target player is already in combat".to_string() 
                                    };
                                    if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &challenge_failed_msg).await {
                                        error!("Failed to send challenge failed message: {}", e);
                                    }
                                    continue;
                                }
                                
                                // All checks passed, send challenge to target player
                                let challenge_received_msg = ServerMessage::ChallengeReceived { 
                                    challenger_id: player_id_for_receiver.clone(),
                                    challenger_username: challenger_state.value().username.clone(),
                                };
                                
                                if let Err(e) = lobby_for_receiver.send_to_player(&target_player_id, &challenge_received_msg).await {
                                    // Target player might have disconnected
                                    let challenge_failed_msg = ServerMessage::ChallengeFailed { 
                                        reason: "Failed to send challenge to target player".to_string() 
                                    };
                                    if let Err(send_err) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &challenge_failed_msg).await {
                                        error!("Failed to send challenge failed message: {}", send_err);
                                    }
                                    error!("Failed to send challenge to target player: {}", e);
                                }
                            } else {
                                // Target player not found
                                let challenge_failed_msg = ServerMessage::ChallengeFailed { 
                                    reason: "Target player not found".to_string() 
                                };
                                if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &challenge_failed_msg).await {
                                    error!("Failed to send challenge failed message: {}", e);
                                }
                            }
                        }
                    },
                    Ok(ClientMessage::RespondToChallenge { challenger_id, accepted }) => {
                        info!("Player {} is responding to challenge from {}: accepted={}", player_id_for_receiver, challenger_id, accepted);
                        
                        // TODO: Security improvement needed - maintain a record of active challenges
                        // to verify that 'challenger_id' actually sent a challenge to this player.
                        // Without this check, players could fake responses to challenges that were never sent.
                        // This could be implemented with a DashMap of active challenges in the Lobby struct
                        // or by adding a temporary challenge_requests field to PlayerState.
                        
                        // Verify both players exist and are online
                        if !lobby_for_receiver.player_positions.contains_key(&challenger_id) {
                            let response_failed_msg = ServerMessage::ChallengeFailed { 
                                reason: "Challenger is no longer online".to_string() 
                            };
                            if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &response_failed_msg).await {
                                error!("Failed to send response failed message: {}", e);
                            }
                            continue;
                        }
                        
                        // Verify neither player is in combat
                        let challenger_in_combat = lobby_for_receiver.player_positions.get(&challenger_id)
                            .map(|state| state.value().in_combat)
                            .unwrap_or(false);
                            
                        let responder_in_combat = lobby_for_receiver.player_positions.get(&player_id_for_receiver)
                            .map(|state| state.value().in_combat)
                            .unwrap_or(false);
                        
                        if challenger_in_combat || responder_in_combat {
                            let response_failed_msg = ServerMessage::ChallengeFailed { 
                                reason: "A player is already in combat".to_string() 
                            };
                            if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &response_failed_msg).await {
                                error!("Failed to send response failed message: {}", e);
                            }
                            
                            // Also notify the challenger if they're not in combat
                            if !challenger_in_combat {
                                if let Err(e) = lobby_for_receiver.send_to_player(&challenger_id, &response_failed_msg).await {
                                    error!("Failed to send response failed message to challenger: {}", e);
                                }
                            }
                            
                            continue;
                        }
                        
                        // Get responder's username
                        let responder_username = lobby_for_receiver.player_positions.get(&player_id_for_receiver)
                            .map(|state| state.value().username.clone())
                            .unwrap_or_else(|| "Unknown".to_string());
                        
                        // Send response to challenger
                        let challenge_response_msg = ServerMessage::ChallengeResponse { 
                            target_player_id: player_id_for_receiver.clone(),
                            target_username: responder_username,
                            accepted,
                        };
                        
                        if let Err(e) = lobby_for_receiver.send_to_player(&challenger_id, &challenge_response_msg).await {
                            error!("Failed to send challenge response to challenger: {}", e);
                            continue;
                        }
                        
                        // If accepted, proceed to start the PvP battle
                        if accepted {
                            info!("Challenge accepted! Starting PvP battle between {} and {}", challenger_id, player_id_for_receiver);
                            
                            // Get the battle manager
                            if let Some(battle_manager) = state_for_tasks.battle_manager.as_ref() {
                                // Mark both players as in combat (temporarily) to prevent race conditions
                                if let Some(mut challenger_state) = lobby_for_receiver.player_positions.get_mut(&challenger_id) {
                                    challenger_state.value_mut().in_combat = true;
                                }
                                
                                if let Some(mut responder_state) = lobby_for_receiver.player_positions.get_mut(&player_id_for_receiver) {
                                    responder_state.value_mut().in_combat = true;
                                }
                                
                                // Call the battle manager to start the battle
                                match battle_manager.start_pvp_battle(
                                    &challenger_id, 
                                    &player_id_for_receiver, 
                                    &lobby_for_receiver, 
                                    state_for_tasks.pokemon_collection_manager.as_ref().unwrap()
                                ).await {
                                    Ok(battle_id) => {
                                        info!("PvP battle {} successfully started", battle_id);
                                        // Battle state is now managed by the battle manager
                                    },
                                    Err(e) => {
                                        error!("Failed to start PvP battle: {}", e);
                                        
                                        // Send error message to both players
                                        let error_msg = ServerMessage::Error {
                                            message: format!("Failed to start battle: {}", e),
                                        };
                                        
                                        if let Err(send_err) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &error_msg).await {
                                            error!("Failed to send error message to responder: {}", send_err);
                                        }
                                        
                                        if let Err(send_err) = lobby_for_receiver.send_to_player(&challenger_id, &error_msg).await {
                                            error!("Failed to send error message to challenger: {}", send_err);
                                        }
                                        
                                        // Reset combat status for both players
                                        if let Some(mut challenger_state) = lobby_for_receiver.player_positions.get_mut(&challenger_id) {
                                            challenger_state.value_mut().in_combat = false;
                                        }
                                        
                                        if let Some(mut responder_state) = lobby_for_receiver.player_positions.get_mut(&player_id_for_receiver) {
                                            responder_state.value_mut().in_combat = false;
                                        }
                                    }
                                }
                            } else {
                                error!("Battle manager not available for PvP battle");
                                
                                // Send error message to both players
                                let error_msg = ServerMessage::Error {
                                    message: "Battle system unavailable".to_string(),
                                };
                                
                                if let Err(e) = lobby_for_receiver.send_to_player(&player_id_for_receiver, &error_msg).await {
                                    error!("Failed to send error message to responder: {}", e);
                                }
                                
                                if let Err(e) = lobby_for_receiver.send_to_player(&challenger_id, &error_msg).await {
                                    error!("Failed to send error message to challenger: {}", e);
                                }
                                
                                // Reset combat status for both players
                                if let Some(mut challenger_state) = lobby_for_receiver.player_positions.get_mut(&challenger_id) {
                                    challenger_state.value_mut().in_combat = false;
                                }
                                
                                if let Some(mut responder_state) = lobby_for_receiver.player_positions.get_mut(&player_id_for_receiver) {
                                    responder_state.value_mut().in_combat = false;
                                }
                            }
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to parse client message: {}", e);
                    },
                }
            }
        }
    });

    // Forward broadcast messages from lobby to this client
    let mut forward_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let mut send_lock = sender.lock().await;
            if send_lock.send(Message::Text(Utf8Bytes::from(msg))).await.is_err() {
                break;
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut player_task => forward_task.abort(),
        _ = &mut forward_task => player_task.abort(),
    }

    // Player disconnected from lobby
    tracing::info!("Player disconnected from lobby {}: {}", lobby_for_forward.id, player_id_for_forward);

    // Check if player was in combat and clean up any active battles
    if let Some(player_state) = lobby_for_forward.player_positions.get(&player_id_for_forward) {
        if player_state.value().in_combat {
            // Player is marked as in combat, try to find and end their battle
            tracing::info!("Player {} disconnected while in combat, cleaning up battles", player_id_for_forward);
            
            if let Some(battle_manager) = state_for_disconnect.battle_manager.as_ref() {
                // We need to find battles where this player is participating
                let active_battles = battle_manager.find_battles_for_player(&player_id_for_forward);
                
                if let Some(pokemon_collection_manager) = state_for_disconnect.pokemon_collection_manager.as_ref() {
                    for battle_id in active_battles {
                        tracing::info!("Ending battle {} due to player disconnect", battle_id);
                        if let Err(e) = battle_manager.end_battle(battle_id, &lobby_for_forward, pokemon_collection_manager, true).await {
                            tracing::error!("Failed to end battle {} on player disconnect: {}", battle_id, e);
                        }
                    }
                }
            }
        }
    }
    info!("Player {} disconnected from lobby {}", player_id_for_forward, lobby_for_forward.id);

    // Clean up player resources
    lobby_for_forward.player_positions.remove(&player_id_for_forward);
    lobby_for_forward.player_last_active.remove(&player_id_for_forward);
    lobby_for_forward.player_connections.remove(&player_id_for_forward);
    
    // Notify other players about the disconnection
    let leave_msg = ServerMessage::PlayerLeft { id: player_id_for_forward };
    let _ = lobby_for_forward.tx.send(serde_json::to_string(&leave_msg).unwrap());
}

// Handler for player interacting with a monster to start combat
pub async fn handle_player_interaction(
    state: &Arc<AppState>,
    lobby_id: &str,
    player_id: &str,
    monster_id: &Option<String>
) {
    let lobby = match state.lobbies.get(lobby_id) {
        Some(lobby) => lobby,
        None => {
            tracing::error!("Lobby not found for interaction: {}", lobby_id);
            return;
        }
    };
        
    // Get the player state
    let player_state = match lobby.player_positions.get(player_id) {
        Some(state) => state.value().clone(),
        None => {
            tracing::error!("Player not found in lobby for interaction: {}", player_id);
            return;
        }
    };
    
    // Check if player is already in combat
    if player_state.in_combat {
        tracing::warn!("Player is already in combat: {}", player_id);
        return;
    }
    
    // If no monster_id specified, try to find a monster at player's position
    let monster_instance_id = match monster_id {
        Some(id) => id.clone(),
        None => {
            // Find monsters near the player
            let nearby_monsters = find_monsters_near_player(&lobby, &player_state, 1).await;
            
            if nearby_monsters.is_empty() {
                tracing::info!("No monsters found near player {}", player_id);
                return;
            }
            
            // For now, just take the first found monster
            nearby_monsters[0].0.clone() // Get instance_id from tuple
        }
    };
    
    // Check if we have the battle manager and pokemon collection manager
    let battle_manager = match &state.battle_manager {
        Some(manager) => manager,
        None => {
            tracing::error!("Battle manager not available");
            return;
        }
    };
    
    let pokemon_collection_manager = match &state.pokemon_collection_manager {
        Some(manager) => manager,
        None => {
            tracing::error!("Pokemon collection manager not available");
            return;
        }
    };
    
    // Start the wild battle
    match battle_manager.start_wild_battle(player_id, &monster_instance_id, &lobby, pokemon_collection_manager).await {
        Ok(battle_id) => {
            tracing::info!("Started wild battle {} between player {} and monster {}", 
                battle_id, player_id, monster_instance_id);
        },
        Err(e) => {
            tracing::error!("Failed to start wild battle: {}", e);
            
            // Send error message to player
            let error_msg = ServerMessage::Error {
                message: format!("Failed to start battle: {}", e),
            };
            
            if let Err(send_err) = lobby.send_to_player(player_id, &error_msg).await {
                tracing::error!("Failed to send error message: {}", send_err);
            }
            
            // Ensure player is not marked as in combat if battle failed to start
            if let Some(mut player_entry) = lobby.player_positions.get_mut(player_id) {
                player_entry.value_mut().in_combat = false;
            }
            
            // Ensure monster is not marked as in combat if battle failed to start
            if let Some(monster_entry) = lobby.active_monsters.get(&monster_instance_id) {
                if let Ok(mut monster) = monster_entry.value().try_lock() {
                    monster.in_combat = false;
                }
            }
        }
    }
}

// Find monsters near a player within a given distance
async fn find_monsters_near_player(
    lobby: &Arc<Lobby>,
    player: &PlayerState,
    max_distance: u32
) -> Vec<(String, DisplayMonster)> {
    let lobby_monsters = MonsterManager::get_monsters_for_lobby(lobby);
    let mut result = Vec::new();
    
    // Filter monsters by distance to player
    for monster_mutex in lobby_monsters {
        // Try to lock the monster to check its properties
        if let Ok(monster) = monster_mutex.try_lock() {
            // Skip monsters in combat
            if monster.in_combat {
                continue;
            }
            
            // Calculate Manhattan distance
            let dx = if monster.position.x > player.x { 
                monster.position.x - player.x 
            } else { 
                player.x - monster.position.x 
            };
            
            let dy = if monster.position.y > player.y { 
                monster.position.y - player.y 
            } else { 
                player.y - monster.position.y 
            };
            
            // Filter by distance
            if (dx + dy) <= max_distance {
                // Store the monster's ID and display info
                result.push((monster.instance_id.clone(), monster.to_display()));
            }
        }
    }
    
    result
}
