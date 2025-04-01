use crate::combat::state::{WildBattleState, BattleEvent, BattleEntityRef, StatusCondition};
use crate::stats::StatName;

/// Helper function to apply move effects
pub fn apply_effect(
    battle_state: &mut WildBattleState,
    battle_events: &mut Vec<BattleEvent>,
    effect: &crate::monsters::move_manager::EffectData,
    source: BattleEntityRef,
    target: BattleEntityRef
) {
    match effect {
        crate::monsters::move_manager::EffectData::ApplyStatus { status, target: effect_target } => {
            let actual_target = match effect_target {
                crate::monsters::move_manager::EffectTarget::User => source.clone(),
                crate::monsters::move_manager::EffectTarget::Target => target.clone(),
            };
            
            // Get target Pokémon name
            let target_name = match actual_target {
                BattleEntityRef::Player { team_index } => battle_state.player.team[team_index].name.clone(),
                BattleEntityRef::Wild => battle_state.wild_pokemon.name.clone(),
                _ => {
                    panic!("Invalid target entity for move");
                }
            };
            
            // Apply status condition
            let status_applied = match actual_target {
                BattleEntityRef::Player { team_index } => {
                    let pokemon = &mut battle_state.player.team[team_index];
                    if pokemon.status.is_none() {
                        pokemon.status = Some(*status);
                        true
                    } else {
                        false
                    }
                },
                BattleEntityRef::Wild => {
                    let pokemon = &mut battle_state.wild_pokemon;
                    if pokemon.status.is_none() {
                        pokemon.status = Some(*status);
                        true
                    } else {
                        false
                    }
                },
                _ => {
                    panic!("Invalid target entity for move");
                }
            };
            
            if status_applied {
                let status_name = match status {
                    StatusCondition::Burn => "burned",
                    StatusCondition::Freeze => "frozen",
                    StatusCondition::Paralysis => "paralyzed",
                    StatusCondition::Poison => "poisoned",
                    StatusCondition::Sleep => "put to sleep",
                    StatusCondition::Toxic => "badly poisoned",
                };
                
                battle_events.push(BattleEvent::GenericMessage { 
                    message: format!("{} was {}!", target_name, status_name) 
                });
                
                battle_events.push(BattleEvent::StatusApplied {
                    target: actual_target,
                    status: *status,
                });
            } else {
                battle_events.push(BattleEvent::GenericMessage { 
                    message: format!("But it failed! {} already has a status condition.", target_name) 
                });
            }
        },
        crate::monsters::move_manager::EffectData::StatChange { changes, target: effect_target } => {
            let actual_target = match effect_target {
                crate::monsters::move_manager::EffectTarget::User => source.clone(),
                crate::monsters::move_manager::EffectTarget::Target => target.clone(),
            };
            
            // Get target Pokémon name
            let target_name = match actual_target {
                BattleEntityRef::Player { team_index } => battle_state.player.team[team_index].name.clone(),
                BattleEntityRef::Wild => battle_state.wild_pokemon.name.clone(),
                _ => {
                    panic!("Invalid target entity for move");
                }
            };
            
            // Apply stat changes
            for change in changes {
                let stat_name = match change.stat {
                    crate::monsters::move_manager::Stat::Attack => StatName::Attack,
                    crate::monsters::move_manager::Stat::Defense => StatName::Defense,
                    crate::monsters::move_manager::Stat::SpecialAttack => StatName::SpecialAttack,
                    crate::monsters::move_manager::Stat::SpecialDefense => StatName::SpecialDefense,
                    crate::monsters::move_manager::Stat::Speed => StatName::Speed,
                    crate::monsters::move_manager::Stat::Accuracy => StatName::Accuracy,
                    crate::monsters::move_manager::Stat::Evasion => StatName::Evasion,
                    _ => continue, // Skip HP stat changes
                };
                
                let stages = change.stages;
                let success = true; // Simplification - in reality there might be abilities that prevent stat changes
                
                let (new_stage, at_limit) = match actual_target {
                    BattleEntityRef::Player { team_index } => {
                        let pokemon = &mut battle_state.player.team[team_index];
                        let current = match stat_name {
                            StatName::Attack => pokemon.stat_modifiers.battle_stats.attack,
                            StatName::Defense => pokemon.stat_modifiers.battle_stats.defense,
                            StatName::SpecialAttack => pokemon.stat_modifiers.battle_stats.special_attack,
                            StatName::SpecialDefense => pokemon.stat_modifiers.battle_stats.special_defense,
                            StatName::Speed => pokemon.stat_modifiers.battle_stats.speed,
                            StatName::Accuracy => pokemon.stat_modifiers.accuracy,
                            StatName::Evasion => pokemon.stat_modifiers.evasion,
                        };
                        let new_stage = (current + stages).clamp(-6, 6);
                        let at_limit = new_stage == current;
                        
                        // Update the stat
                        match stat_name {
                            StatName::Attack => pokemon.stat_modifiers.battle_stats.attack = new_stage,
                            StatName::Defense => pokemon.stat_modifiers.battle_stats.defense = new_stage,
                            StatName::SpecialAttack => pokemon.stat_modifiers.battle_stats.special_attack = new_stage,
                            StatName::SpecialDefense => pokemon.stat_modifiers.battle_stats.special_defense = new_stage,
                            StatName::Speed => pokemon.stat_modifiers.battle_stats.speed = new_stage,
                            StatName::Accuracy => pokemon.stat_modifiers.accuracy = new_stage,
                            StatName::Evasion => pokemon.stat_modifiers.evasion = new_stage,
                        };
                        
                        (new_stage, at_limit)
                    },
                    BattleEntityRef::Wild => {
                        let pokemon = &mut battle_state.wild_pokemon;
                        let current = match stat_name {
                            StatName::Attack => pokemon.stat_modifiers.battle_stats.attack,
                            StatName::Defense => pokemon.stat_modifiers.battle_stats.defense,
                            StatName::SpecialAttack => pokemon.stat_modifiers.battle_stats.special_attack,
                            StatName::SpecialDefense => pokemon.stat_modifiers.battle_stats.special_defense,
                            StatName::Speed => pokemon.stat_modifiers.battle_stats.speed,
                            StatName::Accuracy => pokemon.stat_modifiers.accuracy,
                            StatName::Evasion => pokemon.stat_modifiers.evasion,
                        };
                        let new_stage = (current + stages).clamp(-6, 6);
                        let at_limit = new_stage == current;
                        
                        // Update the stat
                        match stat_name {
                            StatName::Attack => pokemon.stat_modifiers.battle_stats.attack = new_stage,
                            StatName::Defense => pokemon.stat_modifiers.battle_stats.defense = new_stage,
                            StatName::SpecialAttack => pokemon.stat_modifiers.battle_stats.special_attack = new_stage,
                            StatName::SpecialDefense => pokemon.stat_modifiers.battle_stats.special_defense = new_stage,
                            StatName::Speed => pokemon.stat_modifiers.battle_stats.speed = new_stage,
                            StatName::Accuracy => pokemon.stat_modifiers.accuracy = new_stage,
                            StatName::Evasion => pokemon.stat_modifiers.evasion = new_stage,
                        };
                        
                        (new_stage, at_limit)
                    },
                    _ => {
                        panic!("Invalid target entity for move");
                    }
                };
                
                if !at_limit {
                    let stat_name_str = match stat_name {
                        StatName::Attack => "Attack",
                        StatName::Defense => "Defense",
                        StatName::SpecialAttack => "Special Attack",
                        StatName::SpecialDefense => "Special Defense",
                        StatName::Speed => "Speed",
                        StatName::Accuracy => "Accuracy",
                        StatName::Evasion => "Evasion",
                    };
                    
                    let change_desc = if stages > 0 {
                        match stages {
                            1 => "rose",
                            2 => "rose sharply",
                            _ => "rose drastically",
                        }
                    } else {
                        match stages {
                            -1 => "fell",
                            -2 => "harshly fell",
                            _ => "severely fell",
                        }
                    };
                    
                    battle_events.push(BattleEvent::GenericMessage { 
                        message: format!("{}'s {} {}!", target_name, stat_name_str, change_desc) 
                    });
                    
                    battle_events.push(BattleEvent::StatChange {
                        target: actual_target.clone(),
                        stat: stat_name,
                        stages,
                        new_stage,
                        success,
                    });
                } else {
                    let direction = if stages > 0 { "higher" } else { "lower" };
                    battle_events.push(BattleEvent::GenericMessage { 
                        message: format!("{}'s stats won't go any {}!", target_name, direction) 
                    });
                }
            }
        },
        crate::monsters::move_manager::EffectData::Heal { .. } => {
            battle_events.push(BattleEvent::GenericMessage { 
                message: "Healing effect not fully implemented yet.".to_string() 
            });
        },
        _ => {
            battle_events.push(BattleEvent::GenericMessage { 
                message: "This move effect is not implemented yet.".to_string() 
            });
         }
     }
}

