pub mod wild_battle;
pub mod pvp_battle;
pub mod battle_calculations;
pub mod battle_effects;

// Re-export the main entry points
pub use wild_battle::process_turn;
pub use pvp_battle::process_pvp_turn;
