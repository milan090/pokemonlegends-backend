use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::Rng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Mutex};
use tracing::{info, warn};

use crate::game_loop::pokemon_collection::Pokemon;
use crate::lobby::Lobby;
use crate::monsters::monster::MonsterMove;
use crate::monsters::{Monster, MonsterTemplate, Position};
use crate::stats::calculate_stats;
use crate::stats::nature::Nature;
use crate::stats::{StatSet, BaseStats, StatName};

pub const DEFAULT_ALLOWED_MONSTER_IDS: [u32; 50] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
];

/// Defines an area where monsters can spawn in the game world
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPoint {
    pub id: String,
    pub tile_x: u32,
    pub tile_y: u32,
    pub width: u32,
    pub height: u32,
    pub allowed_monsters: Vec<u32>,
    pub max_monsters: u32,
    pub spawn_interval_sec: u64,
    pub spawn_density: Option<f32>, // Probability weight for spawn selection
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapSpawnArea {
    pub map_id: String,
    pub max_level: u32,
    pub spawn_points: Vec<SpawnPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterTemplates {
    pub pokemons: Vec<MonsterTemplate>,
}

/// Represents collision data for a map
#[derive(Clone)]
pub struct ObstacleMap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<bool>, // true = obstacle, false = clear
}

/// Cache of valid positions for a spawn area
#[derive(Clone)]
pub struct ValidPositionsMap {
    pub spawn_point_id: String,
    pub valid_positions: HashSet<(u32, u32)>,
}

/// Shared monster template repository across all lobbies
pub struct MonsterTemplateRepository {
    pub templates: HashMap<u32, MonsterTemplate>,
    pub move_repository: Option<Arc<crate::monsters::move_manager::MoveRepository>>,
}

/// Map-specific data for monster management
pub struct MapData {
    pub map_id: String,
    pub spawn_points: HashMap<String, SpawnPoint>,
    pub obstacle_map: ObstacleMap,
    pub valid_positions: HashMap<String, ValidPositionsMap>,
}

/// Manages monster spawning, movement, and lifecycle for a specific lobby
pub struct MonsterManager {
    pub template_repository: Arc<MonsterTemplateRepository>,
    pub map_data: MapData,
}

/// Factory for creating monster managers
pub struct MonsterManagerFactory {
    pub template_repository: Arc<MonsterTemplateRepository>,
    pub loaded_maps: RwLock<HashMap<String, Arc<MapData>>>,
}

impl MonsterTemplateRepository {
    pub async fn new(templates_path: &str) -> Arc<Self> {
        let templates = Self::load_templates(templates_path);

        let mut template_map = HashMap::new();

        for template in &templates.pokemons {
            template_map.insert(template.id, template.clone());
        }

        Arc::new(MonsterTemplateRepository {
            templates: template_map,
            move_repository: None,
        })
    }

    pub fn with_move_repository(self: Arc<Self>, move_repository: Arc<crate::monsters::move_manager::MoveRepository>) -> Arc<Self> {
        Arc::new(MonsterTemplateRepository {
            templates: self.templates.clone(),
            move_repository: Some(move_repository),
        })
    }

    fn load_templates(path: &str) -> MonsterTemplates {
        let file = File::open(Path::new(path)).expect("Failed to open monster templates file");
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).expect("Failed to parse monster templates JSON")
    }

    pub fn pokemon_from_template(&self, template_id: u32, level: Option<u32>) -> Pokemon {
        let template = self
            .templates
            .get(&template_id)
            .expect("Template not found");
        let mut rng = SmallRng::from_entropy();
        let level = level.unwrap_or(rng.gen_range(template.min_level..=template.max_level));

        // Generate random IVs for each stat
        let ivs = StatSet {
            hp: rng.gen_range(0..=31),
            attack: rng.gen_range(0..=31),
            defense: rng.gen_range(0..=31),
            special_attack: rng.gen_range(0..=31),
            special_defense: rng.gen_range(0..=31),
            speed: rng.gen_range(0..=31),
        };
        
        // Start with zero EVs
        let evs = StatSet {
            hp: 0,
            attack: 0,
            defense: 0,
            special_attack: 0,
            special_defense: 0,
            speed: 0,
        };
        
        // Generate a random nature
        let nature = Nature::random();
        
        // Calculate stats using formula with IVs, EVs, and nature
        let stats = calculate_stats(&template.base_stats, level, &ivs, &evs, &nature);

        // Get the moves for this pokemon
        let moves = self.pokemon_moves_from_template(template, level);

        // Randomly select one ability from the template's abilities
        let ability = template
            .abilities
            .choose(&mut rng)
            .cloned()
            .unwrap_or_else(|| {
                tracing::error!("Failed to select ability for template: {}", template.id);
                "None".to_string()
            });

        Pokemon {
            id: uuid::Uuid::new_v4().to_string(),
            template_id: template.id,
            name: template.name.clone(),
            level,
            exp: 0,
            max_exp: (template.base_experience as f64 * 1.2_f64.powf(level as f64)) as u64,
            current_hp: stats.hp,  // Full HP for a new Pokemon
            ivs,
            evs,
            nature,
            capture_date: chrono::Utc::now().timestamp() as u64,
            moves,
            types: template.types.clone(),
            ability,
            status_condition: None,
        }
    }

    pub fn pokemon_moves_from_template(&self, template: &MonsterTemplate, level: u32) -> Vec<MonsterMove> {
        // If we have a move repository, use it for proper PP values
        if let Some(move_repo) = &self.move_repository {
            return move_repo.select_moves_for_monster(&template.moves, level);
        }
        
        // Legacy fallback implementation if no move repository is available
        let mut rng = SmallRng::from_entropy();
        
        // Filter moves that can be learned at or below the current level
        let available_moves: Vec<(u32, u32)> = template
            .moves
            .iter()
            .filter(|(_, level_learned)| *level_learned <= level)
            .map(|(move_id, level_learned)| (*move_id, *level_learned))
            .collect();
        
        if available_moves.is_empty() {
            return Vec::new();
        }
        
        // Sort moves by level learned (descending) to prioritize most recently learned moves
        let mut sorted_moves = available_moves.clone();
        sorted_moves.sort_by(|a, b| b.1.cmp(&a.1));
        
        // In official Pokémon games, Pokémon typically have up to 4 moves
        // Select up to 4 moves, prioritizing higher-level moves
        let max_moves = 4;
        let selected_moves = if sorted_moves.len() <= max_moves {
            // If we have 4 or fewer moves, use all of them
            sorted_moves
        } else {
            // Otherwise, we want to intelligently select moves
            // Take the most recent 2-3 moves
            let recent_moves = sorted_moves.iter().take(3).cloned().collect::<Vec<_>>();
            
            // For remaining slots, randomly select from other available moves
            let mut selected = recent_moves;
            let remaining_moves: Vec<(u32, u32)> = sorted_moves
                .into_iter()
                .skip(3)
                .collect();
            
            if !remaining_moves.is_empty() && selected.len() < max_moves {
                let slots_left = max_moves - selected.len();
                let mut additional_moves = remaining_moves;
                additional_moves.shuffle(&mut rng);
                
                for move_data in additional_moves.iter().take(slots_left) {
                    selected.push(*move_data);
                }
            }
            
            selected
        };
        
        // Convert to MonsterMove structs using default PP values
        selected_moves
            .into_iter()
            .map(|(move_id, _)| {
                // Use default PP value since we don't have the move repository
                let default_pp = 20;
                
                MonsterMove {
                    id: move_id,
                    pp_remaining: default_pp,
                }
            })
            .collect()
    }
    
    fn get_max_exp_for_level(&self, template_id: u32, level: u32) -> u64 {
        let template = self.templates.get(&template_id).expect("Template not found");
        
        match template.growth_rate {
            crate::monsters::monster::GrowthRate::Fast => {
                (4 * level.pow(3)) as u64 / 5
            },
            crate::monsters::monster::GrowthRate::Medium => {
                level.pow(3) as u64
            },
            crate::monsters::monster::GrowthRate::MediumSlow => {
                ((6 * level.pow(3)) / 5 - 15 * level.pow(2) + 100 * level - 140) as u64
            },
            crate::monsters::monster::GrowthRate::Slow => {
                (5 * level.pow(3)) as u64 / 4
            }
        }
    }
    
    pub fn get_exp_for_next_level(&self, template_id: u32, current_level: u32) -> u64 {
        if current_level == 0 {
            return self.get_max_exp_for_level(template_id, 1);
        }
        
        let current_total_exp = self.get_max_exp_for_level(template_id, current_level);
        let next_level_total_exp = self.get_max_exp_for_level(template_id, current_level + 1);
        
        next_level_total_exp - current_total_exp
    }
}

