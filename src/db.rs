use core::net::IpAddr;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::router;

pub type DB = Arc<Routers>;

pub struct Routers {
    pub routers: Mutex<HashMap<IpAddr, router::Router>>,
}

pub async fn new_db() -> DB {
    Arc::new(Routers {
        routers: Mutex::new(HashMap::new()),
    })
}
