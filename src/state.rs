use bgpkit_parser::models::NetworkPrefix;
use chrono::Utc;
use core::net::IpAddr;
use redis::aio::MultiplexedConnection;
use std::error::Error;
use std::sync::Arc;

use crate::settings::StateConfig;
use crate::update::Update;

pub struct State {
    store: RedisStore,
}

pub async fn new_state(state_config: &StateConfig) -> Result<Arc<State>, Box<dyn Error>> {
    let client = redis::Client::open(format!("redis://{}/", state_config.host))?;
    let connection = client.get_multiplexed_async_connection().await?;
    Ok(Arc::new(State {
        store: RedisStore { connection },
    }))
}

impl State {
    // Get all the updates from the state
    pub async fn get_all(&self) -> Result<Vec<(IpAddr, IpAddr, NetworkPrefix)>, Box<dyn Error>> {
        let updates = self.store.list("risotto|*").await?;
        let mut res = Vec::new();
        for update in updates {
            let parts: Vec<&str> = update.split('|').collect();
            let router_addr = parts[1].parse()?;
            let peer_addr = parts[2].parse()?;
            let prefix = parts[3].parse()?;
            res.push((router_addr, peer_addr, prefix));
        }
        Ok(res)
    }

    // Get the updates for a specific router and peer
    pub async fn get_updates(
        &self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
    ) -> Result<Vec<NetworkPrefix>, Box<dyn Error>> {
        let updates = self
            .store
            .list(&format!("risotto|{}|{}|*", router_addr, peer_addr))
            .await?;
        let mut res = Vec::new();
        for update in updates {
            let parts: Vec<&str> = update.split('|').collect();
            let prefix = parts[3].parse()?;
            res.push(prefix);
        }
        Ok(res)
    }

    // Remove all updates for a specific router and peer
    pub async fn remove_updates(
        &self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
    ) -> Result<(), Box<dyn Error>> {
        let updates = self
            .store
            .list(&format!("risotto|{}|{}|*", router_addr, peer_addr))
            .await?;
        for update in updates {
            self.store.del(&update).await?;
        }
        Ok(())
    }

    // Update the state with a new update
    pub async fn update(
        &self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
        update: &Update,
    ) -> Result<bool, Box<dyn Error>> {
        let key = format!("risotto|{}|{}|{}", router_addr, peer_addr, update.prefix);
        let present = self.store.get(&key).await.is_ok();

        // Will emit the update only if (announced and not present) or  (not announced and present)
        // Which is a XOR operation
        let emit = update.announced ^ present;

        if update.announced {
            // Store the update, overwriting if present already with the new timestamp
            // Note, we are storing now and not the update timestamp,
            // because the only purpose of this is to be able to retrieve missed withdraws at startup
            let now = Utc::now().timestamp_millis();
            self.store.set(&key, &format!("{}", now)).await?;
        } else {
            self.store
                .del(&format!(
                    "risotto|{}|{}|{}",
                    router_addr, peer_addr, update.prefix
                ))
                .await?;
        }
        Ok(emit)
    }
}

pub struct RedisStore {
    connection: MultiplexedConnection,
}

impl RedisStore {
    async fn list(&self, pattern: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let mut connection = self.connection.clone();
        let mut cursor = 0;
        let mut res = Vec::new();
        loop {
            let (new_cursor, keys): (i64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .query_async(&mut connection)
                .await?;
            res.extend(keys);
            cursor = new_cursor;
            if cursor == 0 {
                break;
            }
        }
        Ok(res)
    }

    async fn get(&self, key: &str) -> Result<String, Box<dyn Error>> {
        let mut connection = self.connection.clone();
        let res = redis::cmd("GET")
            .arg(key)
            .query_async(&mut connection)
            .await?;
        Ok(res)
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), Box<dyn Error>> {
        let mut connection = self.connection.clone();
        redis::cmd("SET")
            .arg(key)
            .arg(value)
            .exec_async(&mut connection)
            .await?;
        Ok(())
    }

    async fn del(&self, key: &str) -> Result<(), Box<dyn Error>> {
        let mut connection = self.connection.clone();
        redis::cmd("DEL")
            .arg(key)
            .exec_async(&mut connection)
            .await?;
        Ok(())
    }
}
