use wasm_bindgen::prelude::*;
use web_sys::Worker;

pub struct MinerWorker {
    worker: Worker,
}

impl MinerWorker {
    pub fn new() -> Result<Self, String> {
        let worker = Worker::new("./worker/worker.js")
            .map_err(|e| format!("Worker creation failed: {:?}", e))?;
        Ok(Self { worker })
    }

    pub fn init(&self) {
        let msg = js_sys::Object::new();
        js_sys::Reflect::set(&msg, &"type".into(), &"init".into()).unwrap();
        self.worker.post_message(&msg).unwrap();
    }

    pub fn start(&self, job_json: &str, start_counter: u64) {
        let msg = js_sys::Object::new();
        js_sys::Reflect::set(&msg, &"type".into(), &"start".into()).unwrap();
        js_sys::Reflect::set(&msg, &"job".into(), &job_json.into()).unwrap();
        js_sys::Reflect::set(
            &msg,
            &"startCounter".into(),
            &JsValue::from_f64(start_counter as f64),
        )
        .unwrap();
        self.worker.post_message(&msg).unwrap();
    }

    pub fn new_job(&self, job_json: &str, start_counter: u64) {
        let msg = js_sys::Object::new();
        js_sys::Reflect::set(&msg, &"type".into(), &"newjob".into()).unwrap();
        js_sys::Reflect::set(&msg, &"job".into(), &job_json.into()).unwrap();
        js_sys::Reflect::set(
            &msg,
            &"startCounter".into(),
            &JsValue::from_f64(start_counter as f64),
        )
        .unwrap();
        self.worker.post_message(&msg).unwrap();
    }

    pub fn stop(&self) {
        let msg = js_sys::Object::new();
        js_sys::Reflect::set(&msg, &"type".into(), &"stop".into()).unwrap();
        self.worker.post_message(&msg).unwrap();
    }

    pub fn terminate(&self) {
        self.worker.terminate();
    }

    pub fn set_onmessage(&self, cb: impl FnMut(web_sys::MessageEvent) + 'static) {
        let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut(web_sys::MessageEvent)>);
        self.worker
            .set_onmessage(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }
}
