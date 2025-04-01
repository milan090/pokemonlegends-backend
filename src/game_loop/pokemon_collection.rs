use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;
use rand::Rng;

use crate::monsters::monster::{MonsterMove, PokemonType};
use crate::monsters::monster_manager::MonsterTemplateRepository;
use crate::combat::state::{StatusCondition, BattleMoveView, MoveCategory as CombatMoveCategory};
use crate::monsters::Monster;
use crate::stats::nature::Nature;
use crate::stats::StatSet;
use crate::stats::{calculate_stats, CalculatedStats};
use crate::monsters::move_manager::{MoveRepository, MoveCategory as RepoMoveCategory};
use crate::models::DisplayPokemon;

const MAX_POKEMONS: usize = 6;
const STARTING_POKEMON_IDS: [u32; 3] = [1, 4, 7];

// Manages pokemonmon collections for all players
pub struct PokemonCollectionManager {
    // Map of player ID to their pokemon collection
    collections: RwLock<HashMap<String, PlayerCollection>>,
    template_manager: Arc<MonsterTemplateRepository>,
    move_repository: Arc<MoveRepository>,
    redis_client: redis::Client,
}

// Represents a captured PokemonMon
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pokemon {
    pub id: String,
    pub template_id: u32,
    pub name: String,
    pub level: u32,
    pub exp: u64,
    pub max_exp: u64,
    pub current_hp: u32,
    pub ivs: StatSet<u8>,
    pub evs: StatSet<u16>,
    pub nature: Nature,
    pub capture_date: u64,
    pub moves: Vec<MonsterMove>,
    pub types: Vec<PokemonType>,
    pub ability: String,
    pub status_condition: Option<StatusCondition>
}

// Player's collection of PokemonMons
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerCollection {
    pub player_id: String,
    pub pokemons: HashMap<String, Pokemon>,
    // Six pokemon ordered by index
    pub active_pokemons: Vec<String>,
}

impl PokemonCollectionManager {
    pub fn new(
        redis_client: redis::Client,
        template_manager: Arc<MonsterTemplateRepository>,
        move_repository: Arc<MoveRepository>,
    ) -> Arc<Self> {
        Arc::new(Self {
            collections: RwLock::new(HashMap::new()),
            template_manager,
            move_repository,
            redis_client,
        })
    }

    pub fn pokemon_to_display_pokemon(&self, pokemon: &Pokemon) -> DisplayPokemon {
        let calculated_stats = calculate_stats(
            &self.template_manager.templates.get(&pokemon.template_id).unwrap().base_stats,
            pokemon.level,
            &pokemon.ivs,
            &pokemon.evs,
            &pokemon.nature,
        );
        
        let move_views: Vec<BattleMoveView> = pokemon.moves.iter().filter_map(|monster_move| {
            self.move_repository.get_move(monster_move.id).map(|move_data| {
                let combat_category = match move_data.damage_class {
                    RepoMoveCategory::Physical => CombatMoveCategory::Physical,
                    RepoMoveCategory::Special => CombatMoveCategory::Special,
                    RepoMoveCategory::Status => CombatMoveCategory::Status,
                };

                BattleMoveView {
                    move_id: move_data.id,
                    name: move_data.name.clone(),
                    move_type: move_data.move_type,
                    category: combat_category,
                    current_pp: monster_move.pp_remaining,
                    max_pp: move_data.pp,
                    power: move_data.power,
                    accuracy: move_data.accuracy,
                    description: move_data.description.clone(),
                }
            })
        }).collect();

        DisplayPokemon {
            id: pokemon.id.clone(),
            template_id: pokemon.template_id,
            name: pokemon.name.clone(),
            level: pokemon.level,
            exp: pokemon.exp,
            max_exp: pokemon.max_exp,
            current_hp: pokemon.current_hp,
            max_hp: calculated_stats.hp,
            calculated_stats,
            nature: pokemon.nature,
            capture_date: pokemon.capture_date,
            moves: move_views,
            types: pokemon.types.clone(),
            ability: pokemon.ability.clone(),
            status_condition: pokemon.status_condition,
        }
    }

