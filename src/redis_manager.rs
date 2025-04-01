use redis::AsyncCommands;

pub async fn init_redis_client(redis_url: &str) -> redis::Client {
    let client = redis::Client::open(redis_url).expect("Failed to create Redis client");
    
    // Test the connection
    let mut con = client.get_async_connection().await.expect("Failed to connect to Redis");
    let _: String = redis::cmd("PING").query_async(&mut con).await.expect("Redis connection test failed");
    
    tracing::info!("Successfully connected to Redis at {}", redis_url);
    client
}

pub async fn store_session(
    redis_conn: &mut redis::aio::Connection,
    session_token: &str, 
    player_id: &str,
    expiry_seconds: u64
) -> redis::RedisResult<()> {
    // Use a much longer expiry time for persistent sessions
    // This value makes sessions last for approximately 10 years
    const LONG_TERM_EXPIRY: u64 = 315_360_000; // 60*60*24*365*10 seconds
    
    redis_conn.set_ex(
        &format!("session:{}", session_token),
        player_id,
        LONG_TERM_EXPIRY,
    ).await
}

pub async fn get_player_id(
    redis_conn: &mut redis::aio::Connection,
    session_token: &str
) -> redis::RedisResult<String> {
    redis_conn.get(&format!("session:{}", session_token)).await
}

pub async fn store_player_state(
    redis_conn: &mut redis::aio::Connection,
    lobby_id: &str,
    player_id: &str,
    state_json: &str,
) -> redis::RedisResult<()> {
    redis_conn.set(
        &format!("player:{}:{}", lobby_id, player_id),
        state_json,
    ).await
}

pub async fn get_player_state(
    redis_conn: &mut redis::aio::Connection,
    lobby_id: &str,
    player_id: &str
) -> redis::RedisResult<String> {
    redis_conn.get(&format!("player:{}:{}", lobby_id, player_id)).await
}

pub async fn reset_lobby_data(
    redis_conn: &mut redis::aio::Connection,
    lobby_id: &str
) -> redis::RedisResult<u32> {
    // Find all keys matching the pattern for this lobby
    let pattern = format!("player:{}:*", lobby_id);
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(redis_conn)
        .await?;
    
    // If there are keys to delete, delete them
    if !keys.is_empty() {
        // Use DEL command with all keys
        let deleted: u32 = redis::cmd("DEL")
            .arg(keys)
            .query_async(redis_conn)
            .await?;
        
        tracing::info!("Reset lobby {}: deleted {} keys", lobby_id, deleted);
        Ok(deleted)
    } else {
        tracing::info!("No keys found for lobby {}", lobby_id);
        Ok(0)
    }
} 