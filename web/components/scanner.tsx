"use client";

import {
  Activity,
  ArrowRight,
  Check,
  CircleDot,
  Database,
  Droplets,
  Fingerprint,
  MoveUpRight,
  Save,
  ShieldCheck,
  TriangleAlert,
  UsersRound,
} from "lucide-react";
import { FormEvent, useEffect, useState } from "react";
import {
  Chain,
  EarlyHolderMap,
  fetchEarlyHolders,
  fetchOrigin,
  getHealth,
  Health,
  OriginEvidence,
  scanToken,
  ScanResult,
} from "@/lib/api";

const chains: Array<{ id: Chain; label: string; logoClass: string }> = [
  { id: "solana", label: "Solana", logoClass: "chain-logo-solana" },
  {
    id: "robinhood",
    label: "Robinhood Chain",
    logoClass: "chain-logo-robinhood",
  },
];

export function Scanner() {
  const [chain, setChain] = useState<Chain>("solana");
  const [address, setAddress] = useState("");
  const [result, setResult] = useState<ScanResult | null>(null);
  const [health, setHealth] = useState<Health | null>(null);
  const [healthError, setHealthError] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;

    getHealth()
      .then((value) => {
        if (active) setHealth(value);
      })
      .catch(() => {
        if (active) setHealthError(true);
      });

    return () => {
      active = false;
    };
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const cleanAddress = address.trim();

    if (!cleanAddress) {
      setError("Paste a token contract address first.");
      return;
    }

    setLoading(true);
    setError("");

    try {
      const next = await scanToken(chain, cleanAddress);
      setResult(next);
    } catch (caught) {
      setResult(null);
      setError(
        caught instanceof Error
          ? caught.message
          : "Water could not complete this scan.",
      );
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <section className="hero" aria-labelledby="water-title">
        <div className="hero-copy">
          <div className="eyebrow">
            <Droplets size={14} strokeWidth={1.5} aria-hidden="true" />
            Multi-chain opponent mapping
          </div>
          <h1 id="water-title">Read the other side.</h1>
          <p>
            Water reconstructs who is positioned around a token, what pressure
            they carry, and which conclusions are supported by actual evidence.
          </p>
        </div>

        <SystemState health={health} healthError={healthError} />
      </section>

      <section className="scan-section" aria-label="Token scanner">
        <form className="scan-form" onSubmit={submit}>
          <div className="chain-switch" aria-label="Choose chain">
            {chains.map((item) => (
              <button
                className={
                  chain === item.id ? "chain-button active" : "chain-button"
                }
                type="button"
                key={item.id}
                aria-pressed={chain === item.id}
                onClick={() => {
                  setChain(item.id);
                  setResult(null);
                  setError("");
                }}
              >
                <span
                  className={`chain-logo ${item.logoClass}`}
                  aria-hidden="true"
                />
                <span className="chain-label">{item.label}</span>
              </button>
            ))}
          </div>

          <label className="address-field">
            <span className="sr-only">Token contract address</span>
            <input
              value={address}
              onChange={(event) => setAddress(event.target.value)}
              placeholder={
                chain === "solana"
                  ? "Paste Solana token address"
                  : "Paste 0x token contract"
              }
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
          </label>

          <button
            className="scan-button"
            type="submit"
            disabled={loading || healthError}
          >
            {loading ? (
              <>
                <Activity className="spin" size={16} strokeWidth={1.5} />
                Reading
              </>
            ) : (
              <>
                Read position
                <ArrowRight size={16} strokeWidth={1.5} />
              </>
            )}
          </button>
        </form>

        {error ? (
          <div className="error-line" role="alert">
            <TriangleAlert size={15} strokeWidth={1.5} />
            {error}
          </div>
        ) : null}
      </section>

      {loading ? null : result ? <ResultView result={result} /> : <EmptyState />}
    </>
  );
}

