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
  Users,
  Fingerprint,
} from "lucide-react";
import { FormEvent, useEffect, useRef, useState } from "react";
import {
  Chain,
  getHealth,
  Health,
  scanToken,
  ScanResult,
} from "@/lib/api";
import {
  AssetLinks,
  CounterCasePanel,
  DemandPanel,
  EarlyHolderPanel,
  MemoryPanel,
  OriginPanel,
  WaterPanel,
} from "@/components/intelligence-panels";
import { WalletOverlap } from "@/components/wallet-overlap";

const chains: Array<{ id: Chain; label: string; logoClass: string }> = [
  { id: "solana", label: "Solana", logoClass: "chain-logo-solana" },
  { id: "bnb", label: "BNB Chain", logoClass: "chain-logo-bnb" },
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
  const [healthCheck, setHealthCheck] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const pendingScan = useRef<AbortController | null>(null);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const selected = params.get("chain"); const token = params.get("address");
    if ((selected === "solana" || selected === "robinhood" || selected === "bnb") && token) { setChain(selected); setAddress(token.trim()); }
  }, []);

  useEffect(() => {
    let active = true;
    setHealth(null);
    setHealthError(false);

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
  }, [healthCheck]);

  useEffect(() => () => {
    pendingScan.current?.abort();
    pendingScan.current = null;
  }, []);

  function cancelScan() {
    pendingScan.current?.abort();
    pendingScan.current = null;
    setLoading(false);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pendingScan.current) return;
    const cleanAddress = address.trim();

    if (!cleanAddress) {
      setError("Paste a token contract address first.");
      return;
    }

    const controller = new AbortController();
    pendingScan.current = controller;
    setLoading(true);
    setError("");
    setHealthCheck((value) => value + 1);

    try {
      const next = await scanToken(chain, cleanAddress, controller.signal);
      if (pendingScan.current !== controller) return;
      setResult(next);
    } catch (caught) {
      if (pendingScan.current !== controller || controller.signal.aborted) return;
      setResult(null);
      setError(
        caught instanceof Error
          ? caught.message
          : "Water could not complete this scan.",
      );
    } finally {
      if (pendingScan.current === controller) {
        pendingScan.current = null;
        setLoading(false);
      }
    }
  }

  return (
    <>
      <div className={result ? "scan-intro has-results" : "scan-intro"}>
      <section className="hero" aria-labelledby="water-title">
        <div className="hero-copy">
          <div className="eyebrow">
            <Droplets size={14} strokeWidth={1.5} aria-hidden="true" />
            See beneath the price
          </div>
          <h1 id="water-title">Read the<br /> <span>other side.</span></h1>
          <p>
            Who holds the supply. How deep the liquidity goes.
            Where control began. One token, a clearer picture.
          </p>
          <div className="hero-footnote">Solana · Robinhood · BNB Chain</div>
        </div>
      </section>

      <section className="scan-section" aria-label="Token scanner">
        <div className="scan-heading">
          <h2>Start with a token.</h2>
          <p>Choose a chain and paste its contract address.</p>
        </div>
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
                  if (chain === item.id) return;
                  cancelScan();
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
            <span className="field-label">Token contract address</span>
            <input
              value={address}
              onChange={(event) => {
                if (pendingScan.current) {
                  cancelScan();
                  setResult(null);
                }
                setAddress(event.target.value);
                setError("");
              }}
              placeholder={
                chain === "solana"
                  ? "Paste Solana token address"
                  : "Paste 0x token contract"
              }
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              aria-invalid={Boolean(error)}
              aria-describedby={error ? "scan-error" : undefined}
            />
          </label>

          <button
            className="scan-button"
            type="submit"
            disabled={loading}
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
          <div className="error-line" id="scan-error" role="alert">
            <TriangleAlert size={15} strokeWidth={1.5} />
            {error}
          </div>
        ) : null}
        <SystemState health={health} healthError={healthError} />
        {healthError ? (
          <p className="health-note" role="status">Connection check failed. You can still try a scan.</p>
        ) : null}
      </section>
      </div>

      {loading ? (
        <div className="scan-progress" role="status">
          <Activity className="spin" size={18} aria-hidden="true" />
          Reading market data and chain evidence…
        </div>
      ) : result ? <ResultView result={result} /> : <EmptyState />}
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
        detail="unavailable"
      />
      <span className="system-note">{marketReady ? "Public market data" : "Source availability varies"}</span>
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
    <section className="empty-state" aria-label="What a scan reveals">
      <div className="scan-guide">
        <Users size={20} strokeWidth={1.5} aria-hidden="true" />
        <div><h2>Follow the holders</h2><p>Ownership, concentration, and the wallets around a token.</p></div>
      </div>
      <div className="scan-guide">
        <Droplets size={20} strokeWidth={1.5} aria-hidden="true" />
        <div><h2>Read the pressure</h2><p>Liquidity and trading activity, with the evidence behind each signal.</p></div>
      </div>
      <div className="scan-guide">
        <Fingerprint size={20} strokeWidth={1.5} aria-hidden="true" />
        <div><h2>Trace the origin</h2><p>Launchpad and creator evidence. Unverified details stay unknown.</p></div>
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
          <AssetLinks chain={result.chain} token={result.address} />
        </div>
        <div className="asset-metrics">
          <Metric label="Price" value={formatUsd(result.token.price_usd)} />
          <Metric
            label="Liquidity"
            value={formatUsd(result.token.liquidity_usd)}
          />
          <Metric
            label={result.token.market_cap_basis === "supply_implied" ? "Supply value" : "Market cap"}
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

      <CounterCasePanel result={result} />

      <OriginPanel
        key={`origin-${result.chain}-${result.address}`}
        chain={result.chain}
        token={result.address}
      />

      <WaterPanel result={result} />
      <WalletOverlap chain={result.chain} token={result.address} />

      <MemoryPanel result={result} />

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
              <div className="source-entry" key={source.source}>
                <div className="source-row">
                  <span className={source.ok ? "source-state ok" : "source-state"} />
                  <span>{source.source}</span>
                  <strong>{source.ok ? "received" : "missing"}</strong>
                </div>
                {(!source.ok || source.source.includes("fallback")) && (
                  <p className="source-detail">{source.detail}</p>
                )}
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
  return chain === "solana" ? "Solana" : chain === "bnb" ? "BNB Chain" : "Robinhood Chain";
}
