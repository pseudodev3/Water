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
    "qualified",
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
    setDetail(null);
    try {
      const value = await getWalletDetail(
        wallet.candidate.chain,
        wallet.candidate.wallet,
        controller.signal,
      );
      if (!controller.signal.aborted) setDetail(value);
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
          <span>Discovery checked</span>
          <strong className="wallet-summary-date">
            {date(response?.status.last_discovery_at ?? null)}
          </strong>
        </div>
      </section>
      {response?.status.enabled && (
        <details className="wallet-discovery-coverage">
          <summary>Discovery coverage</summary>
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
        {response?.status.enabled && response.status.detail && (
          <p className="wallet-collection-note" role="status">
            {response.status.detail} Last received evidence is preserved.
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
              <span>Latest 30 days</span>
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
                  </span>
                  <span className="wallet-pnl">
                    {w.status.startsWith("qualified_")
                      ? amount(w.windows[1]?.total_usd ?? null, true)
                      : "Awaiting verification"}
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
            Reconstructing the saved record…
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
      Number(Number(b.quantity) > 0) - Number(Number(a.quantity) > 0) ||
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
          </section>
        ))}
      </div>
      <div className="wallet-coverage">
        <h3>What the history covers.</h3>
        <dl>
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
                {shortAddress(p.asset)}
                <ExternalLink size={12} />
              </a>
              <span>{amount(p.quantity)} held</span>
              <span>Value {amount(p.market_value_usd, true)}</span>
              <span>
                Basis {amount(String(Number(p.basis_coverage) * 100))}%
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
      <div className="wallet-detail-activity">
        <h3>Latest observed activity.</h3>
        {w.activity.slice(0, 12).map((a, i) => (
          <div className="wallet-feed-row" key={`${a.tx}:${i}`}>
            <div>
              <strong>
                {a.kind.replaceAll("_", " ")}
                {!a.finalized && " · provisional"}
              </strong>
              <small>
                {a.asset ? shortAddress(a.asset) : "Amounts unverified"}
                {a.counterparties.length
                  ? ` · counterparty ${shortAddress(a.counterparties[0])}`
                  : ""}
              </small>
            </div>
            <span>{amount(a.quantity)}</span>
            <a
              href={evidenceLink(chain, a.tx, "tx")}
              target="_blank"
              rel="noreferrer"
            >
              {date(a.timestamp)}
              <ExternalLink size={12} />
            </a>
          </div>
        ))}
        {!w.activity.length && <p>No saved activity yet.</p>}
      </div>
      <div className="wallet-detail-notes">
        {w.notes.map((note) => (
          <p key={note}>{note}</p>
        ))}
      </div>
    </section>
  );
}
