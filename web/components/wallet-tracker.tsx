"use client";

import {
  Activity,
  ArrowRight,
  Check,
  ChevronDown,
  ExternalLink,
  RefreshCw,
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
  ActivityPage,
  TransactionEvidence,
  TokenQuote,
  nominateWallet,
  shortAddress,
  WalletAnalysis,
  WalletResponse,
  WalletSummary,
  walletId,
} from "@/lib/wallets";

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
  const [view, setView] = useState<"qualified" | "research" | "followed">(
    "research",
  );
  const [source, setSource] = useState("all");
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
    const frame = window.requestAnimationFrame(() =>
      detailView.current?.scrollIntoView({
        block: "start",
        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches
          ? "auto"
          : "smooth",
      }),
    );
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
  const qualified = wallets.filter((w) => w.status.startsWith("qualified_"));
  const visible = wallets.filter(
    (w) =>
      (chain === "all" || w.candidate.chain === chain) &&
      (view === "qualified"
        ? w.status.startsWith("qualified_")
        : view === "followed"
          ? followed.includes(walletId(w.candidate))
          : true) &&
      (source === "all" ||
        w.candidate.sources.some((s) => s.name.toLowerCase().includes(source))),
  );
  const feed = wallets
    .filter((w) => followed.includes(walletId(w.candidate)))
    .flatMap((w) => w.activity.map((a) => ({ ...a, candidate: w.candidate })))
    .sort((a, b) => b.timestamp - a.timestamp)
    .slice(0, 20);

  return (
    <>
      <section className="wallet-hero" aria-labelledby="wallet-title">
        <div className="eyebrow">
          <Users size={14} strokeWidth={1.5} />
          Wallet intelligence
        </div>
        <h1 id="wallet-title">
          A record worth
          <br />
          <span>paying attention to.</span>
        </h1>
        <p>Discover wallets. Check both months. Follow what they do next.</p>
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
          <span>Wallets under research</span>
          <strong>{response ? wallets.length : "—"}</strong>
        </div>
        <div>
          <span>Following</span>
          <strong>{followed.length}</strong>
        </div>
        <div>
          <span>Discovery attempted</span>
          <strong className="wallet-summary-date">
            {date(response?.status.last_discovery_at ?? null)}
          </strong>
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
            {(["qualified", "research", "followed"] as const).map((v) => (
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
                    ? "Under research"
                    : "Following"}
              </button>
            ))}
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
        <div className="wallet-filters">
          <label>
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
          <label>
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
          <p>
            30-day record required. 60 days preferred. Each month assessed
            separately.
          </p>
        </div>
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
        {loading && !response ? (
          <div className="wallet-empty" role="status">
            <Activity size={22} className="spin" />
            <h2>Reading wallet evidence…</h2>
          </div>
        ) : visible.length === 0 ? (
          <div className="wallet-empty">
            <ShieldCheck size={25} strokeWidth={1.4} />
            <h2>
              {view === "qualified"
                ? "The record comes first."
                : view === "followed"
                  ? "Your watch begins here."
                  : "Waiting for wallet history."}
            </h2>
            <p>
              {view === "qualified"
                ? "No collected wallet meets these filters and the qualification checks yet. Incomplete history stays under research."
                : view === "followed"
                  ? "Follow a wallet from the research list to bring its activity together here. Following does not verify performance."
                  : "Automatic discovery adds candidates as evidence arrives. Collection continues independently of this page."}
            </p>
            {view === "qualified" && (
              <button
                type="button"
                className="wallet-inline-button"
                onClick={() => setView("research")}
              >
                See wallets under research <ArrowRight size={15} />
              </button>
            )}
          </div>
        ) : (
          <div className="wallet-list">
            <div className="wallet-list-head">
              <span>Execution wallet / discovery</span>
              <span>Record</span>
              <span>Latest evidence</span>
              <span>Last collected</span>
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
                >
                  <span className="wallet-identity">
                    <strong>{shortAddress(w.candidate.wallet)}</strong>
                    <small>
                      {chainLabel(w.candidate.chain)} ·{" "}
                      {w.candidate.sources.map((s) => s.name).join(" / ")}
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
                    {w.status.startsWith("qualified_")
                      ? amount(w.windows[1]?.total_usd ?? null, true)
                      : w.coverage.newest_record_at
                        ? `Latest execution ${date(w.coverage.newest_record_at)}`
                        : "No execution received"}
                  </span>
                  <span className="wallet-last">
                    {date(w.coverage.last_collected_at)}
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
        {detail && (
          <div ref={detailView}>
            <WalletDetail
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
                    {a.asset ? shortAddress(a.asset) : "Amounts unverified"}
                  </small>
                </div>
                <span>{amount(a.quantity)}</span>
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
      <section className="wallet-principles">
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
      </section>
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
  const positions = [...w.positions].sort(
    (a, b) =>
      Number(Number(b.valuation?.quantity ?? b.quantity) > 0) -
        Number(Number(a.valuation?.quantity ?? a.quantity) > 0) ||
      (b.last_activity_at ?? 0) - (a.last_activity_at ?? 0),
  );
  return (
    <section
      className="wallet-detail"
      aria-label={`Evidence for ${w.candidate.wallet}`}
    >
      <div className="wallet-detail-title">
        <div>
          <div className="eyebrow">Execution record · {chainLabel(chain)}</div>
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
      <a
        className="wallet-full-address"
        href={evidenceLink(chain, w.candidate.wallet)}
        target="_blank"
        rel="noreferrer"
      >
        {w.candidate.wallet}
        <ExternalLink size={14} />
      </a>
      <p className="wallet-detail-status">
        {statusLabel(w.status)} · As of {date(w.analyzed_at)}
      </p>
      <WalletTransactions key={walletId(w.candidate)} wallet={w} />
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
                <dt>Profit factor</dt>
                <dd>{amount(window.profit_factor)}</dd>
              </div>
            </dl>
            <p className="wallet-value-note">
              Available values describe reconstructed evidence. A rank requires
              every check below.
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
      <div className="wallet-coverage">
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
      <div className="wallet-positions">
        <h3>Positions, including the losses.</h3>
        {w.positions.length ? (
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
              <span>
                {unitPrice(p.valuation?.quantity ?? p.quantity)} tokens
                <small>
                  {p.valuation?.quantity_source ?? "Reconstructed quantity"} ·{" "}
                  {date(p.valuation?.quantity_observed_at ?? w.analyzed_at)}
                </small>
                {p.valuation?.quantity_block && (
                  <small>{p.valuation.quantity_block}</small>
                )}
              </span>
              <span>
                Value{" "}
                {p.market_value_usd === null
                  ? "Unavailable"
                  : unitPrice(p.market_value_usd, true)}
                <small>
                  {p.valuation?.price_usd
                    ? `${unitPrice(p.valuation.price_usd, true)} / token`
                    : "Price unavailable"}
                </small>
                <small>
                  {p.valuation?.source}{" "}
                  {p.valuation?.price_observed_at
                    ? `· ${date(p.valuation.price_observed_at)}`
                    : ""}
                </small>
                <small>{p.valuation?.detail}</small>
              </span>
              <span>
                Basis {amount(String(Number(p.basis_coverage) * 100))}%
                <small>
                  Average open entry{" "}
                  {unitPrice(p.average_entry_usd ?? null, true)}
                </small>
                <a
                  className="wallet-inspect-token"
                  href={`/?chain=${chain}&address=${encodeURIComponent(p.asset)}`}
                >
                  Inspect token <ArrowRight size={12} />
                </a>
              </span>
            </div>
          ))
        ) : (
          <p>No positions can be reconstructed yet.</p>
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
    </section>
  );
}

function WalletTransactions({ wallet: w }: { wallet: WalletAnalysis }) {
  const [page, setPage] = useState<ActivityPage | null>(null);
  const [rows, setRows] = useState<TransactionEvidence[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
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
    [w.candidate.chain, w.candidate.wallet],
  );
  useEffect(() => {
    void load(null);
    return () => controller.current?.abort();
  }, [load, w.analyzed_at]);
  return (
    <div className="wallet-detail-activity">
      <h3>Transactions, with the evidence.</h3>
      <p className="wallet-value-note">
        All saved records can be inspected here. Entry and exit prices use
        received swap amounts. USD conversions use historical candles; fees are
        shown separately.
      </p>
      {page && (
        <p className="wallet-value-note">
          Showing {rows.length.toLocaleString()} of{" "}
          {page.total.toLocaleString()} saved transactions. Source history{" "}
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
    <details className="wallet-transaction">
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
                  ? `${unitPrice(trade.pricing.unit_price_usd, true)} / token`
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
