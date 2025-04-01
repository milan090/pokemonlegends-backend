import json

def extract_move_ids():
    # Read pokemon data
    with open('resources/pokemon.json', 'r') as f:
        pokemon_data = json.load(f)
    
    # Extract all move IDs
    move_ids = set()
    for pokemon in pokemon_data['pokemons']:
        for move in pokemon['moves']:
            move_ids.add(move[0])
    
    # Convert to sorted list
    move_ids = sorted(list(move_ids))
    
    # Save to needmoves.json
    with open('resources/needmoves.json', 'w') as f:
        json.dump(move_ids, f, indent=2)

if __name__ == '__main__':
    extract_move_ids()
