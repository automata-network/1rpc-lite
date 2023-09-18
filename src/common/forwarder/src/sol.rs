use eth_types::{H160, U256};
use std::prelude::v1::*;

pub fn func_sig_matches(data: &[u8], sig: &[u8]) -> bool {
    sig == &data[..4]
}

// `data` offseted
pub fn decode_uint256(data: &[u8]) -> Option<U256> {
    let mut it = data.chunks(32);
    it.next().map(|v| U256::from_big_endian(v))
}

// `data` offseted
pub fn decode_address_array(data: &[u8]) -> Vec<H160> {
    let mut it = data.chunks(32);
    let len = match it.next() {
        Some(v) => U256::from_big_endian(v).as_usize(),
        _ => return vec![],
    };
    it.take(len).map(|v| H160::from_slice(&v[12..])).collect()
}

pub fn encode_uint256_array(vs: &Vec<U256>) -> Vec<u8> {
    let mut ret = Vec::with_capacity(32 * (2 + vs.len()));
    let mut buf = [0; 32];

    // offset
    let offset = U256::from(32);
    offset.to_big_endian(&mut buf);
    ret.extend_from_slice(&buf);

    // len
    let len = U256::from(vs.len());
    len.to_big_endian(&mut buf);
    ret.extend_from_slice(&buf);

    // data
    for v in vs {
        v.to_big_endian(&mut buf);
        ret.extend_from_slice(&buf);
    }
    ret
}
