use crate::combat::state::{WildBattleState, BattleEvent, BattlePhase, TurnOrder, PlayerAction, WildPokemonAction, BattleEntityRef, StatusCondition, BattlePokemonPublicView, BallType};
use crate::combat::logic::battle_calculations::calculate_damage;
use crate::combat::logic::battle_effects::{apply_effect, apply_damage_with_effectiveness};
use crate::combat::CaptureAttempt;
use rand::Rng;

/// Processes a single turn of the battle
pub fn process_turn(battle_state: &mut WildBattleState) -> Vec<BattleEvent> {
    let mut battle_events = Vec::new();

    // --- 1. Pre-action checks (e.g., checking if Pokémon can move due to sleep/paralysis) --- 
    // TODO: Implement pre-action checks

    // --- 2. Determine Turn Order --- 
    // Basic speed check for now
    let player_pokemon = &battle_state.player.team[battle_state.player.active_pokemon_index];
    let wild_pokemon = &battle_state.wild_pokemon;
    
    // TODO: Incorporate priority moves, Trick Room, items (Quick Claw), etc.
    let turn_order = if player_pokemon.calculated_stats.speed >= wild_pokemon.calculated_stats.speed {
        // TODO: Handle speed ties (random or other rule?)
        TurnOrder::PlayerFirst
    } else {
        TurnOrder::WildFirst
    };
    battle_state.turn_order = Some(turn_order.clone());
    
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("Turn {}: {:?} goes first.", battle_state.turn_number, turn_order)
    });

    // --- 3. Execute Actions --- 
    let first_action = match turn_order {
        TurnOrder::PlayerFirst => battle_state.player_action.clone(),
        TurnOrder::WildFirst => battle_state.wild_action.clone().map(|_| PlayerAction::Run), // Map wild action temporarily for execute_action signature
    };
    let second_action = match turn_order {
        TurnOrder::PlayerFirst => battle_state.wild_action.clone().map(|_| PlayerAction::Run), // Map wild action temporarily
        TurnOrder::WildFirst => battle_state.player_action.clone(),
    };
    
    let first_entity = match turn_order {
        TurnOrder::PlayerFirst => BattleEntityRef::Player { team_index: battle_state.player.active_pokemon_index },
        TurnOrder::WildFirst => BattleEntityRef::Wild,
    };
    let second_entity = match turn_order {
        TurnOrder::PlayerFirst => BattleEntityRef::Wild,
        TurnOrder::WildFirst => BattleEntityRef::Player { team_index: battle_state.player.active_pokemon_index },
    };

    if let Some(action) = first_action {
        execute_action(battle_state, &mut battle_events, first_entity, action, true);
        // Check for faints after first action
        if check_faints(battle_state, &mut battle_events) {
             // If someone fainted, the second action might not happen (depends on game rules/specific faint)
             // TODO: Refine this logic - second action might still happen in some cases
        } else if let Some(action) = second_action {
            execute_action(battle_state, &mut battle_events, second_entity, action, false);
            check_faints(battle_state, &mut battle_events);
        }
    }
    

    // --- 4. End-of-Turn Effects --- 
    apply_end_of_turn_effects(battle_state, &mut battle_events);
    check_faints(battle_state, &mut battle_events); // Check faints again after EOT effects

    // --- 5. Battle End Checks --- 
    check_battle_end(battle_state);

    // --- 6. Prepare for next turn / state change --- 
    if battle_state.battle_phase == BattlePhase::ProcessingTurn { // Only if not already ended/waiting switch
        // Increment turn number if the battle continues
        battle_state.turn_number += 1;
        
        // Check if player needs to switch due to faint
        let player_active_fainted = battle_state.player.team[battle_state.player.active_pokemon_index].is_fainted;
        let can_player_switch = battle_state.player.team.iter().any(|p| !p.is_fainted && p.current_hp > 0);

        if player_active_fainted && can_player_switch {
            battle_state.battle_phase = BattlePhase::WaitingForSwitch;
            battle_state.player.must_switch = true;
        } else if player_active_fainted && !can_player_switch {
            // Player's active fainted and no others left -> Battle End
            battle_state.battle_phase = BattlePhase::Finished;
            // Reason will be set in check_battle_end or handled when sending BattleEnd message
        } else {
            // Normal progression to next turn
            battle_state.battle_phase = BattlePhase::WaitingForPlayerAction;
        }
    }

    // Clear actions for the next turn
    battle_state.player_action = None;
    battle_state.wild_action = None;
    battle_state.turn_order = None;

    battle_events
}

