// src/main.rs

mod app_state;
mod config;
mod handlers;
mod lobby;
mod models;
mod redis_manager;
mod monsters;
mod game_loop;

use axum::{routing::get, Router};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tokio::time::Duration;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    // Load configuration
    let config = config::Config::from_env();

    // Set up Redis client
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis_client = redis_manager::init_redis_client(&redis_url).await;
    
    // Set up shared state and initialize empty lobbies
    let state = app_state::AppState::new(redis_client, config.clone());
    
    // Initialize default lobbies
    state.initialize_default_lobbies();

    // Initialize monster manager
    let monster_templates_path = std::env::var("MONSTER_TEMPLATES_PATH")
        .unwrap_or_else(|_| "resources/monster_templates.json".to_string());
    let monster_manager = monsters::monster_manager::MonsterManager::new(&monster_templates_path).await;
    
    // Initialize player movement manager
    let player_movement_manager = Arc::new(game_loop::player_movement::PlayerMovementManager::new());
    
    // Create new state with monster manager and player movement manager
    let state = state.with_monster_manager(monster_manager.clone());
    let state = state.with_player_movement_manager(player_movement_manager.clone());
    
    // Set up CORS layer
    let cors = CorsLayer::new()
        .allow_origin(config.server.cors_origins.iter().map(|origin| origin.parse().unwrap()).collect::<Vec<_>>())
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    // Set up routes
    let app = Router::new()
        .route("/ws/{lobby_id}", get(handlers::ws_lobby_handler))
        .route("/lobbies", get(handlers::public_lobbies_handler))
        .route("/health", get(handlers::health_handler))
        .layer(cors)
        .with_state(state.clone());

    // Start cleanup task for inactive players in lobbies
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            lobby::cleanup_inactive_lobbies(&state_clone).await;
        }
    });
    
    // Start monster spawner task
    let lobbies_for_spawner = Arc::new(state.lobbies.clone());
    let monster_manager_for_spawner = monster_manager.clone();
    tokio::spawn(async move {
        game_loop::monster_spawner::run_monster_spawner(monster_manager_for_spawner, lobbies_for_spawner).await;
    });
    
    // Start monster movement task
    let lobbies_for_movement = Arc::new(state.lobbies.clone());
    let monster_manager_for_movement = monster_manager.clone();
    tokio::spawn(async move {
        game_loop::monster_movement::run_monster_movement(monster_manager_for_movement, lobbies_for_movement).await;
    });
    
    // Start player movement controller task
    let state_for_player_movement = state.clone();
    tokio::spawn(async move {
        game_loop::player_movement::run_player_movement_controller(state_for_player_movement, player_movement_manager).await;
    });

    // Start server
    let addr = config.server_addr();
    tracing::info!("Starting server on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind port");
    axum::serve(listener, app).await.expect("Server failed");
}