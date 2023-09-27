# 1RPC lite

[![License](https://img.shields.io/badge/license-Apache2-green.svg)](LICENSE)

## About

This project is a minimized [1RPC](https://docs.1rpc.io) application which relays JSON-RPC requests.

In addition to normal relays, it sanitizes your JSON-RPC by masking out the metadata (like IP address, User-Agent) carried with HTTP requests, and breaks down special eth_calls which may leak your wallets relationship to ensure zero-tracking.

## How to run

### 1. TLS cert & key

if you are going to run demo with https, plz prepare `domain.key` and `domain.crt`

OR you just touch empty files at above locations for local test purpose

```
> touch domain.key
> touch domain.crt
```

### 2. Config & Run

1RPC lite app can run in 2 modes:
* **relay mode**: relay jsonrpc requests, no protection details conveyed
* **demo mode**: not relay websocket jsonrpc requests, but instead send protection details via websocket channel

1. both modes load routes from static json config file (example in following section)
2. make sure no duplicate keys in routes
3. `wss` routes take no effect in demo mode

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

(`--tls domain` is to lookup `domain.key` and `domain.crt`)

you can add env `SGX=1` to build&run the SGX version, before that you need to setup SGX
environment, You can find the [installation guides](https://download.01.org/intel-sgx/sgx-linux/2.9/docs/)
for Intel SGX software on the 01.org website. Besides, you need to prepare an account as well, to submit the dcap attestation in [Automata Testnet](https://docs.ata.network/protocol/testnet).

Test out:
1. make http jsonrpc requests to `http://127.0.0.1:3400/eth/$token`
2. or `wscat -c ws://127.0.0.1:3400/dot`, then make jsonrpc requests

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

Test out:

0. choose some `token`
1. `wscat -c ws://127.0.0.1:3400/ws/$token` to open ws channel
2. make http jsonrpc requests to `http://127.0.0.1:3400/eth/$token`
3. you'll recv protection details in ws channel

## Contributing

Thank you for considering contributing to 1rpc-lite!

**Before You Contribute**:
* **Raise an Issue**: If you find a bug or wish to suggest a feature, please open an issue first to discuss it. Detail the bug or feature so we understand your intention.  
* **Pull Requests (PR)**: Before submitting a PR, ensure:  
    * Your contribution successfully builds.
    * It includes tests, if applicable.
Your efforts help make 1rpc-lite better, and we truly appreciate your support!
