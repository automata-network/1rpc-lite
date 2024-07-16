use std::prelude::v1::*;
use std::time::Duration;
use std::{ops::DerefMut, time::Instant};

use base::trace::Alive;
use jsonrpc::{Batchable, JsonrpcErrorObj, JsonrpcRawResponseFull, JsonrpcResponseRawResult};
use net_http::{
    HttpConnError, HttpRequestReader, HttpServerConns, HttpServerContext, HttpWsServer,
    HttpWsServerConfig, HttpWsServerContext, HttpWsServerHandler, TickResult, Uri, WsDataType,
    WsError, WsServerConns,
};

use crate::types::{ResponseAndClose, ResponseBody};
use crate::{client, utils};
use crate::{
    client::HttpForwardClient,
    sanitizer,
    types::{JsonrpcForwardContext, JsonrpcForwardRequest, JsonrpcRequestMgr, JsonrpcResponseMgr},
    ForwarderError,
};

pub struct JsonrpcForwarderConfig {
    pub listen_addr: String,
    pub tls_cert: Vec<u8>,
    pub tls_key: Vec<u8>,

    pub http_max_body_length: Option<usize>,
    pub ws_frame_size: usize,
    pub ws_keep_alive: Option<Duration>,
    pub ws_max_body_length: Option<usize>,
}

pub trait JsonrpcForwarderHandler {
    // routes
    fn get_http_uri(&self, key: &str) -> Option<Uri>;
    fn get_ws_uri(&self, key: &str) -> Option<Uri>;

    // hooks
    fn on_http_request(&mut self, ctx: JsonrpcForwardContext, req: &JsonrpcForwardRequest);
    fn on_new_ws_conn(
        &mut self,
        alive: Alive,
        cfg: &JsonrpcForwarderConfig,
        ctx: &mut HttpWsServerContext,
    );
    fn on_ws_request(&mut self, ctx: &mut HttpWsServerContext, ty: WsDataType, data: Vec<u8>);
    fn on_close_ws_conn(&mut self, conn_id: usize);

    // ticks
    fn tick_ws(&mut self, tick: &mut TickResult, ws_conns: &mut WsServerConns);
}

pub trait JsonrpcForwarderWsHandler {
    fn on_connection_open(
        &mut self,
        alive: Alive,
        cfg: &JsonrpcForwarderConfig,
        ctx: &mut HttpWsServerContext,
        ws_responses: &mut JsonrpcResponseMgr,
    );

    fn on_connection_close(&mut self, conn_id: usize);

    // dispatch request
    fn witness_request(
        &mut self,
        ctx: JsonrpcForwardContext,
        req: &JsonrpcForwardRequest,
        ws_responses: &mut JsonrpcResponseMgr,
    );
    fn process_request(
        &mut self,
        ctx: &mut HttpWsServerContext,
        ty: WsDataType,
        data: Vec<u8>,
        ws_responses: &mut JsonrpcResponseMgr,
    );

    fn tick_ws_reqs(&mut self, _tick: &mut TickResult, _ws_conns: &mut WsServerConns) {}
    fn tick_ws_recv_remote(&mut self, _tick: &mut TickResult, _ws_conns: &mut WsServerConns) {}

    fn tick_send_response(ws_responses: &mut JsonrpcResponseMgr, ws_conns: &mut WsServerConns) {
        let mut fin = vec![];
        let mut close_conn = vec![];
        for (conn_id, ResponseAndClose { resp, close }) in ws_responses.iter() {
            let data = match resp {
                ResponseBody::Jsonrpc(results) => {
                    if results.len() > 0 {
                        let all = Batchable::to_batch(results);
                        serde_json::to_vec(&all).unwrap()
                    } else {
                        vec![]
                    }
                }
                ResponseBody::Raw(raw) => raw.clone(),
            };

            match ws_conns.get_mut(*conn_id) {
                Some(conn) => match conn.write_ty(WsDataType::Text, &data) {
                    Ok(_) => {
                        fin.push(*conn_id);
                    }
                    Err(WsError::WouldBlock) => continue,
                    Err(e) => {
                        glog::error!("ws_conn[{}] write error: {:?}", conn_id, e);
                        ws_conns.remove(*conn_id);
                    }
                },
                _ => {}
            }

            if *close {
                close_conn.push(*conn_id); // close at end of loop, it's safe coz write is blocking
            }
        }
        for conn_id in fin {
            ws_responses.remove(&conn_id);
        }
        for conn_id in close_conn {
            ws_conns.remove(conn_id);
        }
    }
}

pub struct JsonrpcForwarder<H: JsonrpcForwarderHandler> {
    server: HttpWsServer<ServerHandler<H>>,
}

