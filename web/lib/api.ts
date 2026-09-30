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
  demand: {
    buys_h1: number | null;
    sells_h1: number | null;
    buy_share_h1: number | null;
    transactions_h1: number | null;
    unique_buyers_h1: number | null;
    unique_sellers_h1: number | null;
    buyer_arrival_vs_h24_hourly: number | null;
    volume_h24_usd: number | null;
    volume_to_liquidity: number | null;
  };
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
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 20_000);

  try {
    const response = await fetch(`${API_URL}/v1/scan`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ chain, address }),
      signal: controller.signal,
    });

    const body = await response.json().catch(() => null);

    if (!response.ok) {
      throw new Error(body?.error ?? "Water could not complete this scan.");
    }

    return body;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new Error(
        "Water stopped waiting after 20 seconds. The public chain provider did not answer in time.",
      );
    }

    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}

export type EarlyHolderMap = {
  chain: Chain;
  token: string;
  observed_at_unix: number;
  wallets_requested: number;
  wallets_listed: number;
  wallets_reconstructed: number;
  complete_movement_histories: number;
  cohort_retained_from_peak: number | null;
  cohort_distributed_fraction: number | null;
  holders: Array<{
    rank: number;
    wallet: string;
    current_quantity: number;
    first_acquired_at: number | null;
    peak_quantity: number | null;
    retained_from_peak: number | null;
    distributed_fraction: number | null;
    basis_coverage: number | null;
    average_entry_usd: number | null;
    current_price_usd: number | null;
    current_multiple_on_entry: number | null;
    basis_status: "verified" | "partial_history" | "incomplete" | null;
    movement_history: "complete" | "partial" | "unavailable";
    detail: string;
  }>;
  notes: string[];
};

export async function fetchEarlyHolders(
  chain: Chain,
  token: string,
): Promise<EarlyHolderMap> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 28_000);

  try {
    const response = await fetch(`${API_URL}/v1/early-holders`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ chain, token, limit: 3 }),
      signal: controller.signal,
    });

    const body = await response.json().catch(() => null);

    if (!response.ok) {
      throw new Error(body?.error ?? "Early-holder reconstruction was unavailable.");
    }

    return body;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new Error("Early-holder reconstruction took too long to verify.");
    }

    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}

export type OriginEvidence = {
  chain: Chain;
  token: string;
  primary_label: string;
  primary_address: string | null;
  secondary_label: string | null;
  secondary_address: string | null;
  primary_balance_percentage: number | null;
  creator_label: string | null;
  launchpad: {
    name: string;
    family: string;
    evidence: string;
    source: string;
  } | null;
  active_controls: string[];
  source: string;
  detail: string;
};

export async function fetchOrigin(
  chain: Chain,
  token: string,
): Promise<OriginEvidence> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 12_000);

  try {
    const response = await fetch(`${API_URL}/v1/origin`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ chain, token }),
      signal: controller.signal,
    });

    const body = await response.json().catch(() => null);

    if (!response.ok) {
      throw new Error(body?.error ?? "Origin evidence was unavailable.");
    }

    return body;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new Error("Origin evidence took too long to verify.");
    }
    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}

export type TokenInfo = {
  image_url: string | null;
  websites: string[];
  twitter_url: string | null;
  telegram_url: string | null;
  discord_url: string | null;
  farcaster_url: string | null;
  zora_url: string | null;
  gt_verified: boolean | null;
};

export async function fetchTokenInfo(
  chain: Chain,
  address: string,
): Promise<TokenInfo> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 10_000);

  try {
    const response = await fetch(`${API_URL}/v1/token-info`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ chain, address }),
      signal: controller.signal,
    });

    const body = await response.json().catch(() => null);

    if (!response.ok) {
      throw new Error(body?.error ?? "Token metadata was unavailable.");
    }

    return body;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new Error("Token metadata took too long to load.");
    }
    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}
