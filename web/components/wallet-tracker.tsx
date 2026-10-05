"use client";

import {
  Activity,
  ArrowRight,
  Check,
  ChevronDown,
  ExternalLink,
  RefreshCw,
  Search,
  ShieldCheck,
  Star,
  Users,
  X,
} from "lucide-react";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";
import type { Chain } from "@/lib/api";
import {
  amount,
  chainLabel,
  evidenceLink,
  getWalletDetail,
  getWallets,
  getWalletActivity,
  getWalletTransaction,
  tokenLabel,
  assetSymbol,
  unitPrice,
  holdingValue,
  walletValueLabel,
  ActivityPage,
  TransactionEvidence,
  TokenQuote,
  nominateWallet,
  shortAddress,
  WalletAnalysis,
  WalletResponse,
  WalletSummary,
  walletId,
  currentPositions,
} from "@/lib/wallets";
import {
  CopyAddress,
  ResearchTabs,
  useResearchTabs,
  ChainAvatar,
} from "@/components/research-ui";

const FOLLOW_KEY = "water:followed-wallets:v1";
function date(timestamp: number | null) {
  return timestamp
    ? new Date(timestamp * 1000).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "Not collected";
}
function utcDate(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
    timeZoneName: "short",
  });
}
function statusLabel(status: string) {
  return (
    (
      {
        qualified_60d: "60-day consistent",
        qualified_30d: "30-day record",
        observed: "Observed",
        incomplete: "Evidence incomplete",
        stale: "Evidence stale",
      } as Record<string, string>
    )[status] ?? status
  );
}

