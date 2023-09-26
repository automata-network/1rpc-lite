// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License..

use std::env;
use std::path::Path;

fn main() {
    let builder = ata_sgx_builder::GeodeBuild::new();
    builder.build_signing_material();

    let signatures = builder.build_sign_with_pem();
    let pubkey_path = Path::new(&env::var("OUT_DIR").unwrap().to_string()).join("public.pem");
    builder.build_signed_material(&pubkey_path, &signatures, ata_sgx_builder::LinkType::Dcap);
}
