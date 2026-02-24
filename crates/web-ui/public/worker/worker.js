// Web Worker for equihash solving
// Loads wasm-pack output (no-modules target)

let initialized = false;
let running = false;
let currentJob = null;
let counter = 0;

// Load the WASM module
importScripts('./wasmminer_web_worker.js');

async function initWasm() {
    await wasm_bindgen('./wasmminer_web_worker_bg.wasm');
    wasm_bindgen.init_solver();
    initialized = true;
    postMessage({ type: 'ready' });
}

function solveLoop() {
    if (!running || !currentJob) return;

    const resultJson = wasm_bindgen.solve_nonce(currentJob, counter);
    postMessage({ type: 'result', counter: counter, result: resultJson });
    counter++;

    // Yield to allow message processing (stop/newjob)
    setTimeout(solveLoop, 0);
}

self.onmessage = function(e) {
    const msg = e.data;

    switch (msg.type) {
        case 'init':
            initWasm().catch(err => {
                postMessage({ type: 'error', message: 'WASM init failed: ' + err.toString() });
            });
            break;

        case 'start':
            if (!initialized) {
                postMessage({ type: 'error', message: 'Not initialized' });
                return;
            }
            currentJob = msg.job;
            counter = msg.startCounter || 0;
            running = true;
            solveLoop();
            break;

        case 'newjob':
            currentJob = msg.job;
            counter = msg.startCounter || 0;
            // If already running, the loop will pick up the new job
            if (!running) {
                running = true;
                solveLoop();
            }
            break;

        case 'stop':
            running = false;
            currentJob = null;
            break;
    }
};
