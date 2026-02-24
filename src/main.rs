use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sha2::{Digest, Sha256};

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
    let num_threads: u64 = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as u64)
                .unwrap_or(4)
        });

    eprintln!("=== Zcash CPU Miner (equihash 200,9) ===");
    eprintln!("Pool:    {}", pool_addr);
    eprintln!("Worker:  {}", worker_name);
    eprintln!("Threads: {}", num_threads);
    eprintln!();

    eprintln!("Connecting to pool...");
    let stream = TcpStream::connect(pool_addr)?;
    eprintln!("Connected!");

    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let mut reader = BufReader::new(stream);

    // --- Subscribe ---
    send_line(
        &writer,
        &serde_json::json!({
            "id": 1,
            "method": "mining.subscribe",
            "params": ["zcash-cpu-miner/0.1.0", null, null, null]
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
                std::thread::spawn(move || {
                    mine_job(
                        header,
                        nonce_1,
                        nonce_2_size,
                        gen,
                        gen_check,
                        writer,
                        job_id,
                        time_hex,
                        worker,
                        target,
                        threads,
                    );
                });
            }
            "client.reconnect" => {
                eprintln!("Server requested reconnect");
            }
            _ => {}
        }
    }
}

fn mine_job(
    header: Vec<u8>,
    nonce_1: Vec<u8>,
    nonce_2_size: usize,
    generation: u64,
    gen_check: Arc<AtomicU64>,
    writer: Arc<Mutex<TcpStream>>,
    job_id: String,
    time_hex: String,
    worker_name: String,
    target: Arc<Mutex<[u8; 32]>>,
    num_threads: u64,
) {
    let start = Instant::now();
    let nonces_tried = Arc::new(AtomicU64::new(0));
    let solutions_total = Arc::new(AtomicU64::new(0));
    let shares_submitted = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    for thread_id in 0..num_threads {
        let header = header.clone();
        let nonce_1 = nonce_1.clone();
        let gen_check = gen_check.clone();
        let writer = writer.clone();
        let job_id = job_id.clone();
        let time_hex = time_hex.clone();
        let worker_name = worker_name.clone();
        let nonces_tried = nonces_tried.clone();
        let solutions_total = solutions_total.clone();
        let shares_submitted = shares_submitted.clone();
        let target = target.clone();

        handles.push(std::thread::spawn(move || {
            let mut counter = thread_id;
            loop {
                if gen_check.load(Ordering::Relaxed) != generation {
                    return;
                }

                let mut nonce = [0u8; 32];
                nonce[..nonce_1.len()].copy_from_slice(&nonce_1);
                let cb = counter.to_le_bytes();
                for i in 0..std::cmp::min(nonce_2_size, 8) {
                    nonce[nonce_1.len() + i] = cb[i];
                }

                let mut used = false;
                let nonce_copy = nonce;
                let solutions = equihash::tromp::solve_200_9(&header, || {
                    if used {
                        return None;
                    }
                    used = true;
                    Some(nonce_copy)
                });

                let n = nonces_tried.fetch_add(1, Ordering::Relaxed) + 1;
                solutions_total.fetch_add(solutions.len() as u64, Ordering::Relaxed);

                if thread_id == 0 {
                    let elapsed = start.elapsed().as_secs_f64();
                    let rate = if elapsed > 0.0 { n as f64 / elapsed } else { 0.0 };
                    let sols = solutions_total.load(Ordering::Relaxed);
                    let shares = shares_submitted.load(Ordering::Relaxed);
                    eprint!(
                        "\r    [job {}] nonces: {} ({:.2}/s) | sols: {} | shares: {} | {:.0}s    ",
                        job_id, n, rate, sols, shares, elapsed
                    );
                }

                let current_target = *target.lock().unwrap();

                for solution in &solutions {
                    let mut full_header = header.clone();
                    full_header.extend_from_slice(&nonce);
                    let mut solution_with_prefix = compact_size(solution.len());
                    solution_with_prefix.extend_from_slice(solution);
                    full_header.extend_from_slice(&solution_with_prefix);

                    let first = Sha256::digest(&full_header);
                    let second = Sha256::digest(first);
                    let hash_bytes: [u8; 32] = second.into();

                    if meets_target(&hash_bytes, &current_target) {
                        shares_submitted.fetch_add(1, Ordering::Relaxed);
                        let nonce_2_hex = hex::encode(&nonce[nonce_1.len()..]);
                        let solution_hex = hex::encode(solution);

                        eprintln!(
                            "\n    SHARE FOUND! hash={}...",
                            &hex::encode(
                                hash_bytes
                                    .iter()
                                    .rev()
                                    .take(4)
                                    .copied()
                                    .collect::<Vec<u8>>()
                            )
                        );

                        let msg = serde_json::json!({
                            "id": 4,
                            "method": "mining.submit",
                            "params": [worker_name, job_id, time_hex, nonce_2_hex, solution_hex]
                        })
                        .to_string();

                        if let Err(e) = send_line(&writer, &msg) {
                            eprintln!("    Submit failed: {}", e);
                        }
                    }
                }

                counter += num_threads;
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    let n = nonces_tried.load(Ordering::Relaxed);
    let sols = solutions_total.load(Ordering::Relaxed);
    let shares = shares_submitted.load(Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(
        "\n    Job {} done: {} nonces, {} sols, {} shares ({:.1}s)",
        job_id, n, sols, shares, elapsed
    );
}

fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    for i in 0..32 {
        let h = hash[31 - i];
        if h < target[i] {
            return true;
        } else if h > target[i] {
            return false;
        }
    }
    true
}

fn compact_size(n: usize) -> Vec<u8> {
    let n = n as u64;
    if n < 253 {
        vec![n as u8]
    } else if n <= 0xFFFF {
        let mut v = vec![0xFD];
        v.extend_from_slice(&(n as u16).to_le_bytes());
        v
    } else {
        let mut v = vec![0xFE];
        v.extend_from_slice(&(n as u32).to_le_bytes());
        v
    }
}

fn parse_target(hex_str: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = hex::decode(hex_str)?;
    if bytes.len() > 32 {
        anyhow::bail!("Target too long");
    }
    let mut arr = [0u8; 32];
    let offset = 32 - bytes.len();
    arr[offset..].copy_from_slice(&bytes);
    Ok(arr)
}

fn send_line(writer: &Arc<Mutex<TcpStream>>, msg: &str) -> anyhow::Result<()> {
    let mut w = writer
        .lock()
        .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
    writeln!(w, "{}", msg)?;
    w.flush()?;
    Ok(())
}

fn read_json(reader: &mut BufReader<TcpStream>) -> anyhow::Result<serde_json::Value> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.is_empty() {
        anyhow::bail!("Connection closed by pool");
    }
    Ok(serde_json::from_str(line.trim())?)
}
