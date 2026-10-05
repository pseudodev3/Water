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
  wallet_value?: WalletValue;
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
    last_state_checked_at?: number | null;
    state_error?: string | null;
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
    average_entry_usd?: string | null;
    valuation?: {
      quantity: string;
      quantity_source: string;
      quantity_observed_at: number | null;
      quantity_block?: string | null;
      price_usd: string | null;
      price_observed_at: number | null;
      source: string | null;
      detail: string;
    } | null;
  }>;
  markets?: Record<string, TokenQuote>;
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
    pricing?: ActivityPricing;
  }>;
};
export type WalletSummary = Omit<WalletAnalysis, "positions" | "notes"> & {
  /** Positive quantities with a fresh positive USD market value. */
  positions_count?: number;
  unpriced_positions_count?: number;
};

/** Also handles older cached responses that include closed positions. */
export function currentPositions(
  positions: WalletAnalysis["positions"],
  includeUnpriced = false,
) {
  return positions.filter((position) => {
    const quantity = position.valuation?.quantity ?? position.quantity;
    if (quantity == null || quantity.trim() === "") return includeUnpriced;
    const value = Number(quantity);
    if (!Number.isFinite(value)) return includeUnpriced;
    if (value <= 0) return false;
    return includeUnpriced || positiveValue(position.market_value_usd);
  });
}
export function positiveValue(value: string | null | undefined) {
  return (
    value != null &&
    value.trim() !== "" &&
    Number.isFinite(Number(value)) &&
    Number(value) > 0
  );
}
export type WalletResponse = {
  status: {
    minimum_wallet_value_usd?: string;
    screening_pool_limit?: number;
    enabled: boolean;
    nomination_enabled?: boolean;
    detail?: string | null;
    policy: string;
    interval_seconds?: number;
    collection_state?: "budget_paused" | "background_paused" | "scheduled";
    budget_resets_at?: number;
    current_refresh_seconds?: number;
    history_detail?: string | null;
    background_requests_today?: number;
    current_request_limit?: number;
    helius_budget_paused?: boolean;
    market_detail?: string | null;
    request_allocations?: Array<{
      purpose: string;
      used: number;
      limit: number;
      available_now: number;
      next_attempt_at: number;
    }>;
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

export type WalletValue = {
  status: "eligible" | "below_minimum" | "awaiting_value";
  minimum_usd: string;
  known_value_usd: string | null;
  total_complete: boolean;
  inventory_complete: boolean;
  positive_assets: number;
  unpriced_assets: number;
  balance_observed_at: number | null;
  oldest_price_at: number | null;
  balance_block: string | null;
  source: string | null;
  next_check_at: number;
  detail: string;
};

export function walletValueLabel(value?: WalletValue) {
  if (!value || value.known_value_usd == null) return "Value pending";
  return `${value.total_complete ? "" : "≥ "}${holdingValue(value.known_value_usd)}`;
}

const API = process.env.NEXT_PUBLIC_WATER_API_URL ?? "http://localhost:8080";
async function request<T>(
  route: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  const timer = window.setTimeout(abort, 30_000);
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

export type TokenQuote = {
  asset: string;
  name: string | null;
  symbol: string | null;
  decimals: number | null;
  price_usd: string | null;
  observed_at: number;
  source: string;
  detail: string;
};
export type HistoricalConversion = {
  asset: string;
  timestamp: number;
  usd: string;
  granularity: string;
  source: string;
};
export type ActivityPricing = {
  unit_price_quote: string | null;
  unit_price_usd: string | null;
  quote_conversion: HistoricalConversion | null;
  fee_asset: string | null;
  fee_quantity: string | null;
  fee_usd: string | null;
  fee_conversion: HistoricalConversion | null;
  detail: string;
};
export type TransactionEvidence = {
  tx: string;
  timestamp: number | null;
  outcome: string;
  block: number | null;
  index: number | null;
  finalized: boolean | null;
  movements: Array<{ asset: string; quantity: string }> | null;
  fee_asset: string | null;
  fee_quantity: string | null;
  swap_evidence: boolean | null;
  movement_complete: boolean | null;
  counterparties: string[] | null;
  notes: string[] | null;
  error: string | null;
  activities: WalletAnalysis["activity"];
  provider: string;
  raw: unknown;
};
export type ActivityPage = {
  transactions: TransactionEvidence[];
  markets: Record<string, TokenQuote>;
  next_cursor: string | null;
  total: number;
  revision: string;
  saved_total?: number;
  hidden_count?: number;
  include_unvalued?: boolean;
};
export const getWalletActivity = (
  chain: Chain,
  wallet: string,
  cursor: string | null,
  signal?: AbortSignal,
  includeUnvalued = false,
) =>
  request<ActivityPage>(
    "/v1/wallets/activity",
    { chain, wallet, cursor, limit: 25, include_unvalued: includeUnvalued },
    signal,
  );
export const getWalletTransaction = (
  chain: Chain,
  wallet: string,
  transaction: string,
  signal?: AbortSignal,
) =>
  request<{ transaction: TransactionEvidence }>(
    "/v1/wallets/activity",
    { chain, wallet, transaction },
    signal,
  );
export function unitPrice(value: string | null, usd = false) {
  if (value == null || value.trim() === "") return "Unavailable";
  const n = Number(value);
  if (!Number.isFinite(n)) return "Unavailable";
  return new Intl.NumberFormat("en-US", {
    maximumSignificantDigits: 9,
    ...(usd ? { style: "currency", currency: "USD" } : {}),
  }).format(n);
}
/** Holding totals use cents; small positive marks must never display as zero. */
export function holdingValue(value: string | null) {
  if (value == null || value.trim() === "") return "Unavailable";
  const n = Number(value);
  if (!Number.isFinite(n)) return "Unavailable";
  const magnitude = Math.abs(n);
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    ...(magnitude > 0 && magnitude < 0.01
      ? {
          maximumSignificantDigits: 4,
          ...(magnitude < 0.000001 ? { notation: "scientific" as const } : {}),
        }
      : { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
  }).format(n);
}
export function tokenLabel(
  asset: string,
  markets?: Record<string, TokenQuote>,
) {
  if (["SOL", "ETH", "BNB"].includes(asset)) return asset;
  const token = markets?.[asset];
  // Promotional metadata must not turn a compact activity row into an advert.
  const name = token?.name && token.name.length <= 80 ? token.name : null;
  const symbol =
    token?.symbol && token.symbol.length <= 24 ? token.symbol : null;
  return symbol
    ? `${name ?? symbol} (${symbol})`
    : (name ?? shortAddress(asset));
}

export function assetSymbol(
  asset: string,
  markets?: Record<string, TokenQuote>,
) {
  return ["SOL", "ETH", "BNB"].includes(asset)
    ? asset
    : (markets?.[asset]?.symbol ?? shortAddress(asset));
}
