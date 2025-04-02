use crate::combat::state::{WildBattleState, PvPBattleState, BattlePlayer, BattlePokemon, BattlePhase, BattlePvPPhase, PlayerSideState, FieldState, BattlePokemonTeamOverview, BattlePokemonPrivateView, BattlePokemonPublicView, PlayerAction, WildBattleOutcome, BattleEndReason, SwitchReason, PvPBattleOutcome};
use crate::combat::{utils, BattleEvent};
use crate::game_loop::pokemon_collection::{Pokemon, PokemonCollectionManager, PokemonUpdate};
use crate::lobby::Lobby;
use crate::models::{DisplayPokemon, ServerMessage};
use crate::monsters::monster::MonsterMove;
use crate::monsters::monster_manager::MonsterTemplateRepository;
use crate::combat::logic;

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use tracing::{info, error, warn};
use rand;

/// Manages active battle instances
pub struct BattleManager {
    // Maps battle ID to battle state
    active_battles: DashMap<Uuid, Arc<Mutex<WildBattleState>>>,
    active_pvp_battles: DashMap<Uuid, Arc<Mutex<PvPBattleState>>>, // New map for PvP battles
    template_repository: Arc<MonsterTemplateRepository>,
}

impl BattleManager {
    /// Create a new BattleManager
    pub fn new(template_repository: Arc<MonsterTemplateRepository>) -> Self {
        BattleManager {
            active_battles: DashMap::new(),
            active_pvp_battles: DashMap::new(),
            template_repository,
        }
    }

    /// Start a PvP battle between two players
    pub async fn start_pvp_battle(
        &self,
        player1_id: &str,
        player2_id: &str,
        lobby: &Arc<Lobby>,
        pokemon_collection_manager: &Arc<PokemonCollectionManager>,
    ) -> Result<Uuid, String> {
        // Generate a new battle ID
        let battle_id = Uuid::new_v4();
        info!("Starting PvP battle {}: player {} vs player {}", battle_id, player1_id, player2_id);
        
        // 1. Get player usernames for messaging
        let player1_username = match lobby.player_positions.get(player1_id) {
            Some(state) => state.value().username.clone(),
            None => return Err(format!("Player {} not found in lobby", player1_id)),
        };
        
        let player2_username = match lobby.player_positions.get(player2_id) {
            Some(state) => state.value().username.clone(),
            None => return Err(format!("Player {} not found in lobby", player2_id)),
        };
        
        // 2. Fetch both players' active Pokémon
        let player1_pokemons = match pokemon_collection_manager.get_active_pokemons(player1_id).await {
            Ok(pokemons) => {
                if pokemons.is_empty() {
                    return Err(format!("Player {} has no active Pokémon", player1_id));
                }
                pokemons
            },
            Err(e) => return Err(format!("Failed to fetch Pokémon for player {}: {}", player1_id, e)),
        };
        
        let player2_pokemons = match pokemon_collection_manager.get_active_pokemons(player2_id).await {
            Ok(pokemons) => {
                if pokemons.is_empty() {
                    return Err(format!("Player {} has no active Pokémon", player2_id));
                }
                pokemons
            },
            Err(e) => return Err(format!("Failed to fetch Pokémon for player {}: {}", player2_id, e)),
        };
        
        // 3. Convert Pokémon to battle format
        let battle_pokemon1 = player1_pokemons.iter().enumerate()
            .map(|(idx, pokemon)| {
                utils::convert_player_pokemon_to_battle_pokemon(pokemon, idx, &self.template_repository)
            })
            .collect::<Vec<_>>();
        
        let battle_pokemon2 = player2_pokemons.iter().enumerate()
            .map(|(idx, pokemon)| {
                utils::convert_player_pokemon_to_battle_pokemon(pokemon, idx, &self.template_repository)
            })
            .collect::<Vec<_>>();
        
        // 4. Create BattlePlayer structs for both players
        let battle_player1 = BattlePlayer {
            player_id: player1_id.to_string(),
            name: player1_username.clone(),
            team: battle_pokemon1,
            active_pokemon_index: 0, // Start with first Pokémon
            side_effects: PlayerSideState::default(),
            last_action_submitted: None,
            must_switch: false,
        };
        
        let battle_player2 = BattlePlayer {
            player_id: player2_id.to_string(),
            name: player2_username.clone(),
            team: battle_pokemon2,
            active_pokemon_index: 0, // Start with first Pokémon
            side_effects: PlayerSideState::default(),
            last_action_submitted: None,
            must_switch: false,
        };
        
        // 5. Create the PvP battle state
        let pvp_battle_state = PvPBattleState::new(
            battle_id,
            battle_player1,
            battle_player2,
            self.template_repository.move_repository.clone(),
        );
        
        // 6. Store the battle in the manager
        let battle_mutex = Arc::new(Mutex::new(pvp_battle_state));
        self.active_pvp_battles.insert(battle_id, battle_mutex.clone());
        
        // Mark both players as in combat
        if let Some(mut player1_state) = lobby.player_positions.get_mut(player1_id) {
            player1_state.value_mut().in_combat = true;
        } else {
            return Err(format!("Player {} not found in lobby state", player1_id));
        }
        
        if let Some(mut player2_state) = lobby.player_positions.get_mut(player2_id) {
            player2_state.value_mut().in_combat = true;
        } else {
            return Err(format!("Player {} not found in lobby state", player2_id));
        }
        
        // 7. Send initial battle messages to both players
        
        // 7.1 Create team overviews for client
        let battle_state = battle_mutex.lock().await;
        
        let team1_overview = battle_state.player1.team.iter()
            .map(|pokemon| BattlePokemonTeamOverview::from_battle_pokemon(pokemon))
            .collect::<Vec<_>>();
        
        let team2_overview = battle_state.player2.team.iter()
            .map(|pokemon| BattlePokemonTeamOverview::from_battle_pokemon(pokemon))
            .collect::<Vec<_>>();
        
        // 7.2 Active Pokémon details
        let active_pokemon1 = &battle_state.player1.team[battle_state.player1.active_pokemon_index];
        let active_pokemon1_private_view = BattlePokemonPrivateView::from_battle_pokemon(
            active_pokemon1,
            self.template_repository.move_repository.as_ref()
        );
        
        let active_pokemon2 = &battle_state.player2.team[battle_state.player2.active_pokemon_index];
        let active_pokemon2_private_view = BattlePokemonPrivateView::from_battle_pokemon(
            active_pokemon2,
            self.template_repository.move_repository.as_ref()
        );
        
        // 7.3 Create public views for opponent's Pokémon
        let active_pokemon1_public_view = BattlePokemonPublicView::from_battle_pokemon(active_pokemon1);
        let active_pokemon2_public_view = BattlePokemonPublicView::from_battle_pokemon(active_pokemon2);
        
        // 7.4 Create field state
        let field_state = FieldState::default();
                
        // Release battle state lock
        drop(battle_state);
        
        // 7.5 Send battle start messages to both players
        // For player 1
        let pvp_start_message1 = crate::models::ServerMessage::PvPBattleStart {
            battle_id,
            player_team: team1_overview.clone(),
            initial_pokemon: active_pokemon1_private_view.clone(),
            opponent_id: player2_id.to_string(),
            opponent_username: player2_username.clone(),
            opponent_initial_pokemon: active_pokemon2_public_view.clone(),
            initial_field_state: field_state.clone(),
            player1_id: player1_id.to_string(),
            player2_id: player2_id.to_string(),
        };
        
        if let Err(e) = lobby.send_to_player(player1_id, &pvp_start_message1).await {
            error!("Failed to send PvP battle start message to player {}: {}", player1_id, e);
            // Revert the in_combat status for both players
            if let Some(mut player1_state) = lobby.player_positions.get_mut(player1_id) {
                player1_state.value_mut().in_combat = false;
            }
            if let Some(mut player2_state) = lobby.player_positions.get_mut(player2_id) {
                player2_state.value_mut().in_combat = false;
            }
            // Remove the battle from the active_pvp_battles map
            self.active_pvp_battles.remove(&battle_id);
            return Err(format!("Failed to send battle start message: {}", e));
        }
        
        // For player 2
        let pvp_start_message2 = crate::models::ServerMessage::PvPBattleStart {
            battle_id,
            player_team: team2_overview.clone(),
            initial_pokemon: active_pokemon2_private_view.clone(),
            opponent_id: player1_id.to_string(),
            opponent_username: player1_username.clone(),
            opponent_initial_pokemon: active_pokemon1_public_view.clone(),
            initial_field_state: field_state.clone(),
            player1_id: player1_id.to_string(),
            player2_id: player2_id.to_string(),
        };
        
        if let Err(e) = lobby.send_to_player(player2_id, &pvp_start_message2).await {
            error!("Failed to send PvP battle start message to player {}: {}", player2_id, e);
            // Revert the in_combat status for both players
            if let Some(mut player1_state) = lobby.player_positions.get_mut(player1_id) {
                player1_state.value_mut().in_combat = false;
            }
            if let Some(mut player2_state) = lobby.player_positions.get_mut(player2_id) {
                player2_state.value_mut().in_combat = false;
            }
            // Remove the battle from the active_pvp_battles map
            self.active_pvp_battles.remove(&battle_id);
            return Err(format!("Failed to send battle start message: {}", e));
        }
        
        info!("Successfully started PvP battle {} between players {} and {}", 
              battle_id, player1_id, player2_id);
        
        // 8. Send RequestAction to both players
        // This will be implemented in the next phase
        
        // For now, just updating the battle state to indicate waiting for both players
        let mut battle_state = battle_mutex.lock().await;
        battle_state.battle_phase = BattlePvPPhase::WaitingForBothPlayersActions;
        drop(battle_state);
        
        Ok(battle_id)
    }

