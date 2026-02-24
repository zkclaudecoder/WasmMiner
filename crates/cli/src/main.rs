mod miner;
mod stratum_io;

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use miner::{mine_job, MineJobParams};
use stratum_io::{read_json, send_line};
use wasmminer_core::stratum_utils::parse_target;

const DEFAULT_TARGET: &str =
    "2000000000000000000000000000000000000000000000000000000000000000";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: zcash-cpu-miner <pool_host:port> <worker_name> [threads]");
        eprintln!("Example: zcash-cpu-miner 127.0.0.1:3333 t1YourAddress.worker1 4");
        std::process::exit(1);
    }
    let pool_addr = &args[1];
    let worker_name = &args[2];

    #[cfg(feature = "native")]
    let num_threads: u64 = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as u64)
                .unwrap_or(4)
        });

    #[cfg(not(feature = "native"))]
    let num_threads: u64 = 1;

    eprintln!("=== Zcash CPU Miner (equihash 200,9) ===");
    eprintln!("Pool:    {}", pool_addr);
    eprintln!("Worker:  {}", worker_name);
    eprintln!("Threads: {}", num_threads);
    eprintln!();

    eprintln!("Connecting to pool...");
    let stream = TcpStream::connect(pool_addr)?;
    eprintln!("Connected!");

    // Native: clone the stream so reader and writer are independent.
    // WASM: try_clone() isn't supported in WASI, so share via Arc<Mutex>
    // with a wrapper that locks the mutex to read.
    #[cfg(feature = "native")]
    let (writer, mut reader) = {
        let writer = Arc::new(Mutex::new(stream.try_clone()?));
        let reader = BufReader::new(stream);
        (writer, Box::new(reader) as Box<dyn BufRead>)
    };

    #[cfg(not(feature = "native"))]
    let (writer, mut reader) = {
        let writer = Arc::new(Mutex::new(stream));
        struct MutexReader(Arc<Mutex<TcpStream>>);
        impl std::io::Read for MutexReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().read(buf)
            }
        }
        let reader = BufReader::new(MutexReader(writer.clone()));
        (writer, Box::new(reader) as Box<dyn BufRead>)
    };

    // --- Subscribe ---
    send_line(
        &writer,
        &serde_json::json!({
            "id": 1,
            "method": "mining.subscribe",
            "params": ["zcash-cpu-miner/0.2.0", null, null, null]
        })
        .to_string(),
    )?;

    let nonce_1_hex;
    loop {
        let msg = read_json(&mut reader)?;
        if msg.get("id") == Some(&serde_json::json!(1)) {
            if let Some(result) = msg.get("result").and_then(|r| r.as_array()) {
                let session_id = result[0].as_str().unwrap_or("");
                nonce_1_hex = result[1].as_str().unwrap_or("").to_string();
                eprintln!(
                    "Subscribed (session={}, nonce_1={})",
                    session_id, nonce_1_hex
                );
                break;
            } else {
                anyhow::bail!("Subscribe failed: {:?}", msg.get("error"));
            }
        }
    }

    // --- Authorize ---
    send_line(
        &writer,
        &serde_json::json!({
            "id": 2,
            "method": "mining.authorize",
            "params": [worker_name, "x"]
        })
        .to_string(),
    )?;

    let generation = Arc::new(AtomicU64::new(0));
    let is_mining = Arc::new(AtomicBool::new(false));
    let target = Arc::new(Mutex::new(parse_target(DEFAULT_TARGET)?));

    // --- Main message loop ---
    loop {
        let msg = read_json(&mut reader)?;

        if let Some(id) = msg.get("id") {
            if !id.is_null() {
                if let Some(id_num) = id.as_u64() {
                    match id_num {
                        2 => {
                            if msg.get("result") == Some(&serde_json::json!(true)) {
                                eprintln!("Worker authorized!");
                            } else {
                                eprintln!("Authorization FAILED: {:?}", msg.get("error"));
                            }
                        }
                        _ => {
                            if msg.get("result") == Some(&serde_json::json!(true)) {
                                eprintln!("Share ACCEPTED!");
                            } else if msg.get("error").is_some()
                                && msg.get("error") != Some(&serde_json::Value::Null)
                            {
                                eprintln!("Share REJECTED: {:?}", msg.get("error"));
                            }
                        }
                    }
                }
                continue;
            }
        }

        let method = match msg.get("method").and_then(|m| m.as_str()) {
            Some(m) => m.to_string(),
            None => continue,
        };

        match method.as_str() {
            "mining.set_target" => {
                if let Some(target_hex) = msg["params"][0].as_str() {
                    match parse_target(target_hex) {
                        Ok(t) => {
                            *target.lock().unwrap() = t;
                            let display_len = std::cmp::min(16, target_hex.len());
                            eprintln!("Target updated: {}...", &target_hex[..display_len]);
                        }
                        Err(e) => eprintln!("Bad target: {}", e),
                    }
                }
            }
            "mining.notify" => {
                let params = match msg["params"].as_array() {
                    Some(p) => p,
                    None => continue,
                };
                let job_id = params[0].as_str().unwrap_or("").to_string();
                let version = params[1].as_str().unwrap_or("").to_string();
                let prev_hash = params[2].as_str().unwrap_or("").to_string();
                let merkle_root = params[3].as_str().unwrap_or("").to_string();
                let reserved = params[4].as_str().unwrap_or("").to_string();
                let time_hex = params[5].as_str().unwrap_or("").to_string();
                let bits = params[6].as_str().unwrap_or("").to_string();
                let clean = params[7].as_bool().unwrap_or(false);

                let currently_mining = is_mining.load(Ordering::Relaxed);

                if currently_mining && !clean {
                    continue;
                }

                if currently_mining && clean {
                    eprintln!();
                    eprintln!(">>> New block! Restarting for job {}", job_id);
                    generation.fetch_add(1, Ordering::SeqCst);
                } else {
                    eprintln!();
                    eprintln!(">>> Starting job: {}", job_id);
                }

                let mut header = Vec::with_capacity(108);
                header.extend_from_slice(&hex::decode(&version)?);
                header.extend_from_slice(&hex::decode(&prev_hash)?);
                header.extend_from_slice(&hex::decode(&merkle_root)?);
                header.extend_from_slice(&hex::decode(&reserved)?);
                header.extend_from_slice(&hex::decode(&time_hex)?);
                header.extend_from_slice(&hex::decode(&bits)?);

                let gen = generation.load(Ordering::SeqCst);
                let gen_check = generation.clone();
                let nonce_1 = hex::decode(&nonce_1_hex)?;
                let nonce_2_size = 32 - nonce_1.len();
                let writer = writer.clone();
                let worker = worker_name.to_string();
                let target = target.clone();

                is_mining.store(true, Ordering::SeqCst);

                let threads = num_threads;

                #[cfg(feature = "native")]
                std::thread::spawn(move || {
                    mine_job(MineJobParams {
                        header,
                        nonce_1,
                        nonce_2_size,
                        generation: gen,
                        gen_check,
                        writer,
                        job_id,
                        time_hex,
                        worker_name: worker,
                        target,
                        num_threads: threads,
                    });
                });

                #[cfg(not(feature = "native"))]
                mine_job(MineJobParams {
                    header,
                    nonce_1,
                    nonce_2_size,
                    generation: gen,
                    gen_check,
                    writer,
                    job_id,
                    time_hex,
                    worker_name: worker,
                    target,
                    num_threads: threads,
                });
            }
            "client.reconnect" => {
                eprintln!("Server requested reconnect");
            }
            _ => {}
        }
    }
}