/// Executes a single action (move, switch, item, run) for an entity
fn execute_action(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>,
    source_entity: BattleEntityRef,
    action: PlayerAction, // Using PlayerAction as a generic container for now
    is_first_action: bool,
) {
    match source_entity {
        BattleEntityRef::Player { .. } => {
            // Player Action
            match action {
                PlayerAction::UseMove { move_index } => execute_move(battle_state, battle_events, source_entity, move_index),
                PlayerAction::SwitchPokemon { team_index } => execute_switch(battle_state, battle_events, team_index),
                PlayerAction::UseItem { item_id, is_capture_item } => {
                    if is_capture_item {
                        execute_capture(battle_state, battle_events, item_id);
                    } else {
                        execute_item(battle_state, battle_events, item_id);
                    }
                },
                PlayerAction::Run => execute_run(battle_state, battle_events),
            }
        }
        BattleEntityRef::Wild => {
            // Wild Action - Currently mapped through PlayerAction::Run placeholder
            // We need the actual WildPokemonAction here
            let wild_action = battle_state.wild_action.clone().unwrap_or(WildPokemonAction::Struggle); // Default to struggle if somehow missing
            match wild_action {
                 WildPokemonAction::UseMove { move_index } => execute_move(battle_state, battle_events, source_entity, move_index),
                 WildPokemonAction::Struggle => execute_struggle(battle_state, battle_events, source_entity),
                 WildPokemonAction::Flee => execute_wild_flee(battle_state, battle_events),
            }
        }
        _ => {
            panic!("Invalid target entity for move");
        }
    }
}

