use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::monsters::PokemonType;
use crate::stats::nature::Nature;
use crate::stats::{BaseStats, BattleStatModifiers, CalculatedStats, StatName, StatSet};

/// Main Battle State Container for a wild Pokémon encounter
#[derive(Debug)]
pub struct WildBattleState {
    pub battle_id: Uuid,
    pub player: BattlePlayer,
    pub wild_pokemon: BattlePokemon,
    pub turn_number: u32,
    pub battle_phase: BattlePhase,
    pub player_action: Option<PlayerAction>, // Stores submitted action for the turn
    pub wild_action: Option<WildPokemonAction>, // Store action for wild Pokémon
    pub turn_order: Option<TurnOrder>, // Determined after actions are submitted
    pub field_state: FieldState,
    pub battle_log: Vec<BattleEvent>, // Log of events for client
    pub capture_attempts: Vec<CaptureAttempt>, // Track Poké Ball throws
    pub move_repository: Option<std::sync::Arc<crate::monsters::move_manager::MoveRepository>>, // Reference to move repository for move info
}

/// Main Battle State Container for a PvP battle between two players
#[derive(Debug)]
pub struct PvPBattleState {
    pub battle_id: Uuid,
    pub player1: BattlePlayer,
    pub player2: BattlePlayer,
    pub turn_number: u32,
    pub battle_phase: BattlePvPPhase,
    pub player1_action: Option<PlayerAction>, // Stores submitted action for player 1
    pub player2_action: Option<PlayerAction>, // Stores submitted action for player 2
    pub turn_order: Option<PvPTurnOrder>, // Determined after actions are submitted
    pub field_state: FieldState,
    pub battle_log: Vec<BattleEvent>, // Log of events for client
    pub move_repository: Option<std::sync::Arc<crate::monsters::move_manager::MoveRepository>>, // Reference to move repository for move info
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TurnOrder {
    PlayerFirst,
    WildFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PvPTurnOrder {
    Player1First,
    Player2First,
    Player1FirstThenPlayer2, // For case where both take different actions that need to be processed in order
    Player2FirstThenPlayer1,
    Simultaneous, // For status checks, weather effects, etc.
}

/// Phases specific to PvP battles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum BattlePvPPhase {
    WaitingForBothPlayersActions, // Waiting for both players to submit actions
    WaitingForPlayer1Action,      // Waiting specifically for player 1
    WaitingForPlayer2Action,      // Waiting specifically for player 2
    ProcessingTurn,               // Server is calculating results
    WaitingForPlayer1Switch,      // Player 1 must switch (due to faint)
    WaitingForPlayer2Switch,      // Player 2 must switch (due to faint)
    Finished,                     // Battle has ended
}

/// Reason the PvP battle ended
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PvPBattleEndReason {
    Player1Victory,         // Player 1 defeated all of player 2's Pokémon
    Player2Victory,         // Player 2 defeated all of player 1's Pokémon
    Player1Surrendered,     // Player 1 forfeited the match
    Player2Surrendered,     // Player 2 forfeited the match
    Player1Disconnected,    // Player 1 disconnected from the battle
    Player2Disconnected,    // Player 2 disconnected from the battle
    TimeLimitReached,       // Battle took too long and timed out
    ServerError,            // Something went wrong on the server
}

/// Outcome of a PvP battle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PvPBattleOutcome {
    Victory,                // You won the battle
    Defeat,                 // You lost the battle
    Surrender,              // You surrendered the battle
    OpponentSurrendered,    // Opponent surrendered the battle
    Disconnected,           // You disconnected from the battle
    OpponentDisconnected,   // Opponent disconnected from the battle
    Draw,                   // The battle ended in a draw
}

/// Represents the player in a battle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattlePlayer {
    pub player_id: String,
    pub name: String,
    #[serde(skip)]
    pub team: Vec<BattlePokemon>,
    pub active_pokemon_index: usize,
    pub side_effects: PlayerSideState, // Effects specific to this player's side
    pub last_action_submitted: Option<PlayerAction>, // Track submitted action
    pub must_switch: bool, // Flag if the player needs to switch due to faint/Roar etc.
}

