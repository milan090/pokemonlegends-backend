use crate::config::Config;
use crate::lobby::Lobby;
use crate::monsters::monster_manager::{MonsterManager, MonsterManagerFactory};
use crate::game_loop::player_movement::PlayerMovementManager;
use crate::game_loop::pokemon_collection::PokemonCollectionManager;
use crate::combat::manager::BattleManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

// Shared application state
pub struct AppState {
    pub redis: redis::Client,
    pub lobbies: DashMap<String, Arc<Lobby>>,
    pub config: Config,
    pub monster_manager: Option<Arc<MonsterManager>>,
    pub monster_manager_factory: Option<Arc<MonsterManagerFactory>>,
    pub player_movement_manager: Option<Arc<PlayerMovementManager>>,
    pub pokemon_collection_manager: Option<Arc<PokemonCollectionManager>>,
    pub battle_manager: Option<Arc<BattleManager>>,
}

impl AppState {
    pub fn new(redis_client: redis::Client, config: Config) -> Arc<Self> {
        Arc::new(AppState {
            redis: redis_client,
            lobbies: DashMap::new(),
            config,
            monster_manager: None,
            monster_manager_factory: None,
            player_movement_manager: None,
            pokemon_collection_manager: None,
            battle_manager: None,
        })
    }

    fn clone_lobbies(&self) -> DashMap<String, Arc<Lobby>> {
        let new_lobbies = DashMap::new();
        for entry in self.lobbies.iter() {
            new_lobbies.insert(entry.key().clone(), entry.value().clone());
        }
        new_lobbies
    }

    pub fn with_monster_manager(self: &Arc<Self>, monster_manager: Arc<MonsterManager>) -> Arc<Self> {
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: self.clone_lobbies(),
            config: self.config.clone(),
            monster_manager: Some(monster_manager),
            monster_manager_factory: self.monster_manager_factory.clone(),
            player_movement_manager: self.player_movement_manager.clone(),
            pokemon_collection_manager: self.pokemon_collection_manager.clone(),
            battle_manager: self.battle_manager.clone(),
        })
    }
    
    pub fn with_monster_manager_factory(self: &Arc<Self>, factory: Arc<MonsterManagerFactory>) -> Arc<Self> {
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: self.clone_lobbies(),
            config: self.config.clone(),
            monster_manager: self.monster_manager.clone(),
            monster_manager_factory: Some(factory),
            player_movement_manager: self.player_movement_manager.clone(),
            pokemon_collection_manager: self.pokemon_collection_manager.clone(),
            battle_manager: self.battle_manager.clone(),
        })
    }

    pub fn with_player_movement_manager(self: &Arc<Self>, player_movement_manager: Arc<PlayerMovementManager>) -> Arc<Self> {
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: self.clone_lobbies(),
            config: self.config.clone(),
            monster_manager: self.monster_manager.clone(),
            monster_manager_factory: self.monster_manager_factory.clone(),
            player_movement_manager: Some(player_movement_manager),
            pokemon_collection_manager: self.pokemon_collection_manager.clone(),
            battle_manager: self.battle_manager.clone(),
        })
    }

    pub fn with_pokemon_collection_manager(self: &Arc<Self>, pokemon_collection_manager: Arc<PokemonCollectionManager>) -> Arc<Self> {
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: self.clone_lobbies(),
            config: self.config.clone(),
            monster_manager: self.monster_manager.clone(),
            monster_manager_factory: self.monster_manager_factory.clone(),
            player_movement_manager: self.player_movement_manager.clone(),
            pokemon_collection_manager: Some(pokemon_collection_manager),
            battle_manager: self.battle_manager.clone(),
        })
    }

    pub fn with_battle_manager(self: &Arc<Self>, battle_manager: Arc<BattleManager>) -> Arc<Self> {
        Arc::new(AppState {
            redis: self.redis.clone(),
            lobbies: self.clone_lobbies(),
            config: self.config.clone(),
            monster_manager: self.monster_manager.clone(),
            monster_manager_factory: self.monster_manager_factory.clone(),
            player_movement_manager: self.player_movement_manager.clone(),
            pokemon_collection_manager: self.pokemon_collection_manager.clone(),
            battle_manager: Some(battle_manager),
        })
    }

    pub async fn initialize_default_lobbies(&self) {
        let default_lobbies = vec!["ABCD-1234", "EFGH-5678", "IJKL-9012"];
        let monster_manager_factory = MonsterManagerFactory::new(&self.config.monsters.templates_path).await;
        for lobby_id in default_lobbies {
            let (lobby_tx, _) = broadcast::channel(self.config.performance.broadcast_channel_size);
            self.lobbies.insert(lobby_id.to_string(), Arc::new(Lobby {
                id: lobby_id.to_string(),
                player_positions: DashMap::new(),
                player_last_active: DashMap::new(),
                tx: lobby_tx,
                map_id: "map1".to_string(),
                active_monsters: DashMap::new(),
                monsters_by_spawn_point: DashMap::new(),
                monster_manager: monster_manager_factory.create_monster_manager("map1").await.unwrap(),
                player_connections: DashMap::new(),
            }));
        }
    }
} 