/// Executes a move
fn execute_move(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>,
    source: BattleEntityRef,
    move_index: usize
) {
    // Get source and target Pokémon names
    let (source_name, move_data) = match source {
        BattleEntityRef::Player { team_index } => {
            let pokemon = &battle_state.player.team[team_index];
            let move_data = pokemon.moves.get(move_index).cloned();
            (pokemon.name.clone(), move_data)
        },
        BattleEntityRef::Wild => {
            let pokemon = &battle_state.wild_pokemon;
            let move_data = pokemon.moves.get(move_index).cloned();
            (pokemon.name.clone(), move_data)
        },
        _ => {
            panic!("Invalid target entity for move");
        }
    };
    
    // Get the move name from the move repository
    let (move_id, move_name) = if let Some(move_data) = move_data {
        let move_id = move_data.move_id;
        // Get move name from repository if available
        let move_name = battle_state.move_repository
            .as_ref()
            .and_then(|repo| repo.get_move(move_id))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| format!("Move {}", move_id));
        (move_id, move_name)
    } else {
        (0, "Unknown Move".to_string())
    };
    
    // Add a more descriptive message
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("{} used {}!", source_name, move_name) 
    });
    
    // Decrement PP
    match source {
        BattleEntityRef::Player { team_index } => {
            if let Some(mv) = battle_state.player.team[team_index].moves.get_mut(move_index) {
                if mv.current_pp > 0 {
                    mv.current_pp -= 1;
                }
            }
        },
        BattleEntityRef::Wild => {
             if let Some(mv) = battle_state.wild_pokemon.moves.get_mut(move_index) {
                 if mv.current_pp > 0 {
                    mv.current_pp -= 1;
                 }
             }
        },
        _ => {
            panic!("Invalid target entity for move");
        }
    }
    
    // Get target
    let target = match source {
        BattleEntityRef::Player { .. } => BattleEntityRef::Wild,
        BattleEntityRef::Wild => BattleEntityRef::Player { team_index: battle_state.player.active_pokemon_index },
        _ => {
            panic!("Invalid target entity for move");
        }
        };
    
    // Get detailed move data from move repository
    let move_details = battle_state.move_repository
        .as_ref()
        .and_then(|repo| repo.get_move(move_id));
    
    if let Some(move_details) = move_details {
        // Clone the effects needed before potentially modifying battle_state
        let primary_effect = move_details.effect.clone();
        let secondary_effect_data = move_details.secondary_effect.clone();

        // If it's a damage-dealing move
        if let Some(power) = move_details.power {
            // Calculate damage using proper formula
            let source_level = match source {
                BattleEntityRef::Player { team_index } => battle_state.player.team[team_index].level,
                BattleEntityRef::Wild { .. } => battle_state.wild_pokemon.level,
                _ => {
                    panic!("Invalid target entity for move");
                }
            };

            let (source_stats, source_types) = match source {
                BattleEntityRef::Player { team_index } => (&battle_state.player.team[team_index].calculated_stats, &battle_state.player.team[team_index].pokemon_types),
                BattleEntityRef::Wild { .. } => (&battle_state.wild_pokemon.calculated_stats, &battle_state.wild_pokemon.pokemon_types),
                _ => {
                    panic!("Invalid target entity for move");
                }
            };

            let (target_stats, target_types) = match target {
                BattleEntityRef::Player { team_index } => (&battle_state.player.team[team_index].calculated_stats, &battle_state.player.team[team_index].pokemon_types),
                BattleEntityRef::Wild { .. } => (&battle_state.wild_pokemon.calculated_stats, &battle_state.wild_pokemon.pokemon_types),
                _ => {
                    panic!("Invalid target entity for move");
                }
            };

            let type_chart = battle_state.move_repository.as_ref().map(|repo| &repo.type_chart);

            let (damage, effectiveness, is_critical) = calculate_damage(
                source_level,
                source_stats,
                source_types,
                target_stats, 
                target_types,
                &move_details,
                type_chart
            );
            // Apply the calculated damage
            if damage > 0 {
                apply_damage_with_effectiveness(
                    battle_state, 
                    battle_events, 
                    target.clone(), 
                    damage, 
                    effectiveness, 
                    is_critical
                );
                
                // Check for secondary effects using the cloned data
                if let Some(secondary) = secondary_effect_data {
                    let proc_chance = secondary.chance;
                    let roll = rand::thread_rng().gen_range(1..=100);
                    
                    if roll <= proc_chance {
                        // Apply secondary effect using the cloned effect
                        apply_effect(battle_state, battle_events, &secondary.effect, source.clone(), target.clone());
                    }
                }
            } else {
                // Damage was 0 (due to immunity or calculation result)
                 if effectiveness == 0.0 {
                     // Message already added above for immunity
                 } else {
                     // Generic fail message if damage was calculated as 0 but not due to immunity
                     battle_events.push(BattleEvent::GenericMessage { 
                        message: "But it failed!".to_string() 
                     });
                }
            }
        } else {
            // Status move - apply primary effect using the cloned effect
            apply_effect(battle_state, battle_events, &primary_effect, source.clone(), target.clone());
        }
    } else {
        // Fallback to simple damage if move details not found
        apply_damage(battle_state, battle_events, target.clone(), 10);
    }

    // Record the move used event with proper move details
    battle_events.push(BattleEvent::MoveUsed { 
        source, 
        move_id, 
        move_name, 
        target 
    });
}