/// Represents a Pokémon in battle with all its dynamic state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattlePokemon {
    // Static Info (copied/derived at battle start)
    pub template_id: u32,
    pub name: String, // Can be nickname
    pub level: u32,
    pub pokemon_types: Vec<PokemonType>, // Current types (can be changed by moves)
    pub ability: String, // Ability ID
    pub moves: Vec<BattleMove>,
    pub instance_id: String, // Unique ID for this instance
    pub base_exp: u32,
    pub exp: u64,
    pub max_exp: u64,
    // Stats
    pub calculated_stats: CalculatedStats, // Stats adjusted for level (pre-battle/stages)
    pub ivs: StatSet<u8>,
    pub evs: StatSet<u16>,
    pub nature: Nature,

    // Dynamic State
    pub current_hp: u32,
    pub max_hp: u32,
    pub status: Option<StatusCondition>,
    pub status_turns: u8, // Counter for sleep, toxic
    pub volatile_statuses: HashMap<VolatileStatusType, VolatileStatusData>, // Confusion, Taunt, Flinch etc.
    pub stat_modifiers: BattleStatModifiers, // The [-6, +6] modifiers
    pub is_fainted: bool,
    pub position: usize, // Position in the team array (0-5) - useful for client UI
    pub is_wild: bool,   // Indicates if this is a wild Pokémon
}

/// A move in battle with PP tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleMove {
    pub move_id: u32,
    pub current_pp: u8,
    pub max_pp: u8,
    // We could store a direct reference/copy of MoveTemplate here if needed frequently
}

/// Temporary status effects that can be applied to Pokémon
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VolatileStatusType {
    Confusion,
    Flinch,
    Taunt,
    LeechSeed,
    Substitute,
    Bound,
    // Other volatile statuses can be added as needed
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StatusCondition {
    Burn, Freeze, Paralysis, Poison, Sleep, Toxic, // Toxic is distinct for damage calculation
}

/// Data associated with a volatile status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatileStatusData {
    pub turns_left: Option<u8>, // None if indefinite until condition met (e.g. switch out)
    // Additional data can be added as needed per status type
}

/// Categorizes moves as physical, special, or status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum MoveCategory {
    Physical,
    Special,
    Status,
}

/// Side effects specific to the player's side of the field
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerSideState {
    pub reflect_turns: u8,
    pub light_screen_turns: u8,
    pub tailwind_turns: u8,
    pub stealth_rock: bool,
    pub spikes_layers: u8,
    pub toxic_spikes_layers: u8,
    pub sticky_web: bool,
    // Other side effects can be added as needed
}

/// Global field state affecting both sides
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldState {
    pub weather: Option<WeatherState>,
    pub trick_room_turns: u8,
    // Other field-wide effects can be added as needed
}

/// Weather state with type and duration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherState {
    pub weather_type: WeatherType,
    pub turns_left: u8, // Can be indefinite for ability-induced weather initially
}

/// Types of weather
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum WeatherType {
    Rain,
    HarshSunlight,
    Sandstorm,
    Hail,
}

/// Current phase of the battle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum BattlePhase {
    WaitingForPlayerAction, // Player needs to choose move/switch/item/run
    ProcessingTurn,         // Server is calculating results
    WaitingForSwitch,       // Player must switch (due to faint)
    CaptureMechanics,       // Processing Poké Ball throw
    Finished,               // Battle has ended
}

/// Action that a player can take during their turn
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "action_type", rename_all = "snake_case")]
pub enum PlayerAction {
    UseMove {
        move_index: usize,
    },
    SwitchPokemon {
        team_index: usize,
    },
    UseItem {
        item_id: String,
        is_capture_item: bool,
    },
    Run,
}

