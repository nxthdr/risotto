use core::net::IpAddr;

pub fn host(address: String, port: i64, accept_fqdn: bool) -> String {
    let host = match address.parse::<IpAddr>() {
        Ok(ip) => {
            if ip.is_ipv4() {
                address
            } else {
                format!("[{}]", ip)
            }
        }
        Err(_) => {
            if accept_fqdn {
                address
            } else {
                panic!("FQDN non supported")
            }
        }
    };
    return format!("{}:{}", host, port);
}