/// Executes Struggle
fn execute_struggle(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>,
    source: BattleEntityRef
) {
    // Get source name for more meaningful message
    let source_name = match source {
        BattleEntityRef::Player { team_index } => battle_state.player.team[team_index].name.clone(),
        BattleEntityRef::Wild => battle_state.wild_pokemon.name.clone(),
        _ => {
            panic!("Invalid source entity for Struggle");
        }
    };
    
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("{} used Struggle!", source_name) 
    });
    
    let target = match source {
        BattleEntityRef::Player { .. } => BattleEntityRef::Wild,
        BattleEntityRef::Wild => BattleEntityRef::Player { team_index: battle_state.player.active_pokemon_index },
        _ => {
            panic!("Invalid source entity for Struggle");
        }
    };
    
    // Struggle is a typeless move with base power 50
    // Create a temporary MoveData for Struggle to use with damage calculation
    let struggle_move = crate::monsters::move_manager::MoveData {
        id: 165,
        name: "Struggle".to_string(),
        accuracy: Some(100),
        power: Some(50),
        pp: 1,
        priority: 0,
        move_type: crate::monsters::PokemonType::Normal, // Typeless in effect, but Normal for calculation
        damage_class: crate::monsters::move_manager::MoveCategory::Physical,
        target: crate::monsters::move_manager::TargetType::NormalOpponent,
        effect: crate::monsters::move_manager::EffectData::Damage { 
            multi_hit: None,
            crit_stage_bonus: None,
            drain_percent: None,
            recoil_damage_percent: Some(25), // 1/4 of damage dealt
        },
        secondary_effect: None,
        description: "Used only if all PP are gone. Hurts the user.".to_string(),
    };
    
    // Calculate damage using our formula
    let (source_level, source_stats, source_types) = match source {
        BattleEntityRef::Player { team_index } => {
            let pokemon = &battle_state.player.team[team_index];
            (pokemon.level, &pokemon.calculated_stats, &pokemon.pokemon_types)
        },
        BattleEntityRef::Wild { .. } => {
            let pokemon = &battle_state.wild_pokemon;
            (pokemon.level, &pokemon.calculated_stats, &pokemon.pokemon_types)
        }
        _ => {
            panic!("Invalid source entity for Struggle");
        }
    };

    let (target_stats, target_types) = match target {
        BattleEntityRef::Player { team_index } => {
            let pokemon = &battle_state.player.team[team_index];
            (&pokemon.calculated_stats, &pokemon.pokemon_types)
        },
        BattleEntityRef::Wild { .. } => {
            let pokemon = &battle_state.wild_pokemon;
            (&pokemon.calculated_stats, &pokemon.pokemon_types)
        }
        _ => {
            panic!("Invalid target entity for Struggle");
        }
    };

    let (damage, effectiveness, is_critical) = calculate_damage(
        source_level,
        source_stats,
        source_types,
        target_stats,
        target_types,
        &struggle_move,
        battle_state.move_repository.as_ref().map(|repo| &repo.type_chart) // Pass proper type chart from repository
    );
    
    // Apply the calculated damage
    if damage > 0 {
        apply_damage_with_effectiveness(
            battle_state,
            battle_events,
            target.clone(),
            damage,
            effectiveness,
            is_critical
        );
        
        // Add critical hit message
        if is_critical {
            battle_events.push(BattleEvent::GenericMessage { 
                message: "A critical hit!".to_string() 
            });
        }
        
        // Calculate and apply recoil damage (1/4 of damage dealt)
        let recoil_damage = damage / 4;
        if recoil_damage > 0 {
            battle_events.push(BattleEvent::GenericMessage { 
                message: format!("{} was damaged by recoil!", source_name) 
            });
            
            apply_damage_with_effectiveness(
                battle_state,
                battle_events,
                source.clone(),
                recoil_damage,
                1.0, // Recoil is always neutral effectiveness
                false // Recoil is never critical
            );
        }
    }
    
    // Get struggle move ID and proper move name
    let struggle_move_id = 165; // Standard ID for Struggle
    
    battle_events.push(BattleEvent::MoveUsed { 
        source, 
        move_id: struggle_move_id, 
        move_name: "Struggle".to_string(), 
        target 
    });
}

