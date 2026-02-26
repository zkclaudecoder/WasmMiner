use leptos::prelude::*;

use crate::app::{LogLevel, MiningState};
use crate::services::mining::start_mining;
use crate::services::mining::stop_mining;

#[component]
pub fn ConfigPanel() -> impl IntoView {
    let state = expect_context::<MiningState>();

    let on_start = move |_| {
        let proxy = state.proxy_url.get_untracked();
        let pool = state.pool_addr.get_untracked();
        let worker = state.worker_name.get_untracked();

        if pool.is_empty() || worker.is_empty() {
            state.log(LogLevel::Error, "Pool address and worker name are required");
            return;
        }

        state.log(LogLevel::Info, format!("Connecting to {} via {}", pool, proxy));
        start_mining(state);
    };

    let on_stop = move |_| {
        state.log(LogLevel::Warn, "Stopping miner...");
        stop_mining(state);
    };

    let is_mining = state.is_mining;
    let connected = state.connected;

    view! {
        <div class="panel">
            <h2>"Configuration"</h2>

            <div class="stat-row">
                <span class="stat-label">"Pool"</span>
                <span class="stat-value accent">{move || state.pool_addr.get()}</span>
            </div>

            <label>"Worker Address"</label>
            <div style="font-size: 0.7rem; color: var(--text-muted); margin-bottom: 0.35rem;">
                "Your Zcash address followed by a dot and any worker name, e.g. "
                <span style="color: var(--accent);">"t1abc...xyz.myworker"</span>
            </div>
            <input type="text"
                prop:value=move || state.worker_name.get()
                on:input=move |ev| {
                    state.worker_name.set(event_target_value(&ev));
                }
                placeholder="t1YourZcashAddress.workerName"
                disabled=move || is_mining.get()
            />

            <div style="margin-top: 0.5rem; font-size: 0.8rem;">
                <span class={move || if connected.get() { "status-dot connected" } else { "status-dot disconnected" }}></span>
                {move || if connected.get() { "Connected" } else { "Disconnected" }}
            </div>

            {move || {
                if is_mining.get() {
                    view! {
                        <button class="stop" on:click=on_stop>"Stop Mining"</button>
                    }.into_any()
                } else {
                    view! {
                        <button class="start" on:click=on_start>"Start Mining"</button>
                    }.into_any()
                }
            }}
        </div>
    }
}
