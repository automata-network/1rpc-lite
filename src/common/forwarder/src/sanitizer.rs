use std::prelude::v1::*;

use base::time;
use eth_types::SU256;
use jsonrpc::{
    Batchable, JsonrpcErrorObj, JsonrpcRawRequest, JsonrpcRawResponseFull, JsonrpcResponseRawResult,
};
use serde::Serialize;

use crate::{sol, utils};

pub struct SanitizedRequest {
    pub original_ids: Batchable<jsonrpc::Id>,
    pub req_body: Batchable<JsonrpcRawRequest>,
    pub tr: Vec<Transform>,
}

impl SanitizedRequest {
    pub fn new(req_body: Batchable<JsonrpcRawRequest>) -> Self {
        let original_ids = match &req_body {
            Batchable::Single(v) => Batchable::Single(v.id.clone()),
            Batchable::Batch(vs) => Batchable::Batch(vs.iter().map(|v| v.id.clone()).collect()),
        };

        Self {
            original_ids,
            req_body,
            tr: vec![],
        }
    }

    // actually we only rewrite for `account_relationship`
    pub fn rewrite_response(
        &self,
        resp: Batchable<JsonrpcResponseRawResult>,
    ) -> Batchable<JsonrpcResponseRawResult> {
        if self.tr.iter().any(|v| v.is_account_relationship()) {
            let single = self.rewrite_account_relationship_response(resp);
            return Batchable::Single(single); // aligh with `resp` type
        }
        resp
    }