impl MapData {
    pub fn new(map_id: &str, map_path: &str) -> Result<Self, String> {
        if !std::path::Path::new(map_path).exists() {
            return Err(format!("Map file not found: {}", map_path));
        }

        let obstacle_map = Self::load_obstacle_map(map_path);
        let (spawn_points, valid_positions) =
            Self::generate_spawn_points_from_map(map_path, &obstacle_map);
        info!("No of Spawn points: {:?}", spawn_points.len());
        let mut spawn_point_map = HashMap::new();
        for spawn_point in &spawn_points {
            spawn_point_map.insert(spawn_point.id.clone(), spawn_point.clone());
        }

        Ok(MapData {
            map_id: map_id.to_string(),
            spawn_points: spawn_point_map,
            obstacle_map,
            valid_positions,
        })
    }

    /// Loads obstacle data from a Tiled map file
    fn load_obstacle_map(map_path: &str) -> ObstacleMap {
        let file = File::open(Path::new(map_path)).expect("Failed to open map file");
        let reader = BufReader::new(file);
        let map_data: serde_json::Value =
            serde_json::from_reader(reader).expect("Failed to parse map JSON");

        let width = map_data["width"].as_u64().unwrap_or(70) as usize;
        let height = map_data["height"].as_u64().unwrap_or(70) as usize;

        let mut obstacle_data = vec![false; width * height];
        let layers = map_data["layers"].as_array().unwrap();

        for layer in layers {
            if let Some(name) = layer["name"].as_str() {
                if name == "obstacles" {
                    if let Some(data) = layer["data"].as_array() {
                        for (i, tile) in data.iter().enumerate() {
                            if i < width * height {
                                obstacle_data[i] = tile.as_u64().unwrap_or(0) != 0;
                            }
                        }
                    }
                    break;
                }
            }
        }

        ObstacleMap {
            width,
            height,
            data: obstacle_data,
        }
    }

