
import requests
import json
from typing import Dict

def fetch_type_data() -> Dict[str, Dict[str, float]]:
    """Fetch type effectiveness data from PokeAPI and format it into a type chart."""
    type_chart = {}
    type_names = [
        "normal", "fire", "water", "grass", "electric", "ice", "fighting", 
        "poison", "ground", "flying", "psychic", "bug", "rock", "ghost",
        "dragon", "steel", "dark", "fairy"
    ]
    
    for type_name in type_names:
        response = requests.get(f"https://pokeapi.co/api/v2/type/{type_name}")
        if response.status_code != 200:
            print(f"Failed to fetch data for {type_name}")
            continue
            
        data = response.json()
        damage_relations = data["damage_relations"]
        
        # Initialize empty dict for this type's effectiveness
        type_chart[type_name] = {}
        
        # Double damage to (super effective)
        for target in damage_relations["double_damage_to"]:
            type_chart[type_name][target["name"]] = 2.0
            
        # Half damage to (not very effective)  
        for target in damage_relations["half_damage_to"]:
            type_chart[type_name][target["name"]] = 0.5
            
        # No damage to (immune)
        for target in damage_relations["no_damage_to"]:
            type_chart[type_name][target["name"]] = 0.0

    return type_chart

def main():
    type_chart = fetch_type_data()
    
    # Write to types.json
    print(type_chart)
    with open("types.json", "w") as f:
        json.dump(type_chart, f, indent=2)
        
if __name__ == "__main__":
    main()
