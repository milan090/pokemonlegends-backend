import json
import requests
import time
import re # For parsing multi-hit ranges

POKEAPI_BASE_URL = "https://pokeapi.co/api/v2"
OUTPUT_FILENAME = "moves.json"
MOVE_RANGE_START = 1
MOVE_RANGE_END = 300 # Adjust as needed (max ~900+, but ~300 covers many common moves)
REQUEST_DELAY = 0.1 # Seconds between requests

# --- Mappings from PokeAPI to Your Structure ---

def map_target(pokeapi_target_name):
    """Maps PokeAPI target names to your TargetType enum strings."""
    target_map = {
        "specific-move": "user", # Needs verification per move, default to user?
        "selected-pokemon-me-first": "normal_opponent", # Seems opponent-targeted
        "ally": "ally", # Assuming you might add this later
        "users-field": "user_side",
        "user-or-ally": "user_or_ally", # Assuming you might add this later
        "opponents-field": "opponent_side",
        "user": "user",
        "random-opponent": "random_opponent", # Assuming you might add this later
        "all-other-pokemon": "all_other_pokemon", # Assuming you might add this later
        "selected-pokemon": "normal_opponent", # Most common single target
        "all-opponents": "all_adjacent_opponents",
        "entire-field": "whole_field",
        "user-and-allies": "user_and_allies", # Assuming you might add this later
        "all-pokemon": "all_pokemon", # Assuming you might add this later
        "all-allies": "all_allies", # Assuming you might add this later
        "fainting-pokemon": "target", # Specific case, maybe needs special handling
    }
    # Default if unknown, might need adjustment
    return target_map.get(pokeapi_target_name, "normal_opponent")

def map_stat_name(pokeapi_stat_name):
    """Maps PokeAPI stat names to your Stat enum strings."""
    stat_map = {
        "hp": "hp",
        "attack": "attack",
        "defense": "defense",
        "special-attack": "special_attack",
        "special-defense": "special_defense",
        "speed": "speed",
        "accuracy": "accuracy",
        "evasion": "evasion",
    }
    return stat_map.get(pokeapi_stat_name) # Return None if not found

def map_status_name(pokeapi_ailment_name):
    """Maps PokeAPI ailment names to your StatusCondition enum strings."""
    status_map = {
        "paralysis": "paralysis",
        "sleep": "sleep",
        "freeze": "freeze",
        "burn": "burn",
        "poison": "poison",
        "confusion": "confusion", # This is Volatile, handle separately if needed
        "infatuation": "infatuation", # Volatile
        "trap": "bound", # Volatile
        "nightmare": "nightmare", # Volatile
        "torment": "torment", # Volatile
        "disable": "disable", # Volatile
        "yawn": "yawn", # Volatile (leads to sleep)
        "heal-block": "heal_block", # Volatile
        "no-type-immunity": "no_type_immunity", # Volatile? Special case
        "leech-seed": "leech_seed", # Volatile
        "embargo": "embargo", # Volatile
        "perish-song": "perish_song", # Volatile
        "ingrain": "ingrain", # Volatile
        "silence": "silence", # Volatile? (Seems unused)
        # --- Needs special handling ---
        "badly-poisoned": "toxic", # Map this specifically
        # "none" means no status effect applied by this move meta field
        "none": None,
        "unknown": None, # Treat unknown as none
    }
    return status_map.get(pokeapi_ailment_name)

def get_english_effect(effect_entries):
    """Extracts the English effect description."""
    for entry in effect_entries:
        if entry['language']['name'] == 'en':
            # Replace PokeAPI's $effect_chance with the actual chance if present
            # Might need refinement based on how effect_chance is used in the text
             return entry['effect'].replace('$effect_chance', '{effect_chance}') # Placeholder
             # return entry['short_effect'].replace('$effect_chance', '{effect_chance}') # short_effect is often better

    return "No English description found."

# --- Effect Parsing Logic (Heuristic) ---