impl<H: JsonrpcForwarderHandler> JsonrpcForwarder<H> {
    pub fn new(
        cfg: JsonrpcForwarderConfig,
        handler: H,
        alive: Alive,
    ) -> Result<Self, ForwarderError> {
        let server = {
            let server_cfg = HttpWsServerConfig {
                listen_addr: cfg.listen_addr.clone(),
                tls_cert: cfg.tls_cert.clone(),
                tls_key: cfg.tls_key.clone(),
                frame_size: cfg.ws_frame_size.clone(),
                http_max_body_length: cfg.http_max_body_length.clone(),
                max_idle_secs: None,
            };

            let srv_handler = ServerHandler {
                alive,
                cfg,
                handler,
                http_reqs: JsonrpcRequestMgr::new(),
                http_responses: JsonrpcResponseMgr::new(),
                http_client: client::HttpForwardClient::new(),
            };
            HttpWsServer::new(server_cfg, srv_handler)
                .map_err(|err| ForwarderError::ListenError(err))
        }?;
        Ok(Self { server })
    }

    pub fn tick(&mut self) -> TickResult {
        self.server.tick()
    }
}

struct ServerHandler<H: JsonrpcForwarderHandler> {
    alive: Alive, // fork to use
    cfg: JsonrpcForwarderConfig,
    handler: H,
    http_reqs: JsonrpcRequestMgr,
    http_responses: JsonrpcResponseMgr,
    http_client: HttpForwardClient,
}

impl<H: JsonrpcForwarderHandler> ServerHandler<H> {
    // todo: tick_*
    //
    // tick_http_req: H::on_http_jsonrpc_req =>
    // ws_handler_demo: ws.send { 1. metadata.ip_mask; 2. (if any) acct_rel.untie }
    // ws_handler_relay: nothing, we do jobs when new_ws_conn/req
    //
    // on_new_ws_conn => dispatch to one of ws_handlers
    // on_new_ws_req => abb
    //
    // on_close_ws_conn => abb, but ws_handler_relay to close remote

    fn tick_http_reqs(&mut self, tick: &mut TickResult, http_conns: &mut HttpServerConns) {
        let mut remove_req = vec![];
        let mut close_conn = vec![];
        for (req_id, req) in self.http_reqs.deref_mut().iter_mut() {
            let key = req.rpc_path.clone();
            match req.last_send {
                Some(send) => {
                    let e = send.elapsed();
                    if e > Duration::from_secs(20) {
                        glog::error!(
                            "[{}] request timeout={:?}: req={}, conn={}",
                            key,
                            e,
                            req_id,
                            req.conn_id
                        );
                        remove_req.push(*req_id);
                        close_conn.push(req.conn_id);
                    }
                }
                _ => {
                    let client = match self.http_client.get_or_new(key, &req.remote_uri) {
                        Ok(v) => v,
                        Err(e) => {
                            glog::error!("get http_client fail: {:?}", e);
                            continue;
                        }
                    };
                    tick.to_busy();
                    // mark send time earlier the better, and `get_or_new` is not likely to fail
                    req.last_send = Some(Instant::now());
                    let mut http_req = req.build_http_request();
                    match client.write_request(*req_id, &mut http_req) {
                        Ok(_) => {}
                        Err(HttpConnError::WouldBlock) => continue,
                        Err(e) => {
                            glog::error!("http_client write error: {:?}", e);
                            continue;
                        }
                    }
                }
            }
        }
        for req_id in remove_req {
            self.http_reqs.pop(&req_id);
        }
        for conn_id in close_conn {
            http_conns.remove_conn(conn_id);
        }
    }

    fn tick_http_recv_remote(&mut self, tick: &mut TickResult, http_conns: &mut HttpServerConns) {
        for (key, conn_pool) in self.http_client.deref_mut().iter_mut() {
            loop {
                let (req_id, http_response) = match conn_pool.read_response() {
                    Ok(v) => v,
                    Err(HttpConnError::WouldBlock) => break,
                    Err(e) => {
                        glog::error!("http_client_conn[{}] read error: {:?}", key, e);
                        // client will be re-built, but all reqs sent via this one get lost
                        // close all conns of such reqs is most straightforward, OR we can
                        // retry reqs with some timeout at `tick_http_reqs`, and finally
                        // close if it fails, too
                        break;
                    }
                };

                match self.http_reqs.pop(&req_id) {
                    Some(req) => {
                        tick.to_busy();

                        let response_full = match Batchable::parse(&http_response.body) {
                            Ok(bat) => {
                                let rewritten = req.sr.rewrite_response(bat);
                                rewritten.map(|res| match res {
                                    JsonrpcResponseRawResult::Ok(v) => JsonrpcRawResponseFull {
                                        jsonrpc: v.jsonrpc,
                                        result: Some(v.result),
                                        error: None,
                                        id: Some(v.id),
                                    },
                                    JsonrpcResponseRawResult::Err(v) => JsonrpcRawResponseFull {
                                        jsonrpc: v.jsonrpc,
                                        result: None,
                                        error: Some(v.error),
                                        id: v.id,
                                    },
                                })
                            }
                            Err(e) => {
                                let err = JsonrpcErrorObj::error(-32700, e.to_string());
                                let err_resp = JsonrpcRawResponseFull::err(err, None);
                                Batchable::Single(err_resp)
                            }
                        };
                        let body = serde_json::to_vec(&response_full).unwrap();
                        let data = utils::create_http_jsonrpc_plain_response(body);
                        if let Err(e) = http_conns.write_to(req.conn_id, &data) {
                            glog::error!("http_conn[{}] write error: {:?}", req.conn_id, e);
                            http_conns.remove_conn(req.conn_id);
                        }
                        // mark close after response
                        http_conns.close_conn(req.conn_id);
                    }
                    _ => {
                        glog::error!("req[{}] not found in http_reqs", req_id);
                    }
                }
            }
        }
    }