    /// Start a wild battle between a player and a monster
    pub async fn start_wild_battle(
        &self,
        player_id: &str,
        monster_instance_id: &str,
        lobby: &Arc<Lobby>,
        pokemon_collection_manager: &Arc<PokemonCollectionManager>,
    ) -> Result<Uuid, String> {
        // Generate a new battle ID
        let battle_id = Uuid::new_v4();
        info!("Starting wild battle {}: player {} vs monster {}", battle_id, player_id, monster_instance_id);
        
        // 1. Fetch the player's active Pokémon
        let player_pokemons: Vec<crate::game_loop::pokemon_collection::Pokemon> = match pokemon_collection_manager.get_active_pokemons(player_id).await {
            Ok(pokemons) => {
                if pokemons.is_empty() {
                    return Err("Player has no active Pokémon".to_string());
                }
                pokemons
            }
            Err(e) => return Err(format!("Failed to fetch player's Pokémon: {}", e)),
        };
        
        // 2. Fetch the wild monster
        let monster_entry = match lobby.active_monsters.get(monster_instance_id) {
            Some(entry) => entry,
            None => return Err(format!("Monster {} not found in lobby", monster_instance_id)),
        };
        
        let monster = match monster_entry.value().try_lock() {
            Ok(monster) => monster.clone(),
            Err(_) => return Err("Failed to acquire lock on monster".to_string()),
        };
        if monster.in_combat {
          return Err("Monster is already in combat".to_string());
        }
        
        // 3. Convert Pokémon to battle format
        let battle_pokemon = player_pokemons.iter().enumerate()
            .map(|(idx, pokemon)| {
                utils::convert_player_pokemon_to_battle_pokemon(pokemon, idx, &self.template_repository)
            })
            .collect::<Vec<_>>();
        
        let wild_pokemon = utils::convert_wild_monster_to_battle_pokemon(&monster, &self.template_repository);
        
        // 4. Create BattlePlayer and initial battle state
        let battle_player = BattlePlayer {
            player_id: player_id.to_string(),
            name: "Player".to_string(), // TODO: Get actual player name
            team: battle_pokemon,
            active_pokemon_index: 0, // Start with first Pokémon
            side_effects: PlayerSideState::default(),
            last_action_submitted: None,
            must_switch: false,
        };
        
        // 5. Create the battle state
        let battle_state = WildBattleState {
            battle_id,
            player: battle_player,
            wild_pokemon,
            turn_number: 1,
            battle_phase: BattlePhase::WaitingForPlayerAction,
            player_action: None,
            wild_action: None,
            turn_order: None,
            field_state: FieldState::default(),
            battle_log: Vec::new(),
            capture_attempts: Vec::new(),
            move_repository: self.template_repository.move_repository.clone(),
        };
        
        // 6. Store the battle in the manager
        let battle_mutex = Arc::new(Mutex::new(battle_state));
        self.active_battles.insert(battle_id, battle_mutex.clone());
        
        // 7. Mark player and monster as in combat
        if let Some(mut player_state) = lobby.player_positions.get_mut(player_id) {
            player_state.value_mut().in_combat = true;
        } else {
            warn!("Player {} not found in lobby state", player_id);
        }
        
        if let Some(monster_ref) = lobby.active_monsters.get(monster_instance_id) {
            if let Ok(mut monster_lock) = monster_ref.value().try_lock() {
                monster_lock.in_combat = true;
            } else {
                warn!("Failed to mark monster {} as in combat", monster_instance_id);
            }
        }
        
        // 8. Send initial battle messages to player
        let battle_state_for_messages = battle_mutex.lock().await;
        
        // 8.1 Create team overview for client
        let team_overview = battle_state_for_messages.player.team.iter()
            .map(|pokemon| BattlePokemonTeamOverview {
                template_id: pokemon.template_id,
                name: pokemon.name.clone(),
                level: pokemon.level,
                current_hp_percent: pokemon.current_hp as f32 / pokemon.max_hp as f32,
                current_hp: pokemon.current_hp,
                max_hp: pokemon.max_hp,
                status: pokemon.status.clone(),
                is_fainted: pokemon.is_fainted,
                team_index: pokemon.position,
            })
            .collect::<Vec<_>>();
        
        // 8.2 Active Pokémon detail
        let active_pokemon = &battle_state_for_messages.player.team[battle_state_for_messages.player.active_pokemon_index];
        let active_pokemon_view = BattlePokemonPrivateView::from_battle_pokemon(
            active_pokemon,
            battle_state_for_messages.move_repository.as_ref()
        );
        
        // 8.3 Wild Pokémon view
        let wild_pokemon_view = BattlePokemonPublicView::from_battle_pokemon(
            &battle_state_for_messages.wild_pokemon
        );
        
        // 8.4 Send battle start message
        let start_message = ServerMessage::WildBattleStart {
            battle_id,
            player_team: team_overview.clone(),
            initial_pokemon: active_pokemon_view.clone(),
            wild_pokemon: wild_pokemon_view.clone(),
            initial_field_state: battle_state_for_messages.field_state.clone(),
        };
        
        if let Err(e) = lobby.send_to_player(player_id, &start_message).await {
            error!("Failed to send battle start message: {}", e);
            // Continue anyway, we've already set up the battle
        }
        
        // 8.5 Send request action message
        let request_action_message = ServerMessage::RequestAction {
            turn_number: battle_state_for_messages.turn_number,
            active_pokemon_state: active_pokemon_view,
            team_overview,
            other_pokemon_state: wild_pokemon_view,
            can_switch: true, // Usually true at start of battle
            must_switch: false,
            field_state: battle_state_for_messages.field_state.clone(),
        };
        
        if let Err(e) = lobby.send_to_player(player_id, &request_action_message).await {
            error!("Failed to send request action message: {}", e);
            // Continue anyway
        }
        
        // Release lock
        drop(battle_state_for_messages);
        
        Ok(battle_id)
    }

