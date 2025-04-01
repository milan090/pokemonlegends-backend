use crate::combat::state::{
    BattleEntityRef, BattleEvent, BattlePokemon, BattlePokemonPublicView, BattlePvPPhase,
    PlayerAction, PvPBattleEndReason, PvPBattleState, PvPTurnOrder,
    StatusCondition,
};
use crate::monsters::monster_manager::MonsterTemplateRepository;
use crate::stats::StatName;
use rand::Rng;
use tracing::info;

use super::battle_calculations::calculate_damage;

/// Processes a single turn of a PvP battle
pub fn process_pvp_turn(battle_state: &mut PvPBattleState, monster_repository: &MonsterTemplateRepository) -> Vec<BattleEvent> {
    let mut battle_events = Vec::new();

    // --- 1. Pre-action checks (e.g., checking if Pokémon can move due to sleep/paralysis) ---
    // Add turn start event
    battle_events.push(BattleEvent::TurnStart {
        turn_number: battle_state.turn_number,
    });

    // --- 2. Determine Turn Order ---
    let player1_pokemon = &battle_state.player1.team[battle_state.player1.active_pokemon_index];
    let player2_pokemon = &battle_state.player2.team[battle_state.player2.active_pokemon_index];

    // Determine actions and their priorities
    let player1_action_type = battle_state
        .player1_action
        .as_ref()
        .map_or("unknown", |action| match action {
            PlayerAction::UseMove { .. } => "move",
            PlayerAction::SwitchPokemon { .. } => "switch",
            PlayerAction::UseItem { .. } => "item",
            PlayerAction::Run => "run",
        });

    let player2_action_type = battle_state
        .player2_action
        .as_ref()
        .map_or("unknown", |action| match action {
            PlayerAction::UseMove { .. } => "move",
            PlayerAction::SwitchPokemon { .. } => "switch",
            PlayerAction::UseItem { .. } => "item",
            PlayerAction::Run => "run",
        });

    // Handle priority rules:
    // 1. Items > 2. Switches > 3. Moves (by priority) > 4. Run
    // Within same category, use speed (or other modifiers)

    let turn_order = if player1_action_type == "item" && player2_action_type != "item" {
        PvPTurnOrder::Player1First
    } else if player2_action_type == "item" && player1_action_type != "item" {
        PvPTurnOrder::Player2First
    } else if player1_action_type == "switch"
        && player2_action_type != "switch"
        && player2_action_type != "item"
    {
        PvPTurnOrder::Player1First
    } else if player2_action_type == "switch"
        && player1_action_type != "switch"
        && player1_action_type != "item"
    {
        PvPTurnOrder::Player2First
    } else if player1_action_type == "move" && player2_action_type == "move" {
        // Determine based on move priority, then speed
        // For simplicity, just using speed for now
        if player1_pokemon.calculated_stats.speed > player2_pokemon.calculated_stats.speed {
            PvPTurnOrder::Player1First
        } else if player2_pokemon.calculated_stats.speed > player1_pokemon.calculated_stats.speed {
            PvPTurnOrder::Player2First
        } else {
            // Speed tie - random for now
            if rand::random::<bool>() {
                PvPTurnOrder::Player1First
            } else {
                PvPTurnOrder::Player2First
            }
        }
    } else if player1_action_type == "move" && player2_action_type == "run" {
        PvPTurnOrder::Player2First // Run always goes first
    } else if player2_action_type == "move" && player1_action_type == "run" {
        PvPTurnOrder::Player1First // Run always goes first
    } else {
        // Both using same category (both switching, both using items) or some unhandled case
        // Use speed as tiebreaker
        if player1_pokemon.calculated_stats.speed >= player2_pokemon.calculated_stats.speed {
            PvPTurnOrder::Player1First
        } else {
            PvPTurnOrder::Player2First
        }
    };

    battle_state.turn_order = Some(turn_order.clone());

    battle_events.push(BattleEvent::GenericMessage {
        message: format!("Turn {}: {:?}", battle_state.turn_number, turn_order),
    });

    // --- 3. Execute Actions ---
    match turn_order {
        PvPTurnOrder::Player1First => {
            if let Some(action) = &battle_state.player1_action {
                execute_pvp_action(
                    battle_state,
                    &mut battle_events,
                    BattleEntityRef::Player1 {
                        team_index: battle_state.player1.active_pokemon_index,
                    },
                    action.clone(),
                    true,
                );

                // Check if player2's Pokemon fainted
                check_pvp_faints(battle_state, &mut battle_events, monster_repository);

                // If battle is not over and player2's active Pokemon is not fainted, process player2's action
                if battle_state.battle_phase != BattlePvPPhase::Finished
                    && !battle_state.player2.team[battle_state.player2.active_pokemon_index]
                        .is_fainted
                {
                    if let Some(action) = &battle_state.player2_action {
                        execute_pvp_action(
                            battle_state,
                            &mut battle_events,
                            BattleEntityRef::Player2 {
                                team_index: battle_state.player2.active_pokemon_index,
                            },
                            action.clone(),
                            false,
                        );
                        check_pvp_faints(battle_state, &mut battle_events, monster_repository);
                    }
                }
            }
        }
        PvPTurnOrder::Player2First => {
            if let Some(action) = &battle_state.player2_action {
                execute_pvp_action(
                    battle_state,
                    &mut battle_events,
                    BattleEntityRef::Player2 {
                        team_index: battle_state.player2.active_pokemon_index,
                    },
                    action.clone(),
                    true,
                );

                // Check if player1's Pokemon fainted
                check_pvp_faints(battle_state, &mut battle_events, monster_repository);

                // If battle is not over and player1's active Pokemon is not fainted, process player1's action
                if battle_state.battle_phase != BattlePvPPhase::Finished
                    && !battle_state.player1.team[battle_state.player1.active_pokemon_index]
                        .is_fainted
                {
                    if let Some(action) = &battle_state.player1_action {
                        execute_pvp_action(
                            battle_state,
                            &mut battle_events,
                            BattleEntityRef::Player1 {
                                team_index: battle_state.player1.active_pokemon_index,
                            },
                            action.clone(),
                            false,
                        );
                        check_pvp_faints(battle_state, &mut battle_events, monster_repository);
                    }
                }
            }
        }
        _ => {
            // Handle other turn order types (simultaneous, complex ordering) if needed
            battle_events.push(BattleEvent::GenericMessage {
                message: "Complex turn order not fully implemented yet".to_string(),
            });
        }
    }

    // --- 4. End-of-Turn Effects ---
    apply_pvp_end_of_turn_effects(battle_state, &mut battle_events);
    check_pvp_faints(battle_state, &mut battle_events, monster_repository); // Check faints again after EOT effects

    // --- 5. Battle End Checks ---
    check_pvp_battle_end(battle_state);

    // --- 6. Prepare for next turn / state change ---
    if battle_state.battle_phase == BattlePvPPhase::ProcessingTurn {
        // Increment turn number if the battle continues
        battle_state.turn_number += 1;

        // Check player1 need to switch
        let player1_active_fainted =
            battle_state.player1.team[battle_state.player1.active_pokemon_index].is_fainted;
        let can_player1_switch = battle_state
            .player1
            .team
            .iter()
            .any(|p| !p.is_fainted && p.current_hp > 0);

        // Check player2 need to switch
        let player2_active_fainted =
            battle_state.player2.team[battle_state.player2.active_pokemon_index].is_fainted;
        let can_player2_switch = battle_state
            .player2
            .team
            .iter()
            .any(|p| !p.is_fainted && p.current_hp > 0);

        // Set appropriate battle phase based on who needs to switch
        if player1_active_fainted && player2_active_fainted {
            // Both need to switch
            if can_player1_switch && can_player2_switch {
                battle_state.battle_phase = BattlePvPPhase::WaitingForBothPlayersActions;
                battle_state.player1.must_switch = true;
                battle_state.player2.must_switch = true;
            } else if can_player1_switch {
                // Only player 1 can switch, player 2 lost
                battle_state.battle_phase = BattlePvPPhase::Finished;
            } else if can_player2_switch {
                // Only player 2 can switch, player 1 lost
                battle_state.battle_phase = BattlePvPPhase::Finished;
            } else {
                // Neither can switch - should be a draw but handled by check_pvp_battle_end
                battle_state.battle_phase = BattlePvPPhase::Finished;
            }
        } else if player1_active_fainted {
            // Only player 1 needs to switch
            if can_player1_switch {
                battle_state.battle_phase = BattlePvPPhase::WaitingForPlayer1Switch;
                battle_state.player1.must_switch = true;
            } else {
                // Player 1 lost
                battle_state.battle_phase = BattlePvPPhase::Finished;
            }
        } else if player2_active_fainted {
            // Only player 2 needs to switch
            if can_player2_switch {
                battle_state.battle_phase = BattlePvPPhase::WaitingForPlayer2Switch;
                battle_state.player2.must_switch = true;
            } else {
                // Player 2 lost
                battle_state.battle_phase = BattlePvPPhase::Finished;
            }
        } else {
            // Normal progression to next turn
            battle_state.battle_phase = BattlePvPPhase::WaitingForBothPlayersActions;
        }
    }

    // Clear actions for the next turn
    battle_state.player1_action = None;
    battle_state.player2_action = None;
    battle_state.turn_order = None;

    battle_events
}

