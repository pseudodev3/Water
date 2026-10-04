import type { Chain } from "@/lib/api";

export type WalletCandidate = {
  chain: Chain;
  wallet: string;
  discovered_at: number;
  sources: Array<{
    name: string;
    observed_at: number;
    detail: string;
    profile: string | null;
  }>;
};
export type WalletWindow = {
  start: number;
  end: number;
  realized_usd: string | null;
  open_change_usd: string | null;
  fees_usd: string | null;
  total_usd: string | null;
  episodes: number;
  tokens: number;
  active_days: number;
  wins: number;
  losses: number;
  profit_factor: string | null;
  largest_winner_usd: string | null;
  profit_without_largest_usd: string | null;
  largest_profit_share: string | null;
  qualified: boolean;
  gates: Array<{ name: string; passed: boolean; detail: string }>;
};
export type WalletAnalysis = {
  candidate: WalletCandidate;
  analyzed_at: number;
  policy: string;
  status:
    | "qualified_60d"
    | "qualified_30d"
    | "observed"
    | "incomplete"
    | "stale";
  coverage: {
    provider: string;
    history_complete: boolean;
    ordering_complete: boolean;
    balances_reconciled: boolean;
    execution_account_verified: boolean;
    fees_complete: boolean;
    last_collected_at: number | null;
    oldest_record_at: number | null;
    newest_record_at: number | null;
    pages: number;
    pending_records: number;
    backfill_done: boolean;
    notes: string[];
  };
  windows: WalletWindow[];
  unresolved_records: number;
  records: number;
  notes: string[];
  positions: Array<{
    asset: string;
    quantity: string;
    known_cost_usd: string;
    basis_coverage: string;
    market_value_usd: string | null;
    realized_usd: string | null;
    first_acquired_at: number | null;
    last_activity_at: number | null;
  }>;
  activity: Array<{
    tx: string;
    timestamp: number;
    kind: string;
    asset: string | null;
    quantity: string | null;
    quote_asset: string | null;
    quote_quantity: string | null;
    value_usd: string | null;
    finalized: boolean;
    counterparties: string[];
  }>;
};
export type WalletSummary = Omit<WalletAnalysis, "positions" | "notes">;
export type WalletResponse = {
  status: {
    enabled: boolean;
    nomination_enabled?: boolean;
    detail?: string | null;
    policy: string;
    interval_seconds?: number;
    cohort_limit?: number;
    requests_today?: number;
    daily_request_limit?: number;
    solana_indexed_access?: boolean;
    rh_indexed_access?: boolean;
    helius_key_count?: number;
    helius_credits_reserved_31d?: number;
    helius_credit_limit_31d?: number;
    bnb_history_scope?: string;
    fomo_discovery_access?: boolean;
    last_discovery_at?: number | null;
    discovery_notes?: string[] | null;
  };
  wallets: WalletSummary[];
};

const API = process.env.NEXT_PUBLIC_WATER_API_URL ?? "http://localhost:8080";
async function request<T>(
  route: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  const timer = window.setTimeout(abort, 12_000);
  signal?.addEventListener("abort", abort, { once: true });
  if (signal?.aborted) abort();
  try {
    const response = await fetch(`${API}${route}`, {
      method: body === undefined ? "GET" : "POST",
      cache: "no-store",
      ...(body === undefined
        ? {}
        : {
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
          }),
      signal: controller.signal,
    });
    const result = await response.json().catch(() => null);
    if (!response.ok || !result)
      throw new Error(
        result?.error ?? "Wallet evidence is unavailable. Try again shortly.",
      );
    return result;
  } catch (error) {
    if (controller.signal.aborted && !signal?.aborted)
      throw new Error("Wallet evidence took too long. Try again shortly.");
    throw error;
  } finally {
    window.clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}

export const getWallets = (signal?: AbortSignal) =>
  request<WalletResponse>("/v1/wallets", undefined, signal);
export const getWalletDetail = (
  chain: Chain,
  wallet: string,
  signal?: AbortSignal,
) => request<WalletAnalysis>("/v1/wallets/detail", { chain, wallet }, signal);
export const nominateWallet = (
  chain: Chain,
  wallet: string,
  signal?: AbortSignal,
) => request<WalletAnalysis>("/v1/wallets/nominate", { chain, wallet }, signal);
export const getWalletOverlap = (
  chain: Chain,
  address: string,
  signal?: AbortSignal,
) =>
  request<{
    wallets: Array<{
      wallet: string;
      status: string;
      position: { quantity: string };
      analyzed_at: number;
    }>;
    scope?: string;
  }>("/v1/wallets/token", { chain, address }, signal);

export function walletId(candidate: WalletCandidate) {
  return `${candidate.chain}:${candidate.wallet}`;
}
export function chainLabel(chain: Chain) {
  return chain === "solana"
    ? "Solana"
    : chain === "bnb"
      ? "BNB Chain"
      : "Robinhood";
}
export function shortAddress(value: string) {
  return value.length > 18 ? `${value.slice(0, 7)}…${value.slice(-6)}` : value;
}
export function evidenceLink(
  chain: Chain,
  value: string,
  type: "address" | "tx" = "address",
) {
  return chain === "solana"
    ? `https://solscan.io/${type === "tx" ? "tx" : "account"}/${encodeURIComponent(value)}`
    : chain === "bnb"
      ? `https://bscscan.com/${type}/${encodeURIComponent(value)}`
      : `https://robinhoodchain.blockscout.com/${type}/${encodeURIComponent(value)}`;
}
export function amount(value: string | null, usd = false) {
  if (value === null) return "—";
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  return new Intl.NumberFormat(
    "en-US",
    usd
      ? { style: "currency", currency: "USD", maximumFractionDigits: 2 }
      : { maximumFractionDigits: 6 },
  ).format(n);
}