/// Executes a switch
fn execute_switch(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>,
    team_index: usize
) {
    // Get the names of the Pokémon being switched
    let outgoing_pokemon_name = battle_state.player.team[battle_state.player.active_pokemon_index].name.clone();
    let incoming_pokemon_name = battle_state.player.team[team_index].name.clone();
    
    // TODO: Implement switch logic
    // 1. Reset volatile statuses, stat stages for the outgoing Pokémon
    // 2. Update active_pokemon_index
    // 3. Apply entry hazards (Stealth Rock, Spikes) to incoming Pokémon
    // 4. Trigger abilities on switch-in (Intimidate)
    // 5. Add BattleEvents (SwitchOut, SwitchIn)
    battle_state.player.active_pokemon_index = team_index;
    
    // Add a descriptive message
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("{} was withdrawn! {} was sent out!", outgoing_pokemon_name, incoming_pokemon_name) 
    });
    
    // Add SwitchIn event with public view
    let new_pokemon = &battle_state.player.team[team_index];
    let view = BattlePokemonPublicView { // Create public view
         template_id: new_pokemon.template_id,
         name: new_pokemon.name.clone(),
         level: new_pokemon.level,
         current_hp_percent: new_pokemon.current_hp as f32 / new_pokemon.max_hp as f32,
         max_hp: new_pokemon.max_hp,
         types: new_pokemon.pokemon_types.clone(),
         status: new_pokemon.status.clone(),
         stat_modifiers: new_pokemon.stat_modifiers.clone(),
         is_fainted: new_pokemon.is_fainted,
         is_wild: false,
    };
    battle_events.push(BattleEvent::SwitchIn { pokemon_view: view, team_index });
}

/// Executes item use
fn execute_item(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>,
    item_id: String
) {
    // TODO: Implement item logic (healing, status cure, X-items)
    // 1. Check item validity
    // 2. Apply item effect
    // 3. Add BattleEvents (ItemUsed, Heal, StatusRemoved)
    
    // Get player name and active Pokémon name for better messages
    let player_name = battle_state.player.name.clone();
    let active_pokemon_name = battle_state.player.team[battle_state.player.active_pokemon_index].name.clone();
    
    // Convert item_id to a readable name for display
    // In a full implementation, this would come from an item repository
    let item_name = match item_id.as_str() {
        "potion" => "Potion",
        "super_potion" => "Super Potion",
        "hyper_potion" => "Hyper Potion",
        "full_restore" => "Full Restore",
        "antidote" => "Antidote",
        "awakening" => "Awakening",
        "paralyze_heal" => "Paralyze Heal",
        "burn_heal" => "Burn Heal",
        "ice_heal" => "Ice Heal",
        "full_heal" => "Full Heal",
        "x_attack" => "X Attack",
        "x_defense" => "X Defense",
        "x_speed" => "X Speed",
        "x_special" => "X Special",
        _ => &item_id,
    };
    
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("{} used {} on {}!", player_name, item_name, active_pokemon_name) 
    });
    
    // Add ItemUsed event
    let target = BattleEntityRef::Player { team_index: battle_state.player.active_pokemon_index };
    battle_events.push(BattleEvent::ItemUsed { 
        item_id: item_id.clone(), 
        item_name: item_name.to_string(), 
        target: Some(target) 
    });
}

/// Executes capture attempt
fn execute_capture(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>,
    ball_id: String
) {
    // TODO: Implement capture logic
    // 1. Check if target is wild
    // 2. Calculate capture chance (HP, status, ball bonus, catch rate)
    // 3. Simulate shakes
    // 4. Determine success
    // 5. Add BattleEvents (CaptureAttempt)
    // 6. If successful, set battle phase to Finished
    
    // Get player name and wild Pokémon name for better messages
    let player_name = battle_state.player.name.clone();
    let wild_pokemon_name = battle_state.wild_pokemon.name.clone();
    
    // Convert ball_id to a ball type and readable name
    let (ball_type, ball_name) = match ball_id.as_str() {
        "poke_ball" => (BallType::PokeBall, "Poké Ball"),
        "great_ball" => (BallType::GreatBall, "Great Ball"),
        "ultra_ball" => (BallType::UltraBall, "Ultra Ball"),
        _ => (BallType::PokeBall, "Poké Ball"),
    };
    
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("{} threw a {} at the wild {}!", player_name, ball_name, wild_pokemon_name) 
    });
    
    // Calculate success chance based on HP percentage
    let hp_percentage = battle_state.wild_pokemon.current_hp as f64 / battle_state.wild_pokemon.max_hp as f64;
    let base_chance = 0.3; // 30% base chance
    let hp_bonus = 0.4 * (1.0 - hp_percentage); // Up to 40% bonus for low HP
    let success = rand::thread_rng().gen_bool(base_chance + hp_bonus);
    
    let shakes = if success { 3 } else { rand::thread_rng().gen_range(0..=2) };
    
    let capture_event = BattleEvent::CaptureAttempt { ball_type: ball_type.clone(), shake_count: shakes, success };
    battle_events.push(capture_event.clone());
    battle_state.capture_attempts.push(CaptureAttempt { ball_type: ball_type.clone(), shake_count: shakes, success, turn_number: battle_state.turn_number });
    
    // Add result message
    if success {
        battle_events.push(BattleEvent::GenericMessage { 
            message: format!("Gotcha! {} was caught!", wild_pokemon_name) 
        });
        battle_state.battle_phase = BattlePhase::Finished;
       
        
        
    } else {
        let shake_message = match shakes {
            0 => format!("The {} broke free immediately!", wild_pokemon_name),
            1 => format!("The {} broke free after 1 shake!", wild_pokemon_name),
            2 => format!("So close! The {} almost got caught!", wild_pokemon_name),
            _ => format!("The {} broke free!", wild_pokemon_name),
        };
        battle_events.push(BattleEvent::GenericMessage { message: shake_message });
    }
}

