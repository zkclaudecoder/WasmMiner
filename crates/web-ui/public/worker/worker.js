// Web Worker for equihash solving
// Loads wasm-pack output (no-modules target)

let initialized = false;
let running = false;
let currentJob = null;
let counter = 0n; // BigInt — wasm-bindgen u64 requires BigInt
let pendingJob = null; // Queue job if received before init completes
let workerId = 0;
let stride = 1n; // BigInt — increment per solve loop iteration

// Load the WASM module
importScripts('./wasmminer_web_worker.js');

async function initWasm() {
    await wasm_bindgen('./wasmminer_web_worker_bg.wasm');
    wasm_bindgen.init_solver();
    initialized = true;
    postMessage({ type: 'ready', workerId: workerId });

    // If a job was queued before init finished, start it now
    if (pendingJob) {
        currentJob = pendingJob.job;
        counter = BigInt(pendingJob.startCounter || 0);
        stride = BigInt(pendingJob.stride || 1);
        pendingJob = null;
        running = true;
        solveLoop();
    }
}

function solveLoop() {
    if (!running || !currentJob || !initialized) return;

    try {
        const resultJson = wasm_bindgen.solve_nonce(currentJob, counter);
        postMessage({ type: 'result', workerId: workerId, counter: Number(counter), result: resultJson });
        counter += stride;
    } catch (err) {
        postMessage({ type: 'error', workerId: workerId, message: 'solve_nonce failed: ' + err.toString() });
        running = false;
        return;
    }

    // Yield to allow message processing (stop/newjob)
    setTimeout(solveLoop, 0);
}

self.onmessage = function(e) {
    const msg = e.data;

    switch (msg.type) {
        case 'init':
            if (msg.workerId !== undefined) {
                workerId = msg.workerId;
            }
            initWasm().catch(err => {
                postMessage({ type: 'error', workerId: workerId, message: 'WASM init failed: ' + err.toString() });
            });
            break;

        case 'start':
            if (!initialized) {
                pendingJob = { job: msg.job, startCounter: msg.startCounter || 0, stride: msg.stride || 1 };
                return;
            }
            currentJob = msg.job;
            counter = BigInt(msg.startCounter || 0);
            stride = BigInt(msg.stride || 1);
            running = true;
            solveLoop();
            break;

        case 'newjob':
            if (!initialized) {
                pendingJob = { job: msg.job, startCounter: msg.startCounter || 0, stride: msg.stride || 1 };
                return;
            }
            currentJob = msg.job;
            counter = BigInt(msg.startCounter || 0);
            stride = BigInt(msg.stride || 1);
            // If already running, the loop will pick up the new job
            if (!running) {
                running = true;
                solveLoop();
            }
            break;

        case 'stop':
            running = false;
            currentJob = null;
            pendingJob = null;
            break;
    }
};