/// Tracks a capture attempt with a Poké Ball
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureAttempt {
    pub ball_type: BallType,
    pub shake_count: u8,    // 0-3 shakes
    pub success: bool,
    pub turn_number: u32,
}

/// Types of Poké Balls
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BallType {
    PokeBall,
    GreatBall,
    UltraBall,
    // Other ball types can be added as needed
}

/// Action that a wild Pokémon can take
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action_type", rename_all = "snake_case")]
pub enum WildPokemonAction {
    UseMove {
        move_index: usize,
    },
    Struggle,
    Flee,
}

/// Reason for requesting a switch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SwitchReason {
    Fainted,
    Forced,
}

/// Reason the battle ended
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BattleEndReason {
    WildPokemonDefeated,
    WildPokemonCaptured,
    WildPokemonFled,
    PlayerRanAway,
    AllPlayerPokemonFainted,
    PlayerDisconnected,
}

/// Outcome of a wild battle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WildBattleOutcome {
    Victory,    // Player defeated the wild Pokémon
    Captured,   // Player captured the wild Pokémon
    Fled,       // Wild Pokémon fled
    PlayerRan,  // Player ran away
    Defeat,     // Player's team was defeated
    PlayerDisconnected, // Player disconnected from battle
}

/// Event that occurs during battle for client-side animation/logging
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "details", rename_all = "snake_case")]
pub enum BattleEvent {
    MoveUsed { source: BattleEntityRef, move_id: u32, move_name: String, target: BattleEntityRef },
    DamageDealt { target: BattleEntityRef, damage: u32, new_hp: u32, max_hp: u32, effectiveness: f32, is_critical: bool },
    Heal { target: BattleEntityRef, amount: u32, new_hp: u32, max_hp: u32 },
    StatusApplied { target: BattleEntityRef, status: StatusCondition },
    StatusRemoved { target: BattleEntityRef, status: StatusCondition },
    StatusDamage { target: BattleEntityRef, status: StatusCondition, damage: u32, new_hp: u32, max_hp: u32 },
    VolatileStatusApplied { target: BattleEntityRef, volatile_status: VolatileStatusType },
    VolatileStatusRemoved { target: BattleEntityRef, volatile_status: VolatileStatusType },
    StatChange { target: BattleEntityRef, stat: StatName, stages: i8, new_stage: i8, success: bool },
    PokemonFainted { target: BattleEntityRef },
    SwitchIn { pokemon_view: BattlePokemonPublicView, team_index: usize },
    FieldEffectApplied { effect_type: FieldEffectType, target_side: EffectTargetSide },
    FieldEffectEnded { effect_type: FieldEffectType, target_side: EffectTargetSide },
    WeatherStarted { weather_type: WeatherType },
    WeatherEnded,
    MoveFailed { source: BattleEntityRef, reason: String },
    ItemUsed { item_id: String, item_name: String, target: Option<BattleEntityRef> },
    CaptureAttempt { ball_type: BallType, shake_count: u8, success: bool },
    WildPokemonFled,
    PlayerRanAway { success: bool },
    GenericMessage { message: String },
    TurnStart { turn_number: u32 },
    ExpGained { source: BattleEntityRef, amount: u64 },
}

/// Reference to either player's Pokémon or wild Pokémon
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "entity_type", rename_all = "snake_case")]
pub enum BattleEntityRef {
    Player { team_index: usize },
    Wild,
    Player1 { team_index: usize },
    Player2 { team_index: usize },
}

/// Types of field effects
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum FieldEffectType {
    Reflect,
    LightScreen,
    Tailwind,
    StealthRock,
    Spikes,
    ToxicSpikes,
    StickyWeb,
    TrickRoom,
}

/// Target side for field effects
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "snake_case")]
pub enum EffectTargetSide {
    Player,
    Opponent,
    Both,
}

// View structs for client communication

