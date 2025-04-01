use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    combat::state::{
        BallType, BattleEndReason, BattleEvent, BattlePokemonPrivateView, BattlePokemonPublicView,
        BattlePokemonTeamOverview, FieldState, PlayerAction, SwitchReason, WildBattleOutcome,
        BattleMoveView, StatusCondition,
    },
    game_loop::pokemon_collection::Pokemon,
    monsters::monster::{DisplayMonster, PokemonType},
    stats::{CalculatedStats, nature::Nature},
};

// Player state
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerState {
    pub id: String,
    pub username: String,  // Added username field
    pub x: u32,
    pub y: u32,
    pub direction: String,
    #[serde(skip)]
    pub in_combat: bool, // Whether player is in combat
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
    #[serde(rename = "interact")]
    Interact {
        monster_id: Option<String>,
    },
    #[serde(rename = "choose_starter")]
    ChooseStarter {
        starter_id: u32,
    },
    // New combat action message
    #[serde(rename = "combat_action")]
    CombatAction {
        battle_id: Uuid,
        action: PlayerAction,
    },
    // New player challenge messages
    #[serde(rename = "challenge_player")]
    ChallengePlayer {
        target_player_id: String,
    },
    #[serde(rename = "respond_to_challenge")]
    RespondToChallenge {
        challenger_id: String,
        accepted: bool,
    },
}

// New struct for client-friendly Pokemon display
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DisplayPokemon {
    pub id: String,
    pub template_id: u32,
    pub name: String,
    pub level: u32,
    pub exp: u64,
    pub max_exp: u64,
    pub current_hp: u32,
    pub max_hp: u32,
    pub calculated_stats: CalculatedStats,
    pub nature: Nature,
    pub capture_date: u64,
    pub moves: Vec<BattleMoveView>, // Use the detailed move view
    pub types: Vec<PokemonType>,
    pub ability: String,
    pub status_condition: Option<StatusCondition>,
}

// Server messages
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "welcome")]
    Welcome { id: String, username: String, x: u32, y: u32 },
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
    MonsterSpawned { monster: DisplayMonster },
    #[serde(rename = "monster_moved")]
    MonsterMoved { monster: DisplayMonster },
    #[serde(rename = "monster_despawned")]
    MonsterDespawned { instance_id: String },
    #[serde(rename = "monsters")]
    Monsters { monsters: Vec<DisplayMonster> },
    #[serde(rename = "new_pokemon")]
    NewPokemon { pokemon: DisplayPokemon, active_index: Option<usize> },
    #[serde(rename = "pokemon_collection")]
    ActivePokemons {
        pokemons: Vec<DisplayPokemon>,
    },
    
    // New combat system messages from the spec
    #[serde(rename = "wild_battle_start")]
    WildBattleStart {
        battle_id: Uuid,
        player_team: Vec<BattlePokemonTeamOverview>,
        initial_pokemon: BattlePokemonPrivateView,
        wild_pokemon: BattlePokemonPublicView,
        initial_field_state: FieldState,
    },
    #[serde(rename = "pvp_battle_start")]
    PvPBattleStart {
        battle_id: Uuid,
        // Own team details
        player_team: Vec<BattlePokemonTeamOverview>,
        initial_pokemon: BattlePokemonPrivateView,
        // Opponent details
        opponent_id: String,
        opponent_username: String,
        opponent_initial_pokemon: BattlePokemonPublicView,
        initial_field_state: FieldState,
        // Whether this player goes first
        player1_id: String,
        player2_id: String,
    },
    #[serde(rename = "request_action")]
    RequestAction {
        turn_number: u32,
        active_pokemon_state: BattlePokemonPrivateView,
        team_overview: Vec<BattlePokemonTeamOverview>,
        other_pokemon_state: BattlePokemonPublicView,
        can_switch: bool,
        must_switch: bool,
        field_state: FieldState,
    },
    #[serde(rename = "turn_update")]
    TurnUpdate {
        turn_number: u32,
        events: Vec<BattleEvent>,
    },
    #[serde(rename = "request_switch")]
    RequestSwitch {
        reason: SwitchReason,
        available_switches: Vec<BattlePokemonTeamOverview>,
    },
    #[serde(rename = "capture_attempt")]
    CaptureAttempt {
        ball_type: BallType,
        shake_count: u8,
        success: bool,
    },
    #[serde(rename = "battle_end")]
    BattleEnd {
        outcome: WildBattleOutcome,
        reason: BattleEndReason,
        pokemon_captured: Option<BattlePokemonPrivateView>,
    },
    // New player challenge messages
    #[serde(rename = "challenge_received")]
    ChallengeReceived {
        challenger_id: String,
        challenger_username: String,
    },
    #[serde(rename = "challenge_response")]
    ChallengeResponse {
        target_player_id: String,
        target_username: String,
        accepted: bool,
    },
    #[serde(rename = "challenge_failed")]
    ChallengeFailed {
        reason: String,
    },
}