function SystemState({
  health,
  healthError,
}: {
  health: Health | null;
  healthError: boolean;
}) {
  const apiOnline = Boolean(health) && !healthError;
  const marketReady = apiOnline && health?.requires_market_api_key === false;

  return (
    <div className="system-state" aria-label="Water system status">
      <StatusRow
        label="Water core"
        ok={apiOnline}
        pending={!health && !healthError}
      />
      <StatusRow
        label="Market data"
        ok={marketReady}
        pending={!health && !healthError}
        detail={healthError ? "offline" : "public"}
      />
      <div className="status-row">
        <span className="status-icon neutral">
          <CircleDot size={12} strokeWidth={1.5} />
        </span>
        <span>Chains</span>
        <strong>SOL · RHC</strong>
      </div>
    </div>
  );
}

function StatusRow({
  label,
  ok,
  pending,
  detail,
}: {
  label: string;
  ok: boolean;
  pending: boolean;
  detail?: string;
}) {
  return (
    <div className="status-row">
      <span className={ok ? "status-icon ok" : "status-icon muted"}>
        {pending ? (
          <CircleDot size={12} strokeWidth={1.5} />
        ) : ok ? (
          <Check size={12} strokeWidth={2} />
        ) : (
          <TriangleAlert size={12} strokeWidth={1.5} />
        )}
      </span>
      <span>{label}</span>
      <strong>{pending ? "checking" : ok ? "ready" : detail ?? "offline"}</strong>
    </div>
  );
}

function EmptyState() {
  return (
    <section className="empty-state">
      <div className="empty-mark" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <div>
        <p className="empty-title">Nothing invented.</p>
        <p>
          Paste a contract. This space stays quiet until Water has real provider
          data and direct chain evidence to show you.
        </p>
      </div>
    </section>
  );
}

function ResultView({ result }: { result: ScanResult }) {
  const title =
    result.token.symbol ?? result.token.name ?? shortenAddress(result.address);
  const subtitle = [result.token.name, chainLabel(result.chain)]
    .filter(Boolean)
    .join(" · ");

  return (
    <section className="results" aria-live="polite">
      <div className="asset-line">
        <div>
          <div className="eyebrow">Observed asset</div>
          <h2>{title}</h2>
          <p>{subtitle}</p>
        </div>
        <div className="asset-metrics">
          <Metric label="Price" value={formatUsd(result.token.price_usd)} />
          <Metric
            label="Liquidity"
            value={formatUsd(result.token.liquidity_usd)}
          />
          <Metric
            label="Market cap"
            value={formatUsd(result.token.market_cap_usd)}
          />
        </div>
      </div>

      <div className="analysis-grid">
        <article className="pressure-panel">
          <div className="section-heading">
            <div>
              <div className="eyebrow">Exit pressure</div>
              <h3>
                {result.pressure.index === null
                  ? "Not enough data"
                  : result.pressure.index}
                {result.pressure.index === null ? "" : <small>/100</small>}
              </h3>
            </div>
            <Activity size={18} strokeWidth={1.5} aria-hidden="true" />
          </div>

          <div className="pressure-components">
            {result.pressure.components.length ? (
              result.pressure.components.map((component) => (
                <div className="pressure-row" key={component.key}>
                  <div className="pressure-label">
                    <span>{component.label}</span>
                    <strong>{component.observed}</strong>
                  </div>
                  <div className="pressure-track" aria-hidden="true">
                    <span
                      style={{
                        width: `${Math.max(2, component.pressure * 100)}%`,
                      }}
                    />
                  </div>
                  <p>{component.detail}</p>
                </div>
              ))
            ) : (
              <p className="muted-copy">
                Public holder and market-structure evidence was not complete
                enough to derive this index.
              </p>
            )}
          </div>

          <p className="method-note">{result.pressure.methodology}</p>
        </article>

        <article className="opponent-panel">
          <div className="section-heading">
            <div>
              <div className="eyebrow">Become the opponent</div>
              <h3>What the other side can see</h3>
            </div>
            <Droplets size={18} strokeWidth={1.5} aria-hidden="true" />
          </div>

          <div className="opponent-notes">
            {result.opponent_notes.map((note, index) => (
              <div className="opponent-note" key={`${index}-${note}`}>
                <span>{String(index + 1).padStart(2, "0")}</span>
                <p>{note}</p>
              </div>
            ))}
          </div>
        </article>
      </div>

      <EarlyHolderPanel
        key={`early-${result.chain}-${result.address}`}
        chain={result.chain}
        token={result.address}
      />

      <DemandPanel demand={result.demand} />

      <OriginPanel
        key={`origin-${result.chain}-${result.address}`}
        chain={result.chain}
        token={result.address}
      />

      <WaterPanel result={result} />

      <div className="evidence-grid">
        <article className="evidence-panel">
          <div className="section-heading compact">
            <div>
              <div className="eyebrow">Direct evidence</div>
              <h3>Chain verification</h3>
            </div>
            <ShieldCheck size={18} strokeWidth={1.5} />
          </div>

          <div className="evidence-callout">
            <span
              className={
                result.chain_evidence.verified
                  ? "evidence-dot ok"
                  : "evidence-dot"
              }
            />
            <div>
              <strong>{result.chain_evidence.source}</strong>
              <p>{result.chain_evidence.detail}</p>
            </div>
          </div>
        </article>

        <article className="evidence-panel">
          <div className="section-heading compact">
            <div>
              <div className="eyebrow">Source ledger</div>
              <h3>What answered</h3>
            </div>
            <Database size={18} strokeWidth={1.5} />
          </div>

          <div className="source-list">
            {result.sources.map((source) => (
              <div className="source-row" key={source.source}>
                <span
                  className={source.ok ? "source-state ok" : "source-state"}
                />
                <span>{source.source}</span>
                <strong>{source.ok ? "received" : "missing"}</strong>
              </div>
            ))}
          </div>
        </article>
      </div>
    </section>
  );
}


