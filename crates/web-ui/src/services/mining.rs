use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use wasmminer_core::stratum_utils::parse_target;
use wasmminer_core::types::{JobParams, SolveResult};

use crate::app::{LogLevel, MiningState};
use crate::services::websocket::WsConnection;
use crate::services::worker_pool::WorkerPool;

const DEFAULT_TARGET: &str =
    "2000000000000000000000000000000000000000000000000000000000000000";

/// Max reconnect window in seconds (5 minutes).
const RECONNECT_WINDOW_SECS: f64 = 300.0;
/// Delay between reconnect attempts in milliseconds.
const RECONNECT_DELAY_MS: u32 = 3000;

// Thread-local storage for the active mining session
thread_local! {
    static ACTIVE_SESSION: RefCell<Option<MiningSession>> = const { RefCell::new(None) };
}

struct MiningSession {
    ws: WsConnection,
    pool: WorkerPool,
    /// Whether the user explicitly requested stop (vs disconnect).
    user_stopped: Rc<RefCell<bool>>,
}

pub fn start_mining(state: MiningState) {
    // Clean up any existing session
    stop_mining(state);

    state.is_mining.set(true);
    state.reset_stats();
    state.workers_ready.set(0);

    let proxy_url = state.proxy_url.get_untracked();
    let pool_addr = state.pool_addr.get_untracked();
    let worker_name = state.worker_name();
    let thread_count = state.thread_count.get_untracked();
    let reconnect_start = Rc::new(RefCell::new(0.0_f64)); // set on first disconnect

    connect_session(state, proxy_url, pool_addr, worker_name, thread_count, reconnect_start);
}