/// Executes run attempt
fn execute_run(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>
) {
    // TODO: Implement run logic
    // 1. Compare speeds, level difference, turn number
    // 2. Check for trapping moves/abilities
    // 3. Determine success
    // 4. Add BattleEvents (PlayerRanAway)
    // 5. If successful, set battle phase to Finished

    // Get player name and wild Pokémon name for better messages
    let player_name = battle_state.player.name.clone();
    let wild_pokemon_name = battle_state.wild_pokemon.name.clone();

    let success = true; // Placeholder: Always successful for now
    
    // Add descriptive message
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("{} fled from the wild {}!", player_name, wild_pokemon_name) 
    });
    
    battle_events.push(BattleEvent::PlayerRanAway { success });
    if success {
        battle_state.battle_phase = BattlePhase::Finished;
    }
}

/// Executes wild flee attempt
fn execute_wild_flee(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>
) {
    // TODO: Implement wild flee logic (similar to player run but for wild)
    
    // Get wild Pokémon name for better messages
    let wild_pokemon_name = battle_state.wild_pokemon.name.clone();
    
    let success = rand::thread_rng().gen_bool(0.1); // Placeholder 10% chance
    
    // Add descriptive message
    battle_events.push(BattleEvent::GenericMessage { 
        message: format!("The wild {} fled!", wild_pokemon_name) 
    });
    
     battle_events.push(BattleEvent::WildPokemonFled);
    if success {
        battle_state.battle_phase = BattlePhase::Finished;
    }
}

/// Applies end-of-turn effects
fn apply_end_of_turn_effects(
    battle_state: &mut WildBattleState, 
    battle_events: &mut Vec<BattleEvent>
) {
    // TODO: Implement EOT logic
    // 1. Weather damage/effects (Rain, Sun, Sand, Hail)
    // 2. Status damage (Burn, Poison, Badly Poisoned)
    // 3. Volatile status effects (Leech Seed drain, Bind damage, Confusion check/damage)
    // 4. Field effect timer decrement (Reflect, Light Screen, Tailwind, Trick Room)
    // 5. Status timer decrement (Sleep)
    // 6. Add BattleEvents

    // Example: Burn damage
    let player_index = battle_state.player.active_pokemon_index;
    let player_target = BattleEntityRef::Player { team_index: player_index };
    let mut player_took_burn_damage = false;
    let mut player_burn_damage_amount = 0;

    // Check condition without holding the borrow for too long
    if battle_state.player.team[player_index].status == Some(StatusCondition::Burn) {
        let damage = battle_state.player.team[player_index].max_hp / 16; // Simplified burn damage
        if damage > 0 { // Only apply if damage is non-zero
            apply_damage(battle_state, battle_events, player_target.clone(), damage);
            player_took_burn_damage = true;
            player_burn_damage_amount = damage;
        }
    }

    // Push event *after* apply_damage call
    if player_took_burn_damage {
        // Re-borrow immutably to get updated values
        let player_pokemon = &battle_state.player.team[player_index];
        battle_events.push(BattleEvent::StatusDamage { 
            target: player_target, 
            status: StatusCondition::Burn, 
            damage: player_burn_damage_amount, 
            new_hp: player_pokemon.current_hp, 
            max_hp: player_pokemon.max_hp 
        });
    }
    
    let wild_target = BattleEntityRef::Wild;
    let mut wild_took_burn_damage = false;
    let mut wild_burn_damage_amount = 0;

    // Check condition without holding the borrow for too long
    if battle_state.wild_pokemon.status == Some(StatusCondition::Burn) {
         let damage = battle_state.wild_pokemon.max_hp / 16; // Simplified burn damage
         if damage > 0 {
            apply_damage(battle_state, battle_events, wild_target.clone(), damage);
            wild_took_burn_damage = true;
            wild_burn_damage_amount = damage;
         }
    }
    
    // Push event *after* apply_damage call
    if wild_took_burn_damage {
        // Re-borrow immutably to get updated values
        let wild_pokemon = &battle_state.wild_pokemon;
        battle_events.push(BattleEvent::StatusDamage { 
            target: wild_target, 
            status: StatusCondition::Burn, 
            damage: wild_burn_damage_amount, 
            new_hp: wild_pokemon.current_hp, 
            max_hp: wild_pokemon.max_hp 
        });
    }
}

