use std::collections::HashMap;
use std::fmt;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

pub enum Payload {
    Data(String),
    MessageEvent(MessageEvent),
    CloseEvent(CloseEvent),
    ErrorEvent(ErrorEvent),
}

impl fmt::Display for Payload {
    // This trait requires `fmt` with this exact signature.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Write strictly the first element into the supplied output
        // stream: `f`. Returns `fmt::Result` which indicates whether the
        // operation succeeded or failed. Note that `write!` uses syntax which
        // is very similar to `println!`.
        match self {
            Payload::Data(val) => write!(f, "{}", val),
            Payload::MessageEvent(msg_evt) => write!(f, "{:?}", msg_evt),
            Payload::CloseEvent(close_evt) => write!(f, "{:?}", close_evt),
            Payload::ErrorEvent(err_evt) => write!(f, "{:?}", err_evt),
        }
    }
}

pub type Callback = Box<dyn Fn(&Payload) + 'static>;

pub struct Emitter {
    handlers: HashMap<String, Callback>,
}

impl Emitter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn on(&mut self, handler_name: String, handler: Callback) {
        self.handlers.insert(handler_name, handler);
    }

    pub fn off(&mut self, handler_name: String) {
        self.handlers.remove(&handler_name);
    }

    pub fn emit(&self, handler_name: String, payload: &Payload) {
        match self.handlers.get(&handler_name) {
            Some(handler) => {
                handler(payload);
            }
            None => console_log!("handler with name: {} not found", handler_name),
        }
    }

    pub fn get_handlers_names(&mut self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}