    /// Get the battle state for a given battle ID
    pub fn get_battle_state(&self, battle_id: Uuid) -> Option<Arc<Mutex<WildBattleState>>> {
        self.active_battles.get(&battle_id).map(|entry| entry.value().clone())
    }

    /// Find all battle IDs in which a player is participating
    pub fn find_battles_for_player(&self, player_id: &str) -> Vec<Uuid> {
        let mut battles = Vec::new();
        
        // Check wild battles
        self.active_battles.iter().for_each(|battle_entry| {
            let battle_id = *battle_entry.key();
            let battle_mutex = battle_entry.value();
            
            if let Ok(battle_state) = battle_mutex.try_lock() {
                if battle_state.player.player_id == player_id {
                    battles.push(battle_id);
                }
            }
        });
        
        // Check PvP battles
        self.active_pvp_battles.iter().for_each(|battle_entry| {
            let battle_id = *battle_entry.key();
            let battle_mutex = battle_entry.value();
            
            if let Ok(battle_state) = battle_mutex.try_lock() {
                if battle_state.player1.player_id == player_id || battle_state.player2.player_id == player_id {
                    battles.push(battle_id);
                }
            }
        });
        
        battles
    }

    /// End a battle and clean up resources
    pub async fn end_battle(
        &self,
        battle_id: Uuid,
        lobby: &Arc<Lobby>,
        pokemon_collection_manager: &Arc<PokemonCollectionManager>,
        is_disconnect: bool
    ) -> Result<(), String> {
        info!("Ending battle {} (Disconnect: {})", battle_id, is_disconnect);

        // --- 1. Retrieve Battle State and Extract Data within a limited scope ---
        let (player_id, wild_monster_id, outcome, reason, exp_gained, captured_pokemon_view) = {
            let battle_mutex = self.active_battles.get(&battle_id)
                .ok_or_else(|| format!("Battle {} not found for ending.", battle_id))?
                .value().clone();

            info!("Waiting to get lock for battle state extraction in battle {}", battle_id);
            let battle_state = battle_mutex.lock().await;
            info!("Got lock for battle state extraction in battle {}", battle_id);

            let player_id = battle_state.player.player_id.clone();
            // Ensure wild_monster_id is correctly assigned
            let wild_monster_id = battle_state.wild_pokemon.instance_id.clone();


            // --- Determine Outcome and Reason ---
            let determined_outcome;
            let determined_reason;
            let mut determined_exp_gained = None;
            let mut determined_captured_pokemon_view = None;

            if is_disconnect {
                determined_outcome = WildBattleOutcome::PlayerDisconnected;
                determined_reason = BattleEndReason::PlayerDisconnected;
            } else if let Some(last_attempt) = battle_state.capture_attempts.last() {
                 if last_attempt.success {
                    determined_outcome = WildBattleOutcome::Captured;
                    determined_reason = BattleEndReason::WildPokemonCaptured;

                    // --- Pokemon Creation and Saving ---
                    let captured_pokemon = Pokemon {
                        id: Uuid::new_v4().to_string(),
                        template_id: battle_state.wild_pokemon.template_id,
                        name: battle_state.wild_pokemon.name.clone(),
                        level: battle_state.wild_pokemon.level,
                        exp: 0,
                        max_exp: self.template_repository.get_exp_for_next_level(battle_state.wild_pokemon.template_id, battle_state.wild_pokemon.level),
                        capture_date: chrono::Utc::now().timestamp() as u64,
                        current_hp: battle_state.wild_pokemon.current_hp,
                        status_condition: battle_state.wild_pokemon.status.clone(), // Clone status
                        types: battle_state.wild_pokemon.pokemon_types.clone(),
                        ability: battle_state.wild_pokemon.ability.clone(),
                        // Clone moves correctly
                        moves: battle_state.wild_pokemon.moves.iter().map(|m| MonsterMove { id: m.move_id, pp_remaining: m.current_pp }).collect(),
                        ivs: battle_state.wild_pokemon.ivs.clone(),
                        evs: crate::stats::StatSet::default(), // TODO: Get EVs
                        nature: crate::stats::nature::Nature::Hardy, // TODO: Get Nature
                    };
                    // Use a separate async block if needed, but await here is fine if not blocking excessively
                    match pokemon_collection_manager.add_pokemon(&player_id, captured_pokemon.clone()).await {
                        Ok(_) => info!("Saved captured Pokemon {} for player {}", captured_pokemon.id, player_id),
                        Err(e) => error!("Failed to save captured Pokemon: {}", e),
                    }
                    let active_pokemons = pokemon_collection_manager.get_active_pokemons(&player_id).await.unwrap();
                    // send to player
                    let active_pokemons_msg = ServerMessage::ActivePokemons { 
                        pokemons: active_pokemons.iter().map(|p| pokemon_collection_manager.pokemon_to_display_pokemon(p)).collect()
                    };
                    if let Err(e) = lobby.send_to_player(&player_id, &active_pokemons_msg).await {
                        error!("Failed to send active Pokémon collection to player {}: {}", player_id, e);
                    }

                     // TODO: Create PrivateView from captured_pokemon if needed
                     determined_captured_pokemon_view = None; // Placeholder view
                     // --- End Pokemon Creation ---

                 } else if battle_state.wild_pokemon.is_fainted {
                    determined_outcome = WildBattleOutcome::Victory;
                    determined_reason = BattleEndReason::WildPokemonDefeated;
                    
                    // Use the utility function to calculate EXP
                    let exp_gain = utils::calculate_exp_gain(&battle_state.wild_pokemon, &self.template_repository);
                    determined_exp_gained = Some(exp_gain);
                } else if battle_state.player.team.iter().all(|p| p.is_fainted) {
                    determined_outcome = WildBattleOutcome::Defeat;
                    determined_reason = BattleEndReason::AllPlayerPokemonFainted;
                } else if let Some(PlayerAction::Run) = battle_state.player_action {
                     determined_outcome = WildBattleOutcome::PlayerRan;
                     determined_reason = BattleEndReason::PlayerRanAway;
                } else {
                     // Default/Fallback logic for end without capture/faint/run
                     warn!("Battle {} ending with unclear state (no capture, faint, or run). Defaulting to Fled.", battle_id);
                     determined_outcome = WildBattleOutcome::Fled;
                     determined_reason = BattleEndReason::WildPokemonFled; // Or maybe Undetermined?
                }
            } else { // No capture attempts
                 if battle_state.wild_pokemon.is_fainted {
                    determined_outcome = WildBattleOutcome::Victory;
                    determined_reason = BattleEndReason::WildPokemonDefeated;
                    
                    // Use the utility function to calculate EXP
                    let exp_gain = utils::calculate_exp_gain(&battle_state.wild_pokemon, &self.template_repository);
                    determined_exp_gained = Some(exp_gain);
                } else if battle_state.player.team.iter().all(|p| p.is_fainted) {
                    determined_outcome = WildBattleOutcome::Defeat;
                    determined_reason = BattleEndReason::AllPlayerPokemonFainted;
                } else if let Some(PlayerAction::Run) = battle_state.player_action {
                     determined_outcome = WildBattleOutcome::PlayerRan;
                     determined_reason = BattleEndReason::PlayerRanAway;
                } else {
                     // Default/Fallback logic for end without capture/faint/run
                     warn!("Battle {} ending with unclear state (no capture, faint, or run). Defaulting to Fled.", battle_id);
                     determined_outcome = WildBattleOutcome::Fled;
                     determined_reason = BattleEndReason::WildPokemonFled; // Or maybe Undetermined?
                }
            }

            info!("Releasing lock for battle state extraction in battle {}", battle_id);
            // Return the extracted data; the lock is released at the end of this scope
            (
                player_id,
                wild_monster_id,
                determined_outcome,
                determined_reason,
                determined_exp_gained,
                determined_captured_pokemon_view,
            )
        }; // <- battle_state lock is released here

        // --- 2. Remove Battle from Active Battles Map ---
        if self.active_battles.remove(&battle_id).is_none() {
            warn!("Battle {} was already removed.", battle_id);
            // If the battle was already removed, it might have been ended by another process.
            // We might want to return early to avoid double-cleanup attempts.
            // However, lobby cleanup might still be needed, so proceed cautiously.
             // return Ok(()); // Option: exit early if battle already gone
        } else {
            info!("Removed battle {} from active battles map.", battle_id);
        }


        // --- 3. Send BattleEnd Message (only if not a disconnect) ---
        if !is_disconnect {
            let end_message = ServerMessage::BattleEnd {
                outcome: outcome.clone(),
                reason: reason.clone(),
                pokemon_captured: captured_pokemon_view,
            };
            info!("Attempting to send BattleEnd message for battle {} to player {}", battle_id, player_id);
            // Check if player still exists in lobby before sending
            if lobby.player_positions.contains_key(&player_id) {
                 if let Err(e) = lobby.send_to_player(&player_id, &end_message).await {
                    // Log error, but don't necessarily stop cleanup
                    error!("Failed to send BattleEnd message to player {}: {}", player_id, e);
                 } else {
                    info!("Successfully sent BattleEnd message for battle {} to player {}", battle_id, player_id);
                    
                    // Apply experience to active Pokémon if this was a victory
                    if outcome == WildBattleOutcome::Victory && exp_gained.is_some() {
                        let experience = exp_gained.unwrap() as u64;
                        
                        // Get the active Pokémon's ID (we need the collection ID, not the battle ID)
                        match pokemon_collection_manager.get_active_pokemons(&player_id).await {
                            Ok(active_pokemons) => {
                                if !active_pokemons.is_empty() {
                                    // Apply experience to the first Pokémon (in a future enhancement, this could be 
                                    // divided among all participating Pokémon)
                                    let first_pokemon = &active_pokemons[0];
                                    
                                    match pokemon_collection_manager.add_experience_to_pokemon(
                                        &player_id, 
                                        &first_pokemon.id, 
                                        experience
                                    ).await {
                                        Ok((updated_pokemon, leveled_up)) => {
                                            info!("Added {} experience to {}, leveled up: {}", 
                                                 experience, updated_pokemon.name, leveled_up);
                                                 
                                            // Send a special level-up message if the Pokémon leveled up
                                            if leveled_up {
                                                let level_up_msg = ServerMessage::Error { 
                                                    message: format!("Congratulations! Your {} grew to level {}!", 
                                                                    updated_pokemon.name, updated_pokemon.level) 
                                                };
                                                
                                                if let Err(e) = lobby.send_to_player(&player_id, &level_up_msg).await {
                                                    error!("Failed to send level-up message: {}", e);
                                                }
                                            }
                                                 
                                            // Send updated Pokémon collection to the client
                                            // Get all active Pokémon again as they could have changed
                                            if let Ok(updated_pokemons) = pokemon_collection_manager.get_active_pokemons(&player_id).await {
                                                let display_pokemons: Vec<crate::models::DisplayPokemon> = updated_pokemons
                                                    .iter()
                                                    .map(|p| pokemon_collection_manager.pokemon_to_display_pokemon(p))
                                                    .collect();
                                                
                                                let active_pokemons_msg = ServerMessage::ActivePokemons { 
                                                    pokemons: display_pokemons 
                                                };
                                                
                                                if let Err(e) = lobby.send_to_player(&player_id, &active_pokemons_msg).await {
                                                    error!("Failed to send updated Pokémon collection: {}", e);
                                                } else {
                                                    info!("Sent updated Pokémon collection to player {}", player_id);
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            error!("Failed to add experience to Pokémon: {}", e);
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Failed to get active Pokémon for player {}: {}", player_id, e);
                            }
                        }
                    }
                 }
            } else {
                 info!("Skipping BattleEnd message for battle {}: Player {} no longer in lobby.", battle_id, player_id);
            }
        } else {
            info!("Skipping BattleEnd message send for battle {} due to disconnect for player {}", battle_id, player_id);
        }

        // --- 4. Cleanup Lobby State (No BattleState lock held) ---

        // --- Player State Update ---
        if !is_disconnect {
            // Only attempt to mark player out of combat if it wasn't a disconnect
            info!("Attempting to update player state for {} in lobby {}", player_id, lobby.id);
            if let Some(mut player_state_entry) = lobby.player_positions.get_mut(&player_id) {
                player_state_entry.value_mut().in_combat = false;
                info!("Marked player {} as no longer in combat in lobby {}", player_id, lobby.id);
            } else {
                // If the player disconnected *just* as the battle ended normally.
                warn!("Player {} not found in lobby state for cleanup (maybe disconnected?) in lobby {}", player_id, lobby.id);
            }
            info!("Finished player state update attempt for {} in lobby {}", player_id, lobby.id);
        } else {
            // If it *was* a disconnect, the disconnect handler is responsible for removing the player.
            // Do not try to modify the player state here to avoid lock contention.
            info!("Skipping player state update for {} in lobby {} due to disconnect flag.", player_id, lobby.id);
        }

        // --- Monster State Update ---
        if !wild_monster_id.is_empty() {
            info!("Attempting to update monster state for {} in lobby {}", wild_monster_id, lobby.id);
            
            // Determine if despawn should happen based on outcome *before* locking the monster
            let should_despawn = match outcome {
                WildBattleOutcome::Victory | WildBattleOutcome::Captured => true,
                _ => false,
            };

            // Mark monster as no longer in combat first
            if let Some(monster_ref) = lobby.active_monsters.get(&wild_monster_id) {
                if let Ok(mut monster_lock) = monster_ref.value().try_lock() {
                    monster_lock.in_combat = false;
                    info!("Marked monster {} as no longer in combat in lobby {}", wild_monster_id, lobby.id);
                    // No longer setting despawn_time here
                } else {
                     warn!("Failed to acquire monster lock (try_lock) for {} in lobby {} during combat state update", wild_monster_id, lobby.id);
                 }
             } else {
                  warn!("Monster {} not found in lobby active_monsters for combat state update in lobby {}", wild_monster_id, lobby.id);
             }

            // If the outcome dictates a despawn, call the despawn function now
            if should_despawn {
                info!("Outcome requires despawn for monster {} in lobby {}. Calling despawn_monster.", wild_monster_id, lobby.id);
                // Call despawn_monster using the lobby's monster_manager
                // This removes the monster from the lobby's active_monsters and monsters_by_spawn_point
                match lobby.monster_manager.despawn_monster(&wild_monster_id, lobby).await {
                    Some(_) => {
                        info!("Successfully despawned monster {} from lobby {} via end_battle", wild_monster_id, lobby.id);
                        
                        // Send MonsterDespawned message to all clients in the lobby
                        let despawn_msg = ServerMessage::MonsterDespawned { instance_id: wild_monster_id.clone() };
                        if let Err(e) = lobby.broadcast_except(&despawn_msg, &[]).await {
                            error!("Failed to broadcast despawn message for {}: {}", wild_monster_id, e);
                        } else {
                            info!("Broadcast monster despawn message for {} to all players in lobby {}", wild_monster_id, lobby.id);
                        }
                    }
                    None => {
                        warn!("despawn_monster called for {}, but it was already removed or not found in lobby {}", wild_monster_id, lobby.id);
                    }
                }
            } else {
                info!("Outcome does not require despawn for monster {} in lobby {}.", wild_monster_id, lobby.id);
            }

            info!("Finished monster state update attempt for {} in lobby {}", wild_monster_id, lobby.id);
         } else {
             warn!("Wild monster ID was empty during cleanup for battle {}", battle_id);
         }


        info!("Battle {} ended processing. Final Outcome: {:?}, Reason: {:?}", battle_id, outcome, reason);
        Ok(())
    }

    /// Handle a player action received from the client
    pub async fn handle_player_action(
        &self, 
        player_id: &str,
        battle_id: Uuid, 
        action: PlayerAction,
        lobby: &Arc<Lobby>, // Add lobby reference
        pokemon_collection_manager: &Arc<PokemonCollectionManager>, // Add PokemonCollectionManager
    ) -> Result<(), String> {
        // First check if this is a PvP battle
        // Check if this is a PvP battle and handle it separately
        let mut is_pvp = false;
        if self.active_pvp_battles.contains_key(&battle_id) {
            is_pvp = true;
        }
        
        if is_pvp {
            return self.handle_pvp_player_action(player_id, battle_id, action, lobby, pokemon_collection_manager).await;
        }
        
        // If not, proceed with handling wild battle action
        let battle_mutex = self.get_battle_state(battle_id)
            .ok_or_else(|| format!("Battle {} not found", battle_id))?;
            
        let mut battle_state = battle_mutex.lock().await;
        
        // Validations (Player ID, Phase)
        if battle_state.player.player_id != player_id {
            return Err("Player ID does not match the battle".to_string());
        }
        if battle_state.battle_phase != BattlePhase::WaitingForPlayerAction {
             return Err(format!("Not expecting player action in phase {:?}", battle_state.battle_phase));
        }
        let validation_result = validate_player_action(&battle_state, &action);
        if let Err(e) = validation_result {
            return Err(format!("Invalid action: {}", e));
        }
        
        // Store actions and set phase
        battle_state.player_action = Some(action.clone());
        let wild_action = determine_wild_action(&battle_state);
        battle_state.wild_action = Some(wild_action.clone());
        battle_state.battle_phase = BattlePhase::ProcessingTurn;
        let current_turn = battle_state.turn_number;
        info!("Stored actions for turn {} battle {}. Processing...", current_turn, battle_id);

        // Process the turn
        let events = logic::process_turn(&mut battle_state);
        info!("Finished processing turn {} for battle {}. Generated {} events. New phase: {:?}", 
            current_turn, battle_id, events.len(), battle_state.battle_phase);
        
        // Send Turn Update
        let turn_update_message = ServerMessage::TurnUpdate {
            turn_number: current_turn, // Send the number of the turn that just finished
            events,
        };
        if let Err(e) = lobby.send_to_player(player_id, &turn_update_message).await {
             error!("Failed to send TurnUpdate message for battle {}: {}", battle_id, e);
             // Don't stop processing, but log error
        }

        // Handle Post-Turn State (Send RequestAction, RequestSwitch, or BattleEnd)
        match battle_state.battle_phase {
            BattlePhase::WaitingForPlayerAction => {
                // Need to construct view models again for the RequestAction message
                let team_overview = battle_state.player.team.iter()
                    .map(|p| BattlePokemonTeamOverview::from_battle_pokemon(p))
                    .collect::<Vec<_>>();
                let active_pokemon_view = BattlePokemonPrivateView::from_battle_pokemon(
                    &battle_state.player.team[battle_state.player.active_pokemon_index], 
                    battle_state.move_repository.as_ref()
                );
                let wild_pokemon_view = BattlePokemonPublicView::from_battle_pokemon(
                    &battle_state.wild_pokemon
                );
                
                let request_action_message = ServerMessage::RequestAction {
                    turn_number: battle_state.turn_number,
                    active_pokemon_state: active_pokemon_view,
                    team_overview,
                    other_pokemon_state: wild_pokemon_view,
                    can_switch: battle_state.player.team.iter().filter(|p| !p.is_fainted).count() > 1,
                    must_switch: false, // Reset must_switch flag if applicable
                    field_state: battle_state.field_state.clone(),
                };
                if let Err(e) = lobby.send_to_player(player_id, &request_action_message).await {
                    error!("Failed to send RequestAction message for battle {}: {}", battle_id, e);
                }
            }
            BattlePhase::WaitingForSwitch => {
                 // Create list of available switches
                 let available_switches = battle_state.player.team.iter()
                    .filter(|p| !p.is_fainted)
                    .map(|p| BattlePokemonTeamOverview::from_battle_pokemon(p))
                    .collect::<Vec<_>>();

                 let request_switch_message = ServerMessage::RequestSwitch {
                     reason: SwitchReason::Fainted, // Assuming faint is the only reason for now
                     available_switches,
                 };
                 if let Err(e) = lobby.send_to_player(player_id, &request_switch_message).await {
                     error!("Failed to send RequestSwitch message for battle {}: {}", battle_id, e);
                 }
            }
            BattlePhase::Finished => {
                 // Battle End logic is now handled by calling end_battle
                 // Need to drop the lock before calling end_battle to avoid deadlock
                 drop(battle_state);
                 if let Err(e) = self.end_battle(battle_id, lobby, pokemon_collection_manager, false).await {
                      error!("Error during battle cleanup for {}: {}", battle_id, e);
                 }
                 // NOTE: end_battle removes the battle from the map, so further access will fail
                 return Ok(()); // Exit early as the battle is over and state removed
            }
            _ => { // CaptureMechanics, ProcessingTurn (shouldn't happen here)
                error!("Unexpected battle phase {:?} after turn processing for battle {}", battle_state.battle_phase, battle_id);
            }
        }

        Ok(())
    }

    /// Handle a player action for a PvP battle
    pub async fn handle_pvp_player_action(
        &self, 
        player_id: &str,
        battle_id: Uuid, 
        action: PlayerAction,
        lobby: &Arc<Lobby>,
        pokemon_collection_manager: &Arc<PokemonCollectionManager>,
    ) -> Result<(), String> {
        let battle_entry = self.active_pvp_battles.get(&battle_id)
            .ok_or_else(|| format!("PvP Battle {} not found", battle_id))?;
        
        let battle_mutex = battle_entry.value().clone();
        let mut battle_state = battle_mutex.lock().await;
        
        // Determine which player is submitting the action
        let is_player1 = battle_state.player1.player_id == player_id;
        let is_player2 = battle_state.player2.player_id == player_id;
        
        if !is_player1 && !is_player2 {
            return Err("Player ID does not match any player in this battle".to_string());
        }
        
        // Check if this player's action is expected in the current phase
        match battle_state.battle_phase {
            BattlePvPPhase::WaitingForBothPlayersActions => {
                // Both players can submit actions
                if is_player1 {
                    battle_state.player1_action = Some(action.clone());
                    info!("Player 1 submitted action for PvP battle {}", battle_id);
                } else {
                    battle_state.player2_action = Some(action.clone());
                    info!("Player 2 submitted action for PvP battle {}", battle_id);
                }
            },
            BattlePvPPhase::WaitingForPlayer1Action => {
                if !is_player1 {
                    return Err("Waiting for Player 1's action, but Player 2 submitted".to_string());
                }
                battle_state.player1_action = Some(action.clone());
                info!("Player 1 submitted action for PvP battle {}", battle_id);
            },
            BattlePvPPhase::WaitingForPlayer2Action => {
                if !is_player2 {
                    return Err("Waiting for Player 2's action, but Player 1 submitted".to_string());
                }
                battle_state.player2_action = Some(action.clone());
                info!("Player 2 submitted action for PvP battle {}", battle_id);
            },
            BattlePvPPhase::WaitingForPlayer1Switch => {
                if !is_player1 {
                    return Err("Waiting for Player 1 to switch, but Player 2 submitted".to_string());
                }
                if let PlayerAction::SwitchPokemon { team_index } = action {
                    // Validate switch is to a non-fainted Pokémon
                    if team_index >= battle_state.player1.team.len() {
                        return Err("Invalid team_index for switch".to_string());
                    }
                    if battle_state.player1.team[team_index].is_fainted {
                        return Err("Cannot switch to a fainted Pokémon".to_string());
                    }
                    battle_state.player1_action = Some(action.clone());
                    info!("Player 1 submitted switch for PvP battle {}", battle_id);
                } else {
                    return Err("Expected a switch action from Player 1".to_string());
                }
            },
            BattlePvPPhase::WaitingForPlayer2Switch => {
                if !is_player2 {
                    return Err("Waiting for Player 2 to switch, but Player 1 submitted".to_string());
                }
                if let PlayerAction::SwitchPokemon { team_index } = action {
                    // Validate switch is to a non-fainted Pokémon
                    if team_index >= battle_state.player2.team.len() {
                        return Err("Invalid team_index for switch".to_string());
                    }
                    if battle_state.player2.team[team_index].is_fainted {
                        return Err("Cannot switch to a fainted Pokémon".to_string());
                    }
                    battle_state.player2_action = Some(action.clone());
                    info!("Player 2 submitted switch for PvP battle {}", battle_id);
                } else {
                    return Err("Expected a switch action from Player 2".to_string());
                }
            },
            _ => {
                return Err(format!("Not expecting player action in phase {:?}", battle_state.battle_phase));
            }
        }
        
        // Check if we have received all expected actions and can process the turn
        if battle_state.ready_for_processing() {
            info!("All required actions received for PvP battle {}. Processing turn...", battle_id);
            
            battle_state.battle_phase = BattlePvPPhase::ProcessingTurn;
            let current_turn = battle_state.turn_number;
            
            // Process the turn using the PvP-specific function
            let events = logic::process_pvp_turn(&mut battle_state, &self.template_repository);
            
            info!("Finished processing turn {} for PvP battle {}. Generated {} events. New phase: {:?}", 
                current_turn, battle_id, events.len(), battle_state.battle_phase);
            
            // Send Turn Update to both players
            let turn_update_message = ServerMessage::TurnUpdate {
                turn_number: current_turn,
                events: events.clone(),
            };
            
            // Send to player 1
            if let Err(e) = lobby.send_to_player(&battle_state.player1.player_id, &turn_update_message).await {
                error!("Failed to send TurnUpdate message to player 1 for battle {}: {}", battle_id, e);
            }
            
            // Send to player 2
            if let Err(e) = lobby.send_to_player(&battle_state.player2.player_id, &turn_update_message).await {
                error!("Failed to send TurnUpdate message to player 2 for battle {}: {}", battle_id, e);
            }
            
            // Determine next steps based on the new battle phase
            match battle_state.battle_phase {
                BattlePvPPhase::WaitingForBothPlayersActions => {
                    // Send RequestAction to both players for the next turn
                    self.send_pvp_request_actions(&battle_state, lobby).await?;
                },
                BattlePvPPhase::WaitingForPlayer1Switch => {
                    // Send switch request to player 1
                    self.send_pvp_switch_request(&battle_state, &battle_state.player1.player_id, lobby).await?;
                },
                BattlePvPPhase::WaitingForPlayer2Switch => {
                    // Send switch request to player 2
                    self.send_pvp_switch_request(&battle_state, &battle_state.player2.player_id, lobby).await?;
                },
                BattlePvPPhase::Finished => {
                    info!("PvP battle {} finished. Determining outcome...", battle_id);
                    // End the battle
                    let player1_id = battle_state.player1.player_id.clone();
                    let player2_id = battle_state.player2.player_id.clone();
                    
                    // Determine the outcome
                    let (player1_outcome, player2_outcome) = {
                        let all_player1_fainted = battle_state.player1.team.iter().all(|p| p.is_fainted);
                        let all_player2_fainted = battle_state.player2.team.iter().all(|p| p.is_fainted);
                        
                        if all_player1_fainted && all_player2_fainted {
                            // Draw
                            (PvPBattleOutcome::Draw, PvPBattleOutcome::Draw)
                        } else if all_player1_fainted {
                            // Player 2 wins
                            (PvPBattleOutcome::Defeat, PvPBattleOutcome::Victory)
                        } else if all_player2_fainted {
                            // Player 1 wins
                            (PvPBattleOutcome::Victory, PvPBattleOutcome::Defeat)
                        } else {
                            // Something unusual ended the battle - fallback to a draw
                            error!("Battle ended but no clear winner in PvP battle {}", battle_id);
                            (PvPBattleOutcome::Draw, PvPBattleOutcome::Draw)
                        }
                    };
                    info!("PvP battle {} ended. Player 1 outcome: {:?}, Player 2 outcome: {:?}", battle_id, player1_outcome, player2_outcome);
                    
                    // Track which pokemon leveled up for each player
                    let mut player1_leveled_pokemon = Vec::new();
                    let mut player2_leveled_pokemon = Vec::new();
                    
                    // Check and update pokemon levels/exp for both players
                    for (player_id, team, leveled_pokemon) in [
                        (player1_id.clone(), &battle_state.player1.team, &mut player1_leveled_pokemon), 
                        (player2_id.clone(), &battle_state.player2.team, &mut player2_leveled_pokemon)
                    ] {
                        // Get player's collection
                        if let Ok(collection) = pokemon_collection_manager.get_collection(&player_id).await {
                            for battle_pokemon in team.iter() {
                                // Find matching pokemon in collection
                                if let Some(collection_pokemon) = collection.pokemons.get(&battle_pokemon.instance_id) {
                                    // Check if level or exp changed during battle
                                    if battle_pokemon.level != collection_pokemon.level 
                                        || battle_pokemon.exp != collection_pokemon.exp {
                                        
                                        let update = PokemonUpdate {
                                            name: None,
                                            level: Some(battle_pokemon.level),
                                            exp: Some(battle_pokemon.exp),
                                            max_exp: Some(battle_pokemon.max_exp),
                                            current_hp: Some(battle_pokemon.max_hp),
                                        };

                                        if let Err(e) = pokemon_collection_manager.update_pokemon(
                                            &player_id, 
                                            &battle_pokemon.instance_id, 
                                            &update
                                        ).await {
                                            error!("Failed to update pokemon stats after battle: {}", e);
                                        }
                                        
                                        // Track if pokemon leveled up
                                        if battle_pokemon.level > collection_pokemon.level {
                                            leveled_pokemon.push(battle_pokemon.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Send collection updates for leveled pokemon
                    if !player1_leveled_pokemon.is_empty() {
                      let collection = pokemon_collection_manager.get_active_pokemons(&player1_id).await.unwrap();
                        let collection_update = ServerMessage::ActivePokemons {
                            pokemons: collection.iter()
                                .map(|p| pokemon_collection_manager.pokemon_to_display_pokemon(p))
                                .collect()
                        };
                        if let Err(e) = lobby.send_to_player(&player1_id, &collection_update).await {
                            error!("Failed to send collection update to player 1: {}", e);
                        }
                    }

                    if !player2_leveled_pokemon.is_empty() {
                        let collection = pokemon_collection_manager.get_active_pokemons(&player2_id).await.unwrap();
                        let collection_update = ServerMessage::ActivePokemons {
                            pokemons: collection.iter()
                                .map(|p| pokemon_collection_manager.pokemon_to_display_pokemon(p))
                                .collect()
                        };
                        if let Err(e) = lobby.send_to_player(&player2_id, &collection_update).await {
                            error!("Failed to send collection update to player 2: {}", e);
                        }
                    }

                    // Prepare battle end messages
                    let player1_end_message = ServerMessage::BattleEnd {
                        outcome: self.convert_pvp_outcome_to_wild(player1_outcome),
                        reason: BattleEndReason::AllPlayerPokemonFainted, // Simplified for now
                        pokemon_captured: None, // No captures in PvP
                    };
                    
                    let player2_end_message = ServerMessage::BattleEnd {
                        outcome: self.convert_pvp_outcome_to_wild(player2_outcome),
                        reason: BattleEndReason::AllPlayerPokemonFainted, // Simplified for now
                        pokemon_captured: None, // No captures in PvP
                    };
                                        
                    // Drop lock before any external operations to avoid deadlocks
                    drop(battle_state);
                    
                    // Important: Drop the battle_entry reference before removing from the map
                    // This prevents deadlock when trying to remove while still holding a reference
                    drop(battle_entry);
                    
                    // Now remove the battle from active battles
                    self.active_pvp_battles.remove(&battle_id);
                    
                    // Reset combat flags for both players
                    if let Some(mut player1_state) = lobby.player_positions.get_mut(&player1_id) {
                        player1_state.value_mut().in_combat = false;
                    }
                    if let Some(mut player2_state) = lobby.player_positions.get_mut(&player2_id) {
                        player2_state.value_mut().in_combat = false;
                    }
                    
                    // Send messages to players
                    if let Err(e) = lobby.send_to_player(&player1_id, &player1_end_message).await {
                        error!("Failed to send battle end message to player 1: {}", e);
                    }
                    if let Err(e) = lobby.send_to_player(&player2_id, &player2_end_message).await {
                        error!("Failed to send battle end message to player 2: {}", e);
                    }
                    
                    info!("PvP battle {} ended", battle_id);
                    return Ok(());
                },
                _ => {
                    error!("Unexpected battle phase {:?} after turn processing for PvP battle {}", battle_state.battle_phase, battle_id);
                }
            }
        } else {
            // Still waiting for the other player's action
            // Determine which player we're still waiting for
            if battle_state.player1_action.is_none() && battle_state.player2_action.is_some() {
                battle_state.battle_phase = BattlePvPPhase::WaitingForPlayer1Action;
                info!("Waiting for Player 1's action in PvP battle {}", battle_id);
            } else if battle_state.player1_action.is_some() && battle_state.player2_action.is_none() {
                battle_state.battle_phase = BattlePvPPhase::WaitingForPlayer2Action;
                info!("Waiting for Player 2's action in PvP battle {}", battle_id);
            }
        }
        
        Ok(())
    }
    
    /// Send request actions to both players in a PvP battle
    async fn send_pvp_request_actions(
        &self,
        battle_state: &PvPBattleState,
        lobby: &Arc<Lobby>,
    ) -> Result<(), String> {
        // Prepare data for player 1
        let player1_team_overview = battle_state.player1.team.iter()
            .map(|p| BattlePokemonTeamOverview::from_battle_pokemon(p))
            .collect::<Vec<_>>();
        
        let player1_active_pokemon = &battle_state.player1.team[battle_state.player1.active_pokemon_index];
        let player1_active_view = BattlePokemonPrivateView::from_battle_pokemon(
            player1_active_pokemon,
            battle_state.move_repository.as_ref()
        );
        
        let player2_active_pokemon = &battle_state.player2.team[battle_state.player2.active_pokemon_index];
        let player1_opponent_view = BattlePokemonPublicView::from_battle_pokemon(player2_active_pokemon);
        
        // Prepare data for player 2
        let player2_team_overview = battle_state.player2.team.iter()
            .map(|p| BattlePokemonTeamOverview::from_battle_pokemon(p))
            .collect::<Vec<_>>();
        
        let player2_active_view = BattlePokemonPrivateView::from_battle_pokemon(
            player2_active_pokemon,
            battle_state.move_repository.as_ref()
        );
        
        let player2_opponent_view = BattlePokemonPublicView::from_battle_pokemon(player1_active_pokemon);
        
        // For now, reuse the existing RequestAction message
        // In the future, we might want a dedicated PvPRequestAction message
        
        // Send request to player 1
        let player1_request = ServerMessage::RequestAction {
            turn_number: battle_state.turn_number,
            active_pokemon_state: player1_active_view,
            team_overview: player1_team_overview,
            other_pokemon_state: player1_opponent_view, // Using opponent's public view here
            can_switch: battle_state.player1.team.iter().filter(|p| !p.is_fainted).count() > 1,
            must_switch: battle_state.player1.must_switch,
            field_state: battle_state.field_state.clone(),
        };
        
        if let Err(e) = lobby.send_to_player(&battle_state.player1.player_id, &player1_request).await {
            error!("Failed to send action request to player 1: {}", e);
            return Err(format!("Failed to send action request to player 1: {}", e));
        }
        
        // Send request to player 2
        let player2_request = ServerMessage::RequestAction {
            turn_number: battle_state.turn_number,
            active_pokemon_state: player2_active_view,
            team_overview: player2_team_overview,
            other_pokemon_state: player2_opponent_view, // Using opponent's public view here
            can_switch: battle_state.player2.team.iter().filter(|p| !p.is_fainted).count() > 1,
            must_switch: battle_state.player2.must_switch,
            field_state: battle_state.field_state.clone(),
        };
        
        if let Err(e) = lobby.send_to_player(&battle_state.player2.player_id, &player2_request).await {
            error!("Failed to send action request to player 2: {}", e);
            return Err(format!("Failed to send action request to player 2: {}", e));
        }
        
        Ok(())
    }
    
    /// Send a switch request to a player in a PvP battle
    async fn send_pvp_switch_request(
        &self,
        battle_state: &PvPBattleState,
        player_id: &str,
        lobby: &Arc<Lobby>,
    ) -> Result<(), String> {
        let (available_switches, is_player1) = if battle_state.player1.player_id == player_id {
            // Player 1
            let switches = battle_state.player1.team.iter()
                .filter(|p| !p.is_fainted)
                .map(|p| BattlePokemonTeamOverview::from_battle_pokemon(p))
                .collect::<Vec<_>>();
            (switches, true)
        } else if battle_state.player2.player_id == player_id {
            // Player 2
            let switches = battle_state.player2.team.iter()
                .filter(|p| !p.is_fainted)
                .map(|p| BattlePokemonTeamOverview::from_battle_pokemon(p))
                .collect::<Vec<_>>();
            (switches, false)
        } else {
            return Err("Player ID not found in battle".to_string());
        };
        
        let switch_request = ServerMessage::RequestSwitch {
            reason: SwitchReason::Fainted,
            available_switches,
        };
        
        if let Err(e) = lobby.send_to_player(player_id, &switch_request).await {
            error!("Failed to send switch request to player {}: {}", player_id, e);
            return Err(format!("Failed to send switch request to player: {}", e));
        }
        
        Ok(())
    }

    /// Get the PvP battle state for a given battle ID
    pub fn get_pvp_battle_state(&self, battle_id: Uuid) -> Option<Arc<Mutex<PvPBattleState>>> {
        self.active_pvp_battles.get(&battle_id).map(|entry| entry.value().clone())
    }

    /// Find the opponent's ID in a PvP battle for a given player
    pub fn find_pvp_opponent(&self, battle_id: Uuid, player_id: &str) -> Option<String> {
        if let Some(battle_entry) = self.active_pvp_battles.get(&battle_id) {
            if let Ok(battle_state) = battle_entry.value().try_lock() {
                if battle_state.player1.player_id == player_id {
                    return Some(battle_state.player2.player_id.clone());
                } else if battle_state.player2.player_id == player_id {
                    return Some(battle_state.player1.player_id.clone());
                }
            }
        }
        
        None
    }
    

    // Added conversion method for PvP to wild battle outcomes
    fn convert_pvp_outcome_to_wild(&self, outcome: PvPBattleOutcome) -> WildBattleOutcome {
        match outcome {
            PvPBattleOutcome::Victory => WildBattleOutcome::Victory,
            PvPBattleOutcome::Defeat => WildBattleOutcome::Defeat,
            PvPBattleOutcome::Surrender => WildBattleOutcome::PlayerRan,
            PvPBattleOutcome::OpponentSurrendered => WildBattleOutcome::Victory,
            PvPBattleOutcome::Disconnected => WildBattleOutcome::PlayerDisconnected,
            PvPBattleOutcome::OpponentDisconnected => WildBattleOutcome::Victory,
            PvPBattleOutcome::Draw => WildBattleOutcome::Defeat, // Default to defeat for draws
        }
    }
}

// Placeholder validation function
fn validate_player_action(battle_state: &WildBattleState, action: &PlayerAction) -> Result<(), String> {
    match action {
        PlayerAction::UseMove { move_index } => {
            let active_pokemon = &battle_state.player.team[battle_state.player.active_pokemon_index];
            if *move_index >= active_pokemon.moves.len() {
                return Err("Invalid move index".to_string());
            }
            let battle_move = &active_pokemon.moves[*move_index];
            if battle_move.current_pp == 0 {
                return Err("Move has no PP left".to_string());
            }
            // TODO: Add more checks (imprisoned, disabled, taunted etc.)
        },
        PlayerAction::SwitchPokemon { team_index } => {
            if *team_index >= battle_state.player.team.len() {
                return Err("Invalid team index for switch".to_string());
            }
            if *team_index == battle_state.player.active_pokemon_index {
                return Err("Cannot switch to the already active Pokemon".to_string());
            }
            let target_pokemon = &battle_state.player.team[*team_index];
            if target_pokemon.is_fainted {
                return Err("Cannot switch to a fainted Pokemon".to_string());
            }
            // TODO: Add checks for trapping moves/abilities (Arena Trap, Shadow Tag)
        },
        PlayerAction::UseItem { item_id, .. } => {
            // TODO: Validate item usability (e.g., cannot use Revive on non-fainted)
            warn!("Item usage validation not implemented yet for item {}", item_id);
        },
        PlayerAction::Run => {
            // Running is always a valid *choice*, success is determined later
        }
    }
    Ok(())
}

// Placeholder AI function
fn determine_wild_action(battle_state: &WildBattleState) -> crate::combat::state::WildPokemonAction {
    // Very basic: just use the first available move
    if let Some(first_move) = battle_state.wild_pokemon.moves.iter().position(|m| m.current_pp > 0) {
        crate::combat::state::WildPokemonAction::UseMove { move_index: first_move }
    } else {
        // If no moves have PP, use Struggle
        crate::combat::state::WildPokemonAction::Struggle
    }
}

// Add helper From implementations for view structs (can be moved to state.rs or utils.rs)
impl BattlePokemonTeamOverview {
    fn from_battle_pokemon(pokemon: &BattlePokemon) -> Self {
        BattlePokemonTeamOverview {
             template_id: pokemon.template_id,
             name: pokemon.name.clone(),
             level: pokemon.level,
             current_hp_percent: if pokemon.max_hp == 0 { 0.0 } else { pokemon.current_hp as f32 / pokemon.max_hp as f32 },
             current_hp: pokemon.current_hp,
             max_hp: pokemon.max_hp,
             status: pokemon.status.clone(),
             is_fainted: pokemon.is_fainted,
             team_index: pokemon.position,
        }
    }
}

impl BattlePokemonPublicView {
     fn from_battle_pokemon(pokemon: &BattlePokemon) -> Self {
        BattlePokemonPublicView {
             template_id: pokemon.template_id,
             name: pokemon.name.clone(),
             level: pokemon.level,
             current_hp_percent: if pokemon.max_hp == 0 { 0.0 } else { pokemon.current_hp as f32 / pokemon.max_hp as f32 },
             max_hp: pokemon.max_hp,
             types: pokemon.pokemon_types.clone(),
             status: pokemon.status.clone(),
             stat_modifiers: pokemon.stat_modifiers.clone(),
             is_fainted: pokemon.is_fainted,
             is_wild: pokemon.is_wild,
        }
    }
}

impl BattlePokemonPrivateView {
     fn from_battle_pokemon(pokemon: &BattlePokemon, move_repo: Option<&Arc<crate::monsters::move_manager::MoveRepository>>) -> Self {
         BattlePokemonPrivateView {
             template_id: pokemon.template_id,
             name: pokemon.name.clone(),
             level: pokemon.level,
             current_hp: pokemon.current_hp,
             current_hp_percent: if pokemon.max_hp == 0 { 0.0 } else { pokemon.current_hp as f32 / pokemon.max_hp as f32 },
             max_hp: pokemon.max_hp,
             types: pokemon.pokemon_types.clone(),
             ability: pokemon.ability.clone(),
             status: pokemon.status.clone(),
             volatile_statuses: pokemon.volatile_statuses.keys().cloned().collect(),
             stat_modifiers: pokemon.stat_modifiers.clone(),
             moves: pokemon.moves.iter().map(|m| {
                 // Use move repository if available
                 if let Some(repo) = move_repo {
                     if let Some(move_data) = repo.get_move(m.move_id) {
                         crate::combat::state::BattleMoveView {
                             move_id: m.move_id,
                             name: move_data.name.clone(),
                             move_type: move_data.move_type.clone(),
                             category: match move_data.damage_class {
                                 crate::monsters::move_manager::MoveCategory::Physical => 
                                     crate::combat::state::MoveCategory::Physical,
                                 crate::monsters::move_manager::MoveCategory::Special => 
                                     crate::combat::state::MoveCategory::Special,
                                 crate::monsters::move_manager::MoveCategory::Status => 
                                     crate::combat::state::MoveCategory::Status,
                             },
                             current_pp: m.current_pp,
                             max_pp: m.max_pp,
                             power: move_data.power,
                             accuracy: move_data.accuracy,
                             description: move_data.description.clone(),
                         }
                     } else {
                         // Fallback for unknown move
                         crate::combat::state::BattleMoveView {
                             move_id: m.move_id,
                             name: format!("Move {}", m.move_id),
                             move_type: crate::monsters::monster::PokemonType::Normal,
                             category: crate::combat::state::MoveCategory::Physical,
                             current_pp: m.current_pp,
                             max_pp: m.max_pp,
                             power: Some(50),
                             accuracy: Some(100),
                             description: "".to_string(),
                         }
                     }
                 } else {
                     // Fallback if no repo available
                     crate::combat::state::BattleMoveView {
                         move_id: m.move_id,
                         name: format!("Move {}", m.move_id), 
                         move_type: crate::monsters::monster::PokemonType::Normal,
                         category: crate::combat::state::MoveCategory::Physical,
                         current_pp: m.current_pp,
                         max_pp: m.max_pp,
                         power: Some(50),
                         accuracy: Some(100),
                         description: "".to_string(),
                     }
                 }
             }).collect(),
             is_fainted: pokemon.is_fainted,
             team_index: pokemon.position,
         }
     }
} 