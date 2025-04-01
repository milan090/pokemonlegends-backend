pub use game_server::*;

use axum::{routing::get, Router};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tokio::time::Duration;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let config = config::Config::from_env();
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis_client = redis_manager::init_redis_client(&redis_url).await;
    
    let state = app_state::AppState::new(redis_client.clone(), config.clone());
    state.initialize_default_lobbies().await;

    // Load move data from moves.json
    let move_repository = monsters::MoveRepository::new(&config.monsters.moves_path, &config.monsters.type_chart_path);
    
    // Load monster templates
    let monster_template_repository = monsters::monster_manager::MonsterTemplateRepository::new(&config.monsters.templates_path).await;
    let monster_template_repository = monster_template_repository.with_move_repository(move_repository.clone());
    
    let monster_manager_factory = Arc::new(monsters::monster_manager::MonsterManagerFactory {
        template_repository: monster_template_repository.clone(),
        loaded_maps: tokio::sync::RwLock::new(HashMap::new()),
    });
    
    let player_movement_manager = Arc::new(game_loop::player_movement::PlayerMovementManager::new());
    let pokemon_collection_manager = game_loop::pokemon_collection::PokemonCollectionManager::new(
        redis_client.clone(), 
        monster_template_repository.clone(),
        move_repository.clone()
    );
    
    // Create the battle manager, passing the template repository
    let battle_manager = Arc::new(combat::manager::BattleManager::new(monster_template_repository.clone()));
    
    let state = state
        .with_monster_manager_factory(monster_manager_factory.clone())
        .with_player_movement_manager(player_movement_manager.clone())
        .with_pokemon_collection_manager(pokemon_collection_manager.clone())
        .with_battle_manager(battle_manager.clone());
    
    let cors = CorsLayer::new()
        .allow_origin(config.server.cors_origins.iter().map(|origin| origin.parse().unwrap()).collect::<Vec<_>>())
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/ws/{lobby_id}", get(handlers::ws_lobby_handler))
        .route("/lobbies", get(handlers::public_lobbies_handler))
        .route("/health", get(handlers::health_handler))
        .layer(cors)
        .with_state(state.clone());

    spawn_background_tasks(state.clone());

    let addr = config.server_addr();
    tracing::info!("Starting server on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind port");
    axum::serve(listener, app).await.expect("Server failed");
}

fn spawn_background_tasks(state: Arc<app_state::AppState>) {
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            lobby::cleanup_inactive_lobbies(&state_clone).await;
        }
    });
    
    let lobbies_for_spawner = Arc::new(state.lobbies.clone());
    tokio::spawn(async move {
        game_loop::monster_spawner::run_monster_spawner(
            lobbies_for_spawner,
            game_loop::monster_spawner::SpawnerConfig::default()
        ).await;
    });
    
    let lobbies_for_movement = Arc::new(state.lobbies.clone());
    tokio::spawn(async move {
        game_loop::monster_movement::run_monster_movement(lobbies_for_movement).await;
    });
    
    
    let state_for_player_movement = state.clone();
    let player_movement_manager = state.player_movement_manager.clone().unwrap();
    tokio::spawn(async move {
        game_loop::player_movement::run_player_movement_controller(state_for_player_movement, player_movement_manager).await;
    });


}