    // Convert a wild monster to a pokemonmon
    pub fn monster_to_pokemon(&self, monster: &Monster) -> Pokemon {
        // Use the monster's existing IVs, EVs, and nature
        let ivs = monster.ivs.clone();
        let evs = monster.evs.clone();
        let nature = monster.nature.clone();
        
        // Get base stats from the template
        let template = self.template_manager.templates.get(&monster.template_id)
            .expect("Monster template not found");
        
        // Calculate stats (should be equivalent to monster.calculated_stats)
        let stats = calculate_stats(&template.base_stats, monster.level, &ivs, &evs, &nature);
        
        Pokemon {
            id: Uuid::new_v4().to_string(),
            template_id: monster.template_id,
            name: monster.name.clone(),
            level: monster.level,
            exp: 0,
            max_exp: self.get_next_max_exp(monster.level, monster.template_id),
            
            // Use calculated stats
            current_hp: monster.current_hp,
            
            // Use the monster's existing IVs, EVs, and nature
            ivs,
            evs,
            nature,
            
            capture_date: chrono::Utc::now().timestamp() as u64,
            moves: monster.moves.clone(),
            types: monster.types.clone(),
            ability: monster.ability.clone(),
            status_condition: monster.status_condition.clone(),
        }
    }

    // Add a new pokemon to a player's collection
    // Returns the index of the pokemon if it was added to the active list
    pub async fn add_pokemon(&self, player_id: &str, pokemon: Pokemon) -> Result<Option<usize>, String> {
        // First try to load the collection if we don't have it in memory
        self.load_collection_if_needed(player_id).await?;

        // Add pokemon to collection
        let pokemon_id = pokemon.id.clone();
        let mut collections = self.collections.write().await;
        if let Some(collection) = collections.get_mut(player_id) {
            collection.pokemons.insert(pokemon_id.clone(), pokemon.clone());

            let active_index = if collection.active_pokemons.len() < MAX_POKEMONS {
                collection.active_pokemons.push(pokemon_id);
                Some(collection.active_pokemons.len() - 1)
            } else {
                None
            };

            // Save to Redis
            match self.save_collection(player_id, collection).await {
                Ok(_) => {
                    info!(
                        "Added pokemon {} to player {}'s collection",
                        pokemon.id, player_id
                    );
                    Ok(active_index)
                }
                Err(e) => {
                    warn!(
                        "Failed to save pokemon collection for player {}: {}",
                        player_id, e
                    );
                    Err(format!("Failed to save collection: {}", e))
                }
            }
        } else {
            // Create new collection with this pokemon
            let mut pokemons = HashMap::new();
            pokemons.insert(pokemon.id.clone(), pokemon.clone());
            
            let collection = PlayerCollection {
                player_id: player_id.to_string(),
                pokemons,
                active_pokemons: vec![pokemon.id.clone()],
            };

            collections.insert(player_id.to_string(), collection.clone());

            // Save to Redis
            match self.save_collection(player_id, &collection).await {
                Ok(_) => {
                    info!(
                        "Created new collection for player {} with pokemon {}",
                        player_id, pokemon.id
                    );
                    Ok(Some(0))
                }
                Err(e) => {
                    warn!(
                        "Failed to save new pokemon collection for player {}: {}",
                        player_id, e
                    );
                    Err(format!("Failed to save collection: {}", e))
                }
            }
        }
    }

    pub async fn choose_starting_pokemons(
        &self,
        player_id: &str,
        starter_id: u32,
    ) -> Result<DisplayPokemon, String> {
        if !STARTING_POKEMON_IDS.contains(&starter_id) {
            return Err(format!("Invalid starter id: {}", starter_id));
        }

        let collection = self.get_collection(player_id).await?;
        if collection.pokemons.len() > 0 {
            return Err("Player already has a pokemon".to_string());
        }

        let starter_pokemon_raw = self.template_manager.pokemon_from_template(starter_id, Some(10));
        
        self.add_pokemon(player_id, starter_pokemon_raw.clone()).await?;

        let display_pokemon = self.pokemon_to_display_pokemon(&starter_pokemon_raw);

        Ok(display_pokemon)
    }

