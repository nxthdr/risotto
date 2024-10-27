use chrono::{DateTime, Utc};

use bgpkit_parser::bmp::messages::RouteMonitoring;
use bgpkit_parser::models::{AsPath, MetaCommunity, NetworkPrefix, Origin};

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub prefix: NetworkPrefix,
    pub announced: bool,
    pub origin: Origin,
    pub path: Option<AsPath>,
    pub communities: Vec<MetaCommunity>,
    pub timestamp: DateTime<Utc>,
    pub synthetic: bool,
}

pub fn decode_updates(message: RouteMonitoring) -> Option<Vec<Update>> {
    let mut updates = Vec::new();

    match message.bgp_message {
        bgpkit_parser::models::BgpMessage::Update(bgp_update) => {
            let mut prefixes_to_update = Vec::new();
            for prefix in bgp_update.announced_prefixes {
                prefixes_to_update.push((prefix, true));
            }
            for prefix in bgp_update.withdrawn_prefixes {
                prefixes_to_update.push((prefix, false));
            }

            let attributes = bgp_update.attributes;
            let origin = attributes.origin();
            let path = match attributes.as_path() {
                Some(path) => Some(path.clone()),
                None => None,
            };
            let communities: Vec<MetaCommunity> = attributes.iter_communities().collect();
            for (prefix, announced) in prefixes_to_update {
                updates.push(Update {
                    prefix: prefix,
                    announced: announced,
                    origin: origin,
                    path: path.clone(),
                    communities: communities.clone(),
                    timestamp: Utc::now(),
                    synthetic: false,
                });
            }

            return Some(updates);
        }
        _ => None,
    }
}
