use leptos::prelude::*;

use crate::app::{LogLevel, MiningState};

#[component]
pub fn LogPanel() -> impl IntoView {
    let state = expect_context::<MiningState>();

    view! {
        <div class="panel full-width">
            <h2>"Event Log"</h2>
            <div class="log-panel" id="log-panel">
                <For
                    each=move || {
                        let logs = state.log_messages.get();
                        logs.into_iter().rev().enumerate().collect::<Vec<_>>()
                    }
                    key=|(_i, entry)| format!("{}-{}", entry.time, entry.message)
                    children=move |(_i, entry)| {
                        let class = match entry.level {
                            LogLevel::Info => "log-info",
                            LogLevel::Success => "log-success",
                            LogLevel::Error => "log-error",
                            LogLevel::Warn => "log-warn",
                        };
                        view! {
                            <div>
                                <span class="log-time">{entry.time.clone()}</span>
                                <span class={class}>{entry.message.clone()}</span>
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}
