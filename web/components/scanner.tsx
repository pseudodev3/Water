"use client";

import {
  Activity,
  ArrowRight,
  Check,
  CircleDot,
  Database,
  Droplets,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import { FormEvent, useEffect, useState } from "react";
import {
  Chain,
  getHealth,
  Health,
  scanToken,
  ScanResult,
} from "@/lib/api";

const chains: Array<{ id: Chain; label: string; short: string }> = [
  { id: "solana", label: "Solana", short: "SOL" },
  { id: "robinhood", label: "Robinhood Chain", short: "RHC" },
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
                <span>{item.short}</span>
                {item.label}
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
            disabled={loading || health?.gmgn_configured === false}
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

      {result ? <ResultView result={result} /> : <EmptyState />}
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
  const gmgnReady = health?.gmgn_configured === true;

  return (
    <div className="system-state" aria-label="Water system status">
      <StatusRow
        label="Water core"
        ok={apiOnline}
        pending={!health && !healthError}
      />
      <StatusRow
        label="GMGN"
        ok={gmgnReady}
        pending={!health && !healthError}
        detail={health && !gmgnReady ? "key needed" : undefined}
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
                Holder/trader fields were not complete enough to derive this
                index.
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
