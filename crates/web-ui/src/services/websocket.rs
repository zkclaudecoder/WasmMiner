use wasm_bindgen::prelude::*;
use web_sys::WebSocket;

pub struct WsConnection {
    pub ws: WebSocket,
}

impl WsConnection {
    pub fn new(url: &str) -> Result<Self, String> {
        let ws = WebSocket::new(url).map_err(|e| format!("WebSocket creation failed: {:?}", e))?;
        Ok(Self { ws })
    }

    pub fn send(&self, msg: &str) -> Result<(), String> {
        self.ws
            .send_with_str(msg)
            .map_err(|e| format!("WebSocket send failed: {:?}", e))
    }

    pub fn close(&self) {
        let _ = self.ws.close();
    }

    pub fn set_onmessage(&self, cb: impl FnMut(web_sys::MessageEvent) + 'static) {
        let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut(web_sys::MessageEvent)>);
        self.ws.set_onmessage(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    pub fn set_onopen(&self, cb: impl FnMut() + 'static) {
        let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut()>);
        self.ws.set_onopen(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    pub fn set_onerror(&self, cb: impl FnMut(web_sys::ErrorEvent) + 'static) {
        let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut(web_sys::ErrorEvent)>);
        self.ws.set_onerror(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    pub fn set_onclose(&self, cb: impl FnMut(web_sys::CloseEvent) + 'static) {
        let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut(web_sys::CloseEvent)>);
        self.ws.set_onclose(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }
}