    // relay results is sent in `tick_http_recv_remote .. write_to()`
    // this one deals with `http_responses` generated by ourselves
    fn tick_http_send_response(
        &mut self,
        _tick: &mut TickResult,
        http_conns: &mut HttpServerConns,
    ) {
        for (conn_id, ResponseAndClose { resp, close }) in self.http_responses.iter() {
            let data = match resp {
                ResponseBody::Jsonrpc(results) => {
                    let mut buf = vec![];
                    for v in results {
                        let body = serde_json::to_vec(v).unwrap();
                        let data = utils::create_http_jsonrpc_plain_response(body);
                        buf.extend_from_slice(&data);
                    }
                    buf
                }
                ResponseBody::Raw(raw) => raw.clone(),
            };

            if let Err(e) = http_conns.write_to(*conn_id, &data) {
                glog::error!("http_conn[{}] write error: {:?}", conn_id, e);
                http_conns.remove_conn(*conn_id);
            }

            if *close {
                http_conns.close_conn(*conn_id); // close after resp back
            }
        }
        // clear queue no matter write success or error(conn broken)
        self.http_responses.clear();
    }

    fn tick_ws(&mut self, tick: &mut TickResult, ws_conns: &mut WsServerConns) {
        self.handler.tick_ws(tick, ws_conns);
    }
}

impl<H: JsonrpcForwarderHandler> HttpWsServerHandler for ServerHandler<H> {
    fn on_new_http_request(&mut self, ctx: &mut HttpServerContext, mut req: HttpRequestReader) {
        if let Some(max) = self.cfg.http_max_body_length {
            if req.body().len() > max {
                self.http_responses.add_single_error_msg_closed(
                    ctx.conn_id,
                    -32600,
                    "JSONRPC request is too large",
                );
                return;
            }
        }

        let path = req.path().to_owned();
        let (rpc_path, token) = extract_path_and_token(&path);
        let rpc_path = match rpc_path {
            Some(v) => v,
            _ => {
                self.http_responses.add_single_error_msg_closed(
                    ctx.conn_id,
                    -32600,
                    "No path specified",
                );
                return;
            }
        };
        let remote_uri = match self.handler.get_http_uri(&rpc_path) {
            Some(v) => v,
            _ => {
                self.http_responses.add_single_error_msg_closed(
                    ctx.conn_id,
                    -32600,
                    "Unknown path",
                );
                return;
            }
        };

        let req_body = match Batchable::parse(req.body()) {
            Ok(v) => v,
            Err(_) => {
                self.http_responses
                    .add_single_error_msg_closed(ctx.conn_id, -32600, "Parse error");
                return;
            }
        };

        if let Batchable::Batch(vs) = &req_body {
            if vs.len() > 30 {
                self.http_responses.add_single_error_msg_closed(
                    ctx.conn_id,
                    -32600,
                    "Batch size is too large",
                );
                return;
            }
        }

        let mut sr = sanitizer::SanitizedRequest::new(req_body);
        // account relationship
        sr = sanitizer::protect_account_relationship(sr);
        // metadata
        sr = sanitizer::protect_metadata(sr, ctx, &mut req);

        let fwd_ctx = JsonrpcForwardContext { token };
        let fwd_req = JsonrpcForwardRequest {
            conn_id: ctx.conn_id,
            rpc_path: rpc_path.to_owned(),
            remote_uri,
            sr,
            last_send: None,
        };

        self.handler.on_http_request(fwd_ctx, &fwd_req);

        self.http_reqs.push(fwd_req);
    }

    fn on_new_ws_conn(&mut self, ctx: &mut HttpWsServerContext) {
        self.handler
            .on_new_ws_conn(self.alive.clone(), &self.cfg, ctx)
    }

    fn on_new_ws_request(&mut self, ctx: &mut HttpWsServerContext, ty: WsDataType, data: Vec<u8>) {
        // todo: parse and enqueue ws_reqs
        self.handler.on_ws_request(ctx, ty, data)
    }

    fn on_close_ws_conn(&mut self, conn_id: usize) {
        self.handler.on_close_ws_conn(conn_id)
    }

    fn on_tick(
        &mut self,
        http_conns: &mut HttpServerConns,
        ws_conns: &mut WsServerConns,
    ) -> TickResult {
        let mut tick = TickResult::Idle;
        self.tick_http_reqs(&mut tick, http_conns);
        self.tick_http_recv_remote(&mut tick, http_conns);
        self.tick_http_send_response(&mut tick, http_conns);
        self.tick_ws(&mut tick, ws_conns);
        tick
    }
}

// "/{rpc_network}/{token}/.."
fn extract_path_and_token(path: &str) -> (Option<&str>, Option<&str>) {
    let mut it = path.split("/");
    let path = it.nth(1);
    let token = it.nth(0);
    (path, token)
}