    /// Extracts spawn point data from a Tiled map file and generates valid positions
    fn generate_spawn_points_from_map(
        map_path: &str,
        obstacle_map: &ObstacleMap,
    ) -> (Vec<SpawnPoint>, HashMap<String, ValidPositionsMap>) {
        let file = File::open(Path::new(map_path)).expect("Failed to open map file");
        let reader = BufReader::new(file);
        let map_data: serde_json::Value =
            serde_json::from_reader(reader).expect("Failed to parse map JSON");

        let mut spawn_points = Vec::new();
        let mut valid_positions_map = HashMap::new();

        let layers = map_data["layers"].as_array().unwrap();
        for layer in layers {
            if let Some(name) = layer["name"].as_str() {
                if name == "monster_spawn" {
                    if let Some(objects) = layer["objects"].as_array() {
                        for (i, object) in objects.iter().enumerate() {
                            let id = format!("spawn_area_{}", i + 1);

                            // Convert pixel coordinates to tile coordinates (32px tile size)
                            let x = (object["x"].as_f64().unwrap() / 32.0) as u32;
                            let y = (object["y"].as_f64().unwrap() / 32.0) as u32;
                            let width = (object["width"].as_f64().unwrap() / 32.0) as u32;
                            let height = (object["height"].as_f64().unwrap() / 32.0) as u32;

                            let mut spawn_density = None;
                            if let Some(properties) =
                                object.get("properties").and_then(|p| p.as_array())
                            {
                                for prop in properties {
                                    if let (Some("spawn_density"), Some(value)) = (
                                        prop.get("name").and_then(|v| v.as_str()),
                                        prop.get("value").and_then(|v| v.as_f64()),
                                    ) {
                                        spawn_density = Some(value as f32);
                                    }
                                }
                            }

                            let spawn_point = SpawnPoint {
                                id: id.clone(),
                                tile_x: x,
                                tile_y: y,
                                width,
                                height,
                                allowed_monsters: DEFAULT_ALLOWED_MONSTER_IDS.to_vec(),
                                max_monsters: 3,
                                spawn_interval_sec: 15,
                                spawn_density,
                            };

                            let valid_positions =
                                Self::generate_valid_positions(&spawn_point, obstacle_map);

                            valid_positions_map.insert(
                                id.clone(),
                                ValidPositionsMap {
                                    spawn_point_id: id.clone(),
                                    valid_positions,
                                },
                            );

                            spawn_points.push(spawn_point);
                        }
                    }
                    break;
                }
            }
        }

        if spawn_points.is_empty() {
            tracing::warn!("No spawn points found in map file, using defaults.");
            let default_spawn_point = SpawnPoint {
                id: "spawn_area_1".to_string(),
                tile_x: 20,
                tile_y: 7,
                width: 7,
                height: 5,
                allowed_monsters: DEFAULT_ALLOWED_MONSTER_IDS.to_vec(),
                max_monsters: 3,
                spawn_interval_sec: 15,
                spawn_density: None,
            };

            let valid_positions =
                Self::generate_valid_positions(&default_spawn_point, obstacle_map);

            valid_positions_map.insert(
                default_spawn_point.id.clone(),
                ValidPositionsMap {
                    spawn_point_id: default_spawn_point.id.clone(),
                    valid_positions,
                },
            );

            spawn_points.push(default_spawn_point);
        }

        (spawn_points, valid_positions_map)
    }

