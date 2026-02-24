use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use wasmminer_core::equihash_solver::Solver;
use wasmminer_core::stratum_utils::{hash_solution, meets_target};

use crate::stratum_io::send_line;

#[allow(dead_code)]
pub struct MineJobParams {
    pub header: Vec<u8>,
    pub nonce_1: Vec<u8>,
    pub nonce_2_size: usize,
    pub generation: u64,
    pub gen_check: Arc<AtomicU64>,
    pub writer: Arc<Mutex<TcpStream>>,
    pub job_id: String,
    pub time_hex: String,
    pub worker_name: String,
    pub target: Arc<Mutex<[u8; 32]>>,
    pub num_threads: u64,
}

#[cfg(feature = "native")]
pub fn mine_job(params: MineJobParams) {
    let MineJobParams {
        header,
        nonce_1,
        nonce_2_size,
        generation,
        gen_check,
        writer,
        job_id,
        time_hex,
        worker_name,
        target,
        num_threads,
    } = params;

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
            let mut solver = Solver::new();
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

                let solutions = solver.solve(&header, &nonce);

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
                    let hash_bytes = hash_solution(&header, &nonce, solution);

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

#[cfg(not(feature = "native"))]
pub fn mine_job(params: MineJobParams) {
    let MineJobParams {
        header,
        nonce_1,
        nonce_2_size,
        generation,
        gen_check,
        writer,
        job_id,
        time_hex,
        worker_name,
        target,
        num_threads: _,
    } = params;

    let start = Instant::now();
    let mut nonces_tried: u64 = 0;
    let mut solutions_total: u64 = 0;
    let mut shares_submitted: u64 = 0;
    let mut solver = Solver::new();
    let mut counter: u64 = 0;

    loop {
        if gen_check.load(Ordering::Relaxed) != generation {
            break;
        }

        let mut nonce = [0u8; 32];
        nonce[..nonce_1.len()].copy_from_slice(&nonce_1);
        let cb = counter.to_le_bytes();
        for i in 0..std::cmp::min(nonce_2_size, 8) {
            nonce[nonce_1.len() + i] = cb[i];
        }

        let solutions = solver.solve(&header, &nonce);

        nonces_tried += 1;
        solutions_total += solutions.len() as u64;

        let elapsed = start.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 {
            nonces_tried as f64 / elapsed
        } else {
            0.0
        };
        eprint!(
            "\r    [job {}] nonces: {} ({:.2}/s) | sols: {} | shares: {} | {:.0}s    ",
            job_id, nonces_tried, rate, solutions_total, shares_submitted, elapsed
        );

        let current_target = *target.lock().unwrap();

        for solution in &solutions {
            let hash_bytes = hash_solution(&header, &nonce, solution);

            if meets_target(&hash_bytes, &current_target) {
                shares_submitted += 1;
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

        counter += 1;
    }

    eprintln!(
        "\n    Job {} done: {} nonces, {} sols, {} shares ({:.1}s)",
        job_id, nonces_tried, solutions_total, shares_submitted,
        start.elapsed().as_secs_f64()
    );
}
