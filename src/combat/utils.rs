use crate::combat::state::{BattlePokemon, BattleMove};
use crate::game_loop::pokemon_collection::Pokemon;
use crate::monsters::Monster;
use crate::monsters::monster::PokemonType;
use crate::monsters::monster_manager::MonsterTemplateRepository;
use crate::stats::{BaseStats, CalculatedStats, BattleStatModifiers};
use std::collections::HashMap;
use std::sync::Arc;

/// Convert a player-owned Pokemon to a battle Pokemon
pub fn convert_player_pokemon_to_battle_pokemon(
    pokemon: &Pokemon, 
    position: usize, 
    template_repository: &Arc<MonsterTemplateRepository>,
) -> BattlePokemon {
    // Get base stats from the template
    let template = template_repository.templates.get(&pokemon.template_id)
        .expect("Template not found for player pokemon"); // Better error handling might be needed

    // Calculate full stats
    let calculated_stats = crate::stats::calculate_stats(
        &template.base_stats,
        pokemon.level,
        &pokemon.ivs,
        &pokemon.evs,
        &pokemon.nature,
    );

    BattlePokemon {
        template_id: pokemon.template_id,
        name: pokemon.name.clone(),
        level: pokemon.level,
        calculated_stats: calculated_stats.clone(), // Clone calculated stats
        pokemon_types: pokemon.types.clone(),
        ability: pokemon.ability.clone(),
        moves: pokemon.moves.iter().map(|m| {
            let max_pp =  match template_repository.move_repository {
                Some(ref move_repo) => move_repo.get_move(m.id).map(|m| m.pp).unwrap_or(20),
                None => 20,
            };
            BattleMove {
                move_id: m.id,
                current_pp: m.pp_remaining,
                max_pp,
            }
        }).collect(),
        ivs: pokemon.ivs.clone(),
        evs: pokemon.evs.clone(),
        nature: pokemon.nature.clone(),
        current_hp: pokemon.current_hp,
        max_hp: calculated_stats.hp, // Max HP comes from calculated stats
        status: pokemon.status_condition,
        status_turns: 0,
        volatile_statuses: HashMap::new(),
        stat_modifiers: BattleStatModifiers::default(),
        is_fainted: pokemon.current_hp == 0,
        position,
        is_wild: false,
        instance_id: pokemon.id.clone(),
        base_exp: template.base_experience,
        exp: pokemon.exp,
        max_exp: pokemon.max_exp,
    }
}

/// Convert a wild Monster to a battle Pokemon
pub fn convert_wild_monster_to_battle_pokemon(monster: &Monster, template_repository: &Arc<MonsterTemplateRepository>) -> BattlePokemon {
  let template = template_repository.templates.get(&monster.template_id)
    .expect("Template not found for wild monster");
    BattlePokemon {
        template_id: monster.template_id,
        name: monster.name.clone(),
        level: monster.level,
        calculated_stats: monster.calculated_stats.clone(),
        pokemon_types: monster.types.clone(),
        ability: monster.ability.clone(),
        moves: monster.moves.iter().map(|m| {
            let max_pp =  match template_repository.move_repository {
                Some(ref move_repo) => move_repo.get_move(m.id).map(|m| m.pp).unwrap_or(20),
                None => 20,
            };
            BattleMove {
                move_id: m.id,
                current_pp: m.pp_remaining,
                max_pp,
            }
        }).collect(),
        ivs: monster.ivs.clone(),
        evs: monster.evs.clone(),
        nature: monster.nature.clone(),
        current_hp: monster.current_hp,
        max_hp: monster.calculated_stats.hp,
        status: monster.status_condition,
        status_turns: 0,
        volatile_statuses: HashMap::new(),
        stat_modifiers: BattleStatModifiers::default(),
        is_fainted: monster.current_hp == 0,
        position: 0, // Wild Pokemon is always at position 0
        is_wild: true,
        instance_id: monster.instance_id.clone(),
        base_exp: template.base_experience,
        exp: 0,
        max_exp: 0,
    }
}

/// Calculate experience gained from defeating a wild Pokémon
pub fn calculate_exp_gain(
    wild_pokemon: &crate::combat::state::BattlePokemon,
    template_repository: &Arc<MonsterTemplateRepository>
) -> u32 {
    // Get wild Pokémon's template for base experience
    let wild_template = template_repository.templates.get(&wild_pokemon.template_id);
    let wild_base_exp = wild_template.map(|t| t.base_experience).unwrap_or(50);
    let wild_level = wild_pokemon.level;
    
    // Calculate EXP: (Base EXP × Wild Pokémon Level) / 7
    // This is a simplified version of the main formula
    let base_exp_gain = (wild_base_exp as f32 * wild_level as f32 / 7.0).ceil() as u32;
    
    // Apply growth rate modifier
    let growth_modifier = match wild_template.and_then(|t| Some(t.growth_rate.clone())) {
        Some(crate::monsters::monster::GrowthRate::Fast) => 0.8,
        Some(crate::monsters::monster::GrowthRate::Medium) => 1.0,
        Some(crate::monsters::monster::GrowthRate::MediumSlow) => 1.2,
        Some(crate::monsters::monster::GrowthRate::Slow) => 1.25,
        None => 1.0, // Default to Medium rate
    };
    
    // Calculate final exp with growth rate applied
    let exp_gain = (base_exp_gain as f32 * growth_modifier).ceil() as u32;
    
    tracing::info!("Calculated EXP: {} (base: {}, wild level: {}, wild base exp: {}, growth mod: {})",
        exp_gain, base_exp_gain, wild_level, wild_base_exp, growth_modifier);
        
    exp_gain
}