/// Executes a single action for a player in a PvP battle
fn execute_pvp_action(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    source_entity: BattleEntityRef,
    action: PlayerAction,
    is_first_action: bool,
) {
    match action {
        PlayerAction::UseMove { move_index } => {
            execute_pvp_move(battle_state, battle_events, source_entity, move_index)
        }
        PlayerAction::SwitchPokemon { team_index } => {
            execute_pvp_switch(battle_state, battle_events, source_entity, team_index)
        }
        PlayerAction::UseItem {
            item_id,
            is_capture_item,
        } => execute_pvp_item(
            battle_state,
            battle_events,
            source_entity,
            item_id,
            is_capture_item,
        ),
        PlayerAction::Run => execute_pvp_surrender(battle_state, battle_events, source_entity),
    }
}
/// Execute a move in a PvP battle
fn execute_pvp_move(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    source: BattleEntityRef,
    move_index: usize,
) {
    // Get source info first before any mutable borrows
    let (source_name, move_data, source_level, source_stats, source_types) = match source {
        BattleEntityRef::Player1 { team_index } => {
            let pokemon = &battle_state.player1.team[team_index];
            (
                pokemon.name.clone(),
                pokemon.moves.get(move_index).cloned(),
                pokemon.level,
                pokemon.calculated_stats.clone(),
                pokemon.pokemon_types.clone(),
            )
        }
        BattleEntityRef::Player2 { team_index } => {
            let pokemon = &battle_state.player2.team[team_index];
            (
                pokemon.name.clone(),
                pokemon.moves.get(move_index).cloned(),
                pokemon.level,
                pokemon.calculated_stats.clone(),
                pokemon.pokemon_types.clone(),
            )
        }
        _ => {
            // Handle unexpected entity types (should never happen in PvP)
            battle_events.push(BattleEvent::GenericMessage {
                message: "Error: Invalid entity type in PvP move execution".to_string(),
            });
            return;
        }
    };

    // Get target info
    let (target, target_stats, target_types) = match source {
        BattleEntityRef::Player1 { .. } => {
            let target = BattleEntityRef::Player2 {
                team_index: battle_state.player2.active_pokemon_index,
            };
            let pokemon = &battle_state.player2.team[battle_state.player2.active_pokemon_index];
            (target, pokemon.calculated_stats.clone(), pokemon.pokemon_types.clone())
        }
        BattleEntityRef::Player2 { .. } => {
            let target = BattleEntityRef::Player1 {
                team_index: battle_state.player1.active_pokemon_index,
            };
            let pokemon = &battle_state.player1.team[battle_state.player1.active_pokemon_index];
            (target, pokemon.calculated_stats.clone(), pokemon.pokemon_types.clone())
        }
        _ => {
            // Handle unexpected entity types
            battle_events.push(BattleEvent::GenericMessage {
                message: "Error: Invalid entity type in PvP move execution".to_string(),
            });
            return;
        }
    };

    if let Some(move_data) = move_data {
        let move_id = move_data.move_id;
        
        if let Some(move_details) = battle_state
            .move_repository
            .as_ref()
            .and_then(|repo| repo.get_move(move_id))
        {
            let move_name = move_details.name.clone();

            // Add move used message
            battle_events.push(BattleEvent::GenericMessage {
                message: format!("{} used {}!", source_name, move_name),
            });

            // Decrement PP - now safe since we have no active borrows
            match source {
                BattleEntityRef::Player1 { team_index } => {
                    if let Some(mv) = battle_state.player1.team[team_index]
                        .moves
                        .get_mut(move_index)
                    {
                        if mv.current_pp > 0 {
                            mv.current_pp -= 1;
                        }
                    }
                }
                BattleEntityRef::Player2 { team_index } => {
                    if let Some(mv) = battle_state.player2.team[team_index]
                        .moves
                        .get_mut(move_index)
                    {
                        if mv.current_pp > 0 {
                            mv.current_pp -= 1;
                        }
                    }
                }
                _ => {} // Should not happen in PvP
            }

            // Calculate and apply damage
            let type_chart = battle_state.move_repository.as_ref().map(|repo| &repo.type_chart);
            
            let (damage, effectiveness, is_critical) = calculate_damage(
                source_level,
                &source_stats,
                &source_types,
                &target_stats,
                &target_types,
                &move_details,
                type_chart
            );

            apply_pvp_damage(battle_state, battle_events, target.clone(), damage, effectiveness, is_critical);

            // Record move used event
            battle_events.push(BattleEvent::MoveUsed {
                source: source.clone(),
                move_id,
                move_name,
                target: target.clone(), 
            });
        }
    }
}