def parse_effect_data(move_data):
    """
    Attempts to parse PokeAPI move data into primary and secondary effects.
    This is heuristic and will need manual refinement for many moves.
    Returns a tuple: (primary_effect, secondary_effect, description)
    """
    primary_effect = None
    secondary_effect = None

    meta = move_data.get('meta', {})
    damage_class = move_data['damage_class']['name']
    power = move_data['power']
    effect_chance = move_data.get('effect_chance') # Can be None
    move_name = move_data['name']

    # --- Calculate description early ---
    description = get_english_effect(move_data.get('effect_entries', []))
    # Placeholder replacement will happen later after potential secondary effect calculation


    # --- Hardcoded Overrides for Specific Moves (Essential) ---
    if move_name in ["light-screen", "reflect", "mist", "safeguard", "tailwind"]:
        field_map = {
            "light-screen": {"type": "light_screen", "duration": 5, "side": "user"},
            "reflect": {"type": "reflect", "duration": 5, "side": "user"},
            "mist": {"type": "mist", "duration": 5, "side": "user"}, # Add Mist if needed
            "safeguard": {"type": "safeguard", "duration": 5, "side": "user"}, # Add Safeguard if needed
            "tailwind": {"type": "tailwind", "duration": 4, "side": "user"},
        }
        fm = field_map[move_name]
        primary_effect = {
            "type": "apply_field_effect",
            "parameters": {
                "effect_type": fm["type"],
                "duration": fm["duration"],
                "target_side": fm["side"]
            }
        }
        # Return all three values now
        return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name in ["spikes", "toxic-spikes", "stealth-rock", "sticky-web"]:
         hazard_map = {
             "spikes": {"type": "spikes", "side": "opponent"}, # Needs layer tracking logic
             "toxic-spikes": {"type": "toxic_spikes", "side": "opponent"}, # Needs layer tracking
             "stealth-rock": {"type": "stealth_rock", "side": "opponent"},
             "sticky-web": {"type": "sticky_web", "side": "opponent"},
         }
         hm = hazard_map[move_name]
         primary_effect = {
             "type": "apply_field_effect",
             "parameters": {
                "effect_type": hm["type"],
                "duration": None, # Hazards are persistent
                "target_side": hm["side"]
             }
         }
         # Return all three values now
         return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name in ["rain-dance", "sunny-day", "sandstorm", "hail"]:
         weather_map = {
             "rain-dance": "rain",
             "sunny-day": "harsh_sunlight",
             "sandstorm": "sandstorm",
             "hail": "hail",
         }
         primary_effect = {
             "type": "apply_field_effect", # Or maybe a dedicated "set_weather" type
             "parameters": {
                 "effect_type": weather_map[move_name],
                 "duration": 5,
                 "target_side": "whole_field"
             }
         }
         # Return all three values now
         return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name == "trick-room":
         primary_effect = {
             "type": "apply_field_effect",
             "parameters": {
                 "effect_type": "trick_room",
                 "duration": 5,
                 "target_side": "whole_field"
             }
         }
         # Return all three values now
         return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name == "toxic":
        primary_effect = {
            "type": "apply_status",
            "parameters": {"status": "toxic", "target": "target"}
        }
         # Return all three values now
        return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name == "swords-dance": # Move 14 - this was the cause!
         primary_effect = {
             "type": "stat_change",
             "parameters": {
                 "changes": [{"stat": "attack", "stages": 2}],
                 "target": "user"
             }
         }
         # Return all three values now
         return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name == "recover" or move_name == "roost" or move_name == "soft-boiled": # Add other simple heal moves
         primary_effect = {
             "type": "heal",
             "parameters": {"percent": 50, "target": "user"}
         }
         # Return all three values now
         return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name in ["seismic-toss", "night-shade"]:
        primary_effect = {
             "type": "fixed_damage",
             "parameters": {"damage_source": "user_level"}
        }
         # Return all three values now
        return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here

    if move_name in ["roar", "whirlwind"]:
        primary_effect = {
             "type": "switch_target",
             "parameters": {} # No specific params needed here
        }
         # Return all three values now
        return primary_effect, None, description.replace('{effect_chance}', '0') # No chance here


    # --- General Parsing Logic (continues as before) ---
    ailment = map_status_name(meta.get('ailment', {}).get('name', 'none'))
    ailment_chance = meta.get('ailment_chance', 0)
    healing = meta.get('healing', 0)
    stat_changes = move_data.get('stat_changes', [])
    stat_chance = meta.get('stat_chance', 0)
    flinch_chance = meta.get('flinch_chance', 0)
    min_hits = meta.get('min_hits')
    max_hits = meta.get('max_hits')

    # 1. Determine Primary Effect (continues as before)
    if damage_class in ["physical", "special"] and power is not None:
        primary_effect = {"type": "damage", "parameters": {}}
        if min_hits is not None and max_hits is not None:
             primary_effect["parameters"]["multi_hit"] = {"min": min_hits, "max": max_hits}
             if move_name == "triple-kick":
                 primary_effect["parameters"]["multi_hit"] = {"min": 3, "max": 3}

    elif damage_class == "status":
        if ailment and ailment_chance == 0:
            if ailment in ["confusion", "leech_seed", "bound", "infatuation", "torment", "disable", "yawn", "heal_block", "embargo", "perish_song", "ingrain"]:
                 primary_effect = {"type": "apply_volatile_status", "parameters": {"status": ailment, "target": "target"}}
            elif ailment in ["toxic", "paralysis", "sleep", "freeze", "burn", "poison"]:
                 primary_effect = {"type": "apply_status", "parameters": {"status": ailment, "target": "target"}}

        elif stat_changes and stat_chance == 0:
            changes = []
            target = "user"
            for change in stat_changes:
                stat = map_stat_name(change['stat']['name'])
                if stat:
                    changes.append({"stat": stat, "stages": change['change']})
                    if change['change'] < 0: target = "target"
                    else: target = "user"

            if changes:
                primary_effect = {
                    "type": "stat_change",
                    "parameters": {"changes": changes, "target": target}
                }
        elif healing > 0:
             primary_effect = {"type": "heal", "parameters": {"percent": healing, "target": "user"}}


    # If no primary effect determined yet, default based on damage class
    if primary_effect is None:
        if damage_class in ["physical", "special"]:
             primary_effect = {"type": "damage", "parameters": {}}
             if min_hits is not None and max_hits is not None:
                 primary_effect["parameters"]["multi_hit"] = {"min": min_hits, "max": max_hits}
                 if move_name == "triple-kick":
                    primary_effect["parameters"]["multi_hit"] = {"min": 3, "max": 3}
        else:
             primary_effect = {"type": "unknown_status", "parameters": {}}


    # 2. Determine Secondary Effect (continues as before)
    secondary_candidates = []

    if ailment and ailment_chance > 0:
        status_type = "apply_status"
        if ailment in ["toxic", "paralysis", "sleep", "freeze", "burn", "poison"]:
             status_type = "apply_status"
        elif ailment in ["confusion", "leech_seed", "bound", "infatuation", "torment", "disable", "yawn", "heal_block", "embargo", "perish_song", "ingrain"]:
             status_type = "apply_volatile_status"
        else:
             status_type = None

        if status_type:
             secondary_candidates.append({
                 "chance": ailment_chance,
                 "effect": {"type": status_type, "parameters": {"status": ailment, "target": "target"}}
             })

    if stat_changes and stat_chance > 0:
        changes = []
        target = "target"
        for change in stat_changes:
             stat = map_stat_name(change['stat']['name'])
             if stat:
                 changes.append({"stat": stat, "stages": change['change']})
                 if change['change'] < 0: target = "target"
                 else: target = "user"

        if changes:
             secondary_candidates.append({
                 "chance": stat_chance,
                 "effect": {"type": "stat_change", "parameters": {"changes": changes, "target": target}}
             })

    if flinch_chance > 0:
         secondary_candidates.append({
             "chance": flinch_chance,
             "effect": {"type": "apply_volatile_status", "parameters": {"status": "flinch", "target": "target"}}
         })

    if secondary_candidates:
         secondary_effect = secondary_candidates[0]


    # Final cleanup/adjustment based on effect_chance field
    if secondary_effect is None and effect_chance is not None and effect_chance > 0:
        desc_lower = description.lower() # Use pre-calculated description
        inferred_secondary = None
        if "paralyze" in desc_lower: inferred_secondary = {"type": "apply_status", "parameters": {"status": "paralysis", "target": "target"}}
        elif "burn" in desc_lower: inferred_secondary = {"type": "apply_status", "parameters": {"status": "burn", "target": "target"}}
        elif "freeze" in desc_lower: inferred_secondary = {"type": "apply_status", "parameters": {"status": "freeze", "target": "target"}}
        elif "poison" in desc_lower: inferred_secondary = {"type": "apply_status", "parameters": {"status": "poison", "target": "target"}}
        elif "flinch" in desc_lower: inferred_secondary = {"type": "apply_volatile_status", "parameters": {"status": "flinch", "target": "target"}}
        elif "confuse" in desc_lower or "confusion" in desc_lower: inferred_secondary = {"type": "apply_volatile_status", "parameters": {"status": "confusion", "target": "target"}}
        elif re.search(r"lower.*attack", desc_lower): inferred_secondary = {"type": "stat_change", "parameters": {"changes": [{"stat": "attack", "stages": -1}], "target": "target"}}
        elif re.search(r"lower.*defense", desc_lower): inferred_secondary = {"type": "stat_change", "parameters": {"changes": [{"stat": "defense", "stages": -1}], "target": "target"}}
        elif re.search(r"lower.*special attack", desc_lower): inferred_secondary = {"type": "stat_change", "parameters": {"changes": [{"stat": "special_attack", "stages": -1}], "target": "target"}}
        elif re.search(r"lower.*special defense", desc_lower): inferred_secondary = {"type": "stat_change", "parameters": {"changes": [{"stat": "special_defense", "stages": -1}], "target": "target"}}
        elif re.search(r"lower.*speed", desc_lower): inferred_secondary = {"type": "stat_change", "parameters": {"changes": [{"stat": "speed", "stages": -1}], "target": "target"}}
        elif re.search(r"lower.*accuracy", desc_lower): inferred_secondary = {"type": "stat_change", "parameters": {"changes": [{"stat": "accuracy", "stages": -1}], "target": "target"}}


        if inferred_secondary:
            secondary_effect = {
                 "chance": effect_chance,
                 "effect": inferred_secondary
            }

    # --- Final description formatting ---
    # Determine the actual chance to display (either from secondary effect or effect_chance field)
    display_chance = '0'
    if secondary_effect and 'chance' in secondary_effect:
        display_chance = str(secondary_effect['chance'])
    elif effect_chance is not None:
        display_chance = str(effect_chance)

    # Replace placeholder in the description string
    description = description.replace('{effect_chance}', display_chance)


    # The final return, always returning three values
    return primary_effect, secondary_effect, description


