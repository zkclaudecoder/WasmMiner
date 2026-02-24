use serde::{Deserialize, Serialize};

/// Parameters for a mining job, shared between CLI and browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobParams {
    pub job_id: String,
    pub header: Vec<u8>,
    pub nonce_1: Vec<u8>,
    pub nonce_2_size: usize,
    pub target: [u8; 32],
    pub time_hex: String,
    pub worker_name: String,
}

/// Result from solving a single nonce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveResult {
    pub nonce_counter: u64,
    pub num_solutions: usize,
    pub shares: Vec<ShareCandidate>,
}

/// A solution that meets the current target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareCandidate {
    pub nonce_2_hex: String,
    pub solution_hex: String,
    pub hash_preview: String,
}