/// Execute a switch in a PvP battle
fn execute_pvp_switch(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    source: BattleEntityRef,
    team_index: usize,
) {
    // Get the names of the Pokémon being switched
    let (outgoing_pokemon_name, incoming_pokemon_name, outgoing_index) = match source {
        BattleEntityRef::Player1 { .. } => {
            let out_name = battle_state.player1.team[battle_state.player1.active_pokemon_index]
                .name
                .clone();
            let in_name = battle_state.player1.team[team_index].name.clone();
            (out_name, in_name, battle_state.player1.active_pokemon_index)
        }
        BattleEntityRef::Player2 { .. } => {
            let out_name = battle_state.player2.team[battle_state.player2.active_pokemon_index]
                .name
                .clone();
            let in_name = battle_state.player2.team[team_index].name.clone();
            (out_name, in_name, battle_state.player2.active_pokemon_index)
        }
        _ => {
            // Handle unexpected entity types
            battle_events.push(BattleEvent::GenericMessage {
                message: "Error: Invalid entity type in PvP switch execution".to_string(),
            });
            return;
        }
    };

    // TODO: Implement proper switch logic (reset stats, entry hazards, abilities)

    // Update active Pokémon index
    match source {
        BattleEntityRef::Player1 { .. } => {
            battle_state.player1.active_pokemon_index = team_index;
            // Reset must_switch flag if it was set
            battle_state.player1.must_switch = false;
        }
        BattleEntityRef::Player2 { .. } => {
            battle_state.player2.active_pokemon_index = team_index;
            // Reset must_switch flag if it was set
            battle_state.player2.must_switch = false;
        }
        _ => {} // Should not happen
    }

    // Add a descriptive message
    battle_events.push(BattleEvent::GenericMessage {
        message: format!(
            "{} was withdrawn! {} was sent out!",
            outgoing_pokemon_name, incoming_pokemon_name
        ),
    });

    // Add SwitchIn event with public view
    match source {
        BattleEntityRef::Player1 { .. } => {
            let new_pokemon = &battle_state.player1.team[team_index];
            let view = BattlePokemonPublicView {
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
            battle_events.push(BattleEvent::SwitchIn {
                pokemon_view: view,
                team_index,
            });
        }
        BattleEntityRef::Player2 { .. } => {
            let new_pokemon = &battle_state.player2.team[team_index];
            let view = BattlePokemonPublicView {
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
            battle_events.push(BattleEvent::SwitchIn {
                pokemon_view: view,
                team_index,
            });
        }
        _ => {} // Should not happen
    }
}

/// Execute item use in a PvP battle
fn execute_pvp_item(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    source: BattleEntityRef,
    item_id: String,
    is_capture_item: bool,
) {
    // Capture items are not allowed in PvP
    if is_capture_item {
        battle_events.push(BattleEvent::GenericMessage {
            message: "Capture items cannot be used in PvP battles.".to_string(),
        });
        return;
    }

    // Determine player name and target
    let (player_name, target_entity, target_pokemon_index) = match source {
        BattleEntityRef::Player1 { team_index } => (
            battle_state.player1.name.clone(),
            BattleEntityRef::Player1 { team_index },
            team_index,
        ),
        BattleEntityRef::Player2 { team_index } => (
            battle_state.player2.name.clone(),
            BattleEntityRef::Player2 { team_index },
            team_index,
        ),
        _ => {
            // Handle unexpected entity types
            battle_events.push(BattleEvent::GenericMessage {
                message: "Error: Invalid entity type in PvP item execution".to_string(),
            });
            return;
        }
    };

    // Convert item_id to a readable name for display
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

    // Placeholder: just heal for 20 HP if it's a healing item
    if item_id.contains("potion") || item_id == "full_restore" {
        let heal_amount = match item_id.as_str() {
            "potion" => 20,
            "super_potion" => 50,
            "hyper_potion" => 100,
            "full_restore" => 999, // Full heal
            _ => 20,
        };

        // First, apply healing and get the necessary data
        let (actual_heal, new_hp, max_hp) = match source {
            BattleEntityRef::Player1 { team_index } => {
                let pokemon = &mut battle_state.player1.team[team_index];
                let old_hp = pokemon.current_hp;
                pokemon.current_hp = (pokemon.current_hp + heal_amount).min(pokemon.max_hp);
                let actual_heal = pokemon.current_hp - old_hp;
                (actual_heal, pokemon.current_hp, pokemon.max_hp)
            }
            BattleEntityRef::Player2 { team_index } => {
                let pokemon = &mut battle_state.player2.team[team_index];
                let old_hp = pokemon.current_hp;
                pokemon.current_hp = (pokemon.current_hp + heal_amount).min(pokemon.max_hp);
                let actual_heal = pokemon.current_hp - old_hp;
                (actual_heal, pokemon.current_hp, pokemon.max_hp)
            }
            _ => (0, 0, 0), // Should not happen
        };

        // Then push the event with the collected data
        if actual_heal > 0 {
            battle_events.push(BattleEvent::Heal {
                target: target_entity.clone(),
                amount: actual_heal,
                new_hp,
                max_hp,
            });
        }
    }

    battle_events.push(BattleEvent::GenericMessage {
        message: format!("{} used {}!", player_name, item_name),
    });

    battle_events.push(BattleEvent::ItemUsed {
        item_id: item_id.clone(),
        item_name: item_name.to_string(),
        target: Some(target_entity),
    });
}

/// Execute surrender in a PvP battle
fn execute_pvp_surrender(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    source: BattleEntityRef,
) {
    let (player_name, reason) = match source {
        BattleEntityRef::Player1 { .. } => (
            battle_state.player1.name.clone(),
            PvPBattleEndReason::Player2Victory,
        ), // P1 surrendered, P2 wins
        BattleEntityRef::Player2 { .. } => (
            battle_state.player2.name.clone(),
            PvPBattleEndReason::Player1Victory,
        ), // P2 surrendered, P1 wins
        _ => {
            // Handle unexpected entity types
            battle_events.push(BattleEvent::GenericMessage {
                message: "Error: Invalid entity type in PvP surrender execution".to_string(),
            });
            return;
        }
    };

    battle_events.push(BattleEvent::GenericMessage {
        message: format!("{} surrendered the battle!", player_name),
    });

    // Set battle end state
    battle_state.battle_phase = BattlePvPPhase::Finished;

    // Store the end reason (Optional: Add a field to PvPBattleState for this)
    // battle_state.end_reason = Some(reason);

    // Add a message indicating the winner based on surrender
    match reason {
        PvPBattleEndReason::Player1Victory => {
            battle_events.push(BattleEvent::GenericMessage {
                message: format!("Player 2 surrendered. Player 1 wins!"),
            });
        }
        PvPBattleEndReason::Player2Victory => {
            battle_events.push(BattleEvent::GenericMessage {
                message: format!("Player 1 surrendered. Player 2 wins!"),
            });
        }
        _ => {} // Other reasons handled elsewhere
    }
}
/// Apply damage in a PvP battle
fn apply_pvp_damage(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    target: BattleEntityRef,
    damage: u32,
    effectiveness: f32,
    is_critical: bool,
) {
    match target {
        BattleEntityRef::Player1 { team_index } => {
            let pokemon = &mut battle_state.player1.team[team_index];
            let new_hp = pokemon.current_hp.saturating_sub(damage);
            pokemon.current_hp = new_hp;

            // Store values before pushing event to avoid borrowing issues
            let current_hp = pokemon.current_hp;
            let max_hp = pokemon.max_hp;

            battle_events.push(BattleEvent::DamageDealt {
                target: target.clone(),
                damage,
                new_hp: current_hp,
                max_hp,
                effectiveness,
                is_critical,
            });
        }
        BattleEntityRef::Player2 { team_index } => {
            let pokemon = &mut battle_state.player2.team[team_index];
            let new_hp = pokemon.current_hp.saturating_sub(damage);
            pokemon.current_hp = new_hp;

            // Store values before pushing event to avoid borrowing issues
            let current_hp = pokemon.current_hp;
            let max_hp = pokemon.max_hp;

            battle_events.push(BattleEvent::DamageDealt {
                target: target.clone(),
                damage,
                new_hp: current_hp,
                max_hp,
                effectiveness,
                is_critical,
            });
        }
        _ => {
            // Handle unexpected entity types
            battle_events.push(BattleEvent::GenericMessage {
                message: "Error: Invalid entity type in damage application".to_string(),
            });
        }
    }
}

/// Apply end of turn effects in a PvP battle
fn apply_pvp_end_of_turn_effects(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
) {
    // TODO: Implement proper EOT logic (weather, status, volatile status, field effects)

    // Player 1's active Pokémon
    let player1_idx = battle_state.player1.active_pokemon_index;
    let player1_target = BattleEntityRef::Player1 {
        team_index: player1_idx,
    };

    // Check for burn damage
    if battle_state.player1.team[player1_idx].status == Some(StatusCondition::Burn) {
        let damage = battle_state.player1.team[player1_idx].max_hp / 16; // Simplified burn damage
        if damage > 0 {
            apply_pvp_damage(battle_state, battle_events, player1_target.clone(), damage, 1.0, false);

            battle_events.push(BattleEvent::StatusDamage {
                target: player1_target,
                status: StatusCondition::Burn,
                damage,
                new_hp: battle_state.player1.team[player1_idx].current_hp,
                max_hp: battle_state.player1.team[player1_idx].max_hp,
            });
        }
    }

    // Player 2's active Pokémon
    let player2_idx = battle_state.player2.active_pokemon_index;
    let player2_target = BattleEntityRef::Player2 {
        team_index: player2_idx,
    };

    // Check for burn damage
    if battle_state.player2.team[player2_idx].status == Some(StatusCondition::Burn) {
        let damage = battle_state.player2.team[player2_idx].max_hp / 16; // Simplified burn damage
        if damage > 0 {
            apply_pvp_damage(battle_state, battle_events, player2_target.clone(), damage, 1.0, false);

            battle_events.push(BattleEvent::StatusDamage {
                target: player2_target,
                status: StatusCondition::Burn,
                damage,
                new_hp: battle_state.player2.team[player2_idx].current_hp,
                max_hp: battle_state.player2.team[player2_idx].max_hp,
            });
        }
    }
}

/// Check for fainted Pokémon in a PvP battle
/// Returns a tuple indicating if a faint occurred and if the battle ended
fn check_pvp_faints(
    battle_state: &mut PvPBattleState,
    battle_events: &mut Vec<BattleEvent>,
    monster_repository: &MonsterTemplateRepository,
) -> (bool, bool) {
    let mut fainted = false;
    let mut battle_ended = false;

    // Check Player 1's active Pokémon
    let player1_idx = battle_state.player1.active_pokemon_index;
    if !battle_state.player1.team[player1_idx].is_fainted
        && battle_state.player1.team[player1_idx].current_hp == 0
    {
        battle_state.player1.team[player1_idx].is_fainted = true;
        battle_events.push(BattleEvent::PokemonFainted {
            target: BattleEntityRef::Player1 {
                team_index: player1_idx,
            },
        });

        // Calculate and award experience to player2's active Pokémon
        let player2_active_idx = battle_state.player2.active_pokemon_index;
        let fainted_pokemon = &battle_state.player1.team[player1_idx];
        let exp_gained = calculate_pvp_exp_gain(fainted_pokemon.base_exp, fainted_pokemon.level);

        battle_events.push(BattleEvent::GenericMessage {
            message: format!(
                "{} gained {} experience points!",
                battle_state.player2.team[player2_active_idx].name, exp_gained
            ),
        });
        
        // Level up check for player 2's active pokemon
        let levels_gained = level_up_battle_pokemon(&mut battle_state.player2.team[player2_active_idx], exp_gained, monster_repository);
        if levels_gained > 0 {
            battle_events.push(BattleEvent::GenericMessage {
                message: format!(
                    "{} grew to level {}!",
                    battle_state.player2.team[player2_active_idx].name,
                    battle_state.player2.team[player2_active_idx].level
                ),
            });
        }

        battle_events.push(BattleEvent::ExpGained {
            source: BattleEntityRef::Player1 {
                team_index: player1_idx,
            },
            amount: exp_gained,
        });

        fainted = true;

        // Check if player1 has any Pokémon left
        let player1_has_pokemon_left = battle_state.player1.team.iter().any(|p| !p.is_fainted);
        if !player1_has_pokemon_left {
            // Player 1 has no usable Pokémon left, player 2 wins
            battle_state.battle_phase = BattlePvPPhase::Finished;
            battle_events.push(BattleEvent::GenericMessage {
                message: format!(
                    "{} has no usable Pokémon left! {} wins the battle!",
                    battle_state.player1.name, battle_state.player2.name
                ),
            });
            battle_ended = true;
        }
    }

    // Check Player 2's active Pokémon
    let player2_idx = battle_state.player2.active_pokemon_index;
    if !battle_ended
        && !battle_state.player2.team[player2_idx].is_fainted
        && battle_state.player2.team[player2_idx].current_hp == 0
    {
        battle_state.player2.team[player2_idx].is_fainted = true;
        battle_events.push(BattleEvent::PokemonFainted {
            target: BattleEntityRef::Player2 {
                team_index: player2_idx,
            },
        });

        // Calculate and award experience to player1's active Pokémon
        let player1_active_idx = battle_state.player1.active_pokemon_index;
        let fainted_pokemon = &battle_state.player2.team[player2_idx];
        let exp_gained = calculate_pvp_exp_gain(fainted_pokemon.base_exp, fainted_pokemon.level);

        battle_events.push(BattleEvent::GenericMessage {
            message: format!(
                "{} gained {} experience points!",
                battle_state.player1.team[player1_active_idx].name, exp_gained
            ),
        });

        // Level up check for player 1's active pokemon
        let levels_gained = level_up_battle_pokemon(&mut battle_state.player1.team[player1_active_idx], exp_gained, monster_repository);
        if levels_gained > 0 {
            battle_events.push(BattleEvent::GenericMessage {
                message: format!(
                    "{} grew to level {}!",
                    battle_state.player1.team[player1_active_idx].name,
                    battle_state.player1.team[player1_active_idx].level
                ),
            });
        }
        
        battle_events.push(BattleEvent::ExpGained {
            source: BattleEntityRef::Player2 {
                team_index: player2_idx,
            },
            amount: exp_gained,
        });
        
        fainted = true;

        // Check if player2 has any Pokémon left
        let player2_has_pokemon_left = battle_state.player2.team.iter().any(|p| !p.is_fainted);
        if !player2_has_pokemon_left {
            // Player 2 has no usable Pokémon left, player 1 wins
            battle_state.battle_phase = BattlePvPPhase::Finished;
            battle_events.push(BattleEvent::GenericMessage {
                message: format!(
                    "{} has no usable Pokémon left! {} wins the battle!",
                    battle_state.player2.name, battle_state.player1.name
                ),
            });
            battle_ended = true;
        }
    }

    (fainted, battle_ended)
}

/// Level up a battle pokemon if it has gained enough experience
/// Returns true if the pokemon leveled up
fn level_up_battle_pokemon(pokemon: &mut BattlePokemon, exp_gained: u64, monster_repository: &MonsterTemplateRepository) -> u32 {
    let template = monster_repository.templates.get(&pokemon.template_id).unwrap();
    let mut levels_gained = 0;
    
    pokemon.exp += exp_gained;
    
    // Keep leveling up while we have enough exp and haven't hit max level
    while pokemon.exp >= pokemon.max_exp && pokemon.level < 100 {
        pokemon.level += 1;
        levels_gained += 1;
        
        // Recalculate stats for new level
        let old_max_hp = pokemon.max_hp;
        
        // Recalculate all stats using the existing stat calculation system
        pokemon.calculated_stats = crate::stats::calculate_stats(
            &template.base_stats,
            pokemon.level,
            &pokemon.ivs,
            &pokemon.evs,
            &pokemon.nature,
        );
        
        // Update max HP and heal the difference
        pokemon.max_hp = pokemon.calculated_stats.hp;
        let hp_gained = pokemon.max_hp - old_max_hp;
        pokemon.current_hp += hp_gained;
        
        // Update max exp for next level
        pokemon.max_exp = (pokemon.base_exp as f64 * 1.2_f64.powf(pokemon.level as f64)) as u64;
    }
    
    levels_gained
}

/// Calculate experience gained from defeating an opponent's Pokémon in PvP battles
fn calculate_pvp_exp_gain(fainted_pokemon_base_exp: u32, fainted_pokemon_level: u32) -> u64 {
    // Use canonical Pokémon exp formula: (a * b * L) / 7
    // where a is base exp, b is trainer battle multiplier (1.5), L is level
    info!("Calculating PvP exp gain for base exp: {}, level: {}", fainted_pokemon_base_exp, fainted_pokemon_level);
    
    let base_exp = fainted_pokemon_base_exp as f32;
    let level = fainted_pokemon_level as f32;
    
    let exp_gained = ((base_exp * 1.5 * level) / 7.0) as u64;
    
    // Add minimum bound to ensure meaningful exp gain
    exp_gained.max(50)
}

/// Check for battle end conditions in a PvP battle
fn check_pvp_battle_end(battle_state: &mut PvPBattleState)  {
    // Already handled: Surrender

    // Check if all Pokémon on either team have fainted
    let all_player1_fainted = battle_state.player1.team.iter().all(|p| p.is_fainted);
    let all_player2_fainted = battle_state.player2.team.iter().all(|p| p.is_fainted);

    if all_player1_fainted && all_player2_fainted {
        info!("Both teams fainted - it's a draw");
        // Both teams fainted - it's a draw
        battle_state.battle_phase = BattlePvPPhase::Finished;
    } else if all_player1_fainted {
        info!("Player 1 has lost");
        // Player 1 has lost
        battle_state.battle_phase = BattlePvPPhase::Finished;
    } else if all_player2_fainted {
        info!("Player 2 has lost");
        // Player 2 has lost
        battle_state.battle_phase = BattlePvPPhase::Finished;
    }
}