    /// Creates a set of valid (non-obstacle) positions within a spawn area
    fn generate_valid_positions(
        spawn_point: &SpawnPoint,
        obstacle_map: &ObstacleMap,
    ) -> HashSet<(u32, u32)> {
        let mut valid_positions = HashSet::new();

        for x in 0..spawn_point.width {
            for y in 0..spawn_point.height {
                let tile_x = spawn_point.tile_x + x;
                let tile_y = spawn_point.tile_y + y;

                if tile_x as usize >= obstacle_map.width || tile_y as usize >= obstacle_map.height {
                    continue;
                }

                let index = (tile_y as usize) * obstacle_map.width + (tile_x as usize);
                if !obstacle_map.data.get(index).copied().unwrap_or(true) {
                    valid_positions.insert((tile_x, tile_y));
                }
            }
        }

        valid_positions
    }

    /// Checks if a position is not blocked by an obstacle
    pub fn is_valid_position(&self, tile_x: u32, tile_y: u32) -> bool {
        if tile_x as usize >= self.obstacle_map.width || tile_y as usize >= self.obstacle_map.height
        {
            return false;
        }

        let index = tile_y as usize * self.obstacle_map.width + tile_x as usize;
        !self.obstacle_map.data.get(index).copied().unwrap_or(true)
    }

    /// Checks if position is in the pre-computed valid positions set
    pub fn is_position_in_valid_set(&self, spawn_point_id: &str, tile_x: u32, tile_y: u32) -> bool {
        if let Some(valid_pos_map) = self.valid_positions.get(spawn_point_id) {
            return valid_pos_map.valid_positions.contains(&(tile_x, tile_y));
        }
        false
    }
}

impl MonsterManagerFactory {
    pub async fn new(templates_path: &str) -> Arc<Self> {
        let template_repository = MonsterTemplateRepository::new(templates_path).await;

        Arc::new(MonsterManagerFactory {
            template_repository,
            loaded_maps: RwLock::new(HashMap::new()),
        })
    }

    pub async fn new_with_move_repository(
        templates_path: &str,
        move_repository: Arc<crate::monsters::move_manager::MoveRepository>,
    ) -> Arc<Self> {
        let template_repository = MonsterTemplateRepository::new(templates_path).await;
        let template_repository = template_repository.with_move_repository(move_repository);

        Arc::new(MonsterManagerFactory {
            template_repository,
            loaded_maps: RwLock::new(HashMap::new()),
        })
    }

