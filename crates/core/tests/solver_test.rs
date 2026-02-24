use wasmminer_core::equihash_compress::indices_from_minimal;
use wasmminer_core::equihash_solver::Solver;

/// Run with: cargo test --release -p wasmminer-core -- --nocapture
#[test]
fn solver_produces_valid_solutions() {
    let input = b"Equihash is an asymmetric PoW based on the Generalised Birthday problem.";

    let mut solver = Solver::new();
    let mut total_solutions = 0;

    for nonce_val in 0u32..=32 {
        let mut nonce = [0u8; 32];
        let nonce_bytes = nonce_val.to_le_bytes();
        nonce[..4].copy_from_slice(&nonce_bytes);

        let solutions = solver.solve(input, &nonce);
        eprintln!(
            "Nonce {}: {} solutions",
            nonce_val,
            solutions.len()
        );

        for (i, solution) in solutions.iter().enumerate() {
            assert_eq!(
                solution.len(),
                1344,
                "Solution should be 1344 bytes (compressed equihash 200,9)"
            );

            equihash::is_valid_solution(200, 9, input, &nonce, solution).unwrap_or_else(|e| {
                let indices = indices_from_minimal(solution).unwrap();
                panic!(
                    "Solution {} for nonce {} failed verification: {:?}\nfirst 8 indices: {:?}",
                    i,
                    nonce_val,
                    e,
                    &indices[..8]
                );
            });
        }

        total_solutions += solutions.len();
    }

    eprintln!(
        "\nTotal: {} solutions from 33 nonces ({:.2} per nonce)",
        total_solutions,
        total_solutions as f64 / 33.0
    );

    assert!(
        total_solutions > 10,
        "Expected at least 10 solutions from 33 nonces, got {}",
        total_solutions
    );
}