    fn rewrite_account_relationship_response(
        &self,
        resp: Batchable<JsonrpcResponseRawResult>,
    ) -> JsonrpcResponseRawResult {
        let err =
            JsonrpcRawResponseFull::err(JsonrpcErrorObj::unknown("unknown error"), None).into();

        let response = match resp {
            Batchable::Single(_) => {
                glog::error!("protect_account_error: remote_response not batch");
                return err;
            }
            Batchable::Batch(rs) => {
                let mut vs = vec![];
                for r in rs {
                    match r {
                        JsonrpcResponseRawResult::Ok(v) => vs.push(v),
                        JsonrpcResponseRawResult::Err(e) => {
                            glog::error!(
                                "protect_account_error: remote_response contains error {:?}",
                                e.error
                            );
                            return err;
                        }
                    }
                }
                vs
            }
        };

        match &self.req_body {
            Batchable::Batch(vs) => {
                if vs.len() != response.len() {
                    glog::error!(
                        "protect_account_error: req.len({}) != resp.len({})",
                        vs.len(),
                        response.len()
                    );
                    err
                } else {
                    // get balances from batch
                    let mut balances = vec![];
                    for v in response {
                        match serde_json::from_raw_value::<SU256>(&v.result) {
                            Ok(v) => balances.push(*v),
                            Err(_) => {
                                glog::error!("protected_account_error: deser jsonrpc");
                                return err;
                            }
                        }
                    }
                    // encode balances
                    let encoded = sol::encode_uint256_array(&balances);
                    let result = {
                        let hex_str = String::from("0x") + &hex::encode(encoded);
                        serde_json::to_raw_value(&hex_str).ok()
                    };
                    let id = match &self.original_ids {
                        Batchable::Single(v) => Some(v.clone()),
                        Batchable::Batch(vs) => vs.first().cloned(),
                    };
                    JsonrpcRawResponseFull {
                        jsonrpc: "2.0".into(),
                        result,
                        error: None,
                        id,
                    }
                    .into()
                }
            }
            _ => {
                glog::error!("protect_account_error: req not batch",);
                err
            }
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Transform {
    #[serde(rename = "accountRelationship")]
    AccountRelationship {
        protected: Vec<AccountRelationship>,
        unprotected: AccountRelationship,
    },
    #[serde(rename = "metadata")]
    Metadata {
        protected: Metadata,
        unprotected: Metadata,
    },
}

impl Transform {
    pub fn is_account_relationship(&self) -> bool {
        match self {
            Transform::AccountRelationship {
                protected: _,
                unprotected: _,
            } => true,
            _ => false,
        }
    }

    pub fn is_metadata(&self) -> bool {
        match self {
            Transform::Metadata {
                protected: _,
                unprotected: _,
            } => true,
            _ => false,
        }
    }
}

#[derive(Serialize)]
pub struct AccountRelationship {
    pub accounts: Vec<String>,
    pub method: &'static str,
    pub params: Vec<String>,
    pub time: String, // utc date
}

#[derive(Serialize)]
pub struct Metadata {
    pub ip: String,
    pub ua: String,
    pub time: String, // utc date
}

pub fn protect_account_relationship(mut sr: SanitizedRequest) -> SanitizedRequest {
    macro_rules! decode_fail {
        ($msg:expr) => {{
            glog::warn!("protect_account_relationship abort: {}", $msg);
            return sr;
        }};
    }

    let req = match &sr.req_body {
        Batchable::Single(v) => v,
        _ => return sr,
    };

    if req.method != "eth_call" {
        return sr;
    }

    if let Some(calldata) = utils::get_eth_call_data_from_jsonrpc(&req) {
        // 0xf0002ea9 is the 4 byte signature of balances(address[],address[])
        // https://www.4byte.directory/signatures/?bytes4_signature=0xf0002ea9
        let sig = hex::decode("f0002ea9").unwrap();
        let data = calldata.as_bytes();
        if !sol::func_sig_matches(data, &sig) {
            return sr;
        }

        let data = &data[4..]; // skip sig
        let users = if let Some(v) = sol::decode_uint256(data) {
            let offset = v.as_usize();
            sol::decode_address_array(&data[offset..])
        } else {
            decode_fail!("users");
        };

        let tokens = if let Some(v) = sol::decode_uint256(&data[32..]) {
            let offset = v.as_usize();
            sol::decode_address_array(&data[offset..])
        } else {
            decode_fail!("tokens");
        };

        // native token address only
        if tokens.first() != Some(&eth_types::H160::zero()) {
            decode_fail!("contains non-native-token address");
        }

        let accts = users
            .into_iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>();
        let new_req_body = {
            let reqs = accts
                .iter()
                .enumerate()
                .map(|(id, v)| {
                    let params = serde_json::json!([v, "latest"]);
                    JsonrpcRawRequest::new(id as u64, "eth_getBalance", &params).unwrap()
                })
                .collect::<Vec<_>>();
            Batchable::Batch(reqs)
        };
        let now = time::Date::from(time::now()).to_string();
        let tr = Transform::AccountRelationship {
            protected: accts
                .iter()
                .map(|v| AccountRelationship {
                    accounts: vec![v.clone()],
                    method: "eth_getBalance",
                    params: vec![],
                    time: now.clone(),
                })
                .collect(),
            unprotected: AccountRelationship {
                accounts: accts,
                method: "eth_call",
                params: vec!["0xf0002ea9".into(), "latest".into()],
                time: now.clone(),
            },
        };

        sr.tr.push(tr);
        return SanitizedRequest {
            original_ids: sr.original_ids,
            req_body: new_req_body,
            tr: sr.tr,
        };
    }
    return sr;
}

pub fn protect_metadata(
    sr: SanitizedRequest,
    ctx: &net_http::HttpServerContext,
    req: &mut net_http::HttpRequestReader,
) -> SanitizedRequest {
    let SanitizedRequest {
        original_ids,
        req_body,
        mut tr,
    } = sr;
    let now = time::Date::from(time::now()).to_string();
    tr.push(Transform::Metadata {
        protected: Metadata {
            ip: utils::get_host_ip(req),
            ua: "1rpc-demo/0.1".into(),
            time: now.clone(),
        },
        unprotected: Metadata {
            ip: utils::get_client_ip(req, ctx.peer_addr),
            ua: utils::get_cilent_ua(req),
            time: now.clone(),
        },
    });
    SanitizedRequest {
        original_ids,
        req_body,
        tr,
    }
}
