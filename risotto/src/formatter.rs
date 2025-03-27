use risotto_lib::update::Update;

// Returns a CSV line corresponding to this schema
// timestamp,router_addr,router_port,peer_addr,peer_bgp_id,peer_asn,prefix_addr,prefix_len,announced,is_post_policy,is_adj_rib_out,origin,path,communities,synthetic
pub fn format_update(update: &Update) -> String {
    let as_path_str = update
        .path
        .clone()
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<String>>()
        .join(",");
    let as_path_str = format!("\"[{}]\"", as_path_str);

    let communities_str = update
        .communities
        .iter()
        .map(|x| format!("({},{})", x.0, x.1))
        .collect::<Vec<String>>()
        .join(",");
    let communities_str = format!("\"[{}]\"", communities_str);

    let mut row: Vec<String> = Vec::new();
    row.push(format!("{}", update.timestamp.timestamp_millis()));
    row.push(format!("{}", update.router_addr));
    row.push(format!("{}", update.router_port));
    row.push(format!("{}", update.peer_addr));
    row.push(format!("{}", update.peer_bgp_id));
    row.push(format!("{}", update.peer_asn));
    row.push(format!("{}", update.prefix_addr));
    row.push(format!("{}", update.prefix_len));
    row.push(format!("{}", update.announced));
    row.push(format!("{}", update.is_post_policy));
    row.push(format!("{}", update.is_adj_rib_out));
    row.push(format!("{}", update.origin));
    row.push(format!("{}", as_path_str));
    row.push(format!("{}", communities_str));
    row.push(format!("{}", update.synthetic));

    row.join(",")
}
