use leptos::prelude::*;

use crate::app::MiningState;

#[component]
pub fn StatsDisplay() -> impl IntoView {
    let state = expect_context::<MiningState>();

    view! {
        <div class="panel">
            <h2>"Mining Stats"</h2>

            <div class="stat-row">
                <span class="stat-label">"Status"</span>
                <span class={move || {
                    if state.is_mining.get() { "stat-value green" } else { "stat-value red" }
                }}>
                    {move || if state.is_mining.get() {
                        view! { <><span class="status-dot mining"></span>"Mining"</> }.into_any()
                    } else {
                        view! { <>"Idle"</> }.into_any()
                    }}
                </span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Hashrate"</span>
                <span class="stat-value accent">
                    {move || format!("{:.2} nonces/s", state.hashrate.get())}
                </span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Hashrate (1m)"</span>
                <span class="stat-value accent">
                    {move || format!("{:.2} nonces/s", state.hashrate_1m.get())}
                </span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Nonces Tried"</span>
                <span class="stat-value">{move || state.nonces_tried.get().to_string()}</span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Solutions Found"</span>
                <span class="stat-value">{move || state.solutions_found.get().to_string()}</span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Shares Submitted"</span>
                <span class="stat-value yellow">{move || state.shares_submitted.get().to_string()}</span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Accepted"</span>
                <span class="stat-value green">{move || state.shares_accepted.get().to_string()}</span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Rejected"</span>
                <span class="stat-value red">{move || state.shares_rejected.get().to_string()}</span>
            </div>
        </div>
    }
}
