# 1RPC lite

[![License](https://img.shields.io/badge/license-Apache2-green.svg)](LICENSE)

## How to run

### 1. TLS cert & key

if you are going to run demo with https, plz prepare `domain.key` and `domain.crt`

OR you just touch empty files at above locations for local test purpose

```
> touch domain.key
> touch domain.crt
```

### 2. Config & Run

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


```
> RELEASE=1 ./scripts/1rpc.sh -r config-demo-example.json --tls domain
```

(`--tls domain` is to lookup `domain.key` and `domain.crt`)

you can add env `SGX=1` to build&run the SGX version, before that you need to setup SGX
environment, You can find the [installation guides](https://download.01.org/intel-sgx/sgx-linux/2.9/docs/)
for Intel SGX software on the 01.org website. Besides, you need to prepare an account as well, to submit the dcap attestation in [Automata Testnet](https://docs.ata.network/protocol/testnet).

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

```
> RELEASE=1 ./scripts/1rpc.sh -r config-relay-example.json -d false --tls domain
```

Test out:
1. make http jsonrpc requests to `http://127.0.0.1:3400/eth/$token`
2. or `wscat -c ws://127.0.0.1:3400/dot`, then make jsonrpc requests
