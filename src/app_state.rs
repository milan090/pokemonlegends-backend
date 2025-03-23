use crate::config::Config;
use crate::models::Lobby;
use crate::monsters::monster_manager::MonsterManager;
use crate::game_loop::player_movement::PlayerMovementManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

// Shared application state
pub struct AppState {
    pub redis: redis::Client,
    pub lobbies: DashMap<String, Arc<Lobby>>,
    pub config: Config,
    pub monster_manager: Option<Arc<MonsterManager>>,
    pub player_movement_manager: Option<Arc<PlayerMovementManager>>,
}

impl AppState {
    pub fn new(redis_client: redis::Client, config: Config) -> Arc<Self> {
        Arc::new(AppState {
            redis: redis_client,
            lobbies: DashMap::new(),
            config,
            monster_manager: None,
            player_movement_manager: None,
        })
    }

    pub fn with_monster_manager(self: &Arc<Self>, monster_manager: Arc<MonsterManager>) -> Arc<Self> {
        let mut new_lobbies = DashMap::new();
        
        // Copy existing lobbies
        for entry in self.lobbies.iter() {
            new_lobbies.insert(entry.key().clone(), entry.value().clone());
        }
        
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: new_lobbies,
            config: self.config.clone(),
            monster_manager: Some(monster_manager),
            player_movement_manager: self.player_movement_manager.clone(),
        })
    }

    pub fn with_player_movement_manager(self: &Arc<Self>, player_movement_manager: Arc<PlayerMovementManager>) -> Arc<Self> {
        let mut new_lobbies = DashMap::new();
        
        // Copy existing lobbies
        for entry in self.lobbies.iter() {
            new_lobbies.insert(entry.key().clone(), entry.value().clone());
        }
        
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: new_lobbies,
            config: self.config.clone(),
            monster_manager: self.monster_manager.clone(),
            player_movement_manager: Some(player_movement_manager),
        })
    }

    pub fn initialize_default_lobbies(&self) {
        // Insert default lobbies
        let default_lobbies = vec!["ABCD-1234", "EFGH-5678", "IJKL-9012"];
        for lobby_id in default_lobbies {
            let (lobby_tx, _) = broadcast::channel(self.config.performance.broadcast_channel_size);
            self.lobbies.insert(lobby_id.to_string(), Arc::new(Lobby {
                id: lobby_id.to_string(),
                player_positions: DashMap::new(),
                player_last_active: DashMap::new(),
                tx: lobby_tx,
            }));
        }
    }
} 