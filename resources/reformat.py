import json
import requests

# Open the pokemon.json file
with open('resources/pokemon.json', 'r') as file:
    pokemon_data = json.load(file)

# Add growth rate for each pokemon
for pokemon in pokemon_data['pokemons']:
    print(f"Fetching growth rate for {pokemon['name']}")
    # Get pokemon data from PokeAPI
    response = requests.get(f"https://pokeapi.co/api/v2/pokemon-species/{pokemon['id']}")
    if response.status_code == 200:
        species_data = response.json()
        growth_rate = species_data['growth_rate']['name']
        pokemon['growth_rate'] = growth_rate
    else:
        print(f"Failed to fetch growth rate for {pokemon['name']}")
        pokemon['growth_rate'] = 'medium' # Default fallback

# Save to new file
with open('resources/pokemon2.json', 'w') as file:
    json.dump(pokemon_data, file, indent=2)
