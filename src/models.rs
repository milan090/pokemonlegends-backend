use serde::{Deserialize, Serialize};
use dashmap::DashMap;
use tokio::sync::broadcast;
use tokio::time::Instant;
use std::sync::Arc;

use crate::monsters::Monster;

// Player state
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerState {
    pub id: String,
    pub x: u32,
    pub y: u32,
    pub direction: String,
}

// Client messages
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "join")]
    Join { session_token: String },
    #[serde(rename = "move")]
    Move {
        x: u32,
        y: u32,
        direction: String,
    },
    #[serde(rename = "ping")]
    Ping,
}

// Server messages
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "welcome")]
    Welcome { id: String, x: u32, y: u32 },
    #[serde(rename = "players")]
    Players { players: Vec<PlayerState> },
    #[serde(rename = "player_joined")]
    PlayerJoined { player: PlayerState },
    #[serde(rename = "player_moved")]
    PlayerMoved { player: PlayerState },
    #[serde(rename = "player_left")]
    PlayerLeft { id: String },
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "players_moved")]
    PlayersMoved { players: Vec<PlayerState>, timestamp: u64 },
    #[serde(rename = "monster_spawned")]
    MonsterSpawned { monster: Monster },
    #[serde(rename = "monster_moved")]
    MonsterMoved { monster: Monster },
    #[serde(rename = "monster_despawned")]
    MonsterDespawned { instance_id: String },
    #[serde(rename = "monsters")]
    Monsters { monsters: Vec<Monster> },
}

// Lobby struct representing a game lobby
pub struct Lobby {
    pub id: String,
    pub player_positions: DashMap<String, PlayerState>,
    pub player_last_active: DashMap<String, Instant>,
    pub tx: broadcast::Sender<String>,
} 