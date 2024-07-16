use std::prelude::v1::*;

use apps::{Const, Getter, Var};
use base::fs::read_file;
use base::trace::Alive;
use std::time::Duration;

use crate::{
    handler::{DemoWsHandler, RelayWsHandler, RelayWsHandlerConfig},
    one_rpc::{OneRpc, OneRpcConfig, OneRpcRouter},
    Args,
};

#[derive(Default)]
pub struct App {
    pub alive: Alive,
    pub arg: Const<Args>,
    pub cfg: Var<OneRpcConfig>,
}

impl apps::App for App {
    fn run(&self, env: apps::AppEnv) -> Result<(), String> {
        self.arg.set(Args::from_args(env.args));
        let cfg = self.cfg.get(self);

        #[cfg(feature = "dcap")]
        {
            use crypto::Secp256k1PrivateKey;
            use jsonrpc::MixRpcClient;
            use sgxlib_ra::ExecutionClient;

            let mut mix = MixRpcClient::new(None);
            mix.add_endpoint(&Alive::new(), &["https://automata-testnet.alt.technology".to_string()]).unwrap();
            let el = ExecutionClient::new(mix);

            if self.arg.get().check_default_private_key() {
                let err_msg = "Unable to find submitter account".to_string();
                glog::error!("{}", err_msg);
                return Err(err_msg);
            }
            let submitter_prvkey = &self.arg.get().submitter[..];
            let submitter: Secp256k1PrivateKey = submitter_prvkey.into();

            let quote = sgxlib_ra::dcap_quote(&el, &submitter);
            glog::info!("quote: {:?}", quote);
        }

        if self.arg.get().is_demo {
            let router = OneRpcRouter::from_static(&self.arg.get().routes);
            let ws_handler = DemoWsHandler::default();
            let mut onerpc = OneRpc::new(&cfg, router, ws_handler, self.alive.clone());
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
            let mut onerpc = OneRpc::new(&cfg, router, ws_handler, self.alive.clone());
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

impl Getter<OneRpcConfig> for App {
    fn generate(&self) -> OneRpcConfig {
        let arg = self.arg.get();
        let (tls_cert, tls_key) = match arg.tls.as_str() {
            "" => (Vec::new(), Vec::new()),
            path => (
                read_file(&format!("{}.crt", path)).unwrap().into(),
                read_file(&format!("{}.key", path)).unwrap().into(),
            ),
        };
        OneRpcConfig {
            listen_addr: arg.addr.clone(),
            tls_cert,
            tls_key,
            http_max_body_length: Some(2 << 20),
            ws_frame_size: 64 << 10,
            ws_keep_alive: Some(Duration::from_secs(10)),
            ws_max_body_length: Some(2 << 20),
        }
    }
}
