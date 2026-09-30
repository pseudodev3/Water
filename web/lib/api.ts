export type Chain = "solana" | "robinhood";

export type Health = {
  status: string;
  market_provider: string;
  requires_market_api_key: boolean;
  chains: Chain[];
};

export type ScanResult = {
  chain: Chain;
  address: string;
  scanned_at_unix: number;
  token: {
    name: string | null;
    symbol: string | null;
    price_usd: number | null;
    liquidity_usd: number | null;
    market_cap_usd: number | null;
  };
  pressure: {
    index: number | null;
    methodology: string;
    components: Array<{
      key: string;
      label: string;
      observed: string;
      pressure: number;
      detail: string;
    }>;
  };
  opponent_notes: string[];
  chain_evidence: {
    source: string;
    verified: boolean;
    chain_id: number | null;
    detail: string;
  };
  holder_evidence: {
    top_ten_percentage: number | null;
    source: string;
    detail: string;
  };
  sources: Array<{
    source: string;
    ok: boolean;
    detail: string;
  }>;
};

const API_URL =
  process.env.NEXT_PUBLIC_WATER_API_URL ?? "http://localhost:8080";

export async function getHealth(): Promise<Health> {
  const response = await fetch(`${API_URL}/health`, { cache: "no-store" });

  if (!response.ok) {
    throw new Error("Water core is offline.");
  }

  return response.json();
}

export async function scanToken(
  chain: Chain,
  address: string,
): Promise<ScanResult> {
  const response = await fetch(`${API_URL}/v1/scan`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ chain, address }),
  });

  const body = await response.json().catch(() => null);

  if (!response.ok) {
    throw new Error(body?.error ?? "Water could not complete this scan.");
  }

  return body;
}