/// Apply damage with proper effectiveness and critical hit information
pub fn apply_damage_with_effectiveness(
    battle_state: &mut WildBattleState,
    battle_events: &mut Vec<BattleEvent>,
    target: BattleEntityRef,
    damage: u32,
    effectiveness: f32,
    is_critical: bool
) {
    match target {
        BattleEntityRef::Player { team_index } => {
            let pokemon = &mut battle_state.player.team[team_index];
            let new_hp = pokemon.current_hp.saturating_sub(damage);
            pokemon.current_hp = new_hp;
            battle_events.push(BattleEvent::DamageDealt { 
                target: target.clone(), 
                damage, 
                new_hp: pokemon.current_hp, 
                max_hp: pokemon.max_hp, 
                effectiveness, 
                is_critical 
            });
        }
        BattleEntityRef::Wild => {
            let pokemon = &mut battle_state.wild_pokemon;
            let new_hp = pokemon.current_hp.saturating_sub(damage);
            pokemon.current_hp = new_hp;
            battle_events.push(BattleEvent::DamageDealt { 
                target: target.clone(), 
                damage, 
                new_hp: pokemon.current_hp, 
                max_hp: pokemon.max_hp, 
                effectiveness, 
                is_critical 
            });
        },
        _ => {
            panic!("Invalid target entity for move");
        }
    }
} 