    // Get a player's collection
    pub async fn get_collection(&self, player_id: &str) -> Result<PlayerCollection, String> {
        // Try to load from memory first
        {
            let collections = self.collections.read().await;
            if let Some(collection) = collections.get(player_id) {
                return Ok(collection.clone());
            }
        }

        // If not in memory, try to load from Redis
        let redis_key = format!("pokemon_collection:{}", player_id);
        let mut con = match self.redis_client.get_async_connection().await {
            Ok(con) => con,
            Err(e) => return Err(format!("Redis connection error: {}", e)),
        };

        let collection_json: Option<String> = match redis::cmd("GET")
            .arg(&redis_key)
            .query_async(&mut con)
            .await
        {
            Ok(res) => res,
            Err(e) => return Err(format!("Redis query error: {}", e)),
        };

        if let Some(json) = collection_json {
            // Parse JSON into PlayerCollection
            match serde_json::from_str::<PlayerCollection>(&json) {
                Ok(collection) => {
                    // Cache in memory
                    let mut collections = self.collections.write().await;
                    collections.insert(player_id.to_string(), collection.clone());
                    Ok(collection)
                }
                Err(e) => Err(format!("Failed to parse collection JSON: {}", e)),
            }
        } else {
            // No collection yet, create an empty one
            let empty_collection = PlayerCollection {
                player_id: player_id.to_string(),
                pokemons: HashMap::new(),
                active_pokemons: Vec::new(),
            };

            // Cache in memory
            let mut collections = self.collections.write().await;
            collections.insert(player_id.to_string(), empty_collection.clone());

            Ok(empty_collection)
        }
    }

    // Get a player's active pokemons by index, returning Pokemon
    pub async fn get_active_pokemons(&self, player_id: &str) -> Result<Vec<Pokemon>, String> {
        let collection = self.get_collection(player_id).await?;
        let active_pokemon_ids = &collection.active_pokemons;
        
        let active_pokemons = active_pokemon_ids
            .iter()
            .filter_map(|id| collection.pokemons.get(id).cloned())
            .collect();
            
        Ok(active_pokemons)
    }

    // Save a collection to Redis
    async fn save_collection(
        &self,
        player_id: &str,
        collection: &PlayerCollection,
    ) -> Result<(), String> {
        let redis_key = format!("pokemon_collection:{}", player_id);

        // Serialize collection to JSON
        let json = match serde_json::to_string(collection) {
            Ok(j) => j,
            Err(e) => return Err(format!("Failed to serialize collection: {}", e)),
        };

        // Save to Redis
        let mut con = match self.redis_client.get_async_connection().await {
            Ok(con) => con,
            Err(e) => return Err(format!("Redis connection error: {}", e)),
        };

        // Use SET instead of SET with expiry to make it permanent
        match redis::cmd("SET")
            .arg(&redis_key)
            .arg(&json)
            .query_async::<_, String>(&mut con)
            .await
        {
            Ok(_) => {
                tracing::info!("Successfully saved pokemon collection for player {}", player_id);
                Ok(())
            },
            Err(e) => Err(format!("Redis save error: {}", e)),
        }
    }

    // Helper method to load collection if not already in memory
    async fn load_collection_if_needed(&self, player_id: &str) -> Result<(), String> {
        let needs_loading = {
            let collections = self.collections.read().await;
            !collections.contains_key(player_id)
        };

        if needs_loading {
            // Try to load from Redis
            let _ = self.get_collection(player_id).await?;
        }

        Ok(())
    }

    pub fn get_next_max_exp(&self, level: u32, template_id: u32) -> u64 {
        let template = self
            .template_manager
            .templates
            .get(&template_id)
            .expect("Template not found");
        let base_exp = template.base_experience;

        // Calculate required exp for next level using standard RPG formula
        // Each level requires progressively more exp
        let growth_rate: f64 = 1.2;
        let next_level_exp = (base_exp as f64 * growth_rate.powf(level as f64)).floor() as u64;

        next_level_exp
    }

