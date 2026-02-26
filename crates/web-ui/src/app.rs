use leptos::prelude::*;

use crate::components::config_panel::ConfigPanel;
use crate::components::job_info::JobInfo;
use crate::components::log_panel::LogPanel;
use crate::components::stats_display::StatsDisplay;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub time: String,
    pub message: String,
    pub level: LogLevel,
}

#[derive(Debug, Clone)]
pub enum LogLevel {
    Info,
    Success,
    Error,
    Warn,
}

/// Global mining state, provided via Leptos context.
#[derive(Clone, Copy)]
pub struct MiningState {
    // Connection
    pub connected: RwSignal<bool>,
    pub authorized: RwSignal<bool>,

    // Job
    pub job_id: RwSignal<String>,
    pub target_hex: RwSignal<String>,

    // Stats
    pub nonces_tried: RwSignal<u64>,
    pub solutions_found: RwSignal<u64>,
    pub shares_submitted: RwSignal<u64>,
    pub shares_accepted: RwSignal<u64>,
    pub shares_rejected: RwSignal<u64>,
    pub hashrate: RwSignal<f64>,

    // Control
    pub is_mining: RwSignal<bool>,

    // Log
    pub log_messages: RwSignal<Vec<LogEntry>>,

    // Config (stored so services can read)
    pub proxy_url: RwSignal<String>,
    pub pool_addr: RwSignal<String>,
    pub zcash_address: RwSignal<String>,
    pub worker_label: RwSignal<String>,
}

impl MiningState {
    pub fn new() -> Self {
        let rand_id = (js_sys::Math::random() * 90000.0) as u32 + 10000;
        Self {
            connected: RwSignal::new(false),
            authorized: RwSignal::new(false),
            job_id: RwSignal::new(String::new()),
            target_hex: RwSignal::new(String::new()),
            nonces_tried: RwSignal::new(0),
            solutions_found: RwSignal::new(0),
            shares_submitted: RwSignal::new(0),
            shares_accepted: RwSignal::new(0),
            shares_rejected: RwSignal::new(0),
            hashrate: RwSignal::new(0.0),
            is_mining: RwSignal::new(false),
            log_messages: RwSignal::new(Vec::new()),
            proxy_url: RwSignal::new("ws://pool.tazminer.com:9144".to_string()),
            pool_addr: RwSignal::new("pool.tazminer.com:3333".to_string()),
            zcash_address: RwSignal::new("tmRhAwek1qaG3bqy4W8nih9NQsycYrLuV4n".to_string()),
            worker_label: RwSignal::new(format!("wasmbrowser{}", rand_id)),
        }
    }

    /// Returns "address.worker" combined string for stratum protocol.
    pub fn worker_name(&self) -> String {
        let addr = self.zcash_address.get_untracked();
        let label = self.worker_label.get_untracked();
        if label.is_empty() {
            addr
        } else {
            format!("{}.{}", addr, label)
        }
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        let now = js_sys::Date::new_0();
        let time = format!(
            "{:02}:{:02}:{:02}",
            now.get_hours(),
            now.get_minutes(),
            now.get_seconds()
        );
        self.log_messages.update(|logs| {
            logs.push(LogEntry {
                time,
                message: message.into(),
                level,
            });
            // Keep last 200 entries
            if logs.len() > 200 {
                logs.drain(0..logs.len() - 200);
            }
        });
    }

    pub fn reset_stats(&self) {
        self.nonces_tried.set(0);
        self.solutions_found.set(0);
        self.shares_submitted.set(0);
        self.shares_accepted.set(0);
        self.shares_rejected.set(0);
        self.hashrate.set(0.0);
        self.job_id.set(String::new());
        self.target_hex.set(String::new());
    }
}

#[component]
pub fn App() -> impl IntoView {
    let state = MiningState::new();
    provide_context(state);

    view! {
        <h1>"WasmMiner Dashboard"</h1>
        <div class="dashboard">
            <ConfigPanel />
            <StatsDisplay />
            <JobInfo />
            <LogPanel />
        </div>
    }
}
