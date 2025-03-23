use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rand::Rng;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::monsters::{Monster, MonsterTemplate, Position};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnArea {
    pub id: String,
    pub name: String,
    pub positions: Vec<Position>, // List of possible spawn positions
    pub allowed_monsters: Vec<String>, // Monster template IDs that can spawn here
    pub max_monsters: u32, // Maximum monsters in this area at once
    pub spawn_interval_sec: u64, // Time between spawn attempts
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterTemplates {
    pub monsters: Vec<MonsterTemplate>,
    pub spawn_areas: Vec<SpawnArea>,
}

pub struct MonsterManager {
    pub templates: HashMap<String, MonsterTemplate>,
    pub spawn_areas: HashMap<String, SpawnArea>,
    pub active_monsters: RwLock<HashMap<String, Monster>>, // Instance ID -> Monster
    pub monsters_by_area: RwLock<HashMap<String, Vec<String>>>, // Area ID -> List of monster instance IDs
}

impl MonsterManager {
    pub async fn new(templates_path: &str) -> Arc<Self> {
        let templates = Self::load_templates(templates_path);
        
        let mut template_map = HashMap::new();
        let mut spawn_area_map = HashMap::new();
        
        for template in &templates.monsters {
            template_map.insert(template.id.clone(), template.clone());
        }
        
        for area in &templates.spawn_areas {
            spawn_area_map.insert(area.id.clone(), area.clone());
        }
        
        Arc::new(MonsterManager {
            templates: template_map,
            spawn_areas: spawn_area_map,
            active_monsters: RwLock::new(HashMap::new()),
            monsters_by_area: RwLock::new(HashMap::new()),
        })
    }
    
    fn load_templates(path: &str) -> MonsterTemplates {
        let file = File::open(Path::new(path)).expect("Failed to open monster templates file");
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).expect("Failed to parse monster templates JSON")
    }
    
    // Spawn a monster of the given template in a specific area
    pub async fn spawn_monster(&self, template_id: &str, area_id: &str) -> Option<Monster> {
        // Check if template exists
        let template = self.templates.get(template_id)?;
        let area = self.spawn_areas.get(area_id)?;
        
        // Check if the monster is allowed in this area
        if !area.allowed_monsters.contains(&template.id) {
            return None;
        }
        
        // Check if we've reached the spawn limit for this area
        let monsters_by_area = self.monsters_by_area.read().await;
        let default_area_monsters = Vec::new();
        let area_monsters = monsters_by_area.get(area_id).unwrap_or(&default_area_monsters);
        if area_monsters.len() >= area.max_monsters as usize {
            return None;
        }
        drop(monsters_by_area); // Release read lock
        
        // Choose a random spawn position
        let mut rng = SmallRng::from_entropy();
        if area.positions.is_empty() {
            return None;
        }
        let position_idx = rng.gen_range(0..area.positions.len());
        let position = area.positions[position_idx].clone();
        
        // Create the monster
        let monster = Monster::new(template, position);
        
        // Store the monster in active monsters
        let mut active_monsters = self.active_monsters.write().await;
        active_monsters.insert(monster.instance_id.clone(), monster.clone());
        
        // Add to area tracking
        let mut monsters_by_area = self.monsters_by_area.write().await;
        let area_monsters = monsters_by_area.entry(area_id.to_string()).or_insert_with(Vec::new);
        area_monsters.push(monster.instance_id.clone());
        
        Some(monster)
    }
    
    // Remove a monster from the game
    pub async fn despawn_monster(&self, instance_id: &str) -> Option<Monster> {
        let mut active_monsters = self.active_monsters.write().await;
        let monster = active_monsters.remove(instance_id)?;
        
        // Remove from area tracking
        let mut monsters_by_area = self.monsters_by_area.write().await;
        for (_, instance_ids) in monsters_by_area.iter_mut() {
            if let Some(pos) = instance_ids.iter().position(|id| id == instance_id) {
                instance_ids.remove(pos);
                break;
            }
        }
        
        Some(monster)
    }
    
    // Get a random monster template weighted by spawn rate
    pub fn get_random_monster_for_area(&self, area_id: &str) -> Option<&MonsterTemplate> {
        let area = self.spawn_areas.get(area_id)?;
        let allowed_templates: Vec<&MonsterTemplate> = area.allowed_monsters
            .iter()
            .filter_map(|id| self.templates.get(id))
            .collect();
        
        if allowed_templates.is_empty() {
            return None;
        }
        
        // Calculate total spawn rate
        let total_spawn_rate: f32 = allowed_templates.iter().map(|t| t.spawn_rate).sum();
        
        // Choose based on weighted probability
        let mut rng = SmallRng::from_entropy();
        let random_value = rng.gen_range(0.0..total_spawn_rate);
        
        let mut cumulative = 0.0;
        for template in &allowed_templates {
            cumulative += template.spawn_rate;
            if random_value <= cumulative {
                return Some(template);
            }
        }
        
        // Fallback to first template
        allowed_templates.first().copied()
    }
    
    // Get list of active monsters in an area
    pub async fn get_monsters_in_area(&self, area_id: &str) -> Vec<Monster> {
        let monsters_by_area = self.monsters_by_area.read().await;
        let active_monsters = self.active_monsters.read().await;
        
        if let Some(instance_ids) = monsters_by_area.get(area_id) {
            instance_ids
                .iter()
                .filter_map(|id| active_monsters.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }
    
    // Get all active monsters
    pub async fn get_all_active_monsters(&self) -> Vec<Monster> {
        let active_monsters = self.active_monsters.read().await;
        active_monsters.values().cloned().collect()
    }
} 