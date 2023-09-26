# 1RPC Backend

## How to run

### 1. Initialize SGX Env

You can refer [here](./Dockerfile).

### 2. TLS cert & key

if you are going to run demo with https, plz prepare `src/apps/onerpc/src/domain.key` and `src/apps/onerpc/src/domain.crt`, both will be compile into SGX enclave

OR you just `touch` empty files at above locations for local test purpose

### 3. Config & Run

onerpc app can run in 2 modes:
* **demo mode**: not relay websocket jsonrpc requests, but instead send protection details via websocket channel
* **ws-relay mode**: relay websocket jsonrpc requests, no protection details conveyed

1. both modes load routes from static json config file (example in following section)
2. make sure no duplicate keys in routes
3. `wss` routes take no effect in demo mode


#### Run in demo mode

edit config-demo-example.json
```json
{
    "eth": "https://eth.llamarpc.com" // <- replace with your prefered endpoint
}
```

`SGX=1 RELEASE=1 RUST_LOG=debug bash ./scripts/onerpc.sh -r config-demo-example.json`

Test out:

0. choose some `token`
1. `wscat -c ws://127.0.0.1:3400/ws/$token` to open ws channel
2. make http jsonrpc requests to `http://127.0.0.1:3400/eth/$token`
3. you'll recv protection details in ws channel

#### Run in relay mode

edit config-relay-example.json
```json
{
    "eth": "https://eth.llamarpc.com", // <- replace with your prefered endpoint
    "dot": "wss://rpc.polkadot.io", // etc
}
```

`SGX=1 RELEASE=1 RUST_LOG=debug bash ./scripts/onerpc.sh -r config-relay-example.json -d false`

Test out:
1. make http jsonrpc requests to `http://127.0.0.1:3400/eth/$token`
2. or `wscat -c ws://127.0.0.1:3400/dot`, then make jsonrpc requests
