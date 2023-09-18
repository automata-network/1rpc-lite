use std::collections::HashMap;
use std::prelude::v1::*;
use std::sync::Arc;
use std::time::Duration;

use base::trace::Alive;
use forwarder::{
    JsonrpcForwarder, JsonrpcForwarderConfig, JsonrpcForwarderHandler, JsonrpcForwarderWsHandler,
    JsonrpcResponseMgr,
};
use net_http::{HttpWsServerContext, TickResult, WsDataType, WsServerConns};

pub struct OneRpcConfig {
    pub listen_addr: String,
    pub tls_cert: Vec<u8>,
    pub tls_key: Vec<u8>,
    pub http_max_body_length: Option<usize>,
    pub ws_frame_size: usize,
    pub ws_keep_alive: Option<Duration>,
    pub ws_max_body_length: Option<usize>,
}

// static only
#[derive(Clone)]
pub struct OneRpcRouter {
    endpoints: Arc<HashMap<String, net_http::Uri>>,
}

impl OneRpcRouter {
    pub fn from_static(file_path: &str) -> Self {
        let hmap = base::fs::parse_file::<HashMap<String, String>>(file_path).unwrap();
        glog::info!("static routes: {}", serde_json::to_string(&hmap).unwrap());
        let hmap = hmap
            .into_iter()
            .map(|(k, v)| net_http::Uri::new(&v).map(|uri| (k, uri)))
            .collect::<Result<HashMap<_, _>, _>>();
        let endpoints = Arc::new(hmap.unwrap());
        Self { endpoints }
    }

    pub fn get_route(&self, key: &str) -> Option<net_http::Uri> {
        self.endpoints.get(key).cloned()
    }
}

pub struct OneRpcJsonrpcForwarderHandler<W: JsonrpcForwarderWsHandler> {
    router: OneRpcRouter,
    ws_handler: W,
    ws_responses: JsonrpcResponseMgr,
}

impl<W> JsonrpcForwarderHandler for OneRpcJsonrpcForwarderHandler<W>
where
    W: JsonrpcForwarderWsHandler,
{
    fn get_http_uri(&self, key: &str) -> Option<net_http::Uri> {
        self.router.get_route(key).filter(|v| v.scheme() == "https")
    }

    fn get_ws_uri(&self, key: &str) -> Option<net_http::Uri> {
        self.router.get_route(key).filter(|v| v.scheme() == "wss")
    }

    fn on_http_request(
        &mut self,
        ctx: forwarder::JsonrpcForwardContext,
        req: &forwarder::JsonrpcForwardRequest,
    ) {
        self.ws_handler
            .witness_request(ctx, req, &mut self.ws_responses)
    }

    fn on_new_ws_conn(
        &mut self,
        alive: Alive,
        cfg: &forwarder::JsonrpcForwarderConfig,
        ctx: &mut net_http::HttpWsServerContext,
    ) {
        self.ws_handler
            .on_connection_open(alive, cfg, ctx, &mut self.ws_responses)
    }

    fn on_ws_request(&mut self, ctx: &mut HttpWsServerContext, ty: WsDataType, data: Vec<u8>) {
        self.ws_handler
            .process_request(ctx, ty, data, &mut self.ws_responses)
    }

    fn on_close_ws_conn(&mut self, conn_id: usize) {
        self.ws_handler.on_connection_close(conn_id)
    }

    fn tick_ws(&mut self, tick: &mut TickResult, ws_conns: &mut WsServerConns) {
        self.ws_handler.tick_ws_reqs(tick, ws_conns);
        self.ws_handler.tick_ws_recv_remote(tick, ws_conns);
        W::tick_send_response(&mut self.ws_responses, ws_conns);
    }
}

pub struct OneRpc<W: JsonrpcForwarderWsHandler> {
    alive: Alive,
    forwarder: JsonrpcForwarder<OneRpcJsonrpcForwarderHandler<W>>,
}

impl<W: JsonrpcForwarderWsHandler> OneRpc<W> {
    pub fn new(cfg: &OneRpcConfig, router: OneRpcRouter, ws_handler: W, alive: Alive) -> Self {
        let handler = OneRpcJsonrpcForwarderHandler {
            router,
            ws_handler,
            ws_responses: JsonrpcResponseMgr::new(),
        };
        let forwarder = JsonrpcForwarder::new(
            JsonrpcForwarderConfig {
                listen_addr: cfg.listen_addr.clone(),
                tls_cert: cfg.tls_cert.clone(),
                tls_key: cfg.tls_key.clone(),
                http_max_body_length: cfg.http_max_body_length,
                ws_frame_size: cfg.ws_frame_size,
                ws_keep_alive: cfg.ws_keep_alive,
                ws_max_body_length: cfg.ws_max_body_length,
            },
            handler,
            alive.clone(),
        )
        .unwrap();

        Self { alive, forwarder }
    }

    pub fn serve(&mut self) {
        let dur = Duration::from_millis(10);
        loop {
            if !self.alive.is_alive() {
                break;
            }
            match self.forwarder.tick() {
                TickResult::Idle => {
                    std::thread::sleep(dur);
                }
                TickResult::Busy => {}
                TickResult::Error => break,
            }
        }
    }
}