    pub async fn create_monster_manager(
        &self,
        map_id: &str,
    ) -> Result<Arc<MonsterManager>, String> {
        // Check if map data is already loaded
        let loaded_maps = self.loaded_maps.read().await;
        if let Some(map_data) = loaded_maps.get(map_id) {
            return Ok(Arc::new(MonsterManager {
                template_repository: self.template_repository.clone(),
                map_data: MapData {
                    map_id: map_data.map_id.clone(),
                    spawn_points: map_data.spawn_points.clone(),
                    obstacle_map: map_data.obstacle_map.clone(),
                    valid_positions: map_data.valid_positions.clone(),
                },
            }));
        }
        drop(loaded_maps);

        // Load the map data
        let map_path = format!("resources/{}.json", map_id);
        let map_data = match MapData::new(map_id, &map_path) {
            Ok(data) => Arc::new(data),
            Err(e) => return Err(e),
        };

        // Cache the loaded map data
        let mut loaded_maps = self.loaded_maps.write().await;
        loaded_maps.insert(map_id.to_string(), map_data.clone());

        Ok(Arc::new(MonsterManager {
            template_repository: self.template_repository.clone(),
            map_data: MapData {
                map_id: map_data.map_id.clone(),
                spawn_points: map_data.spawn_points.clone(),
                obstacle_map: map_data.obstacle_map.clone(),
                valid_positions: map_data.valid_positions.clone(),
            },
        }))
    }
}

impl MonsterManager {
    /// Returns a random valid position within a spawn area
    fn get_random_spawn_position(&self, spawn_point: &SpawnPoint) -> Option<Position> {
        // Try using pre-computed valid positions
        if let Some(valid_pos_map) = self.map_data.valid_positions.get(&spawn_point.id) {
            if !valid_pos_map.valid_positions.is_empty() {
                let mut rng = SmallRng::from_entropy();
                let valid_positions = valid_pos_map.valid_positions.iter().collect::<Vec<_>>();
                let random_index = rng.gen_range(0..valid_positions.len());
                let (x, y) = *valid_positions[random_index];
                return Some(Position { x, y });
            }
        }

        // Fallback method - try random positions
        let mut rng = SmallRng::from_entropy();
        for _ in 0..10 {
            let offset_x = rng.gen_range(0..spawn_point.width);
            let offset_y = rng.gen_range(0..spawn_point.height);

            let tile_x = spawn_point.tile_x + offset_x;
            let tile_y = spawn_point.tile_y + offset_y;

            if self.map_data.is_valid_position(tile_x, tile_y) {
                return Some(Position {
                    x: tile_x,
                    y: tile_y,
                });
            }
        }

        None
    }

    /// Creates a new monster at the specified spawn point
    pub async fn spawn_monster(
        &self,
        template_id: u32,
        spawn_point_id: &str,
        lobby: &Arc<Lobby>,
    ) -> Option<Monster> {
        let template = match self.template_repository.templates.get(&template_id) {
            Some(template) => template,
            None => {
                tracing::error!("Monster template not found: {}", template_id);
                return None;
            }
        };

        let spawn_point = match self.map_data.spawn_points.get(spawn_point_id) {
            Some(spawn_point) => spawn_point,
            None => {
                tracing::error!("Spawn point not found: {}", spawn_point_id);
                return None;
            }
        };

        // Check if we've reached the maximum monsters for this spawn point in this lobby
        let monsters_in_spawn = lobby
            .monsters_by_spawn_point
            .get(spawn_point_id)
            .map(|monsters| monsters.len())
            .unwrap_or(0) as u32;

        if monsters_in_spawn >= spawn_point.max_monsters {
            return None;
        }

        // Get a random valid position within the spawn point
        let position = match self.get_random_spawn_position(spawn_point) {
            Some(pos) => pos,
            None => {
                tracing::error!(
                    "No valid positions found for spawn point: {}",
                    spawn_point_id
                );
                return None;
            }
        };

        // Determine a random level for the monster
        let min_level = template.min_level;
        let max_level = template.max_level;
        let level = if min_level == max_level {
            min_level
        } else {
            let mut rng = SmallRng::from_entropy();
            rng.gen_range(min_level..=max_level)
        };

        // Create a new monster instance, passing the move repository if available
        let monster = Monster::new(
            template, 
            position, 
            level, 
            self.template_repository.move_repository.as_ref(),
        );

        // Update lobby's active monsters
        lobby
            .active_monsters
            .insert(monster.instance_id.clone(), Arc::new(Mutex::new(monster.clone())));

        // Add to spawn point mapping in the lobby
        lobby
            .monsters_by_spawn_point
            .entry(spawn_point_id.to_string())
            .or_insert_with(Vec::new)
            .push(monster.instance_id.clone());

        tracing::info!(
            "Spawned new monster: {} ({})",
            monster.name,
            monster.instance_id
        );

        Some(monster)
    }

