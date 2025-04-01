use std::collections::HashMap;

use crate::combat::state::{WildBattleState, BattleEntityRef};
use crate::monsters::move_manager::MoveData;
use crate::monsters::PokemonType;
use crate::stats::CalculatedStats;
use rand::Rng;

/// Calculate damage using the traditional Pokémon game formula
pub fn calculate_damage(
    source_level: u32,
    source_stats: &CalculatedStats,
    source_types: &Vec<PokemonType>,
    target_stats: &CalculatedStats,
    target_types: &Vec<PokemonType>,
    move_details: &MoveData,
    type_chart: Option<&HashMap<PokemonType, HashMap<PokemonType, f32>>>
) -> (u32, f32, bool) {
    // Get base power (already checked for Some in caller)
    let power = move_details.power.unwrap_or(0);
    if power == 0 {
        return (0, 1.0, false);
    }
    
    // Determine attack and defense stats based on move category
    let (attack, defense) = match move_details.damage_class {
        crate::monsters::move_manager::MoveCategory::Physical => (
            source_stats.attack,
            target_stats.defense
        ),
        crate::monsters::move_manager::MoveCategory::Special => (
            source_stats.special_attack,
            target_stats.special_defense
        ),
        _ => return (0, 1.0, false), // Status moves don't deal direct damage
    };
    
    // Calculate type effectiveness
    let type_effectiveness = calculate_type_effectiveness(
        type_chart,
        &move_details.move_type,
        target_types
    );
    
    // Calculate STAB (Same Type Attack Bonus)
    let stab = if source_types.contains(&move_details.move_type) {
        1.5
    } else {
        1.0
    };
    
    // Determine if critical hit (simplified - 6.25% chance)
    let is_critical = rand::thread_rng().gen_bool(0.0625);
    let critical_mod = if is_critical { 1.5 } else { 1.0 };
    
    // Random factor (between 0.85 and 1.0)
    let random_factor = rand::thread_rng().gen_range(0.85..=1.0);
    
    // Calculate final damage using the formula:
    // Damage = (((2 * Level / 5 + 2) * Power * A/D) / 50 + 2) * Modifier
    let base_damage = (((2.0 * source_level as f32 / 5.0 + 2.0) * power as f32 * attack as f32 / defense as f32) / 50.0 + 2.0);
    
    // Apply modifiers: STAB, Type effectiveness, Critical, Random
    let modifier = stab * type_effectiveness * critical_mod * random_factor;
    
    // Calculate final damage (round down)
    let final_damage = (base_damage * modifier).floor() as u32;
    
    // For zero effectiveness, ensure damage is 0
    let damage = if type_effectiveness == 0.0 { 0 } else { final_damage };
    
    (damage, type_effectiveness, is_critical)
}

/// Calculate type effectiveness based on the type chart
fn calculate_type_effectiveness(
    type_chart: Option<&HashMap<PokemonType, HashMap<PokemonType, f32>>>,
    attack_type: &PokemonType,
    defender_types: &Vec<PokemonType>
) -> f32 {
    // Default to neutral effectiveness if no type chart available
    if type_chart.is_none() {
        return 1.0;
    }
    
    let type_chart = type_chart.unwrap();
    
    // Get effectiveness for each defender type and multiply them
    let mut total_effectiveness = 1.0;
    
    for defender_type in defender_types {
        // Get effectiveness from type chart
        if let Some(type_map) = type_chart.get(attack_type) {
            if let Some(effectiveness) = type_map.get(defender_type) {
                total_effectiveness *= effectiveness;
            }
        }
    }
    
    total_effectiveness
}