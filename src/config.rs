use serde::{Deserialize, Serialize};
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub game: GameConfig,
    pub performance: PerformanceConfig,
    pub monsters: MonstersConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MonstersConfig {
    pub templates_path: String,
    pub moves_path: String,
    pub type_chart_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GameConfig {
    pub max_players: usize,
    pub update_rate_ms: u64,
    pub inactive_timeout_sec: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PerformanceConfig {
    pub broadcast_channel_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                port: 8080,
                cors_origins: vec!["*".to_string()],
            },
            game: GameConfig {
                max_players: 50,
                update_rate_ms: 100,
                inactive_timeout_sec: 315_360_000, // 10 years (60*60*24*365*10 seconds)
            },
            performance: PerformanceConfig {
                broadcast_channel_size: 100,
            },
            monsters: MonstersConfig {
                templates_path: "resources/pokemon.json".to_string(),
                moves_path: "resources/moves.json".to_string(),
                type_chart_path: "resources/types.json".to_string(),
            },
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        // Load .env file if available
        dotenv::dotenv().ok();

        let mut config = Config::default();

        // Server config
        if let Ok(port) = env::var("PORT") {
            if let Ok(port) = port.parse::<u16>() {
                config.server.port = port;
            }
        }

        if let Ok(host) = env::var("HOST") {
            if let Ok(host) = host.parse::<IpAddr>() {
                config.server.host = host;
            }
        }

        if let Ok(cors) = env::var("CORS_ORIGINS") {
            config.server.cors_origins = cors.split(',').map(|s| s.trim().to_string()).collect();
        }

        // Game config
        if let Ok(max_players) = env::var("MAX_PLAYERS") {
            if let Ok(max_players) = max_players.parse::<usize>() {
                config.game.max_players = max_players;
            }
        }

        if let Ok(update_rate) = env::var("UPDATE_RATE_MS") {
            if let Ok(update_rate) = update_rate.parse::<u64>() {
                config.game.update_rate_ms = update_rate;
            }
        }

        if let Ok(timeout) = env::var("INACTIVE_TIMEOUT_SEC") {
            if let Ok(timeout) = timeout.parse::<u64>() {
                config.game.inactive_timeout_sec = timeout;
            }
        }

        // Performance config
        if let Ok(channel_size) = env::var("BROADCAST_CHANNEL_SIZE") {
            if let Ok(channel_size) = channel_size.parse::<usize>() {
                config.performance.broadcast_channel_size = channel_size;
            }
        }

        // Monster config
        if let Ok(templates_path) = env::var("MONSTER_TEMPLATES_PATH") {
            config.monsters.templates_path = templates_path;
        }
        
        if let Ok(moves_path) = env::var("MOVES_PATH") {
            config.monsters.moves_path = moves_path;
        }

        info!("Configuration loaded: {:?}", config);
        config
    }

    pub fn server_addr(&self) -> SocketAddr {
        SocketAddr::new(self.server.host, self.server.port)
    }
}