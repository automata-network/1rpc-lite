use std::{
    net::{SocketAddr, ToSocketAddrs},
    prelude::v1::*,
};

use hex::HexBytes;
use jsonrpc::JsonrpcRawRequest;
use net_http::HttpResponseBuilder;
use serde::Deserialize;

#[derive(Deserialize)]
struct EthCallParamTxn {
    #[allow(unused)]
    to: String,
    // omit fields
    // from: Option<String>,
    // gas: Option<U256>,
    // gas_price: Option<U256>,
    // value: Option<U256>,
    data: Option<HexBytes>,
}

pub fn get_eth_call_data_from_jsonrpc(req: &JsonrpcRawRequest) -> Option<HexBytes> {
    let params = match serde_json::from_raw_value::<Vec<serde_json::Value>>(&req.params) {
        Ok(v) => v,
        Err(_) => return None,
    };
    params
        .into_iter()
        .nth(0)
        .and_then(|v| serde_json::from_value::<EthCallParamTxn>(v).ok())
        .and_then(|v| v.data)
}

fn get_header_from_http_req(
    req: &mut net_http::HttpRequestReader,
    hdr: &'static str,
) -> Option<String> {
    req.headers(|headers| {
        for header in headers {
            if header.name.eq_ignore_ascii_case(hdr) {
                return Some(String::from_utf8_lossy(header.value).to_string());
            }
        }
        return None;
    })
}

pub fn get_host_ip(req: &mut net_http::HttpRequestReader) -> String {
    let na = String::from("N/A");

    let host = match get_header_from_http_req(req, "host") {
        Some(v) => v,
        _ => return na,
    };

    if host.contains("127.0.0.1") || host.contains("localhost") {
        return "127.0.0.1".into();
    }

    glog::debug!("query host: {}", host);
    match host.to_socket_addrs() {
        Ok(v) => match v.into_iter().next() {
            Some(v) => v.ip().to_string(),
            _ => na,
        },
        Err(_) => na,
    }
}

pub fn get_client_ip(req: &mut net_http::HttpRequestReader, default: Option<SocketAddr>) -> String {
    let na = String::from("N/A");

    match get_header_from_http_req(req, "cf-connecting-ip")
        .or_else(|| get_header_from_http_req(req, "x-forwarded-for"))
        .or_else(|| default.map(|v| v.ip().to_string()))
    {
        Some(v) => v,
        _ => na,
    }
}

pub fn get_cilent_ua(req: &mut net_http::HttpRequestReader) -> String {
    get_header_from_http_req(req, "user-agent").unwrap_or_default()
}

pub fn create_http_jsonrpc_plain_response(body: Vec<u8>) -> Vec<u8> {
    let mut builder = HttpResponseBuilder::new(200).close().json(body);
    builder.to_vec()
}
