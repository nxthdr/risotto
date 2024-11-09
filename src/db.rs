use core::net::IpAddr;
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

use crate::router;

pub type DB = Arc<Mutex<HashMap<IpAddr, router::Router>>>;

pub async fn new_db() -> Result<DB, Box<dyn Error>> {
    Ok(Arc::new(Mutex::new(HashMap::new())))
}
