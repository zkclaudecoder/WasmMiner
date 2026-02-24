use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use wasmminer_core::equihash_solver::Solver;
use wasmminer_core::stratum_utils::{hash_solution, meets_target};
use wasmminer_core::types::{JobParams, ShareCandidate, SolveResult};

thread_local! {
    static SOLVER: RefCell<Option<Solver>> = const { RefCell::new(None) };
}

/// Allocate the solver (~144MB). Call once from the worker.
#[wasm_bindgen]
pub fn init_solver() {
    SOLVER.with(|s| {
        *s.borrow_mut() = Some(Solver::new());
    });
}

/// Solve a single nonce for the given job.
///
/// `job_json` is a JSON-serialized `JobParams`.
/// `counter` is the nonce counter value.
///
/// Returns a JSON-serialized `SolveResult`.
#[wasm_bindgen]
pub fn solve_nonce(job_json: &str, counter: u64) -> String {
    let job: JobParams = match serde_json::from_str(job_json) {
        Ok(j) => j,
        Err(e) => {
            return serde_json::json!({"error": e.to_string()}).to_string();
        }
    };

    SOLVER.with(|s| {
        let mut solver_ref = s.borrow_mut();
        let solver = match solver_ref.as_mut() {
            Some(s) => s,
            None => {
                return serde_json::json!({"error": "Solver not initialized"}).to_string();
            }
        };

        // Build nonce
        let mut nonce = [0u8; 32];
        nonce[..job.nonce_1.len()].copy_from_slice(&job.nonce_1);
        let cb = counter.to_le_bytes();
        for i in 0..std::cmp::min(job.nonce_2_size, 8) {
            nonce[job.nonce_1.len() + i] = cb[i];
        }

        let solutions = solver.solve(&job.header, &nonce);
        let num_solutions = solutions.len();

        let mut shares = Vec::new();
        for solution in &solutions {
            let hash_bytes = hash_solution(&job.header, &nonce, solution);
            if meets_target(&hash_bytes, &job.target) {
                let nonce_2_hex = hex::encode(&nonce[job.nonce_1.len()..]);
                let solution_hex = hex::encode(solution);
                let hash_preview = hex::encode(
                    hash_bytes
                        .iter()
                        .rev()
                        .take(4)
                        .copied()
                        .collect::<Vec<u8>>(),
                );
                shares.push(ShareCandidate {
                    nonce_2_hex,
                    solution_hex,
                    hash_preview,
                });
            }
        }

        let result = SolveResult {
            nonce_counter: counter,
            num_solutions,
            shares,
        };

        serde_json::to_string(&result).unwrap_or_else(|e| {
            serde_json::json!({"error": e.to_string()}).to_string()
        })
    })
}
