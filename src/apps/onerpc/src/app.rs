use std::prelude::v1::*;
use std::time::Duration;

use apps::Const;
use base::trace::Alive;

use crate::{
    handler::{DemoWsHandler, RelayWsHandler, RelayWsHandlerConfig},
    one_rpc::{OneRpc, OneRpcConfig, OneRpcRouter},
    Args,
};

const TLS_CERT: &[u8] = include_bytes!("domain.crt");
const TLS_KEY: &[u8] = include_bytes!("domain.key");

#[derive(Default)]
pub struct App {
    pub alive: Alive,
    pub arg: Const<Args>,
}

impl apps::App for App {
    fn run(&self, env: apps::AppEnv) -> Result<(), String> {
        self.arg.set(Args::from_args(env.args));
        let cfg = OneRpcConfig {
            listen_addr: self.arg.get().addr.clone(),
            tls_cert: TLS_CERT.into(),
            tls_key: TLS_KEY.into(),
            http_max_body_length: Some(2 << 20),
            ws_frame_size: 64 << 10,
            ws_keep_alive: Some(Duration::from_secs(10)),
            ws_max_body_length: Some(2 << 20),
        };
        
        #[cfg(feature = "dcap")]
        {
            let quote = sgxlib_ra::dcap_quote();
            glog::info!("quote: {:?}", quote);
        }
        
        if self.arg.get().is_demo {
            let router = OneRpcRouter::from_static(&self.arg.get().routes);
            let ws_handler = DemoWsHandler::default();
            let mut onerpc = OneRpc::new(cfg, router, ws_handler, self.alive.clone());
            glog::info!("start demo ..");
            onerpc.serve();
            glog::info!("quit ..");
        } else {
            let ws_cfg = RelayWsHandlerConfig {
                ws_frame_size: cfg.ws_frame_size,
                ws_keep_alive: cfg.ws_keep_alive.clone(),
                ws_max_body_length: cfg.ws_max_body_length.clone(),
            };
            let router = OneRpcRouter::from_static(&self.arg.get().routes);
            let ws_handler = RelayWsHandler::new(ws_cfg, router.clone());
            let mut onerpc = OneRpc::new(cfg, router, ws_handler, self.alive.clone());
            glog::info!("start relay ..");
            onerpc.serve();
            glog::info!("quit ..");
        }
        Ok(())
    }

    fn terminate(&self) {
        self.alive.shutdown()
    }
}
