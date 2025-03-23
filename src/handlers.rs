use crate::app_state::AppState;
use crate::models::{ClientMessage, Lobby, PlayerState, ServerMessage};
use crate::lobby::{validate_lobby_id, get_lobby};
use crate::redis_manager;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Path,
    },
    response::IntoResponse,
    Json,
};
use axum::extract::ws::Utf8Bytes;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;
use tracing::info;
use uuid::Uuid;
use std::sync::Arc;

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
) -> impl IntoResponse {
    if !validate_lobby_id(&lobby_id) {
        return (axum::http::StatusCode::BAD_REQUEST, "Invalid lobby id").into_response();
    }
    
    // Get the lobby, but don't create if it doesn't exist
    match get_lobby(&state, &lobby_id) {
        Some(lobby) => ws.on_upgrade(move |socket| handle_lobby_socket(socket, state, lobby)),
        None => (axum::http::StatusCode::NOT_FOUND, "Lobby not found").into_response()
    }
}

// Health check endpoint
pub async fn health_handler() -> impl IntoResponse {
    "OK"
}

// Handle WebSocket connection for a lobby
pub async fn handle_lobby_socket(socket: WebSocket, state: Arc<AppState>, lobby: Arc<Lobby>) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(tokio::sync::Mutex::new(sender));

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
    let mut redis_conn = state.redis.get_async_connection().await.expect("Failed to connect to Redis");
    
    // Retrieve or create player id via Redis
    let player_id = match redis_manager::get_player_id(&mut redis_conn, &session_token).await {
        Ok(id) => id,
        Err(_) => Uuid::new_v4().to_string(),
    };
    tracing::info!("Player connected to lobby {}: {} (session: {})", lobby.id, player_id, session_token);

    // Store session-token mapping in Redis
    redis_manager::store_session(
        &mut redis_conn, 
        &session_token, 
        &player_id,
        state.config.game.inactive_timeout_sec
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
                x: 5 + rand::random::<u32>() % 16,
                y: 5,
                direction: "down".to_string(),
            };
            
            // Store the new state in Redis
            let state_json = serde_json::to_string(&new_state).unwrap();
            if let Err(e) = redis_manager::store_player_state(
                &mut redis_conn,
                &lobby.id,
                &player_id,
                &state_json,
                state.config.game.inactive_timeout_sec
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

    // Send welcome message
    let welcome_msg = ServerMessage::Welcome { id: player_id.clone(), x: player_state.x, y: player_state.y };
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
    if let Some(monster_manager) = &state.monster_manager {
        let monsters = monster_manager.get_all_active_monsters().await;
        if !monsters.is_empty() {
            let monsters_msg = ServerMessage::Monsters { monsters };
            if let Err(e) = sender.lock().await.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&monsters_msg).unwrap()))).await {
                tracing::error!("Failed to send monsters message: {}", e);
                return;
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
                    Ok(ClientMessage::Move { x, y, direction }) => {
                        // Get current player state
                        let current_state = match lobby_for_receiver.player_positions.get(&player_id_for_receiver) {
                            Some(state) => state.value().clone(),
                            None => continue,
                        };
                        
                        // Validate movement with the movement manager
                        let movement_valid = state.player_movement_manager.as_ref().unwrap()
                            .validate_movement(&player_id_for_receiver, &current_state, x, y);
                        
                        // Only process movement if it's valid
                        if movement_valid {
                            let updated_player = PlayerState {
                                id: player_id_for_receiver.clone(),
                                x,
                                y,
                                direction,
                            };
                            
                            // Update player state in Redis
                            if let Ok(mut redis_conn) = state.redis.get_async_connection().await {
                                let player_json = serde_json::to_string(&updated_player).unwrap();
                                let _ = redis_manager::store_player_state(
                                    &mut redis_conn,
                                    &lobby_for_receiver.id,
                                    &player_id_for_receiver,
                                    &player_json,
                                    state.config.game.inactive_timeout_sec
                                ).await.unwrap_or_else(|e| {
                                    tracing::error!("Failed to persist player state: {}", e);
                                });
                            }
                            
                            // Update player state in lobby
                            lobby_for_receiver.player_positions.insert(player_id_for_receiver.clone(), updated_player.clone());
                            
                            // Register the validated movement in the movement manager
                            state.player_movement_manager.as_ref().unwrap()
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
                        }
                    },
                    Ok(ClientMessage::Ping) => {
                        let pong_msg = ServerMessage::Pong;
                        let mut sender_lock = sender_for_receiver.lock().await;
                        if let Err(e) = sender_lock.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&pong_msg).unwrap()))).await {
                            tracing::error!("Failed to send pong message: {}", e);
                            break;
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to parse client message: {}", e);
                    },
                    _ => {}
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
    lobby_for_forward.player_positions.remove(&player_id_for_forward);
    lobby_for_forward.player_last_active.remove(&player_id_for_forward);
    let leave_msg = ServerMessage::PlayerLeft { id: player_id_for_forward };
    let _ = lobby_for_forward.tx.send(serde_json::to_string(&leave_msg).unwrap());
}