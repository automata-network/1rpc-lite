use std::collections::BTreeMap;
use std::prelude::v1::*;

use std::collections::HashMap;
use std::time::Duration;

use base::trace::Alive;
use forwarder::{
    sanitizer::SanitizedRequest, JsonrpcForwardContext, JsonrpcForwardRequest,
    JsonrpcForwarderConfig, JsonrpcForwarderWsHandler, JsonrpcRequestMgr, JsonrpcResponseMgr,
};
use jsonrpc::Batchable;
use net_http::{
    HttpWsServerContext, TickResult, WsDataType, WsError, WsServerConns, WsStreamClient,
    WsStreamConfig,
};

use crate::one_rpc::OneRpcRouter;

// Demo
#[derive(Default)]
pub struct DemoWsHandler {
    token_to_conn: HashMap<String, usize>, // token -> conn_id
    conn_to_token: HashMap<usize, String>, // conn_id -> token
}

impl JsonrpcForwarderWsHandler for DemoWsHandler {
    fn on_connection_open(
        &mut self,
        _: Alive,
        _: &forwarder::JsonrpcForwarderConfig,
        ctx: &mut HttpWsServerContext,
        ws_responses: &mut JsonrpcResponseMgr,
    ) {
        if !ctx.path.starts_with("/ws/") {
            ws_responses.add_raw_closed(ctx.conn_id, b"Bad path".to_vec());
            return;
        }

        let token = match extract_token(ctx.path) {
            Some(v) => v.to_owned(),
            _ => {
                ws_responses.add_raw_closed(ctx.conn_id, b"No token".to_vec());
                return;
            }
        };

        if let Some(prev_conn) = self.token_to_conn.insert(token.clone(), ctx.conn_id) {
            glog::warn!("ws_conn[token={}]: {} -> {}", token, prev_conn, ctx.conn_id);
        }
        self.conn_to_token.insert(ctx.conn_id, token.clone());
        glog::debug!("ws_conn[token={}]: +{}", token, ctx.conn_id);
    }

    fn on_connection_close(&mut self, conn_id: usize) {
        if let Some(token) = self.conn_to_token.remove(&conn_id) {
            self.token_to_conn.remove(&token);
            glog::debug!("ws_conn[token={}]: -{}", token, conn_id);
        }
    }

    fn witness_request(
        &mut self,
        ctx: JsonrpcForwardContext,
        req: &JsonrpcForwardRequest,
        ws_responses: &mut JsonrpcResponseMgr,
    ) {
        if let Some(token) = ctx.token {
            if let Some(conn_id) = self.token_to_conn.get(token) {
                let raw = serde_json::to_vec(&req.sr.tr).unwrap();
                ws_responses.add_raw(*conn_id, raw);
            } else {
                glog::warn!("ws_conn[token={}]: nil", token);
            }
        }
    }

    fn process_request(
        &mut self,
        _: &mut HttpWsServerContext,
        _: WsDataType,
        _: Vec<u8>,
        _: &mut JsonrpcResponseMgr,
    ) {
    }
}

// "/ws/{token}"
fn extract_token(path: &str) -> Option<&str> {
    let mut it = path.split("/");
    it.nth(2)
}

// Relay
pub struct RelayWsHandlerConfig {
    pub ws_frame_size: usize,
    pub ws_keep_alive: Option<Duration>,
    pub ws_max_body_length: Option<usize>,
}

pub struct RelayWsHandler {
    cfg: RelayWsHandlerConfig,
    router: OneRpcRouter,
    remote_ws_conns: BTreeMap<usize, (String, WsStreamClient)>, // conn_id(local) -> (rpc_path, ws_stream)
    ws_reqs: JsonrpcRequestMgr,
}

impl RelayWsHandler {
    pub fn new(cfg: RelayWsHandlerConfig, router: OneRpcRouter) -> Self {
        Self {
            cfg,
            router,
            remote_ws_conns: BTreeMap::new(),
            ws_reqs: JsonrpcRequestMgr::new(),
        }
    }
}

impl JsonrpcForwarderWsHandler for RelayWsHandler {
    fn on_connection_open(
        &mut self,
        alive: Alive,
        cfg: &JsonrpcForwarderConfig,
        ctx: &mut HttpWsServerContext,
        ws_responses: &mut JsonrpcResponseMgr,
    ) {
        let rpc_path = match extract_rpc_path(ctx.path) {
            Some(v) => v,
            _ => {
                ws_responses.add_single_error_msg_closed(ctx.conn_id, -32600, "No path specified");
                return;
            }
        };
        let remote_uri = match self.router.get_route(&rpc_path) {
            Some(v) => v,
            _ => {
                ws_responses.add_single_error_msg_closed(ctx.conn_id, -32600, "Unknown path");
                return;
            }
        };
        glog::debug!("ws_conn: +{}", ctx.conn_id);

        let ws_cfg = WsStreamConfig {
            endpoint: remote_uri,
            frame_size: cfg.ws_frame_size,
            keep_alive: cfg.ws_keep_alive,
            auto_reconnect: false,
            alive: alive.clone(),
        };
        let ws_stream = match WsStreamClient::new(ws_cfg) {
            Ok(v) => v,
            Err(e) => {
                glog::error!("ws connect error: {:?}", e);
                ws_responses.add_single_error_msg_closed(
                    ctx.conn_id,
                    -32600,
                    "Failed to connect remote client",
                );
                return;
            }
        };
        self.remote_ws_conns
            .insert(ctx.conn_id, (rpc_path.into(), ws_stream));
        glog::debug!("remote_ws_conn: +{}", ctx.conn_id);
    }