export function WalletTracker() {
  const [response, setResponse] = useState<WalletResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [chain, setChain] = useState<Chain | "all">("all");
  const [view, setView] = useState<
    "qualified" | "research" | "followed" | "screening"
  >("research");
  const [source, setSource] = useState("all");
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState("value");
  const [followed, setFollowed] = useState<string[]>([]);
  const [detail, setDetail] = useState<WalletAnalysis | null>(null);
  const [detailLoading, setDetailLoading] = useState("");
  const [addChain, setAddChain] = useState<Chain>("solana");
  const [address, setAddress] = useState("");
  const [adding, setAdding] = useState(false);
  const listRequest = useRef<AbortController | null>(null);
  const detailRequest = useRef<AbortController | null>(null);
  const addRequest = useRef<AbortController | null>(null);
  const detailView = useRef<HTMLDivElement | null>(null);
  const openingButton = useRef<HTMLButtonElement | null>(null);
  const scrollDetail = useRef(false);

  const refresh = useCallback(async () => {
    listRequest.current?.abort();
    const controller = new AbortController();
    listRequest.current = controller;
    setLoading(true);
    try {
      const value = await getWallets(controller.signal);
      if (!controller.signal.aborted) {
        setResponse(value);
        setError("");
      }
    } catch (caught) {
      if (!controller.signal.aborted)
        setError(
          caught instanceof Error
            ? caught.message
            : "Wallet evidence is unavailable.",
        );
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }, []);

  useEffect(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(FOLLOW_KEY) ?? "[]");
      if (Array.isArray(saved))
        setFollowed(
          saved.filter((x): x is string => typeof x === "string").slice(0, 100),
        );
    } catch {
      setError("This browser could not read your followed wallets.");
    }
    void refresh();
    const timer = window.setInterval(() => {
      if (!document.hidden) void refresh();
    }, 60_000);
    return () => {
      window.clearInterval(timer);
      listRequest.current?.abort();
      detailRequest.current?.abort();
      addRequest.current?.abort();
    };
  }, [refresh]);

  function follow(candidate: WalletAnalysis["candidate"]) {
    const id = walletId(candidate);
    const next = followed.includes(id)
      ? followed.filter((x) => x !== id)
      : [...followed, id];
    try {
      localStorage.setItem(FOLLOW_KEY, JSON.stringify(next));
      setFollowed(next);
    } catch {
      setError(
        "Your browser could not save this follow. Allow local storage and try again.",
      );
    }
  }

  const open = useCallback(async (wallet: WalletSummary, scroll = true) => {
    scrollDetail.current = scroll;
    detailRequest.current?.abort();
    const controller = new AbortController();
    detailRequest.current = controller;
    const id = walletId(wallet.candidate);
    setDetailLoading(id);
    setDetail((current) =>
      current && walletId(current.candidate) === id ? current : null,
    );
    try {
      const value = await getWalletDetail(
        wallet.candidate.chain,
        wallet.candidate.wallet,
        controller.signal,
      );
      if (!controller.signal.aborted) {
        setDetail(value);
        setError("");
      }
    } catch (caught) {
      if (!controller.signal.aborted)
        setError(
          caught instanceof Error
            ? caught.message
            : "Wallet history is unavailable.",
        );
    } finally {
      if (!controller.signal.aborted) setDetailLoading("");
    }
  }, []);

  useEffect(() => {
    if (!detail) return;
    const current = response?.wallets.find(
      (w) => walletId(w.candidate) === walletId(detail.candidate),
    );
    if (current && current.analyzed_at > detail.analyzed_at)
      void open(current, false);
    else if (
      current &&
      detail.status.startsWith("qualified_") &&
      !current.status.startsWith("qualified_")
    ) {
      setDetail({
        ...detail,
        status: current.status,
        coverage: current.coverage,
        windows: current.windows,
      });
    }
  }, [response, detail, open]);

  useEffect(() => {
    if (!detail) return;
    const timer = window.setInterval(() => {
      if (!document.hidden) void open(detail, false);
    }, 60_000);
    return () => window.clearInterval(timer);
  }, [detail, open]);

  useEffect(() => {
    if (!detail || !scrollDetail.current) return;
    scrollDetail.current = false;
    const frame = window.requestAnimationFrame(() => {
      detailView.current?.focus({ preventScroll: true });
      const target = window.matchMedia("(max-width: 800px)").matches
        ? detailView.current
        : detailView.current?.closest(".wallet-workspace");
      target?.scrollIntoView({
        block: "start",
        behavior: "auto",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [detail]);

  async function add(event: FormEvent) {
    event.preventDefault();
    if (adding) return;
    const clean = address.trim();
    if (!clean) {
      setError("Paste a wallet address first.");
      return;
    }
    const controller = new AbortController();
    addRequest.current = controller;
    setAdding(true);
    setError("");
    try {
      await nominateWallet(addChain, clean, controller.signal);
      if (!controller.signal.aborted) {
        setAddress("");
        setView("research");
        await refresh();
      }
    } catch (caught) {
      if (!controller.signal.aborted)
        setError(
          caught instanceof Error
            ? caught.message
            : "The wallet could not be added.",
        );
    } finally {
      if (!controller.signal.aborted) setAdding(false);
    }
  }

  const wallets = response?.wallets ?? [];
  const collection = response?.status;
  const budgetPaused =
    collection?.collection_state === "budget_paused" ||
    (!collection?.collection_state &&
      collection?.requests_today !== undefined &&
      collection.daily_request_limit !== undefined &&
      collection.requests_today >= collection.daily_request_limit);
  const qualified = wallets.filter(
    (w) =>
      w.wallet_value?.status === "eligible" &&
      w.status.startsWith("qualified_"),
  );
  const eligible = wallets.filter((w) => w.wallet_value?.status === "eligible");
  const waiting = wallets.filter((w) => w.wallet_value?.status !== "eligible");
  const visible = wallets
    .filter(
      (w) =>
        (chain === "all" || w.candidate.chain === chain) &&
        (view === "screening"
          ? w.wallet_value?.status !== "eligible"
          : w.wallet_value?.status === "eligible") &&
        (view === "qualified"
          ? w.status.startsWith("qualified_")
          : view === "followed"
            ? followed.includes(walletId(w.candidate))
            : true) &&
        (source === "all" ||
          w.candidate.sources.some((s) =>
            s.name.toLowerCase().includes(source),
          )) &&
        [w.candidate.wallet, ...w.candidate.sources.map((s) => s.name)]
          .join(" ")
          .toLowerCase()
          .includes(search.trim().toLowerCase()),
    )
    .sort((a, b) =>
      sort === "records"
        ? b.records - a.records
        : sort === "value"
          ? Number(b.wallet_value?.known_value_usd ?? -1) -
            Number(a.wallet_value?.known_value_usd ?? -1)
          : (b.coverage.last_collected_at ?? 0) -
            (a.coverage.last_collected_at ?? 0),
    );
  const feed = wallets
    .filter(
      (w) =>
        w.wallet_value?.status === "eligible" &&
        followed.includes(walletId(w.candidate)),
    )
    .flatMap((w) =>
      w.activity.map((a) => ({
        ...a,
        candidate: w.candidate,
        markets: w.markets,
      })),
    )
    .sort((a, b) => b.timestamp - a.timestamp)
    .slice(0, 20);

  return (
    <>
      <section className="wallet-hero" aria-labelledby="wallet-title">
        <div className="eyebrow">
          <Users size={14} strokeWidth={1.5} />
          Wallet intelligence
        </div>
        <h1 id="wallet-title">Find wallets worth watching.</h1>
        <p>Follow the capital. Verify the record. See every received trade.</p>
        <div className="wallet-method">
          <ShieldCheck size={16} strokeWidth={1.5} />
          <span>
            Ranked only after history, fees, open losses and consistency checks.
          </span>
        </div>
      </section>
      <section
        className="wallet-summary"
        aria-label="Collected wallet evidence"
      >
        <div>
          <span>Qualified records</span>
          <strong>{response ? qualified.length : "—"}</strong>
        </div>
        <div>
          <span>Meets minimum</span>
          <strong>{response ? eligible.length : "—"}</strong>
        </div>
        <div>
          <span>Saved follows</span>
          <strong>{followed.length}</strong>
        </div>
        <div>
          <span>In value checks</span>
          <strong>{response ? waiting.length : "—"}</strong>
        </div>
      </section>
      {response?.status.enabled && (
        <details className="wallet-discovery-coverage">
          <summary>Discovery coverage</summary>
          {collection?.requests_today !== undefined &&
            collection.daily_request_limit !== undefined && (
              <p>
                {collection.background_requests_today !== undefined
                  ? `${collection.background_requests_today.toLocaleString()} / ${collection.daily_request_limit.toLocaleString()} background requests; ${collection.request_allocations?.find((a) => a.purpose === "current")?.used.toLocaleString() ?? "0"} / ${collection.current_request_limit?.toLocaleString() ?? "—"} fresh activity requests today.`
                  : `${collection.requests_today.toLocaleString()} / ${collection.daily_request_limit.toLocaleString()} collection requests used today.`}
                {collection.current_refresh_seconds !== undefined &&
                  ` Current history checks target every ${Math.round(collection.current_refresh_seconds / 60)} minutes; historical reconstruction continues separately.`}
              </p>
            )}
          <p>
            {response.status.solana_indexed_access
              ? "Indexed Solana history access is configured."
              : "Solana uses public history access; complete wallet qualification needs indexed history."}
          </p>
          <p>
            {response.status.rh_indexed_access
              ? "RH indexed history access is configured; archive and fee evidence are checked per execution."
              : "RH indexed history access is not configured."}
          </p>
          <p>
            {response.status.fomo_discovery_access
              ? "Independent Fomo discovery access is configured."
              : "Fomo association is unavailable until permitted discovery access is configured."}
          </p>
          <p>
            BNB uses public token-transfer history. Native-only and failed
            transactions may be missing, so complete 30/60-day qualification is
            unavailable.
          </p>
          {response.status.solana_indexed_access && (
            <p>
              {(
                response.status.helius_credits_reserved_31d ?? 0
              ).toLocaleString()}{" "}
              /{" "}
              {(
                response.status.helius_credit_limit_31d ?? 800000
              ).toLocaleString()}{" "}
              Helius credits reserved over 31 days. This is Water’s conservative
              estimate; other apps share the provider quota. Configured keys use
              one budget.
            </p>
          )}
          {response.status.discovery_notes?.map((note, i) => (
            <p key={i}>{note}</p>
          ))}
        </details>
      )}
      <section
        className="wallet-workspace"
        aria-label="Wallet discovery and ranking"
      >
        <div className="wallet-toolbar">
          <div className="wallet-views" aria-label="Wallet view">
            {(["research", "qualified", "followed", "screening"] as const).map(
              (v) => (
                <button
                  key={v}
                  type="button"
                  aria-pressed={view === v}
                  className={view === v ? "active" : ""}
                  onClick={() => {
                    setView(v);
                    detailRequest.current?.abort();
                    setDetail(null);
                    setDetailLoading("");
                  }}
                >
                  {v === "qualified"
                    ? "Qualified"
                    : v === "research"
                      ? "Discover"
                      : v === "screening"
                        ? "Value checks"
                        : "Following"}
                  {response && (
                    <span className="tab-count">
                      {v === "qualified"
                        ? qualified.length
                        : v === "research"
                          ? eligible.length
                          : v === "screening"
                            ? waiting.length
                            : eligible.filter((w) =>
                                followed.includes(walletId(w.candidate)),
                              ).length}
                    </span>
                  )}
                </button>
              ),
            )}
          </div>
          <button
            className="wallet-refresh"
            type="button"
            onClick={() => void refresh()}
            disabled={loading}
            aria-label="Refresh wallet evidence"
          >
            <RefreshCw size={16} className={loading ? "spin" : ""} />
            <span>Refresh</span>
          </button>
        </div>
        <div className="wallet-filters" data-open={filtersOpen}>
          <label id="wallet-chain-filter">
            Chain
            <select
              aria-label="Chain"
              value={chain}
              onChange={(e) => {
                setChain(e.target.value as Chain | "all");
                detailRequest.current?.abort();
                setDetail(null);
                setDetailLoading("");
              }}
            >
              <option value="all">All chains</option>
              <option value="solana">Solana</option>
              <option value="robinhood">Robinhood</option>
              <option value="bnb">BNB Chain</option>
            </select>
          </label>
          <label id="wallet-source-filter">
            Discovery
            <select
              aria-label="Discovery"
              value={source}
              onChange={(e) => {
                setSource(e.target.value);
                detailRequest.current?.abort();
                setDetail(null);
                setDetailLoading("");
              }}
            >
              <option value="all">All sources</option>
              <option value="pump">Pump.fun</option>
              <option value="fomo">Fomo</option>
              <option value="onchain">Onchain activity</option>
            </select>
          </label>
          <label className="wallet-search">
            <Search size={15} aria-hidden="true" />
            <input
              aria-label="Search collected wallets"
              placeholder="Search address or source"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              type="search"
            />
          </label>
          <label id="wallet-sort-filter">
            Sort
            <select
              aria-label="Sort wallets"
              value={sort}
              onChange={(event) => setSort(event.target.value)}
            >
              <option value="value">Highest wallet value</option>
              <option value="latest">Latest collection</option>
              <option value="records">Records saved</option>
            </select>
          </label>
          <button
            className="wallet-filter-toggle"
            type="button"
            aria-label="Wallet filters"
            aria-expanded={filtersOpen}
            aria-controls="wallet-chain-filter wallet-source-filter wallet-sort-filter"
            onClick={() => setFiltersOpen(!filtersOpen)}
          >
            Filters
            {chain !== "all" || source !== "all" || sort !== "value"
              ? ` (${Number(chain !== "all") + Number(source !== "all") + Number(sort !== "value")})`
              : ""}
            <ChevronDown size={14} aria-hidden="true" />
          </button>
        </div>
        <p className="wallet-floor-note">
          <ShieldCheck size={14} aria-hidden="true" />
          Collection starts at{" "}
          {holdingValue(collection?.minimum_wallet_value_usd ?? "1000")} in
          native coins and tokens. Missing values receive limited checks; saved
          history stays available.
        </p>
        {error && (
          <div className="error-line" role="alert">
            {error}
          </div>
        )}
        {response && !response.status.enabled && (
          <p className="wallet-collection-note" role="status">
            Collection is paused. Wallet history is not being added yet.
          </p>
        )}
        {response?.status.enabled &&
          (budgetPaused || response.status.detail) && (
            <p className="wallet-collection-note" role="status">
              {budgetPaused
                ? `Collection is paused: today’s request budget is used. It resumes ${collection?.budget_resets_at ? `at ${utcDate(collection.budget_resets_at)}` : "after UTC midnight"}. Saved records and observed activity remain available below.`
                : `${response.status.detail} Last received evidence is preserved.`}
            </p>
          )}
        {collection?.collection_state === "background_paused" && (
          <p className="wallet-collection-note" role="status">
            Historical reconstruction and discovery have used today’s
            allocation. Fresh activity checks and market enrichment continue
            within their own provider limits.
          </p>
        )}
        {collection?.helius_budget_paused && (
          <p className="wallet-collection-note" role="status">
            Solana’s rolling credit allocation is exhausted. Received evidence
            is retained; Solana collection resumes as credits become available.
          </p>
        )}
        <div className={`wallet-master-detail ${detail ? "has-detail" : ""}`}>
          <div className="wallet-master">
            {loading && !response ? (
              <div className="wallet-empty" role="status">
                <Activity size={22} className="spin" />
                <h2>Reading wallet evidence…</h2>
              </div>
            ) : !response && error ? (
              <div className="wallet-empty wallet-unavailable">
                <Activity size={25} aria-hidden="true" />
                <h2>Wallet service unavailable</h2>
                <p>
                  The API did not return wallet data. This does not tell us
                  whether saved wallets or history exist. Retry when the service
                  is responding.
                </p>
                <button
                  type="button"
                  className="wallet-inline-button"
                  onClick={() => void refresh()}
                  disabled={loading}
                >
                  Retry connection <RefreshCw size={15} aria-hidden="true" />
                </button>
              </div>
            ) : visible.length === 0 ? (
              <div className="wallet-empty">
                <ShieldCheck size={25} strokeWidth={1.4} />
                <h2>
                  {search.trim()
                    ? "No matching wallets"
                    : view === "qualified"
                      ? "The record comes first."
                      : view === "followed"
                        ? "Your watch begins here."
                        : view === "screening"
                          ? "All wallet values are checked."
                          : `Checking for wallets at ${holdingValue(collection?.minimum_wallet_value_usd ?? "1000")}+.`}
                </h2>
                <p>
                  {search.trim()
                    ? "Try another address or clear the search. Chain and discovery filters still apply."
                    : view === "qualified"
                      ? "No collected wallet meets these filters and the qualification checks yet. Incomplete history stays under research."
                      : view === "followed"
                        ? "Follow a wallet from the research list to bring its activity together here. Following does not verify performance."
                        : view === "screening"
                          ? "Wallets with missing values or holdings below the minimum appear here."
                          : "Wallets appear here once received holdings meet the minimum. Open Value checks to inspect balances and remaining gaps."}
                </p>
                {(view === "qualified" || view === "research") && (
                  <button
                    type="button"
                    className="wallet-inline-button"
                    onClick={() =>
                      setView(view === "research" ? "screening" : "research")
                    }
                  >
                    {view === "research"
                      ? "View value checks"
                      : "Discover wallets"}{" "}
                    <ArrowRight size={15} />
                  </button>
                )}
              </div>
            ) : (
              <div className="wallet-list">
                <div className="wallet-list-head">
                  <span>Wallet / source</span>
                  <span>Wallet value · USD</span>
                  <span>History status</span>
                  <span>Latest activity</span>
                </div>
                {visible.map((w) => (
                  <div className="wallet-row" key={walletId(w.candidate)}>
                    <button
                      className="wallet-row-main"
                      type="button"
                      onClick={(event) => {
                        openingButton.current = event.currentTarget;
                        void open(w);
                      }}
                      aria-busy={detailLoading === walletId(w.candidate)}
                      aria-expanded={
                        detail?.candidate.wallet === w.candidate.wallet &&
                        detail.candidate.chain === w.candidate.chain
                      }
                      data-selected={
                        detail?.candidate.wallet === w.candidate.wallet &&
                        detail.candidate.chain === w.candidate.chain
                      }
                    >
                      <span className="wallet-identity">
                        <ChainAvatar chain={w.candidate.chain} />
                        <span>
                          <strong>{shortAddress(w.candidate.wallet)}</strong>
                          <small>
                            {chainLabel(w.candidate.chain)} ·{" "}
                            {w.candidate.sources.map((s) => s.name).join(" / ")}
                          </small>
                        </span>
                      </span>
                      <span className="wallet-capital">
                        <small className="mobile-field-label">
                          Wallet value · USD
                        </small>
                        <strong>{walletValueLabel(w.wallet_value)}</strong>
                        <small>
                          {w.wallet_value?.status === "below_minimum"
                            ? "Below minimum · paused"
                            : w.wallet_value?.status === "eligible"
                              ? "Meets collection minimum"
                              : "Value check pending"}
                        </small>
                      </span>
                      <span
                        className={`wallet-record ${w.status.startsWith("qualified_") ? "qualified" : ""}`}
                      >
                        {statusLabel(w.status)}
                        <small className="wallet-record-progress">
                          {w.records.toLocaleString()} saved ·{" "}
                          {w.coverage.pending_records.toLocaleString()} awaiting
                          reconstruction
                        </small>
                      </span>
                      <span className="wallet-pnl">
                        <small className="mobile-field-label">
                          {w.status.startsWith("qualified_")
                            ? "Latest 30-day result"
                            : "Latest execution"}
                        </small>
                        {w.status.startsWith("qualified_")
                          ? amount(w.windows[1]?.total_usd ?? null, true)
                          : w.coverage.newest_record_at
                            ? date(w.coverage.newest_record_at)
                            : "No execution received"}
                      </span>
                      <span className="wallet-row-open">
                        {detailLoading === walletId(w.candidate) ? (
                          <Activity size={14} className="spin" />
                        ) : (
                          <ChevronDown size={14} />
                        )}
                      </span>
                    </button>
                    <button
                      className={`wallet-follow ${followed.includes(walletId(w.candidate)) ? "active" : ""}`}
                      type="button"
                      aria-pressed={followed.includes(walletId(w.candidate))}
                      aria-label={`${followed.includes(walletId(w.candidate)) ? "Unfollow" : "Follow"} ${w.candidate.wallet}`}
                      onClick={() => follow(w.candidate)}
                    >
                      <Star size={17} />
                    </button>
                  </div>
                ))}
              </div>
            )}
            {detailLoading && (
              <p className="wallet-detail-loading" role="status">
                <Activity size={15} className="spin" />
                Loading saved wallet evidence…
              </p>
            )}
          </div>
          {detail && (
            <div
              ref={detailView}
              tabIndex={-1}
              role="group"
              aria-label={`Wallet profile ${shortAddress(detail.candidate.wallet)}`}
            >
              <WalletDetail
                key={walletId(detail.candidate)}
                wallet={detail}
                onClose={() => {
                  detailRequest.current?.abort();
                  setDetail(null);
                  window.requestAnimationFrame(() =>
                    openingButton.current?.focus(),
                  );
                }}
              />
            </div>
          )}
        </div>
        {response?.status.nomination_enabled && (
          <details className="wallet-nominate">
            <summary>Nominate an execution wallet</summary>
            <p>
              Automatic discovery runs separately. A nomination starts evidence
              collection; it does not give a wallet a rank.
            </p>
            <form onSubmit={add}>
              <label className="wallet-add-chain">
                Chain
                <select
                  value={addChain}
                  onChange={(e) => setAddChain(e.target.value as Chain)}
                >
                  <option value="solana">Solana</option>
                  <option value="robinhood">Robinhood</option>
                  <option value="bnb">BNB Chain</option>
                </select>
              </label>
              <label className="wallet-add-address">
                Execution wallet
                <input
                  value={address}
                  onChange={(e) => setAddress(e.target.value)}
                  placeholder={
                    addChain === "solana"
                      ? "Solana wallet address"
                      : "0x wallet address"
                  }
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                />
              </label>
              <button
                className="wallet-inline-button"
                disabled={adding || !response?.status.enabled}
              >
                {adding ? "Adding…" : "Start research"}
                <ArrowRight size={15} />
              </button>
            </form>
          </details>
        )}
      </section>
      {view === "followed" && (
        <section className="wallet-feed" aria-labelledby="activity-title">
          <div className="wallet-section-heading">
            <h2 id="activity-title">What happened next.</h2>
            <p>
              Observed executions and transfers. Qualification is shown
              separately.
            </p>
          </div>
          {feed.length ? (
            feed.map((a, i) => (
              <div
                className="wallet-feed-row"
                key={`${walletId(a.candidate)}:${a.tx}:${i}`}
              >
                <div>
                  <strong>
                    {a.kind.replaceAll("_", " ")}
                    {!a.finalized && " · provisional"}
                  </strong>
                  <small>
                    {shortAddress(a.candidate.wallet)} ·{" "}
                    {chainLabel(a.candidate.chain)} ·{" "}
                    {a.asset
                      ? tokenLabel(a.asset, a.markets)
                      : "Amounts unverified"}
                  </small>
                </div>
                <span>
                  {unitPrice(a.quantity)}
                  <small>
                    {a.value_usd === null
                      ? "USD value unavailable"
                      : `Est. ${unitPrice(a.value_usd, true)}`}
                  </small>
                </span>
                <a
                  href={evidenceLink(a.candidate.chain, a.tx, "tx")}
                  target="_blank"
                  rel="noreferrer"
                >
                  {date(a.timestamp)}
                  <ExternalLink size={13} />
                </a>
              </div>
            ))
          ) : (
            <p className="wallet-feed-empty">
              No saved activity for followed wallets yet. Collection continues
              independently of this page.
            </p>
          )}
        </section>
      )}
      <details className="wallet-principles">
        <summary>How wallet qualification works</summary>
        <h2>Consistency has a paper trail.</h2>
        <p>
          Two profitable months, enough completed positions, losses included,
          and profit that survives removing the largest winner. A large balance
          or an app badge does not establish that record.
        </p>
        <p>
          Discovery covers a bounded sample. Historical USD conversions are
          estimates. A shared funder does not establish shared ownership, and
          past profit does not measure the price or liquidity available to
          someone following later.
        </p>
      </details>
    </>
  );
}

function WalletDetail({
  wallet: w,
  onClose,
}: {
  wallet: WalletAnalysis;
  onClose: () => void;
}) {
  const chain = w.candidate.chain;
  const [positionLimit, setPositionLimit] = useState(20);
  const [assetSearch, setAssetSearch] = useState("");
  const [positionSort, setPositionSort] = useState("value-desc");
  const [includeUnpriced, setIncludeUnpriced] = useState(false);
  const tabs = useResearchTabs<
    "activity" | "positions" | "performance" | "evidence"
  >("activity");
  const valuedHoldings = currentPositions(w.positions);
  const allHoldings = currentPositions(w.positions, true);
  const holdings = includeUnpriced ? allHoldings : valuedHoldings;
  const positions = [...holdings]
    .sort((a, b) => {
      const recent = (b.last_activity_at ?? 0) - (a.last_activity_at ?? 0);
      if (positionSort === "recent") return recent;
      const value = (p: WalletAnalysis["positions"][number]) => {
        if (p.market_value_usd == null || p.market_value_usd.trim() === "")
          return null;
        const usd = Number(p.market_value_usd);
        return Number.isFinite(usd) ? usd : null;
      };
      const av = value(a);
      const bv = value(b);
      if (av === null) return bv === null ? recent : 1;
      if (bv === null) return -1;
      return (positionSort === "value-desc" ? bv - av : av - bv) || recent;
    })
    .filter((p) =>
      [p.asset, tokenLabel(p.asset, w.markets)]
        .join(" ")
        .toLowerCase()
        .includes(assetSearch.trim().toLowerCase()),
    );
  return (
    <section
      className="wallet-detail"
      aria-label={`Evidence for ${w.candidate.wallet}`}
    >
      <div className="wallet-detail-title">
        <div>
          <div className="eyebrow">Wallet profile · {chainLabel(chain)}</div>
          <h2>{shortAddress(w.candidate.wallet)}</h2>
        </div>
        <button
          type="button"
          className="wallet-close"
          onClick={onClose}
          aria-label="Close wallet detail"
        >
          <X size={19} />
        </button>
      </div>
      <div className="wallet-address-line">
        <a
          className="wallet-full-address"
          href={evidenceLink(chain, w.candidate.wallet)}
          target="_blank"
          rel="noreferrer"
        >
          {w.candidate.wallet}
          <ExternalLink size={14} />
        </a>
        <CopyAddress value={w.candidate.wallet} label="wallet address" />
      </div>
      <p className="wallet-detail-status">
        {statusLabel(w.status)} · As of {date(w.analyzed_at)}
      </p>
      <div className="wallet-value-hero">
        <span>Wallet holdings · USD</span>
        <strong>{walletValueLabel(w.wallet_value)}</strong>
        <small>
          Native coins + tokens ·{" "}
          {w.wallet_value?.total_complete
            ? "Received inventory valued"
            : "Known value; remaining holdings may be unpriced"}
        </small>
      </div>
      <div className="profile-metrics">
        <div>
          <span>Saved records</span>
          <strong>{w.records.toLocaleString()}</strong>
        </div>
        <div>
          <span>Unresolved</span>
          <strong>{w.unresolved_records.toLocaleString()}</strong>
        </div>
        <div>
          <span>Valued positions</span>
          <strong>{valuedHoldings.length.toLocaleString()}</strong>
        </div>
        <div>
          <span>History</span>
          <strong className="metric-word">
            {w.coverage.history_complete ? "Complete" : "Incomplete"}
          </strong>
        </div>
      </div>
      <ResearchTabs
        id={tabs.id}
        active={tabs.active}
        onChange={tabs.select}
        label="Wallet profile views"
        tabs={[
          { id: "activity", label: "Activity" },
          { id: "positions", label: "Positions", count: holdings.length },
          { id: "performance", label: "Performance" },
          { id: "evidence", label: "Evidence" },
        ]}
      />
      <div {...tabs.panel("activity")}>
        <WalletTransactions key={walletId(w.candidate)} wallet={w} />
      </div>
      <div {...tabs.panel("performance")}>
        <p className="view-description">
          Each 30-day window is assessed separately. Incomplete records do not
          establish profitability.
        </p>
        <div className="wallet-window-grid">
          {w.windows.map((window, i) => (
            <section key={window.start} className="wallet-window">
              <div className="eyebrow">
                {i === 0 ? "Previous 30 days" : "Latest 30 days"}
              </div>
              <h3>
                {window.qualified ? "Checks passed" : "Record under review"}
              </h3>
              <p>
                {date(window.start)} — {date(window.end)}
              </p>
              <dl>
                <div>
                  <dt>Realized result</dt>
                  <dd>{amount(window.realized_usd, true)}</dd>
                </div>
                <div>
                  <dt>Change in open PnL</dt>
                  <dd>{amount(window.open_change_usd, true)}</dd>
                </div>
                <div>
                  <dt>Network fees</dt>
                  <dd>{amount(window.fees_usd, true)}</dd>
                </div>
                <div>
                  <dt>Total after fees</dt>
                  <dd>{amount(window.total_usd, true)}</dd>
                </div>
                <div>
                  <dt>Completed episodes</dt>
                  <dd>{window.episodes}</dd>
                </div>
                <div>
                  <dt>Winning / losing episodes</dt>
                  <dd>
                    {window.wins} / {window.losses}
                  </dd>
                </div>
                <div>
                  <dt>Active days</dt>
                  <dd>{window.active_days}</dd>
                </div>
                <div>
                  <dt>Profit factor</dt>
                  <dd>{amount(window.profit_factor)}</dd>
                </div>
              </dl>
              <p className="wallet-value-note">
                Available values describe reconstructed evidence. A rank
                requires every check below.
              </p>
              <details className="wallet-qualification-checks">
                <summary>
                  {window.gates.filter((g) => g.passed).length} /{" "}
                  {window.gates.length} qualification checks passed
                </summary>
                <ul className="wallet-gates">
                  {window.gates.map((g) => (
                    <li key={g.name}>
                      <span className={g.passed ? "pass" : "pending"}>
                        {g.passed ? (
                          <Check size={14} />
                        ) : (
                          <span aria-hidden="true">—</span>
                        )}
                      </span>
                      <div>
                        <strong>{g.name}</strong>
                        <p>{g.detail}</p>
                      </div>
                    </li>
                  ))}
                </ul>
              </details>
            </section>
          ))}
        </div>
      </div>
      <div {...tabs.panel("positions")}>
        <div className="wallet-positions">
          <div className="view-heading">
            <h3>Positions</h3>
            <span>
              {positions.length} of {holdings.length} assets
            </span>
          </div>
          <p className="view-description">
            Showing holdings with a positive USD value. Unpriced and zero-value
            holdings are hidden by default; missing prices do not prove zero
            value. Closed trades remain in Activity and Performance.
          </p>
          {allHoldings.length > valuedHoldings.length && (
            <label className="wallet-visibility-filter">
              <input
                type="checkbox"
                checked={includeUnpriced}
                onChange={(event) => {
                  setIncludeUnpriced(event.target.checked);
                  setPositionLimit(20);
                }}
              />
              Show unpriced or zero-value holdings (
              {allHoldings.length - valuedHoldings.length})
            </label>
          )}
          <div className="wallet-position-controls">
            <label className="wallet-search position-search">
              <Search size={15} aria-hidden="true" />
              <input
                type="search"
                aria-label="Search wallet assets"
                placeholder="Search token, ticker or address"
                value={assetSearch}
                onChange={(event) => {
                  setAssetSearch(event.target.value);
                  setPositionLimit(20);
                }}
              />
            </label>
            <label className="wallet-position-sort">
              <span>Sort by</span>
              <select
                aria-label="Sort wallet holdings"
                value={positionSort}
                onChange={(event) => {
                  setPositionSort(event.target.value);
                  setPositionLimit(20);
                }}
              >
                <option value="value-desc">Highest value first</option>
                <option value="value-asc">Lowest value first</option>
                <option value="recent">Recent activity</option>
              </select>
            </label>
          </div>
          <div className="wallet-position-head" aria-hidden="true">
            <span>Token</span>
            <span>Quantity</span>
            <span>Holding value (USD)</span>
            <span>Basis coverage</span>
          </div>
          {positions.length ? (
            positions.slice(0, positionLimit).map((p) => (
              <div className="wallet-position-row" key={p.asset}>
                <a
                  href={evidenceLink(chain, p.asset)}
                  target="_blank"
                  rel="noreferrer"
                >
                  <span>
                    {tokenLabel(p.asset, w.markets)}
                    <small>{shortAddress(p.asset)}</small>
                  </span>
                  <ExternalLink size={12} />
                </a>
                <span className="wallet-position-quantity">
                  <small className="mobile-field-label">Quantity</small>
                  {unitPrice(p.valuation?.quantity ?? p.quantity)} tokens
                </span>
                <span className="wallet-position-value">
                  <small className="mobile-field-label">
                    Holding value · USD
                  </small>
                  <strong
                    className={
                      p.market_value_usd === null
                        ? "value-unavailable"
                        : undefined
                    }
                  >
                    {holdingValue(p.market_value_usd)}
                  </strong>
                  <small>
                    {p.valuation?.price_usd
                      ? `Price / token ${unitPrice(p.valuation.price_usd, true)}`
                      : "Price unavailable"}
                  </small>
                </span>
                <span>
                  <small className="mobile-field-label">Basis coverage</small>
                  {amount(String(Number(p.basis_coverage) * 100))}% known
                  <small>
                    Avg. entry / token{" "}
                    {unitPrice(p.average_entry_usd ?? null, true)}
                  </small>
                  <a
                    className="wallet-inspect-token"
                    href={`/?chain=${chain}&address=${encodeURIComponent(p.asset)}`}
                  >
                    Inspect token <ArrowRight size={12} />
                  </a>
                </span>
                <details className="position-evidence">
                  <summary>Balance &amp; price evidence</summary>
                  <dl>
                    <div>
                      <dt>Quantity source</dt>
                      <dd>
                        {p.valuation?.quantity_source ??
                          "Reconstructed from saved transactions"}
                      </dd>
                    </div>
                    <div>
                      <dt>Quantity observed</dt>
                      <dd>
                        {date(
                          p.valuation?.quantity_observed_at ?? w.analyzed_at,
                        )}
                      </dd>
                    </div>
                    {p.valuation?.quantity_block && (
                      <div>
                        <dt>Block / slot</dt>
                        <dd>{p.valuation.quantity_block}</dd>
                      </div>
                    )}
                    <div>
                      <dt>Price source</dt>
                      <dd>{p.valuation?.source ?? "Not received"}</dd>
                    </div>
                    <div>
                      <dt>Price observed</dt>
                      <dd>
                        {p.valuation?.price_observed_at
                          ? date(p.valuation.price_observed_at)
                          : "Not received"}
                      </dd>
                    </div>
                  </dl>
                  {p.valuation?.detail && <p>{p.valuation.detail}</p>}
                </details>
              </div>
            ))
          ) : (
            <p>
              {assetSearch.trim()
                ? "No assets match this search."
                : w.records || w.coverage.last_state_checked_at
                  ? includeUnpriced
                    ? "No non-zero token balances in the saved evidence."
                    : "No holdings with a received positive USD value."
                  : "No token balances have been collected yet."}
            </p>
          )}
          {positions.length > positionLimit && (
            <button
              type="button"
              className="wallet-inline-button"
              onClick={() => setPositionLimit((n) => n + 20)}
            >
              Show more assets ({positionLimit} of {positions.length}){" "}
              <ArrowRight size={14} />
            </button>
          )}
        </div>
      </div>
      <div {...tabs.panel("evidence")}>
        <div className="wallet-coverage">
          <h3>Collection value check.</h3>
          <p>
            {w.wallet_value?.detail ?? "Wallet value has not been checked yet."}
          </p>
          <dl>
            <div>
              <dt>Received wallet value</dt>
              <dd>{walletValueLabel(w.wallet_value)}</dd>
            </div>
            <div>
              <dt>Balance source</dt>
              <dd>{w.wallet_value?.source ?? "Not received"}</dd>
            </div>
            <div>
              <dt>Balances checked</dt>
              <dd>{date(w.wallet_value?.balance_observed_at ?? null)}</dd>
            </div>
            <div>
              <dt>Next value check</dt>
              <dd>{date(w.wallet_value?.next_check_at || null)}</dd>
            </div>
            <div>
              <dt>Inventory</dt>
              <dd>
                {w.wallet_value?.inventory_complete
                  ? "Wallet token accounts received"
                  : "Partial; completeness unproved"}
              </dd>
            </div>
            <div>
              <dt>Unpriced holdings</dt>
              <dd>{w.wallet_value?.unpriced_assets ?? "Unknown"}</dd>
            </div>
            {w.wallet_value?.balance_block && (
              <div>
                <dt>Block / slots</dt>
                <dd>{w.wallet_value.balance_block}</dd>
              </div>
            )}
          </dl>
          <h3>What the history covers.</h3>
          <dl>
            <div>
              <dt>Current history checked</dt>
              <dd>{date(w.coverage.last_collected_at)}</dd>
            </div>
            <div>
              <dt>Account and balances checked</dt>
              <dd>{date(w.coverage.last_state_checked_at ?? null)}</dd>
            </div>
            <div>
              <dt>Records saved</dt>
              <dd>{w.records}</dd>
            </div>
            <div>
              <dt>Unresolved records</dt>
              <dd>{w.unresolved_records}</dd>
            </div>
            <div>
              <dt>Oldest execution</dt>
              <dd>{date(w.coverage.oldest_record_at)}</dd>
            </div>
            <div>
              <dt>Latest execution</dt>
              <dd>{date(w.coverage.newest_record_at)}</dd>
            </div>
            <div>
              <dt>History</dt>
              <dd>{w.coverage.history_complete ? "Complete" : "Incomplete"}</dd>
            </div>
            <div>
              <dt>Execution account</dt>
              <dd>
                {w.coverage.execution_account_verified
                  ? "Supported wallet"
                  : "Unverified"}
              </dd>
            </div>
            <div>
              <dt>Ending balances</dt>
              <dd>
                {w.coverage.balances_reconciled ? "Reconciled" : "Unverified"}
              </dd>
            </div>
          </dl>
          {w.coverage.state_error && (
            <p>Latest balance read: {w.coverage.state_error}</p>
          )}
          {w.coverage.notes.map((note, i) => (
            <p key={i}>{note}</p>
          ))}
        </div>
        <div className="wallet-sources">
          <h3>Discovery evidence.</h3>
          {w.candidate.sources.map((s, i) => (
            <div key={i}>
              <strong>
                {s.name}
                {s.profile ? ` · ${s.profile}` : ""}
              </strong>
              <p>{s.detail}</p>
              <small>{date(s.observed_at)}</small>
            </div>
          ))}
        </div>
        <div className="wallet-detail-notes">
          {w.notes.map((note) => (
            <p key={note}>{note}</p>
          ))}
        </div>
      </div>
    </section>
  );
}

function WalletTransactions({ wallet: w }: { wallet: WalletAnalysis }) {
  const [page, setPage] = useState<ActivityPage | null>(null);
  const [rows, setRows] = useState<TransactionEvidence[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [includeUnvalued, setIncludeUnvalued] = useState(false);
  const controller = useRef<AbortController | null>(null);
  const load = useCallback(
    async (cursor: string | null) => {
      controller.current?.abort();
      const request = new AbortController();
      controller.current = request;
      setBusy(true);
      setError("");
      try {
        const next = await getWalletActivity(
          w.candidate.chain,
          w.candidate.wallet,
          cursor,
          request.signal,
          includeUnvalued,
        );
        if (!request.signal.aborted) {
          setPage(next);
          setRows((old) =>
            cursor ? [...old, ...next.transactions] : next.transactions,
          );
        }
      } catch (caught) {
        if (!request.signal.aborted)
          setError(
            caught instanceof Error
              ? caught.message
              : "Activity is unavailable.",
          );
      } finally {
        if (!request.signal.aborted) setBusy(false);
      }
    },
    [w.candidate.chain, w.candidate.wallet, includeUnvalued],
  );
  useEffect(() => {
    setRows([]);
    setPage(null);
    void load(null);
    return () => controller.current?.abort();
  }, [load, w.analyzed_at]);
  return (
    <div className="wallet-detail-activity">
      <h3>Transactions, with the evidence.</h3>
      <p className="wallet-value-note">
        Trades and executions stay visible. Transfer-only records without a
        received value are hidden by default. Entry and exit prices use received
        swap amounts. USD conversions use historical candles; fees are shown
        separately.
      </p>
      <label className="wallet-visibility-filter">
        <input
          type="checkbox"
          checked={includeUnvalued}
          onChange={(event) => setIncludeUnvalued(event.target.checked)}
        />
        Show transfers without a received value
      </label>
      {page && (
        <p className="wallet-value-note">
          Showing {rows.length.toLocaleString()} of{" "}
          {page.total.toLocaleString()} {includeUnvalued ? "saved" : "visible"}{" "}
          transactions
          {page.hidden_count
            ? ` · ${page.hidden_count.toLocaleString()} hidden from ${page.saved_total?.toLocaleString()} saved`
            : ""}
          . Source history{" "}
          {w.coverage.history_complete ? "complete" : "incomplete"}.
        </p>
      )}
      {rows.map((row) => (
        <TransactionRow
          key={row.tx}
          row={row}
          chain={w.candidate.chain}
          wallet={w.candidate.wallet}
          markets={page?.markets ?? w.markets}
        />
      ))}
      {!page && !busy && !error && (
        <p>No transaction records have been saved.</p>
      )}
      {page && !rows.length && !busy && !error && (
        <p>
          {page.hidden_count
            ? "Only transfers without a received value have been collected. Enable the filter above to inspect them."
            : "No transaction records have been saved."}
        </p>
      )}
      {busy && <p role="status">Loading saved transactions…</p>}
      {error && (
        <div role="alert">
          <p>{error}</p>
          <button
            className="wallet-inline-button"
            type="button"
            onClick={() => void load(null)}
          >
            Reload activity <RefreshCw size={14} />
          </button>
        </div>
      )}
      {page?.next_cursor && !error && (
        <button
          className="wallet-inline-button"
          type="button"
          disabled={busy}
          onClick={() => void load(page.next_cursor)}
        >
          Load more transactions <ArrowRight size={14} />
        </button>
      )}
      {!page &&
        error &&
        w.activity.slice(0, 12).map((a, i) => (
          <div className="wallet-feed-row" key={`${a.tx}:${i}`}>
            <div>
              <strong>{a.kind.replaceAll("_", " ")}</strong>
              <small>
                {a.asset
                  ? tokenLabel(a.asset, w.markets)
                  : "Amounts unverified"}
              </small>
            </div>
            <span>{unitPrice(a.quantity)}</span>
            <a
              href={evidenceLink(w.candidate.chain, a.tx, "tx")}
              target="_blank"
              rel="noreferrer"
            >
              {date(a.timestamp)}
              <ExternalLink size={12} />
            </a>
          </div>
        ))}
    </div>
  );
}
function TransactionRow({
  row,
  chain,
  wallet,
  markets,
}: {
  row: TransactionEvidence;
  chain: Chain;
  wallet: string;
  markets?: Record<string, TokenQuote>;
}) {
  const [raw, setRaw] = useState<TransactionEvidence | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), []);
  const trade = row.activities.find((a) =>
    ["entry", "addition", "exit", "partial_exit"].includes(a.kind),
  );
  const kind =
    trade?.kind ??
    row.activities.find(
      (a) =>
        !["unresolved_execution", "provisional_transaction"].includes(a.kind),
    )?.kind ??
    row.outcome;
  const observedAsset = row.movements?.find(
    (d) => !["SOL", "ETH", "BNB"].includes(d.asset),
  )?.asset;
  const label =
    (
      {
        entry: "Buy",
        addition: "Buy · addition",
        exit: "Sell · exit",
        partial_exit: "Sell · partial exit",
        failed_transaction: "Failed transaction",
        awaiting_evidence: "Awaiting transaction evidence",
      } as Record<string, string>
    )[kind] ?? kind.replaceAll("_", " ");
  const conversion = trade?.pricing?.quote_conversion;
  async function inspect() {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setBusy(true);
    setError("");
    try {
      const value = await getWalletTransaction(
        chain,
        wallet,
        row.tx,
        controller.signal,
      );
      if (!controller.signal.aborted) setRaw(value.transaction);
    } catch (caught) {
      if (!controller.signal.aborted)
        setError(
          caught instanceof Error
            ? caught.message
            : "Source record is unavailable.",
        );
    } finally {
      if (!controller.signal.aborted) setBusy(false);
    }
  }
  return (
    <details className="wallet-transaction" data-kind={kind}>
      <summary>
        <span>
          <strong>
            {label}
            {(trade?.asset ?? observedAsset)
              ? ` · ${tokenLabel((trade?.asset ?? observedAsset)!, markets)}`
              : ""}
          </strong>
          <small>
            {row.timestamp === null ? "Time not received" : date(row.timestamp)}{" "}
            · {row.outcome.replaceAll("_", " ")}
          </small>
        </span>
        <span>
          <span>
            {trade?.quantity
              ? `${unitPrice(trade.quantity)} tokens`
              : row.movements?.length
                ? `${row.movements.length} received movements`
                : "Amounts pending"}
            {trade && (
              <small>
                {trade.pricing?.unit_price_usd
                  ? `Est. ${unitPrice(trade.pricing.unit_price_usd, true)} / token`
                  : `${unitPrice(trade.pricing?.unit_price_quote ?? null)} ${trade.quote_asset ?? ""} / token`}
              </small>
            )}
          </span>
          <ChevronDown size={14} />
        </span>
      </summary>
      <div className="wallet-transaction-evidence">
        <a
          className="wallet-full-address"
          href={evidenceLink(chain, row.tx, "tx")}
          target="_blank"
          rel="noreferrer"
        >
          {row.tx}
          <ExternalLink size={14} />
        </a>
        <dl>
          <div>
            <dt>Block / slot</dt>
            <dd>{row.block?.toLocaleString() ?? "Not received"}</dd>
          </div>
          <div>
            <dt>Order within block</dt>
            <dd>{row.index ?? "Not received"}</dd>
          </div>
          <div>
            <dt>Finality</dt>
            <dd>
              {row.finalized === true
                ? "Finalized"
                : row.finalized === false
                  ? "Provisional"
                  : "Unknown"}
            </dd>
          </div>
        </dl>
        {trade && (
          <dl>
            <div>
              <dt>
                {trade.kind === "entry" || trade.kind === "addition"
                  ? "Entry"
                  : "Exit"}{" "}
                price per token
              </dt>
              <dd>
                {trade.pricing?.unit_price_quote ?? "Unavailable"}{" "}
                {trade.quote_asset
                  ? assetSymbol(trade.quote_asset, markets)
                  : ""}
              </dd>
            </div>
            <div>
              <dt>Estimated USD per token</dt>
              <dd>{unitPrice(trade.pricing?.unit_price_usd ?? null, true)}</dd>
            </div>
            <div>
              <dt>Quote amount</dt>
              <dd>
                {trade.quote_quantity ?? "Unavailable"}{" "}
                {trade.quote_asset
                  ? assetSymbol(trade.quote_asset, markets)
                  : ""}
              </dd>
            </div>
            <div>
              <dt>Estimated USD trade amount</dt>
              <dd>{unitPrice(trade.value_usd, true)}</dd>
            </div>
          </dl>
        )}
        {trade?.pricing?.detail && <p>{trade.pricing.detail}</p>}
        {trade && !conversion && (
          <p>
            Historical quote-to-USD evidence has not been received. The actual
            quote amount and quote unit price remain available.
          </p>
        )}
        {conversion && (
          <p>
            {conversion.source} · {conversion.granularity} candle at{" "}
            {date(
              Math.floor(
                conversion.timestamp /
                  (conversion.granularity === "day" ? 86400 : 3600),
              ) * (conversion.granularity === "day" ? 86400 : 3600),
            )}{" "}
            · {unitPrice(conversion.usd, true)} / {conversion.asset}
          </p>
        )}
        <h4>Received wallet movements</h4>
        {row.movements?.length ? (
          row.movements.map((delta, i) => (
            <div className="wallet-movement" key={`${delta.asset}:${i}`}>
              <a
                href={evidenceLink(chain, delta.asset)}
                target="_blank"
                rel="noreferrer"
              >
                {tokenLabel(delta.asset, markets)}
                <small>{delta.asset}</small>
              </a>
              <span>
                {Number(delta.quantity) > 0 ? "+" : ""}
                {delta.quantity}
              </span>
            </div>
          ))
        ) : (
          <p>Movement amounts have not been received.</p>
        )}
        {!row.movement_complete && (
          <p>
            Movement coverage is incomplete. Received amounts are observations;
            a complete trade price and profit cannot be inferred.
          </p>
        )}
        <dl>
          <div>
            <dt>Network fee</dt>
            <dd>
              {row.fee_quantity ?? "Unavailable"} {row.fee_asset ?? ""}
            </dd>
          </div>
          <div>
            <dt>Estimated fee USD</dt>
            <dd>
              {unitPrice(row.activities[0]?.pricing?.fee_usd ?? null, true)}
            </dd>
          </div>
        </dl>
        {row.activities[0]?.pricing?.fee_conversion && (
          <p>
            Fee conversion: {row.activities[0].pricing.fee_conversion.source} ·{" "}
            {row.activities[0].pricing.fee_conversion.granularity} candle at{" "}
            {date(
              Math.floor(
                row.activities[0].pricing.fee_conversion.timestamp /
                  (row.activities[0].pricing.fee_conversion.granularity ===
                  "day"
                    ? 86400
                    : 3600),
              ) *
                (row.activities[0].pricing.fee_conversion.granularity === "day"
                  ? 86400
                  : 3600),
            )}
          </p>
        )}
        <p>
          Source: {row.provider}. Swap evidence{" "}
          {row.swap_evidence === true ? "received" : "unverified"}.
        </p>
        {row.counterparties?.length ? (
          <p>
            Counterparties:{" "}
            {row.counterparties.map((a, i) => (
              <a
                className="wallet-counterparty"
                key={`${a}:${i}`}
                href={evidenceLink(chain, a)}
                target="_blank"
                rel="noreferrer"
              >
                {shortAddress(a)} <ExternalLink size={12} />
              </a>
            ))}
          </p>
        ) : null}
        {row.notes?.map((note, i) => (
          <p key={i}>{note}</p>
        ))}
        {row.error && <p>{row.error}</p>}
        <button
          className="wallet-inline-button"
          type="button"
          disabled={busy}
          onClick={() => void inspect()}
        >
          {busy
            ? "Reading source…"
            : raw
              ? "Refresh source record"
              : "Inspect saved source record"}
          <ArrowRight size={14} />
        </button>
        {error && <p role="alert">{error}</p>}
        {raw && (
          <pre className="wallet-source-record">
            {JSON.stringify(raw.raw, null, 2)}
          </pre>
        )}
      </div>
    </details>
  );
}
