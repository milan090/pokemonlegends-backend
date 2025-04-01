use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::state::{
    BallType, BattleEndReason, BattleEvent, BattlePokemonPrivateView,
    BattlePokemonPublicView, BattlePokemonTeamOverview, FieldState, PlayerAction,
    SwitchReason, WildBattleOutcome
};

/// Messages sent from the client to the server during combat
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientBattleMessage {
    /// Client submits their action for the turn
    SubmitAction { action: PlayerAction },
    /// Client acknowledges seeing a prompt (e.g., waitscreen)
    Acknowledge,
    /// Client requests initial state if they somehow disconnected/reconnected
    RequestSync,
}

/// Messages sent from the server to the client during combat
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerBattleMessage {
    /// Sent when the wild battle begins
    WildBattleStart {
        battle_id: Uuid,
        player_team: Vec<BattlePokemonTeamOverview>,
        initial_pokemon: BattlePokemonPrivateView,
        wild_pokemon: BattlePokemonPublicView,
        initial_field_state: FieldState,
    },
    /// Sent at the start of each turn (or when action needed)
    RequestAction {
        turn_number: u32,
        active_pokemon_state: BattlePokemonPrivateView,
        team_overview: Vec<BattlePokemonTeamOverview>,
        wild_pokemon: BattlePokemonPublicView,
        can_switch: bool,
        must_switch: bool,
        field_state: FieldState,
    },
    /// Sent after actions are processed
    TurnUpdate {
        turn_number: u32,
        events: Vec<BattleEvent>,
    },
    /// Specific request for a switch when a Pokemon faints
    RequestSwitch {
        reason: SwitchReason,
        available_switches: Vec<BattlePokemonTeamOverview>,
    },
    /// Sent when a capture attempt is made
    CaptureAttempt {
        ball_type: BallType,
        shake_count: u8,
        success: bool,
    },
    /// Sent when the battle concludes
    BattleEnd {
        outcome: WildBattleOutcome,
        reason: BattleEndReason,
        exp_gained: Option<u32>, // Experience gained if defeated
        pokemon_captured: Option<BattlePokemonPrivateView>, // Full details if captured
    },
    /// General error message
    Error {
        message: String,
    },
    /// Simple Pong response for Keepalive
    Pong,
} 