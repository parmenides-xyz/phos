# Phos TypeScript Example

This TypeScript example runs the DATA Network light client in a browser and displays the latest verified block.

## Prerequisites

Build the parent `node-wasm` library:

```bash
cd ..
npm install
npm run build
```

## Setup

The example includes default public DATA Network RPCs and a recent trusted block. To override them, create an environment file:

```bash
cp .env.example .env
```

Update `VITE_DATA_TRUST_HEIGHT` and `VITE_DATA_TRUST_HASH` together when starting from a newer trusted block.

## Development

Install the example dependencies and start Vite:

```bash
npm install
npm run dev
```

The application is available at http://localhost:3000.

## Build

```bash
npm run build
```

The production files are written to `dist`.

## How It Works

The example creates the Phos EIP-1193 provider, waits for CometBFT light-client synchronization, and supplies the provider to Viem through its custom transport.