# --- Main Script ---
all_moves_data = {}

print(f"Fetching moves {MOVE_RANGE_START} to {MOVE_RANGE_END} from PokeAPI...")

with open('resources/needmoves.json', 'r') as f:
    move_ids = json.load(f)

for move_id in move_ids:
    try:
        url = f"{POKEAPI_BASE_URL}/move/{move_id}/"
        response = requests.get(url)
        response.raise_for_status() # Raise an exception for bad status codes (4xx or 5xx)

        move_data = response.json()

        # Skip moves with no effect entries (usually placeholder/unused moves)
        if not move_data.get('effect_entries'):
             print(f"Skipping move {move_id} (no effect entries)")
             continue

        primary_effect, secondary_effect, description = parse_effect_data(move_data)

        move_output = {
            "id": move_data['id'],
            "name": move_data['name'],
            "accuracy": move_data['accuracy'],
            "power": move_data['power'],
            "pp": move_data['pp'],
            "priority": move_data['priority'],
            "type": move_data['type']['name'],
            "damage_class": move_data['damage_class']['name'],
            "target": map_target(move_data['target']['name']),
            "effect": primary_effect,
            "secondary_effect": secondary_effect,
            "description": description,
        }

        all_moves_data[str(move_id)] = move_output
        print(f"Processed move {move_id}: {move_data['name']}")

    except requests.exceptions.RequestException as e:
        print(f"Error fetching move {move_id}: {e}")
    except Exception as e:
        print(f"Error processing move {move_id}: {e}") # Catch other potential errors

    time.sleep(REQUEST_DELAY) # Be polite to the API

print(f"\nFetched and processed {len(all_moves_data)} moves.")

# Write to JSON file
try:
    with open(OUTPUT_FILENAME, 'w') as f:
        json.dump(all_moves_data, f, indent=2)
    print(f"Successfully wrote move data to {OUTPUT_FILENAME}")
except IOError as e:
    print(f"Error writing to file {OUTPUT_FILENAME}: {e}")