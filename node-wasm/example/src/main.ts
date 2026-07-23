import {
  createPublicClient,
  custom,
  defineChain,
  type Block,
  type Chain,
  type PublicClient,
} from "viem";

// Import Phos from the parent directory's built output.
// @ts-ignore - importing local build
import * as heliosExEx from "../../dist/lib.js";

interface NetworkConfig {
  name: string;
  blockTime: number;
  cfg: any;
  provider?: any;
  viemClient?: PublicClient;
  lastSeen?: bigint;
  chain: Chain;
}

const dataNetwork = defineChain({
  id: 1514,
  name: "DATA Network",
  nativeCurrency: {
    name: "DATA",
    symbol: "DATA",
    decimals: 18,
  },
  rpcUrls: {
    default: {
      http: ["https://story-rpc.publicnode.com"],
    },
  },
});

const blockRows = document.getElementById("block-rows");
const blocks: Block[] = [];

function setText(id: string, value: string): void {
  const element = document.getElementById(id);
  if (element) element.textContent = value;
}

function setSyncState(
  label: string,
  state: "syncing" | "live" | "error",
): void {
  setText("sync-status", label);
  document.getElementById("live-badge")?.setAttribute("data-state", state);
  document.getElementById("network-badge")?.setAttribute("data-state", state);

  const clientStatus = document.getElementById("client-status");
  if (clientStatus) {
    clientStatus.textContent = label;
    clientStatus.classList.toggle("positive", state === "live");
    clientStatus.classList.toggle("negative", state === "error");
  }
}

function createCell(
  value: string,
  className: string,
  alignEnd = false,
): HTMLDivElement {
  const cell = document.createElement("div");
  if (alignEnd) cell.classList.add("align-end");

  const content = document.createElement("span");
  content.className = className;
  content.textContent = value;
  content.title = value;
  cell.append(content);
  return cell;
}

function renderBlocks(): void {
  if (!blockRows) return;

  blockRows.replaceChildren(
    ...blocks.map((block, index) => {
      const row = document.createElement("div");
      row.className = "block-row";
      row.setAttribute("role", "row");
      if (index === 0) row.classList.add("block-row-shimmer");

      const timestamp = new Date(Number(block.timestamp) * 1000);
      const time = timestamp.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });

      row.append(
        createCell(`#${block.number ?? 0n}`, "block-number"),
        createCell(block.hash ?? "0x", "block-hash"),
        createCell(time, "block-time", true),
        createCell(String(block.transactions.length), "block-txns", true),
      );
      return row;
    }),
  );
  blockRows.setAttribute("aria-busy", "false");
}

function addBlock(block: Block): void {
  if (blocks.some((current) => current.number === block.number)) return;
  blocks.push(block);
  blocks.sort((a, b) => Number((b.number ?? 0n) - (a.number ?? 0n)));
  blocks.splice(10);
  renderBlocks();

  const height = String(block.number ?? 0n);
  setText("header-block", height);
  setText("verified-height", height);
  setText("block-count", `(${height})`);
}

function renderError(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  setSyncState("Error", "error");
  document.getElementById("sync-progress")?.classList.add("complete");

  if (blockRows) {
    const row = document.createElement("div");
    row.className = "error-row";
    row.textContent = message;
    blockRows.replaceChildren(row);
    blockRows.setAttribute("aria-busy", "false");
  }
  console.error("DATA Network light client failed:", error);
}

async function main(): Promise<void> {
  const executionRpc =
    import.meta.env.VITE_DATA_EXECUTION_RPC ??
    "https://story-rpc.publicnode.com";
  const consensusRpc =
    import.meta.env.VITE_DATA_CONSENSUS_RPC ??
    "https://story-consensus-rpc.publicnode.com";
  const trustHeight = import.meta.env.VITE_DATA_TRUST_HEIGHT ?? "20044541";
  const trustHash =
    import.meta.env.VITE_DATA_TRUST_HASH ??
    "9929C08444D99A82908E007CB0A45AF073A424630E46A1289E6C7C2CB98C8449";

  setText("trust-height", trustHeight);
  setText("trust-hash", trustHash);
  setText("consensus-rpc", consensusRpc);
  setText("execution-rpc", executionRpc);
  setSyncState("Syncing", "syncing");

  const networks: NetworkConfig[] = [
    {
      name: "data-network",
      blockTime: 2,
      cfg: {
        executionRpc,
        consensusRpc,
        network: "mainnet",
        trustHeight,
        trustHash,
        dbType: "localstorage",
      },
      chain: dataNetwork,
    },
  ];

  await Promise.all(
    networks.map(async (network) => {
      network.provider = await heliosExEx.createDataNetworkProvider(network.cfg);
      await network.provider.waitSynced();
      network.viemClient = createPublicClient({
        chain: network.chain,
        transport: custom(network.provider),
      });
    }),
  );

  setSyncState("Live", "live");
  document.getElementById("sync-progress")?.classList.add("complete");

  const update = async (network: NetworkConfig): Promise<void> => {
    if (!network.viemClient) return;

    const latestNumber = await network.viemClient.getBlockNumber();
    if (latestNumber === network.lastSeen) return;

    network.lastSeen = latestNumber;
    const block = await network.viemClient.getBlock({
      blockNumber: latestNumber,
      includeTransactions: true,
    });
    addBlock(block);
  };

  await Promise.all(networks.map(update));
  networks.forEach((network) => {
    window.setInterval(() => {
      void update(network).catch(renderError);
    }, 1000);
  });
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => void main().catch(renderError));
} else {
  void main().catch(renderError);
}