/// Checks for faints
/// Returns true if any active Pokemon fainted this check
fn check_faints(battle_state: &mut WildBattleState, battle_events: &mut Vec<BattleEvent>) -> bool {
    let mut fainted = false;
    let player_index = battle_state.player.active_pokemon_index;
    
    // Check Player Pokemon
    if !battle_state.player.team[player_index].is_fainted && battle_state.player.team[player_index].current_hp == 0 {
        battle_state.player.team[player_index].is_fainted = true;
        battle_events.push(BattleEvent::PokemonFainted { target: BattleEntityRef::Player { team_index: player_index } });
        fainted = true;
    }
    
    // Check Wild Pokemon
    if !battle_state.wild_pokemon.is_fainted && battle_state.wild_pokemon.current_hp == 0 {
        battle_state.wild_pokemon.is_fainted = true;
        battle_events.push(BattleEvent::PokemonFainted { target: BattleEntityRef::Wild });
        // Wild fainted -> Battle End
        battle_state.battle_phase = BattlePhase::Finished;
        fainted = true;
    }
    fainted
}

/// Checks other battle end conditions
fn check_battle_end(battle_state: &mut WildBattleState) {
    // Already checked: Wild Fainted, Player Ran, Wild Fled, Capture Success
    // Check if all player Pokemon fainted
    let all_player_fainted = battle_state.player.team.iter().all(|p| p.is_fainted);
    if all_player_fainted {
        battle_state.battle_phase = BattlePhase::Finished;
    }
}

/// Helper to apply damage and update HP (without effectiveness info)
fn apply_damage(battle_state: &mut WildBattleState, battle_events: &mut Vec<BattleEvent>, target: BattleEntityRef, damage: u32) {
     match target {
         BattleEntityRef::Player { team_index } => {
            let pokemon = &mut battle_state.player.team[team_index];
            let new_hp = pokemon.current_hp.saturating_sub(damage);
            pokemon.current_hp = new_hp;
            // Push a simplified damage event if needed, or rely on the caller (like EOT effects) to push specific events
            // For now, adding a placeholder event for consistency
            battle_events.push(BattleEvent::DamageDealt { 
                target: target.clone(), 
                damage, 
                new_hp: pokemon.current_hp, 
                max_hp: pokemon.max_hp, 
                effectiveness: 1.0, // Placeholder - This function doesn't calculate effectiveness
                is_critical: false // Placeholder
            });
         },
         BattleEntityRef::Wild => {
            let pokemon = &mut battle_state.wild_pokemon;
            let new_hp = pokemon.current_hp.saturating_sub(damage);
            pokemon.current_hp = new_hp;
             battle_events.push(BattleEvent::DamageDealt { 
                target: target.clone(), 
                damage, 
                new_hp: pokemon.current_hp, 
                max_hp: pokemon.max_hp, 
                effectiveness: 1.0, // Placeholder
                is_critical: false // Placeholder
            });
         },
         _ => {}
     }
} 