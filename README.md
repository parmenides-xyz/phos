# Homer (helios-exex)

Rust implementation of DATA Network (formerly Story), able to run natively + in browser-based environments.

## Why a light client?
Homer converts an untrusted RPC endpoint into a safe unmanipulable local RPC for users. Users can interact with + efficiently verify state on DATA Network from any device.

Homer is not a fork, but extends upstream [Helios](https://github.com/a16z/helios), adding DATA-specific consensus (CometBFT) and a custom EVM.

## Running the node
### Running the node natively

A new client requires a trusted CometBFT block height and hash from a trusted source. The block must remain within DATA Network's trusting period.

From the repository root:

```bash
cargo run --release --package helios-exex-cli -- data-network \
  --execution-rpc https://story-rpc.publicnode.com \
  --trust-height <TRUSTED_HEIGHT> \
  --trust-hash <TRUSTED_HASH>
```

Homer synchronizes consensus and exposes a verified JSON-RPC endpoint at `http://127.0.0.1:8545`.

```bash
curl http://127.0.0.1:8545 \
  -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

Trusted state is persisted locally. Subsequent starts can omit `--trust-height` and `--trust-hash`.

### Serving node-wasm

Install the WASM target and build the browser package:

```bash
rustup target add wasm32-unknown-unknown
cd node-wasm
npm ci
npm run build
```

Then start the browser example:

```bash
cd example
npm ci
cp .env.example .env
npm run dev
```

Before starting a fresh browser profile, update `VITE_DATA_TRUST_HEIGHT` and `VITE_DATA_TRUST_HASH` in `.env` with a recent trusted CometBFT block.

Vite serves the application at `http://localhost:3000`.

### Additional CLI Options

```bash
cargo run --package helios-exex-cli -- data-network --help
```

| Option | Environment variable | Description |
| --- | --- | --- |
| `--network` | | Network: `mainnet` or `aeneid`; defaults to `mainnet` |
| `--execution-rpc` | `EXECUTION_RPC` | EVM execution JSON-RPC endpoint |
| `--consensus-rpc` | `CONSENSUS_RPC` | CometBFT RPC endpoint |
| `--verifiable-api` | `VERIFIABLE_API` | Verifiable execution API instead of a standard execution RPC |
| `--trust-height` | `TRUST_HEIGHT` | Initial trusted CometBFT height |
| `--trust-hash` | `TRUST_HASH` | Hash corresponding to the trusted height |
| `--rpc-bind-ip` | `RPC_BIND_IP` | Local JSON-RPC bind address |
| `--rpc-port` | `RPC_PORT` | Local JSON-RPC port; defaults to `8545` |
| `--data-dir` | `DATA_DIR` | Persistent light-client state directory |
| `--tui` | | Run with the terminal interface |