function EarlyHolderPanel({
  chain,
  token,
}: {
  chain: Chain;
  token: string;
}) {
  const [data, setData] = useState<EarlyHolderMap | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    setData(null);

    fetchEarlyHolders(chain, token)
      .then((value) => {
        if (active) setData(value);
      })
      .catch((caught) => {
        if (active) {
          setError(
            caught instanceof Error
              ? caught.message
              : "Holder history was unavailable.",
          );
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [chain, token]);

  return (
    <article className="early-panel">
      <div className="section-heading compact early-heading">
        <div>
          <div className="eyebrow">Early holder map</div>
          <h3>Where the big wallets stand</h3>
          <p className="section-subcopy">
            Current large wallets, ordered by the earliest entry Water can
            reconstruct.
          </p>
        </div>
        <UsersRound size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>

      {loading ? (
        <div className="early-state">
          <Activity className="spin" size={15} strokeWidth={1.5} />
          Reading three wallet histories…
        </div>
      ) : error ? (
        <div className="early-state muted">
          Not enough verified holder history yet.
        </div>
      ) : data && data.holders.length ? (
        <>
          <div className="early-summary">
            <div>
              <span>Still holding</span>
              <strong>{formatPercent(data.cohort_retained_from_peak)}</strong>
              <small>of combined peak position</small>
            </div>
            <div>
              <span>Distributed</span>
              <strong>{formatPercent(data.cohort_distributed_fraction)}</strong>
              <small>of observed acquired tokens</small>
            </div>
            <p>
              {data.wallets_reconstructed} of {data.wallets_requested} wallet
              histories reconstructed.
            </p>
          </div>

          <div className="holder-list">
            {data.holders.map((holder, index) => (
              <div className="holder-row" key={holder.wallet}>
                <div className="holder-identity">
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  <div>
                    <strong>{shortenAddress(holder.wallet)}</strong>
                    <small>
                      {formatFirstSeen(holder.first_acquired_at, data.observed_at_unix)}
                    </small>
                  </div>
                </div>

                <div className="holder-retention">
                  <div>
                    <span>Still holding</span>
                    <strong>{formatPercent(holder.retained_from_peak)}</strong>
                  </div>
                  <div className="holder-track" aria-hidden="true">
                    <span style={{ width: `${Math.max(2, holder.retained_from_peak * 100)}%` }} />
                  </div>
                  <small>
                    {formatQuantity(holder.current_quantity)} now ·{" "}
                    {formatQuantity(holder.peak_quantity)} peak
                  </small>
                </div>

                <div className="holder-economics">
                  <div>
                    <span>Distributed</span>
                    <strong>{formatPercent(holder.distributed_fraction)}</strong>
                  </div>
                  <div>
                    <span>Known entry</span>
                    <strong>{formatUsd(holder.average_entry_usd)}</strong>
                  </div>
                  <div>
                    <span>Now / entry</span>
                    <strong>
                      {holder.current_multiple_on_entry === null
                        ? "—"
                        : `${holder.current_multiple_on_entry.toFixed(1)}×`}
                    </strong>
                  </div>
                </div>

                <span
                  className={`basis-tag ${holder.basis_status.replace("_", "-")}`}
                >
                  {basisLabel(holder.basis_status, holder.basis_coverage)}
                </span>
              </div>
            ))}
          </div>
        </>
      ) : (
        <div className="early-state muted">
          No wallet histories could be reconstructed without guessing.
        </div>
      )}
    </article>
  );
}

function DemandPanel({ demand }: { demand: ScanResult["demand"] }) {
  const buyShare = demand.buy_share_h1;
  const hasTrades =
    buyShare !== null &&
    demand.buys_h1 !== null &&
    demand.sells_h1 !== null;

  return (
    <article className="demand-panel">
      <div className="section-heading compact demand-heading">
        <div>
          <div className="eyebrow">Who buys after me?</div>
          <h3>Is demand still arriving?</h3>
        </div>
        <MoveUpRight size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>

      {hasTrades ? (
        <div className="demand-body">
          <div className="demand-share">
            <strong>{formatPercent(buyShare)}</strong>
            <span>of top-pool trades were buys in the last hour</span>
          </div>

          <div className="demand-track" aria-hidden="true">
            <span style={{ width: `${Math.max(2, buyShare * 100)}%` }} />
          </div>

          <div className="demand-facts">
            <div>
              <span>Buy transactions</span>
              <strong>{demand.buys_h1}</strong>
            </div>
            <div>
              <span>Sell transactions</span>
              <strong>{demand.sells_h1}</strong>
            </div>
            <div>
              <span>24h turnover</span>
              <strong>
                {demand.volume_to_liquidity === null
                  ? "—"
                  : `${demand.volume_to_liquidity.toFixed(1)}×`}
              </strong>
            </div>
          </div>

          <p className="method-note demand-note">
            This shows transaction demand and turnover, not unique-buyer growth
            yet. Water does not infer buyers it cannot verify.
          </p>
        </div>
      ) : (
        <p className="muted-copy demand-empty">
          The top pool did not return enough recent transaction evidence.
        </p>
      )}
    </article>
  );
}

function formatPercent(value: number | null) {
  if (value === null || !Number.isFinite(value)) return "—";
  return `${Math.round(value * 100)}%`;
}

function formatQuantity(value: number) {
  if (!Number.isFinite(value)) return "—";
  if (Math.abs(value) >= 1_000_000_000) {
    return `${(value / 1_000_000_000).toFixed(2)}B`;
  }
  if (Math.abs(value) >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(2)}M`;
  }
  if (Math.abs(value) >= 1_000) {
    return `${(value / 1_000).toFixed(1)}K`;
  }
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 }).format(value);
}

function formatFirstSeen(timestamp: number | null, now: number) {
  if (timestamp === null || timestamp <= 0) return "First entry unknown";

  const seconds = Math.max(0, now - timestamp);
  if (seconds < 60) return "First seen <1m ago";
  if (seconds < 3_600) return `First seen ${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86_400) return `First seen ${Math.floor(seconds / 3_600)}h ago`;
  return `First seen ${Math.floor(seconds / 86_400)}d ago`;
}

function basisLabel(
  status: "verified" | "partial_history" | "incomplete",
  coverage: number,
) {
  if (status === "verified") return "basis verified";
  if (status === "partial_history") {
    return `${Math.round(coverage * 100)}% basis known`;
  }
  return "basis incomplete";
}


function OriginPanel({ chain, token }: { chain: Chain; token: string }) {
  const [data, setData] = useState<OriginEvidence | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setData(null);

    fetchOrigin(chain, token)
      .then((value) => {
        if (active) setData(value);
      })
      .catch(() => {
        if (active) setData(null);
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [chain, token]);

  return (
    <article className="origin-panel">
      <div className="section-heading compact origin-heading">
        <div>
          <div className="eyebrow">Origin &amp; control</div>
          <h3>Who can still touch the machinery?</h3>
        </div>
        <Fingerprint size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>

      {loading ? (
        <div className="origin-state">
          <Activity className="spin" size={15} strokeWidth={1.5} />
          Verifying origin…
        </div>
      ) : data ? (
        <div className="origin-body">
          <div className="origin-addresses">
            <div>
              <span>{data.primary_label}</span>
              <strong>
                {data.primary_address ? shortenAddress(data.primary_address) : "revoked / unknown"}
              </strong>
              {data.primary_balance_percentage !== null ? (
                <small>
                  holds about {data.primary_balance_percentage.toFixed(2)}% of supply
                </small>
              ) : null}
            </div>

            {data.secondary_label ? (
              <div>
                <span>{data.secondary_label}</span>
                <strong>
                  {data.secondary_address
                    ? shortenAddress(data.secondary_address)
                    : "none"}
                </strong>
              </div>
            ) : null}
          </div>

          <div className="origin-controls">
            {data.active_controls.map((control) => (
              <div key={control}>
                <span className="source-state ok" />
                <p>{control}</p>
              </div>
            ))}
          </div>

          <p className="method-note origin-note">{data.detail}</p>
        </div>
      ) : (
        <p className="muted-copy origin-empty">
          Water could not prove origin/control relationships for this token.
        </p>
      )}
    </article>
  );
}


type ThesisSnapshot = {
  savedAt: number;
  holderConcentration: number | null;
  buyShare: number | null;
  liquidityUsd: number | null;
  marketCapUsd: number | null;
};

function WaterPanel({ result }: { result: ScanResult }) {
  const storageKey = `water:thesis:${result.chain}:${result.address.toLowerCase()}`;
  const [saved, setSaved] = useState<ThesisSnapshot | null>(null);
  const current = thesisSnapshot(result);

  useEffect(() => {
    try {
      const raw = window.localStorage.getItem(storageKey);
      setSaved(raw ? (JSON.parse(raw) as ThesisSnapshot) : null);
    } catch {
      setSaved(null);
    }
  }, [storageKey]);

  function saveCurrent() {
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(current));
      setSaved(current);
    } catch {
      // Browser storage can be unavailable in private/locked-down contexts.
    }
  }

  return (
    <article className="water-panel">
      <div className="section-heading compact water-heading">
        <div>
          <div className="eyebrow">Water</div>
          <h3>Remember the world you entered.</h3>
          <p className="section-subcopy">
            Save today&apos;s structure. On the next scan, Water shows what
            changed instead of pretending your old thesis is still current.
          </p>
        </div>
        <Save size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>

      {saved ? (
        <div className="water-body">
          <p className="water-saved-at">
            Compared with {formatSavedTime(saved.savedAt)}
          </p>
          <div className="water-deltas">
            <ThesisDelta
              label="Top wallets"
              before={saved.holderConcentration}
              now={current.holderConcentration}
              kind="percent"
            />
            <ThesisDelta
              label="Buy share"
              before={saved.buyShare}
              now={current.buyShare}
              kind="ratio"
            />
            <ThesisDelta
              label="Liquidity"
              before={saved.liquidityUsd}
              now={current.liquidityUsd}
              kind="usd"
            />
            <ThesisDelta
              label="Market cap"
              before={saved.marketCapUsd}
              now={current.marketCapUsd}
              kind="usd"
            />
          </div>
          <button className="quiet-action" type="button" onClick={saveCurrent}>
            Use current setup as new baseline
          </button>
        </div>
      ) : (
        <div className="water-empty">
          <p>No baseline saved for this token yet.</p>
          <button className="quiet-action" type="button" onClick={saveCurrent}>
            Save this setup
          </button>
        </div>
      )}
    </article>
  );
}

function ThesisDelta({
  label,
  before,
  now,
  kind,
}: {
  label: string;
  before: number | null;
  now: number | null;
  kind: "percent" | "ratio" | "usd";
}) {
  const beforeText = thesisValue(before, kind);
  const nowText = thesisValue(now, kind);
  const delta =
    before !== null && now !== null
      ? kind === "usd"
        ? before === 0
          ? null
          : (now - before) / Math.abs(before)
        : now - before
      : null;

  return (
    <div className="water-delta">
      <span>{label}</span>
      <div>
        <small>{beforeText}</small>
        <ArrowRight size={12} strokeWidth={1.5} aria-hidden="true" />
        <strong>{nowText}</strong>
      </div>
      <em>{formatThesisDelta(delta, kind)}</em>
    </div>
  );
}

function thesisSnapshot(result: ScanResult): ThesisSnapshot {
  return {
    savedAt: Date.now(),
    holderConcentration:
      result.holder_evidence.top_ten_percentage === null
        ? null
        : result.holder_evidence.top_ten_percentage / 100,
    buyShare: result.demand.buy_share_h1,
    liquidityUsd: result.token.liquidity_usd,
    marketCapUsd: result.token.market_cap_usd,
  };
}

function thesisValue(value: number | null, kind: "percent" | "ratio" | "usd") {
  if (value === null || !Number.isFinite(value)) return "—";
  if (kind === "usd") return formatUsd(value);
  return formatPercent(value);
}

function formatThesisDelta(
  value: number | null,
  kind: "percent" | "ratio" | "usd",
) {
  if (value === null || !Number.isFinite(value)) return "—";
  if (kind === "usd") {
    const pct = value * 100;
    return `${pct >= 0 ? "+" : ""}${pct.toFixed(0)}%`;
  }

  const points = value * 100;
  return `${points >= 0 ? "+" : ""}${points.toFixed(1)}pp`;
}

function formatSavedTime(timestamp: number) {
  const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1000));
  if (seconds < 60) return "the setup saved just now";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h ago`;
  return `${Math.floor(seconds / 86_400)}d ago`;
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function formatUsd(value: number | null) {
  if (value === null || !Number.isFinite(value)) return "—";

  if (Math.abs(value) >= 1_000_000_000) {
    return `$${(value / 1_000_000_000).toFixed(2)}B`;
  }
  if (Math.abs(value) >= 1_000_000) {
    return `$${(value / 1_000_000).toFixed(2)}M`;
  }
  if (Math.abs(value) >= 1_000) {
    return `$${(value / 1_000).toFixed(1)}K`;
  }
  if (Math.abs(value) < 0.01) {
    return `$${value.toPrecision(3)}`;
  }

  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 2,
  }).format(value);
}

function shortenAddress(address: string) {
  if (address.length <= 12) return address;
  return `${address.slice(0, 6)}…${address.slice(-5)}`;
}

function chainLabel(chain: Chain) {
  return chain === "solana" ? "Solana" : "Robinhood Chain";
}
