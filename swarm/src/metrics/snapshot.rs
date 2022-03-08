// use std::sync::atomic::Ordering::SeqCst;
// use std::sync::atomic::{AtomicPtr, AtomicUsize};
use std::time::SystemTime;

pub struct SnapShot {
    total: i64,
    rate: f64,
    last_update_time: SystemTime,
}

impl Default for SnapShot {
    fn default() -> Self {
        SnapShot::new()
    }
}

impl SnapShot {
    pub fn new() -> SnapShot {
        SnapShot {
            total: 0,
            rate: 0.0,
            last_update_time: SystemTime::now(),
        }
    }

    pub fn set_total(&mut self, total: i64) {
        self.total = total;
    }
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
    }
    pub fn set_last_update_time(&mut self, last_update_time: SystemTime) {
        self.last_update_time = last_update_time;
    }

    pub fn total(&self) -> i64 {
        self.total
    }
    pub fn rate(&self) -> f64 {
        self.rate
    }
    pub fn last_update_time(&self) -> SystemTime {
        self.last_update_time
    }
}
