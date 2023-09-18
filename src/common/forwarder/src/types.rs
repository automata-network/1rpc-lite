use std::prelude::v1::*;

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::time::Instant;

use jsonrpc::{
    Batchable, JsonrpcErrorObj, JsonrpcErrorResponse, JsonrpcRawResponseFull,
    JsonrpcResponseRawResult,
};
use net_http::{HttpMethod, HttpRequestBuilder, Uri};

use crate::sanitizer::{SanitizedRequest, Transform};

// req
pub struct JsonrpcForwardContext<'a> {
    pub token: Option<&'a str>,
}

pub struct JsonrpcForwardRequest {
    pub conn_id: usize,
    pub rpc_path: String,
    pub remote_uri: Uri,
    pub sr: SanitizedRequest,
    pub last_send: Option<Instant>,
}

impl JsonrpcForwardRequest {
    pub fn build_http_request(&self) -> HttpRequestBuilder {
        let SanitizedRequest {
            original_ids: _,
            req_body,
            tr,
        } = &self.sr;
        let mut header_override = None;
        for v in tr {
            match v {
                Transform::Metadata {
                    protected,
                    unprotected: _,
                } => {
                    header_override = Some(("User-Agent".to_owned(), protected.ua.clone()));
                    break;
                }
                _ => continue,
            }
        }
        let body = serde_json::to_vec(req_body).unwrap();
        HttpRequestBuilder::new_ex(self.remote_uri.clone(), Some(body), |req| {
            req.method(HttpMethod::Post);
            req.header("Content-Type", "application/json");
            req.header("Connection", "keep-alive");
            if let Some((k, v)) = header_override {
                req.header(&k, &v);
            }
        })
    }

    pub fn build_ws_request(&self) -> Vec<u8> {
        serde_json::to_vec(&self.sr.req_body).unwrap()
    }
}

pub struct JsonrpcRequestMgr {
    req_id: usize,
    reqs: BTreeMap<usize, JsonrpcForwardRequest>,
}

impl JsonrpcRequestMgr {
    pub fn new() -> Self {
        Self {
            req_id: 0,
            reqs: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, req: JsonrpcForwardRequest) {
        self.reqs.insert(self.req_id, req);
        self.req_id = self.req_id.wrapping_add(1);
    }

    pub fn pop(&mut self, req_id: &usize) -> Option<JsonrpcForwardRequest> {
        self.reqs.remove(req_id)
    }
}

impl Deref for JsonrpcRequestMgr {
    type Target = BTreeMap<usize, JsonrpcForwardRequest>;

    fn deref(&self) -> &Self::Target {
        &self.reqs
    }
}

impl DerefMut for JsonrpcRequestMgr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reqs
    }
}

// resp
pub enum ResponseBody {
    Jsonrpc(Vec<Batchable<JsonrpcRawResponseFull>>),
    Raw(Vec<u8>),
}

pub struct ResponseAndClose {
    pub resp: ResponseBody,
    pub close: bool,
}

impl ResponseAndClose {
    pub fn new(resp: ResponseBody, close: bool) -> Self {
        Self { resp, close }
    }
}

pub struct JsonrpcResponseMgr(BTreeMap<usize, ResponseAndClose>);
impl JsonrpcResponseMgr {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add_raw(&mut self, conn_id: usize, raw: Vec<u8>) {
        match self.0.get_mut(&conn_id) {
            Some(v) => v.resp = ResponseBody::Raw(raw),
            _ => {
                self.0.insert(
                    conn_id,
                    ResponseAndClose::new(ResponseBody::Raw(raw), false),
                );
            }
        }
    }

    pub fn add_raw_closed(&mut self, conn_id: usize, raw: Vec<u8>) {
        self.0
            .insert(conn_id, ResponseAndClose::new(ResponseBody::Raw(raw), true));
    }

    pub fn add_single_error_msg_closed(&mut self, conn_id: usize, code: i64, msg: &str) {
        let error = JsonrpcErrorObj::error(code, msg.to_owned());
        self.add_single_error_closed(
            conn_id,
            JsonrpcErrorResponse {
                jsonrpc: "2.0".to_owned(),
                error,
                id: None,
            },
        )
    }

    fn add_single_error_closed(&mut self, conn_id: usize, err: JsonrpcErrorResponse) {
        self.add_jsonrpc(
            conn_id,
            Batchable::Single(JsonrpcResponseRawResult::Err(err).to_full()),
            true,
        );
    }

    fn add_jsonrpc(
        &mut self,
        conn_id: usize,
        response: Batchable<JsonrpcRawResponseFull>,
        to_close: bool,
    ) {
        match self.0.get_mut(&conn_id) {
            Some(ResponseAndClose { resp, close }) => {
                match resp {
                    ResponseBody::Raw(_) => {}
                    ResponseBody::Jsonrpc(list) => {
                        list.push(response);
                    }
                }
                if to_close {
                    *close = true;
                }
            }
            _ => {
                self.0.insert(
                    conn_id,
                    ResponseAndClose::new(ResponseBody::Jsonrpc(vec![response]), to_close),
                );
            }
        }
    }

    pub fn to_close(&mut self, conn_id: &usize) {
        if let Some(ResponseAndClose { resp: _, close }) = self.0.get_mut(conn_id) {
            *close = true;
        }
    }
}

impl Deref for JsonrpcResponseMgr {
    type Target = BTreeMap<usize, ResponseAndClose>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for JsonrpcResponseMgr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
