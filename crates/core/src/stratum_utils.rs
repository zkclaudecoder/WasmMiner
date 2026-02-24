use sha2::{Digest, Sha256};

pub fn parse_target(hex_str: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = hex::decode(hex_str)?;
    if bytes.len() > 32 {
        anyhow::bail!("Target too long");
    }
    let mut arr = [0u8; 32];
    let offset = 32 - bytes.len();
    arr[offset..].copy_from_slice(&bytes);
    Ok(arr)
}

pub fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
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

pub fn compact_size(n: usize) -> Vec<u8> {
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

pub fn hash_solution(header: &[u8], nonce: &[u8], solution: &[u8]) -> [u8; 32] {
    let mut full_header = Vec::with_capacity(header.len() + nonce.len() + solution.len() + 3);
    full_header.extend_from_slice(header);
    full_header.extend_from_slice(nonce);
    let mut solution_with_prefix = compact_size(solution.len());
    solution_with_prefix.extend_from_slice(solution);
    full_header.extend_from_slice(&solution_with_prefix);

    let first = Sha256::digest(&full_header);
    let second = Sha256::digest(first);
    second.into()
}
