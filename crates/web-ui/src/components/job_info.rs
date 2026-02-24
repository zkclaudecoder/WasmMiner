use leptos::prelude::*;

use crate::app::MiningState;

#[component]
pub fn JobInfo() -> impl IntoView {
    let state = expect_context::<MiningState>();

    view! {
        <div class="panel">
            <h2>"Current Job"</h2>

            <div class="stat-row">
                <span class="stat-label">"Job ID"</span>
                <span class="stat-value">
                    {move || {
                        let id = state.job_id.get();
                        if id.is_empty() { "---".to_string() } else { id }
                    }}
                </span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Target"</span>
                <span class="stat-value" style="font-size: 0.75rem;">
                    {move || {
                        let t = state.target_hex.get();
                        if t.is_empty() {
                            "---".to_string()
                        } else if t.len() > 24 {
                            format!("{}...", &t[..24])
                        } else {
                            t
                        }
                    }}
                </span>
            </div>

            <div class="stat-row">
                <span class="stat-label">"Authorized"</span>
                <span class={move || {
                    if state.authorized.get() { "stat-value green" } else { "stat-value red" }
                }}>
                    {move || if state.authorized.get() { "Yes" } else { "No" }}
                </span>
            </div>
        </div>
    }
}