fn connect_session(
    state: MiningState,
    proxy_url: String,
    pool_addr: String,
    worker_name: String,
    thread_count: usize,
    reconnect_start: Rc<RefCell<f64>>,
) {
    // Create WebSocket connection
    let ws = match WsConnection::new(&proxy_url) {
        Ok(ws) => ws,
        Err(e) => {
            state.log(LogLevel::Error, format!("Failed to create WebSocket: {}", e));
            state.is_mining.set(false);
            return;
        }
    };

    // Create Worker Pool
    let pool = match WorkerPool::new(thread_count) {
        Ok(p) => p,
        Err(e) => {
            state.log(LogLevel::Error, format!("Failed to create workers: {}", e));
            ws.close();
            state.is_mining.set(false);
            return;
        }
    };

    let total_workers = pool.count();
    let user_stopped = Rc::new(RefCell::new(false));

    // Shared state for callbacks
    let nonce_1_hex: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let current_target: Rc<RefCell<[u8; 32]>> = Rc::new(RefCell::new(
        parse_target(DEFAULT_TARGET).unwrap(),
    ));
    let current_job_json: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let mining_start_time: Rc<RefCell<f64>> = Rc::new(RefCell::new(0.0));
    let nonce_timestamps: Rc<RefCell<VecDeque<f64>>> = Rc::new(RefCell::new(VecDeque::new()));
    let submit_id: Rc<RefCell<u64>> = Rc::new(RefCell::new(10));

    // --- Worker message handler (shared across all workers) ---
    {
        let state = state;
        let current_job_json = current_job_json.clone();
        let mining_start_time = mining_start_time.clone();
        let nonce_timestamps = nonce_timestamps.clone();

        pool.set_on_message(move |ev: web_sys::MessageEvent| {
            let data = ev.data();
            let msg_type = js_sys::Reflect::get(&data, &"type".into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();

            let worker_id = js_sys::Reflect::get(&data, &"workerId".into())
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as usize);

            match msg_type.as_str() {
                "ready" => {
                    state.workers_ready.update(|n| *n += 1);
                    let ready = state.workers_ready.get_untracked();
                    let id = worker_id.unwrap_or(0);
                    state.log(
                        LogLevel::Success,
                        format!("Worker {} ready (~144MB) ({}/{})", id, ready, total_workers),
                    );
                }
                "result" => {
                    let result_json = js_sys::Reflect::get(&data, &"result".into())
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or_default();

                    if let Ok(result) = serde_json::from_str::<SolveResult>(&result_json) {
                        state.nonces_tried.update(|n| *n += 1);
                        state.solutions_found.update(|n| *n += result.num_solutions as u64);

                        // Update hashrate
                        let start = *mining_start_time.borrow();
                        let now = js_sys::Date::now() / 1000.0;
                        let elapsed = now - start;
                        if elapsed > 0.0 {
                            let nonces = state.nonces_tried.get_untracked();
                            state.hashrate.set(nonces as f64 / elapsed);
                        }

                        // Update 1-minute rolling hashrate
                        {
                            let mut ts = nonce_timestamps.borrow_mut();
                            ts.push_back(now);
                            while ts.front().map_or(false, |&t| t < now - 60.0) {
                                ts.pop_front();
                            }
                            state.hashrate_1m.set(ts.len() as f64 / 60.0);
                        }

                        // Submit shares via WebSocket
                        for share in &result.shares {
                            state.shares_submitted.update(|n| *n += 1);
                            let wid = worker_id.unwrap_or(0);
                            state.log(
                                LogLevel::Success,
                                format!("SHARE FOUND (worker {})! hash={}...", wid, share.hash_preview),
                            );

                            // Get current job JSON to extract job_id, time_hex, worker_name
                            if let Some(job_json) = current_job_json.borrow().as_ref() {
                                if let Ok(job) = serde_json::from_str::<JobParams>(job_json) {
                                    let mut id = submit_id.borrow_mut();
                                    let msg = serde_json::json!({
                                        "id": *id,
                                        "method": "mining.submit",
                                        "params": [
                                            job.worker_name,
                                            job.job_id,
                                            job.time_hex,
                                            share.nonce_2_hex,
                                            share.solution_hex
                                        ]
                                    });
                                    *id += 1;

                                    // Send via WS (get from thread-local)
                                    ACTIVE_SESSION.with(|session| {
                                        if let Some(s) = session.borrow().as_ref() {
                                            if let Err(e) = s.ws.send(&msg.to_string()) {
                                                state.log(LogLevel::Error, format!("Submit failed: {}", e));
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
                "error" => {
                    let message = js_sys::Reflect::get(&data, &"message".into())
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or("Unknown worker error".to_string());
                    let wid = worker_id.unwrap_or(0);
                    state.log(LogLevel::Error, format!("Worker {} error: {}", wid, message));
                }
                _ => {}
            }
        });
    }

    // --- WebSocket handlers ---
    {
        let pool_addr = pool_addr.clone();
        let state = state;
        let reconnect_start = reconnect_start.clone();

        ws.set_onopen(move || {
            // Reset reconnect timer on successful connection
            *reconnect_start.borrow_mut() = 0.0;

            state.log(LogLevel::Info, "WebSocket connected, sending pool address...");
            ACTIVE_SESSION.with(|session| {
                if let Some(s) = session.borrow().as_ref() {
                    let connect_msg = serde_json::json!({"pool": pool_addr}).to_string();
                    let _ = s.ws.send(&connect_msg);
                }
            });
        });
    }

    {
        let state = state;

        ws.set_onerror(move |_ev: web_sys::ErrorEvent| {
            state.log(LogLevel::Error, "WebSocket error");
        });
    }

    {
        let state = state;
        let user_stopped = user_stopped.clone();
        let proxy_url = proxy_url.clone();
        let pool_addr = pool_addr.clone();
        let worker_name = worker_name.clone();
        let reconnect_start = reconnect_start.clone();

        ws.set_onclose(move |_ev: web_sys::CloseEvent| {
            state.connected.set(false);

            // If user pressed stop, don't reconnect
            if *user_stopped.borrow() {
                return;
            }

            // Terminate the current workers (they can't do anything without WS)
            ACTIVE_SESSION.with(|session| {
                if let Some(s) = session.borrow_mut().take() {
                    s.pool.stop_all();
                    s.pool.terminate_all();
                }
            });

            let now = js_sys::Date::now() / 1000.0;
            let mut start = reconnect_start.borrow_mut();
            if *start == 0.0 {
                *start = now;
            }
            let elapsed = now - *start;

            if elapsed >= RECONNECT_WINDOW_SECS {
                state.log(LogLevel::Error, "Reconnect timeout (5 min). Stopping miner.");
                state.is_mining.set(false);
                state.authorized.set(false);
                return;
            }

            let remaining = (RECONNECT_WINDOW_SECS - elapsed).ceil() as u32;
            state.log(
                LogLevel::Warn,
                format!("Disconnected. Reconnecting in {}s ({} s remaining)...", RECONNECT_DELAY_MS / 1000, remaining),
            );

            // Schedule reconnect
            let proxy_url = proxy_url.clone();
            let pool_addr = pool_addr.clone();
            let worker_name = worker_name.clone();
            let reconnect_start = reconnect_start.clone();
            let cb = Closure::once(move || {
                // Check if user stopped while waiting
                let stopped = ACTIVE_SESSION.with(|session| {
                    session.borrow().as_ref().map_or(true, |s| *s.user_stopped.borrow())
                });
                // No active session means user stopped or we already cleaned up
                if stopped && !state.is_mining.get_untracked() {
                    return;
                }
                state.log(LogLevel::Info, "Attempting reconnect...");
                state.workers_ready.set(0);
                connect_session(state, proxy_url, pool_addr, worker_name, thread_count, reconnect_start);
            });
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    RECONNECT_DELAY_MS as i32,
                )
                .unwrap();
            cb.forget();
        });
    }

    // --- Main WS message handler (stratum state machine) ---
    {
        let state = state;
        let worker_name = worker_name.clone();
        let nonce_1_hex = nonce_1_hex.clone();
        let current_target = current_target.clone();
        let current_job_json = current_job_json.clone();
        let mining_start_time = mining_start_time.clone();

        ws.set_onmessage(move |ev: web_sys::MessageEvent| {
            let data = match ev.data().as_string() {
                Some(s) => s,
                None => return,
            };

            let msg: serde_json::Value = match serde_json::from_str(&data) {
                Ok(v) => v,
                Err(_) => return,
            };

            // Handle proxy connect confirmation
            if msg.get("connected") == Some(&serde_json::json!(true)) {
                state.connected.set(true);
                state.log(LogLevel::Success, format!("Connected to pool via proxy"));

                // Subscribe
                let subscribe = serde_json::json!({
                    "id": 1,
                    "method": "mining.subscribe",
                    "params": ["wasmminer-web/0.2.0", null, null, null]
                });
                ACTIVE_SESSION.with(|session| {
                    if let Some(s) = session.borrow().as_ref() {
                        let _ = s.ws.send(&subscribe.to_string());
                    }
                });
                return;
            }

            // Handle proxy error
            if msg.get("error").is_some() && msg.get("method").is_none() && msg.get("id").is_none() {
                let err = msg["error"].as_str().unwrap_or("Unknown proxy error");
                state.log(LogLevel::Error, format!("Proxy error: {}", err));
                return;
            }

            // Handle responses (have non-null id)
            if let Some(id) = msg.get("id") {
                if !id.is_null() {
                    if let Some(id_num) = id.as_u64() {
                        match id_num {
                            1 => {
                                // Subscribe response
                                if let Some(result) = msg.get("result").and_then(|r| r.as_array()) {
                                    let session_id = result[0].as_str().unwrap_or("");
                                    let n1 = result[1].as_str().unwrap_or("");
                                    *nonce_1_hex.borrow_mut() = n1.to_string();
                                    state.log(
                                        LogLevel::Success,
                                        format!("Subscribed (session={}, nonce_1={})", session_id, n1),
                                    );

                                    // Authorize
                                    let auth = serde_json::json!({
                                        "id": 2,
                                        "method": "mining.authorize",
                                        "params": [worker_name, "x"]
                                    });
                                    ACTIVE_SESSION.with(|session| {
                                        if let Some(s) = session.borrow().as_ref() {
                                            let _ = s.ws.send(&auth.to_string());
                                        }
                                    });
                                } else {
                                    state.log(LogLevel::Error, format!("Subscribe failed: {:?}", msg.get("error")));
                                }
                            }
                            2 => {
                                // Authorize response
                                if msg.get("result") == Some(&serde_json::json!(true)) {
                                    state.authorized.set(true);
                                    state.log(LogLevel::Success, "Worker authorized!");

                                    // Initialize all solver workers
                                    ACTIVE_SESSION.with(|session| {
                                        if let Some(s) = session.borrow().as_ref() {
                                            s.pool.init_all();
                                        }
                                    });
                                } else {
                                    state.log(LogLevel::Error, format!("Authorization FAILED: {:?}", msg.get("error")));
                                }
                            }
                            _ => {
                                // Share response
                                if msg.get("result") == Some(&serde_json::json!(true)) {
                                    state.shares_accepted.update(|n| *n += 1);
                                    state.log(LogLevel::Success, "Share ACCEPTED!");
                                } else if msg.get("error").is_some()
                                    && msg.get("error") != Some(&serde_json::Value::Null)
                                {
                                    state.shares_rejected.update(|n| *n += 1);
                                    state.log(
                                        LogLevel::Error,
                                        format!("Share REJECTED: {:?}", msg.get("error")),
                                    );
                                }
                            }
                        }
                    }
                    return;
                }
            }

            // Handle notifications (no id, have method)
            let method = match msg.get("method").and_then(|m| m.as_str()) {
                Some(m) => m.to_string(),
                None => return,
            };

            match method.as_str() {
                "mining.set_target" => {
                    if let Some(target_hex) = msg["params"][0].as_str() {
                        match parse_target(target_hex) {
                            Ok(t) => {
                                *current_target.borrow_mut() = t;
                                state.target_hex.set(target_hex.to_string());
                                let display = if target_hex.len() > 16 {
                                    format!("{}...", &target_hex[..16])
                                } else {
                                    target_hex.to_string()
                                };
                                state.log(LogLevel::Info, format!("Target updated: {}", display));
                            }
                            Err(e) => {
                                state.log(LogLevel::Error, format!("Bad target: {}", e));
                            }
                        }
                    }
                }
                "mining.notify" => {
                    let params = match msg["params"].as_array() {
                        Some(p) => p,
                        None => return,
                    };

                    let job_id = params[0].as_str().unwrap_or("").to_string();
                    let version = params[1].as_str().unwrap_or("");
                    let prev_hash = params[2].as_str().unwrap_or("");
                    let merkle_root = params[3].as_str().unwrap_or("");
                    let reserved = params[4].as_str().unwrap_or("");
                    let time_hex = params[5].as_str().unwrap_or("").to_string();
                    let bits = params[6].as_str().unwrap_or("");
                    let _clean = params[7].as_bool().unwrap_or(false);

                    // Build header
                    let mut header = Vec::with_capacity(108);
                    let parts = [version, prev_hash, merkle_root, reserved, &time_hex, bits];
                    for part in &parts {
                        match hex::decode(part) {
                            Ok(bytes) => header.extend_from_slice(&bytes),
                            Err(e) => {
                                state.log(LogLevel::Error, format!("Bad hex in job: {}", e));
                                return;
                            }
                        }
                    }

                    let n1_hex = nonce_1_hex.borrow().clone();
                    let nonce_1 = match hex::decode(&n1_hex) {
                        Ok(b) => b,
                        Err(e) => {
                            state.log(LogLevel::Error, format!("Bad nonce_1 hex: {}", e));
                            return;
                        }
                    };
                    let nonce_2_size = 32 - nonce_1.len();

                    state.job_id.set(job_id.clone());
                    state.log(LogLevel::Info, format!("New job: {}", job_id));

                    // Reset per-job stats
                    state.nonces_tried.set(0);
                    state.solutions_found.set(0);
                    state.hashrate.set(0.0);
                    *mining_start_time.borrow_mut() = js_sys::Date::now() / 1000.0;

                    let target = *current_target.borrow();

                    let job = JobParams {
                        job_id,
                        header,
                        nonce_1,
                        nonce_2_size,
                        target,
                        time_hex,
                        worker_name: worker_name.clone(),
                    };

                    let job_json = serde_json::to_string(&job).unwrap();
                    *current_job_json.borrow_mut() = Some(job_json.clone());

                    // Send job to all workers with strided counters
                    ACTIVE_SESSION.with(|session| {
                        if let Some(s) = session.borrow().as_ref() {
                            s.pool.new_job_all(&job_json, 0);
                        }
                    });
                }
                "client.reconnect" => {
                    state.log(LogLevel::Warn, "Server requested reconnect");
                }
                _ => {}
            }
        });
    }

    // Store the session
    ACTIVE_SESSION.with(|session| {
        *session.borrow_mut() = Some(MiningSession { ws, pool, user_stopped });
    });
}

pub fn stop_mining(state: MiningState) {
    ACTIVE_SESSION.with(|session| {
        if let Some(s) = session.borrow_mut().take() {
            // Signal user-initiated stop so onclose doesn't reconnect
            *s.user_stopped.borrow_mut() = true;
            s.pool.stop_all();
            s.pool.terminate_all();
            s.ws.close();
        }
    });
    state.is_mining.set(false);
    state.connected.set(false);
    state.authorized.set(false);
    state.workers_ready.set(0);
}
