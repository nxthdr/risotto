use std::sync::Arc;
use tokio::sync::Mutex;

pub type AsyncStatistics = Arc<Mutex<ProcessorStatistics>>;

pub fn new_statistics() -> AsyncStatistics {
    Arc::new(Mutex::new(ProcessorStatistics::default()))
}

pub struct ProcessorStatistics {
    pub rx_bmp_messages: usize,
    pub rx_bmp_initiation: usize,
    pub rx_bmp_peer_up: usize,
    pub rx_bmp_route_monitoring: usize,
    pub rx_bmp_route_mirroring: usize,
    pub rx_bmp_peer_down: usize,
    pub rx_bmp_termination: usize,
    pub rx_bmp_stats_report: usize,
}

impl Default for ProcessorStatistics {
    fn default() -> Self {
        Self {
            rx_bmp_messages: 0,
            rx_bmp_initiation: 0,
            rx_bmp_peer_up: 0,
            rx_bmp_route_monitoring: 0,
            rx_bmp_route_mirroring: 0,
            rx_bmp_peer_down: 0,
            rx_bmp_termination: 0,
            rx_bmp_stats_report: 0,
        }
    }
}
