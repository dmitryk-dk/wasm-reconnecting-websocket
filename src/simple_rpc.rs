use core::sync::atomic;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use jsonrpc_core::{Call, Id, MethodCall, Output, Params, Response, Value, Version};
use serde_json::Map;

pub struct RPCResponse {
    pub(crate) id: Option<u64>,
    pub(crate) result: Value,
}

impl fmt::Display for RPCResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "response id: {:?}, response result: {}",
            self.id,
            self.result.to_string()
        )
    }
}

#[derive(Debug)]
pub struct RpcError {
    pub(crate) id: Option<u64>,
    pub(crate) msg: String,
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

pub type RPCHandler = Box<dyn Fn(String) + 'static>;

pub struct RPCSubscriber {
    id: Arc<AtomicUsize>,
    subscriber: HashMap<u64, RPCHandler>,
    error_subscriber: HashMap<u64, RPCHandler>,
}

impl RPCSubscriber {
    pub fn new() -> Self {
        Self {
            id: Arc::new(Default::default()),
            subscriber: HashMap::new(),
            error_subscriber: HashMap::new(),
        }
    }

    pub fn prepare_request(&self, method: &str, params: Params) -> (u64, Call) {
        let id = self.id.fetch_add(1, atomic::Ordering::AcqRel);
        let request = match params {
            Params::Map(val) => Self::build_map_request(id, method, val),
            Params::Array(val) => Self::build_vec_request(id, method, val),
            Params::None => Self::build_none_request(id, method),
        };
        (id as u64, request)
    }

    pub fn set_handler(&mut self, request_id: u64, handler: RPCHandler) {
        self.subscriber.insert(request_id, Box::new(handler));
    }

    pub fn set_error_handler(&mut self, request_id: u64, error_handler: RPCHandler) {
        self.error_subscriber.insert(request_id, error_handler);
    }

    pub fn get_handler(&mut self, request_id: u64) -> Option<&RPCHandler> {
        self.subscriber.get(&request_id)
    }

    pub fn get_error_handler(&mut self, request_id: u64) -> Option<&RPCHandler> {
        self.error_subscriber.get(&request_id)
    }

    pub fn get_response(json: String) -> Result<RPCResponse, RpcError> {
        let response = Response::from_json(json.as_str());
        match response {
            Ok(response) => match response {
                Response::Single(val) => match val {
                    Output::Failure(fail) => {
                        let id = match fail.id {
                            Id::Num(id) => Some(id),
                            Id::Str(str_id) => {
                                let id = str_id.parse::<u64>().unwrap();
                                Some(id)
                            }
                            Id::Null => None,
                        };
                        Err(RpcError {
                            id,
                            msg: fail.error.message,
                        })
                    }
                    Output::Success(success) => {
                        let id = match success.id {
                            Id::Num(id) => Some(id),
                            Id::Str(str_id) => {
                                let id = str_id.parse::<u64>().unwrap();
                                Some(id)
                            }
                            Id::Null => None,
                        };
                        Ok(RPCResponse {
                            id,
                            result: success.result,
                        })
                    }
                },
                _ => Err(RpcError {
                    id: None,
                    msg: String::from("this is batch response"),
                }),
            },
            Err(err) => Err(RpcError {
                id: None,
                msg: err.to_string(),
            }),
        }
    }

    fn build_map_request(id: usize, method: &str, params: Map<String, Value>) -> Call {
        Call::MethodCall(MethodCall {
            jsonrpc: Some(Version::V2),
            method: method.into(),
            params: Params::Map(params),
            id: Id::Num(id as u64),
        })
    }

    fn build_vec_request(id: usize, method: &str, params: Vec<Value>) -> Call {
        Call::MethodCall(MethodCall {
            jsonrpc: Some(Version::V2),
            method: method.into(),
            params: Params::Array(params),
            id: Id::Num(id as u64),
        })
    }

    fn build_none_request(id: usize, method: &str) -> Call {
        Call::MethodCall(MethodCall {
            jsonrpc: Some(Version::V2),
            method: method.into(),
            params: Params::None,
            id: Id::Num(id as u64),
        })
    }
}

impl Drop for RPCSubscriber {
    fn drop(&mut self) {
        self.subscriber.clear();
    }
}