    /// Add experience to a Pokemon, possibly leveling it up
    /// Returns the updated Pokemon and whether it leveled up
    pub async fn add_experience_to_pokemon(&self, player_id: &str, pokemon_id: &str, experience: u64) -> Result<(Pokemon, bool), String> {
        // Load collection if needed
        self.load_collection_if_needed(player_id).await?;
        
        let mut collections = self.collections.write().await;
        let collection = collections.get_mut(player_id)
            .ok_or_else(|| format!("Player collection not found for player {}", player_id))?;
            
        let pokemon = collection.pokemons.get_mut(pokemon_id)
            .ok_or_else(|| format!("Pokemon {} not found in player {}'s collection", pokemon_id, player_id))?;
        
        // Add experience
        let old_level = pokemon.level;
        pokemon.exp += experience;
        
        // Check if Pokemon should level up
        let mut leveled_up = false;
        
        while pokemon.exp >= pokemon.max_exp && pokemon.level < 100 {
            // Level up!
            pokemon.level += 1;
            leveled_up = true;
            
            // Update experience for next level
            pokemon.exp -= pokemon.max_exp;
            pokemon.max_exp = self.get_next_max_exp(pokemon.level, pokemon.template_id);
            
            // TODO: Handle learning new moves on level up
            // This would involve checking the template for moves learnable at this level
            
            // Update Pokemon's stats based on new level
            if let Some(template) = self.template_manager.templates.get(&pokemon.template_id) {
                // Get current stats
                let old_stats = crate::stats::calculate_stats(
                    &template.base_stats,
                    old_level,
                    &pokemon.ivs,
                    &pokemon.evs,
                    &pokemon.nature
                );
                
                // Calculate new stats after level up
                let new_stats = crate::stats::calculate_stats(
                    &template.base_stats,
                    pokemon.level,
                    &pokemon.ivs,
                    &pokemon.evs,
                    &pokemon.nature
                );
                
                // Update current HP proportionally
                let hp_ratio = if pokemon.current_hp > 0 && old_stats.hp > 0 {
                    pokemon.current_hp as f32 / old_stats.hp as f32
                } else {
                    0.0
                };
                
                // Adjust current HP to maintain the same percentage with the new max HP
                pokemon.current_hp = (hp_ratio * new_stats.hp as f32).ceil() as u32;
                
                tracing::info!("Pokemon {} leveled up to {} (HP: {}/{})", 
                    pokemon.name, pokemon.level, pokemon.current_hp, new_stats.hp);
            }
        }
        
        // Save the updated collection
        self.save_collection(player_id, collection).await?;
        
        // Return a clone of the updated pokemon and whether it leveled up
        Ok((collection.pokemons.get(pokemon_id).unwrap().clone(), leveled_up))
    }

    pub async fn update_pokemon(&self, player_id: &str, pokemon_id: &str, update_data: &PokemonUpdate) -> Result<(), String> {
        self.load_collection_if_needed(player_id).await?;

        let mut collections = self.collections.write().await;
        let collection = collections.get_mut(player_id)
            .ok_or_else(|| format!("Player collection not found for player {}", player_id))?;

        let pokemon = collection.pokemons.get_mut(pokemon_id)
            .ok_or_else(|| format!("Pokemon {} not found in player {}'s collection", pokemon_id, player_id))?;

        if let Some(name) = &update_data.name {
            pokemon.name = name.clone();
        }
        if let Some(level) = update_data.level {
            pokemon.level = level;
        }
        if let Some(exp) = update_data.exp {
            pokemon.exp = exp;
        }
        if let Some(max_exp) = update_data.max_exp {
            pokemon.max_exp = max_exp;
        }
        if let Some(current_hp) = update_data.current_hp {
            pokemon.current_hp = current_hp;
        }

        // Save the updated collection
        self.save_collection(player_id, collection).await?;

        Ok(())
    }
}

pub struct PokemonUpdate {
    pub name: Option<String>,
    pub level: Option<u32>,
    pub exp: Option<u64>,
    pub max_exp: Option<u64>,
    pub current_hp: Option<u32>,    
}