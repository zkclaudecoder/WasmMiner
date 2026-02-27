use leptos::prelude::*;

use crate::app::{LogLevel, MiningState};
use crate::services::mining::start_mining;
use crate::services::mining::stop_mining;

#[component]
pub fn ConfigPanel() -> impl IntoView {
    let state = expect_context::<MiningState>();

    let addr_warning = Memo::new(move |_| {
        let addr = state.zcash_address.get();
        if addr.is_empty() {
            return Some("Zcash address is required".to_string());
        }
        if addr.starts_with("u1") || addr.starts_with("utest") {
            if addr.len() < 20 {
                return Some("Unified address looks too short".to_string());
            }
            const BECH32: &[u8] = b"023456789acdefghjklmnpqrstuvwxyz";
            let payload = if addr.starts_with("utest") { &addr[5..] } else { &addr[2..] };
            if let Some(c) = payload.chars().find(|c| !BECH32.contains(&(*c as u8))) {
                return Some(format!("Invalid character '{}' in unified address", c));
            }
            return None;
        }
        if addr.starts_with("t1") || addr.starts_with("t3") || addr.starts_with("tm") {
            if addr.len() < 33 || addr.len() > 36 {
                return Some(format!("Address length {} looks wrong (expected ~35)", addr.len()));
            }
            const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            if let Some(c) = addr.chars().find(|c| !BASE58.contains(&(*c as u8))) {
                return Some(format!("Invalid character '{}' in address", c));
            }
            return None;
        }
        Some("Address should start with t1, t3, tm, u1, or utest".to_string())
    });

    let on_start = move |_| {
        let addr = state.zcash_address.get_untracked();

        if addr.is_empty() {
            state.log(LogLevel::Error, "Zcash address is required");
            return;
        }

        let worker = state.worker_name();
        let pool = state.pool_addr.get_untracked();
        state.log(LogLevel::Info, format!("Connecting to {} as {}", pool, worker));
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

            <label>"Zcash Address"</label>
            <div style="font-size: 0.7rem; color: var(--text-muted); margin-bottom: 0.35rem;">
                "Payouts are sent to this address"
            </div>
            <input type="text"
                prop:value=move || state.zcash_address.get()
                on:input=move |ev| {
                    state.zcash_address.set(event_target_value(&ev));
                }
                placeholder="t1YourZcashAddress"
                disabled=move || is_mining.get()
            />
            {move || addr_warning.get().map(|msg| view! {
                <div style="font-size: 0.7rem; color: var(--yellow); margin-top: 0.25rem;">{msg}</div>
            })}

            <label>"Worker Name"</label>
            <div style="font-size: 0.7rem; color: var(--text-muted); margin-bottom: 0.35rem;">
                "A label to identify this miner (e.g. laptop, desktop)"
            </div>
            <input type="text"
                prop:value=move || state.worker_label.get()
                on:input=move |ev| {
                    state.worker_label.set(event_target_value(&ev));
                }
                placeholder="myworker"
                disabled=move || is_mining.get()
            />

            <label>"Threads"</label>
            <div style="display: flex; align-items: center; gap: 0.5rem; margin-top: 0.25rem;">
                <button
                    class="thread-btn"
                    on:click=move |_| {
                        let v = state.thread_count.get_untracked();
                        if v > 1 { state.thread_count.set(v - 1); }
                    }
                    disabled=move || is_mining.get() || (state.thread_count.get() <= 1)
                >"-"</button>
                <span style="font-size: 1rem; font-weight: bold; min-width: 1.5rem; text-align: center;">
                    {move || state.thread_count.get()}
                </span>
                <button
                    class="thread-btn"
                    on:click=move |_| {
                        let v = state.thread_count.get_untracked();
                        if v < 16 { state.thread_count.set(v + 1); }
                    }
                    disabled=move || is_mining.get() || (state.thread_count.get() >= 16)
                >"+"</button>
                <span style="font-size: 0.7rem; color: var(--text-muted);">
                    {move || {
                        let n = state.thread_count.get();
                        if n == 1 {
                            "144MB".to_string()
                        } else {
                            format!("{} x 144MB = {}MB", n, n * 144)
                        }
                    }}
                </span>
            </div>

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
