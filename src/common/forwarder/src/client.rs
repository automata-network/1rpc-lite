use std::prelude::v1::*;

use net_http::{HttpConnClientPool, Uri};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use crate::ForwarderError;

pub struct HttpForwardClient(BTreeMap<String, HttpConnClientPool>);

impl HttpForwardClient {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn get_or_new(
        &mut self,
        key: String,
        uri: &Uri,
    ) -> Result<&mut HttpConnClientPool, ForwarderError> {
        let conn = match self.0.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let conn = HttpConnClientPool::new(2, uri)?;
                entry.insert(conn)
            }
        };
        Ok(conn)
    }
}

impl Deref for HttpForwardClient {
    type Target = BTreeMap<String, HttpConnClientPool>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HttpForwardClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
