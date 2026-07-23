<div align="center">

# phos

<a href="https://crates.io/crates/phos-cli"><img src="https://img.shields.io/crates/v/phos-cli?label=phos-cli" alt="crates.io phos-cli"></a>
<a href="https://crates.io/crates/phos-data-network"><img src="https://img.shields.io/crates/v/phos-data-network?label=phos-data-network" alt="crates.io phos-data-network"></a>
<a href="https://crates.io/crates/phos-data-network-precompiles"><img src="https://img.shields.io/crates/v/phos-data-network-precompiles?label=phos-data-network-precompiles" alt="crates.io phos-data-network-precompiles"></a>
<a href="https://crates.io/crates/phos-data-network-proto"><img src="https://img.shields.io/crates/v/phos-data-network-proto?label=phos-data-network-proto" alt="crates.io phos-data-network-proto"></a>
<a href="https://crates.io/crates/phos-light-client"><img src="https://img.shields.io/crates/v/phos-light-client?label=phos-light-client" alt="crates.io phos-light-client"></a>
<a href="https://crates.io/crates/phos-light-client-verifier"><img src="https://img.shields.io/crates/v/phos-light-client-verifier?label=phos-light-client-verifier" alt="crates.io phos-light-client-verifier"></a>
<a href="https://crates.io/crates/phos-helios-common"><img src="https://img.shields.io/crates/v/phos-helios-common?label=phos-helios-common" alt="crates.io phos-helios-common"></a>
<a href="https://crates.io/crates/phos-helios-core"><img src="https://img.shields.io/crates/v/phos-helios-core?label=phos-helios-core" alt="crates.io phos-helios-core"></a>
<a href="https://crates.io/crates/phos-helios-revm-utils"><img src="https://img.shields.io/crates/v/phos-helios-revm-utils?label=phos-helios-revm-utils" alt="crates.io phos-helios-revm-utils"></a>
<a href="https://crates.io/crates/phos-helios-verifiable-api-client"><img src="https://img.shields.io/crates/v/phos-helios-verifiable-api-client?label=phos-helios-verifiable-api-client" alt="crates.io phos-helios-verifiable-api-client"></a>
<a href="https://crates.io/crates/phos-helios-verifiable-api-types"><img src="https://img.shields.io/crates/v/phos-helios-verifiable-api-types?label=phos-helios-verifiable-api-types" alt="crates.io phos-helios-verifiable-api-types"></a>

</div>

Light client for DATA Network (formerly Story Protocol) written in Rust, with native + browser + iOS support.

Phos converts an untrusted, third-party RPC endpoint into a safe, unmanipulable RPC for users.

## Installing the node

### Installing with Cargo

Install the native node. Note `phos-cli` does not include browser support; to run Phos in a browser, build `node-wasm` from source.

```bash
cargo install phos-cli --locked
```

### Building from source

Install common dependencies.

```bash
# install dependencies
sudo apt-get install -y build-essential curl git jq pkg-config libssl-dev

# install Rust (if necessary)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# open a new terminal or run
source "$HOME/.cargo/env"

# clone the repository
git clone https://github.com/parmenides-xyz/phos.git
cd phos

# install Phos
cargo install --path cli --locked
```

### Building node-wasm

Install Node.js 18 or newer and npm, then run:

```bash
rustup target add wasm32-unknown-unknown

cargo install wasm-pack --version 0.15.0 --locked
```

Then run:

```bash
cd node-wasm
npm ci
npm run build
```

## Running the node

### Running the node natively

For the first run, supply a trusted block height and hash:

```bash
phos node --trust-height <HEIGHT> --trust-hash <HASH>
```

To retrieve a recent candidate height and hash from DATA Network's default RPC, run:

```bash
curl -s https://story-consensus-rpc.publicnode.com/status \
  | jq -r '.result.sync_info | "height: \(.latest_block_height)\nhash: \(.latest_block_hash)"'
```

For subsequent runs:

```bash
phos node
```

View all configuration options:

```bash
phos node --help
```

### Running the node in a browser

From the repository root:

```bash
cd node-wasm/example
npm ci
npm run dev
```

The browser example is available at http://localhost:3000.