    fn on_connection_close(&mut self, conn_id: usize) {
        glog::debug!("ws_conn: -{}", conn_id);
        self.remote_ws_conns.remove(&conn_id);
        glog::debug!("remote_ws_conn: -{}", conn_id);
    }

    fn witness_request(
        &mut self,
        _: JsonrpcForwardContext,
        _: &JsonrpcForwardRequest,
        _: &mut JsonrpcResponseMgr,
    ) {
    }

    fn process_request(
        &mut self,
        ctx: &mut HttpWsServerContext,
        _: WsDataType,
        data: Vec<u8>,
        ws_responses: &mut JsonrpcResponseMgr,
    ) {
        let (rpc_path, ws) = match self.remote_ws_conns.get(&ctx.conn_id) {
            Some(v) => v,
            _ => {
                glog::error!("remote_ws_conn: {} -> nil", ctx.conn_id);
                ctx.is_close = true;
                return;
            }
        };

        if data.len() > self.cfg.ws_max_body_length.unwrap_or(usize::MAX) {
            ws_responses.add_single_error_msg_closed(
                ctx.conn_id,
                -32600,
                "JSON RPC Request is too large",
            );
            return;
        }

        let req_body = match Batchable::parse(&data) {
            Ok(v) => v,
            Err(_) => {
                ws_responses.add_single_error_msg_closed(ctx.conn_id, -32600, "Parse error");
                return;
            }
        };

        let req = JsonrpcForwardRequest {
            conn_id: ctx.conn_id,
            rpc_path: rpc_path.clone(),
            remote_uri: ws.cfg.endpoint.clone(),
            sr: SanitizedRequest::new(req_body),
            last_send: None,
        };
        self.ws_reqs.push(req);
    }

    fn tick_ws_reqs(&mut self, tick: &mut TickResult, ws_conns: &mut WsServerConns) {
        let mut remove_req = vec![];
        let mut close_remote = vec![];
        for (req_id, req) in self.ws_reqs.iter() {
            let conn_id = req.conn_id;
            match self.remote_ws_conns.get_mut(&conn_id) {
                Some((_, remote_conn)) => {
                    let buf = req.build_ws_request();
                    tick.to_busy();
                    match remote_conn.write_ty(WsDataType::Text, &buf) {
                        Ok(_) => {
                            remove_req.push(*req_id);
                        }
                        Err(WsError::WouldBlock) => continue,
                        Err(e) => {
                            glog::error!("ws_stream write error: {:?}", e);
                            ws_conns.remove(conn_id);
                            // remote conn broken
                            close_remote.push(conn_id);
                        }
                    }
                }
                _ => {
                    glog::error!("remote_ws_conn req({}).conn: {} -> nil", req_id, conn_id);
                    remove_req.push(*req_id);
                    ws_conns.remove(conn_id);
                }
            }
        }
        for req_id in remove_req {
            self.ws_reqs.pop(&req_id);
        }
        for conn_id in close_remote {
            self.remote_ws_conns.remove(&conn_id);
        }
    }

    fn tick_ws_recv_remote(&mut self, tick: &mut TickResult, ws_conns: &mut WsServerConns) {
        let mut close_conn = vec![];
        let mut data = vec![];
        for (conn_id, (_, remote_conn)) in &mut self.remote_ws_conns {
            tick.to_busy();
            match remote_conn.read(&mut data) {
                Ok(ty) => match ws_conns.get_mut(*conn_id) {
                    Some(local_conn) => match local_conn.write_ty(ty, &data) {
                        Ok(_) => {}
                        Err(e) => {
                            glog::error!("ws_conn[{}] write error: {:?}", conn_id, e);
                        }
                    },
                    _ => {
                        glog::error!("ws_conn: {} -> nil", conn_id);
                        close_conn.push(*conn_id);
                    }
                },
                Err(WsError::WouldBlock) => continue,
                Err(e) => {
                    glog::error!("remote_ws_conn[{}] read error: {:?}", conn_id, e);
                    close_conn.push(*conn_id);
                }
            }
        }
        for conn_id in close_conn {
            self.remote_ws_conns.remove(&conn_id);
            ws_conns.remove(conn_id);
        }
    }
}

fn extract_rpc_path(path: &str) -> Option<&str> {
    let mut it = path.split("/");
    it.nth(1)
}