    /// Removes a monster from a lobby
    pub async fn despawn_monster(&self, instance_id: &str, lobby: &Arc<Lobby>) -> Option<Arc<Mutex<Monster>>> {
        let monster = lobby.active_monsters.remove(instance_id)?.1; // Extract the Monster from the tuple

        // Find all spawn points that contain this monster
        let spawn_points: Vec<String> = lobby
            .monsters_by_spawn_point
            .iter()
            .filter(|entry| entry.value().contains(&instance_id.to_string()))
            .map(|entry| entry.key().clone())
            .collect();

        // Remove monster from each spawn point's list
        for spawn_point_id in spawn_points {
            if let Some(mut entry) = lobby.monsters_by_spawn_point.get_mut(&spawn_point_id) {
                let monsters = entry.value_mut();
                if let Some(pos) = monsters.iter().position(|id| id == instance_id) {
                    monsters.remove(pos);
                }
            }
        }

        Some(monster)
    }

    /// Selects a random monster type based on spawn rate weighting
    pub fn get_random_monster_for_spawn_point(
        &self,
        spawn_point_id: &str,
    ) -> Option<&MonsterTemplate> {
        let spawn_point = self.map_data.spawn_points.get(spawn_point_id)?;

        let allowed_templates: Vec<&MonsterTemplate> = spawn_point
            .allowed_monsters
            .iter()
            .filter_map(|id| self.template_repository.templates.get(id))
            .collect();

        if allowed_templates.is_empty() {
            return None;
        }

        // Weighted random selection
        let total_spawn_rate: f32 = allowed_templates.iter().map(|t| t.spawn_rate).sum();
        let mut rng = SmallRng::from_entropy();
        let random_value = rng.gen_range(0.0..total_spawn_rate);

        let mut cumulative = 0.0;
        for template in &allowed_templates {
            cumulative += template.spawn_rate;
            if random_value <= cumulative {
                return Some(template);
            }
        }

        allowed_templates.first().copied()
    }

    /// Gets all monsters in a specific lobby
    pub fn get_monsters_for_lobby(lobby: &Arc<Lobby>) -> Vec<Arc<Mutex<Monster>>> {
        lobby
            .active_monsters
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Gets all monsters in a specific spawn area in a lobby
    pub fn get_monsters_in_spawn_point(lobby: &Arc<Lobby>, spawn_point_id: &str) -> Vec<Arc<Mutex<Monster>>> {
        if let Some(instance_ids) = lobby.monsters_by_spawn_point.get(spawn_point_id) {
            instance_ids
                .iter()
                .filter_map(|id| lobby.active_monsters.get(id).map(|m| m.value().clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Resets a monster's combat state after battle
    pub async fn reset_monster_combat_state(lobby: &Arc<Lobby>, monster_id: &str) -> bool {
        if let Some(monster_entry) = lobby.active_monsters.get(monster_id) {
            if let Ok(mut monster) = monster_entry.value().try_lock() {
                monster.in_combat = false;
                monster.current_hp = monster.current_hp;
                tracing::info!("Reset combat state for monster {}", monster_id);
                return true;
            }
        }

        false
    }
}
