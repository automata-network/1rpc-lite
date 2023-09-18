// use std::prelude::v1::*;

use net_http::HttpConnError;

#[derive(Debug)]
pub enum ForwarderError {
    ListenError(std::io::Error),
    HttpConnError(HttpConnError),
}

impl From<HttpConnError> for ForwarderError {
    fn from(e: HttpConnError) -> Self {
        Self::HttpConnError(e)
    }
}
