use wasm_bindgen::prelude::*;
use web_sys::Worker;

pub struct WorkerPool {
    workers: Vec<Worker>,
}

impl WorkerPool {
    pub fn new(count: usize) -> Result<Self, String> {
        let mut workers = Vec::with_capacity(count);
        for _ in 0..count {
            let w = Worker::new("./worker/worker.js")
                .map_err(|e| format!("Worker creation failed: {:?}", e))?;
            workers.push(w);
        }
        Ok(Self { workers })
    }

    pub fn count(&self) -> usize {
        self.workers.len()
    }

    /// Send `{type: "init", workerId: i}` to each worker.
    pub fn init_all(&self) {
        for (i, w) in self.workers.iter().enumerate() {
            let msg = js_sys::Object::new();
            js_sys::Reflect::set(&msg, &"type".into(), &"init".into()).unwrap();
            js_sys::Reflect::set(&msg, &"workerId".into(), &JsValue::from_f64(i as f64))
                .unwrap();
            w.post_message(&msg).unwrap();
        }
    }

    /// Send a new job to all workers with strided counters.
    /// Worker i starts at counter `i`, increments by `N` (total workers).
    pub fn new_job_all(&self, job_json: &str, _base_counter: u64) {
        let n = self.workers.len() as u64;
        for (i, w) in self.workers.iter().enumerate() {
            let msg = js_sys::Object::new();
            js_sys::Reflect::set(&msg, &"type".into(), &"newjob".into()).unwrap();
            js_sys::Reflect::set(&msg, &"job".into(), &job_json.into()).unwrap();
            js_sys::Reflect::set(
                &msg,
                &"startCounter".into(),
                &JsValue::from_f64(i as f64),
            )
            .unwrap();
            js_sys::Reflect::set(&msg, &"stride".into(), &JsValue::from_f64(n as f64))
                .unwrap();
            w.post_message(&msg).unwrap();
        }
    }

    /// Send `stop` to all workers.
    pub fn stop_all(&self) {
        for w in &self.workers {
            let msg = js_sys::Object::new();
            js_sys::Reflect::set(&msg, &"type".into(), &"stop".into()).unwrap();
            w.post_message(&msg).unwrap();
        }
    }

    /// Terminate all workers.
    pub fn terminate_all(&self) {
        for w in &self.workers {
            w.terminate();
        }
    }

    /// Set the same onmessage callback on all workers.
    pub fn set_on_message(&self, cb: impl FnMut(web_sys::MessageEvent) + 'static) {
        let cb = std::rc::Rc::new(std::cell::RefCell::new(cb));
        for w in &self.workers {
            let cb = cb.clone();
            let closure =
                Closure::wrap(
                    Box::new(move |ev: web_sys::MessageEvent| {
                        (cb.borrow_mut())(ev);
                    }) as Box<dyn FnMut(web_sys::MessageEvent)>,
                );
            w.set_onmessage(Some(closure.as_ref().unchecked_ref()));
            closure.forget();
        }
    }
}