/// Public view of a Pokémon safe to show to others
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattlePokemonPublicView {
    pub template_id: u32,
    pub name: String,
    pub level: u32,
    pub current_hp_percent: f32,
    pub max_hp: u32, // Needed for HP bar rendering
    pub types: Vec<PokemonType>,
    pub status: Option<StatusCondition>,
    pub stat_modifiers: BattleStatModifiers,
    pub is_fainted: bool,
    pub is_wild: bool,
}

/// Private view with full details for the player's own Pokémon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattlePokemonPrivateView {
    pub template_id: u32,
    pub name: String,
    pub level: u32,
    pub current_hp: u32,
    pub current_hp_percent: f32,
    pub max_hp: u32,
    pub types: Vec<PokemonType>,
    pub ability: String,
    pub status: Option<StatusCondition>,
    pub volatile_statuses: Vec<VolatileStatusType>,
    pub stat_modifiers: BattleStatModifiers,
    pub moves: Vec<BattleMoveView>,
    pub is_fainted: bool,
    pub team_index: usize,
}

/// View of a move with details needed for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleMoveView {
    pub move_id: u32,
    pub name: String,
    pub move_type: PokemonType,
    pub category: MoveCategory,
    pub current_pp: u8,
    pub max_pp: u8,
    pub power: Option<u32>,
    pub accuracy: Option<u8>,
    pub description: String, // For tooltips
}

/// Minimal info for team sidebar UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattlePokemonTeamOverview {
    pub template_id: u32,
    pub name: String,
    pub level: u32,
    pub current_hp_percent: f32,
    pub current_hp: u32,
    pub max_hp: u32,
    pub status: Option<StatusCondition>,
    pub is_fainted: bool,
    pub team_index: usize,
}

// Extension methods for PvPBattleState
impl PvPBattleState {
    /// Create a new PvP battle state
    pub fn new(
        battle_id: Uuid,
        player1: BattlePlayer,
        player2: BattlePlayer,
        move_repository: Option<std::sync::Arc<crate::monsters::move_manager::MoveRepository>>,
    ) -> Self {
        PvPBattleState {
            battle_id,
            player1,
            player2,
            turn_number: 1,
            battle_phase: BattlePvPPhase::WaitingForBothPlayersActions,
            player1_action: None,
            player2_action: None,
            turn_order: None,
            field_state: FieldState::default(),
            battle_log: Vec::new(),
            move_repository,
        }
    }

    /// Get a reference to a player by ID
    pub fn get_player_by_id(&self, player_id: &str) -> Option<&BattlePlayer> {
        if self.player1.player_id == player_id {
            Some(&self.player1)
        } else if self.player2.player_id == player_id {
            Some(&self.player2)
        } else {
            None
        }
    }

    /// Get a mutable reference to a player by ID
    pub fn get_player_by_id_mut(&mut self, player_id: &str) -> Option<&mut BattlePlayer> {
        if self.player1.player_id == player_id {
            Some(&mut self.player1)
        } else if self.player2.player_id == player_id {
            Some(&mut self.player2)
        } else {
            None
        }
    }

    /// Get the opponent's ID given a player ID
    pub fn get_opponent_id(&self, player_id: &str) -> Option<&str> {
        if self.player1.player_id == player_id {
            Some(&self.player2.player_id)
        } else if self.player2.player_id == player_id {
            Some(&self.player1.player_id)
        } else {
            None
        }
    }

    /// Check if both players have submitted actions
    pub fn both_actions_submitted(&self) -> bool {
        self.player1_action.is_some() && self.player2_action.is_some()
    }

    /// Check if a battle is ready to be processed
    pub fn ready_for_processing(&self) -> bool {
        match self.battle_phase {
            BattlePvPPhase::WaitingForBothPlayersActions => self.both_actions_submitted(),
            BattlePvPPhase::WaitingForPlayer1Action => self.player1_action.is_some(),
            BattlePvPPhase::WaitingForPlayer2Action => self.player2_action.is_some(),
            _ => false,
        }
    }
} 