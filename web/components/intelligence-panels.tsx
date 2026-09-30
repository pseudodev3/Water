"use client";

import {
  Activity,
  ArrowRight,
  Fingerprint,
  History,
  MoveUpRight,
  Save,
  UsersRound,
} from "lucide-react";
import { useEffect, useState } from "react";
import {
  Chain,
  EarlyHolderMap,
  fetchEarlyHolders,
  fetchOrigin,
  OriginEvidence,
  ScanResult,
} from "@/lib/api";

export function EarlyHolderPanel({
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

export function DemandPanel({ demand }: { demand: ScanResult["demand"] }) {
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
            <strong>
              {demand.unique_buyers_h1 === null
                ? formatPercent(buyShare)
                : demand.unique_buyers_h1}
            </strong>
            <span>
              {demand.unique_buyers_h1 === null
                ? "of top-pool trades were buys in the last hour"
                : "unique buyer wallets in the top pool during the last hour"}
            </span>
          </div>

          <div className="demand-track" aria-hidden="true">
            <span style={{ width: `${Math.max(2, buyShare * 100)}%` }} />
          </div>

          <div className="demand-facts">
            <div>
              <span>Unique sellers</span>
              <strong>{demand.unique_sellers_h1 ?? "—"}</strong>
            </div>
            <div>
              <span>Buy share</span>
              <strong>{formatPercent(demand.buy_share_h1)}</strong>
            </div>
            <div>
              <span>Buyer pace</span>
              <strong>
                {demand.buyer_arrival_vs_h24_hourly === null
                  ? "—"
                  : `${demand.buyer_arrival_vs_h24_hourly.toFixed(1)}×`}
              </strong>
            </div>
          </div>

          <p className="method-note demand-note">
            Buyer pace compares last-hour unique buyers with the 24-hour hourly
            average. Turnover is{" "}
            {demand.volume_to_liquidity === null
              ? "unavailable"
              : `${demand.volume_to_liquidity.toFixed(1)}× liquidity`}
            . These are observed wallets and transactions, not a directional
            prediction.
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


export function OriginPanel({ chain, token }: { chain: Chain; token: string }) {
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
          <h3>Where did control originate?</h3>
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

export function WaterPanel({ result }: { result: ScanResult }) {
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


type ScanMemory = {
  chain: Chain;
  token: string;
  at: number;
  priceUsd: number | null;
  pressure: number | null;
  holderConcentration: number | null;
  liquidityUsd: number | null;
  buyShare: number | null;
};

export function MemoryPanel({ result }: { result: ScanResult }) {
  const [history, setHistory] = useState<ScanMemory[]>([]);

  useEffect(() => {
    const current: ScanMemory = {
      chain: result.chain,
      token: result.address.toLowerCase(),
      at: result.scanned_at_unix,
      priceUsd: result.token.price_usd,
      pressure: result.pressure.index,
      holderConcentration: result.holder_evidence.top_ten_percentage,
      liquidityUsd: result.token.liquidity_usd,
      buyShare: result.demand.buy_share_h1,
    };

    try {
      const key = "water:scan-memory:v1";
      const raw = window.localStorage.getItem(key);
      const parsed = raw ? (JSON.parse(raw) as ScanMemory[]) : [];
      const clean = parsed.filter(
        (item) =>
          item &&
          typeof item.at === "number" &&
          typeof item.token === "string" &&
          (item.chain === "solana" || item.chain === "robinhood"),
      );

      const last = clean[clean.length - 1];
      const sameRecent =
        last &&
        last.chain === current.chain &&
        last.token === current.token &&
        Math.abs(current.at - last.at) < 60;

      const next = sameRecent
        ? [...clean.slice(0, -1), current]
        : [...clean, current];

      const bounded = next.slice(-180);
      window.localStorage.setItem(key, JSON.stringify(bounded));
      setHistory(bounded);
    } catch {
      setHistory([current]);
    }
  }, [result]);

  const tokenHistory = history
    .filter(
      (item) =>
        item.chain === result.chain &&
        item.token === result.address.toLowerCase(),
    )
    .sort((left, right) => left.at - right.at);

  const previous =
    tokenHistory.length >= 2
      ? tokenHistory[tokenHistory.length - 2]
      : null;
  const current = tokenHistory[tokenHistory.length - 1] ?? null;
  const calibration = localCalibration(history);

  return (
    <article className="memory-panel">
      <div className="section-heading compact memory-heading">
        <div>
          <div className="eyebrow">Memory</div>
          <h3>What happened after earlier reads?</h3>
          <p className="section-subcopy">
            Water keeps a small evidence history on this device. No account or
            database required.
          </p>
        </div>
        <History size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>

      <div className="memory-body">
        {previous && current ? (
          <div className="memory-current">
            <span>Since the previous read</span>
            <div>
              <MemoryDelta
                label="Price"
                before={previous.priceUsd}
                now={current.priceUsd}
                kind="relative"
              />
              <MemoryDelta
                label="Exit pressure"
                before={previous.pressure}
                now={current.pressure}
                kind="points"
              />
              <MemoryDelta
                label="Liquidity"
                before={previous.liquidityUsd}
                now={current.liquidityUsd}
                kind="relative"
              />
            </div>
          </div>
        ) : (
          <p className="memory-first">
            First saved read for this token. The next read creates a comparison.
          </p>
        )}

        <div className="calibration-strip">
          <div>
            <span>Saved reads</span>
            <strong>{history.length}</strong>
          </div>
          <CalibrationCell label="~1h" sample={calibration.h1} />
          <CalibrationCell label="~6h" sample={calibration.h6} />
          <CalibrationCell label="~24h" sample={calibration.h24} />
        </div>

        <p className="memory-note">
          Outcome samples use later scans from the same token inside a narrow
          time window. They are calibration evidence, not a forecast.
        </p>
      </div>
    </article>
  );
}

function MemoryDelta({
  label,
  before,
  now,
  kind,
}: {
  label: string;
  before: number | null;
  now: number | null;
  kind: "relative" | "points";
}) {
  let delta: number | null = null;
  if (before !== null && now !== null) {
    if (kind === "relative") {
      delta = before === 0 ? null : (now - before) / Math.abs(before);
    } else {
      delta = now - before;
    }
  }

  return (
    <div className="memory-delta">
      <span>{label}</span>
      <strong>
        {delta === null
          ? "—"
          : kind === "relative"
            ? `${delta >= 0 ? "+" : ""}${(delta * 100).toFixed(0)}%`
            : `${delta >= 0 ? "+" : ""}${delta.toFixed(0)}`}
      </strong>
    </div>
  );
}

function CalibrationCell({
  label,
  sample,
}: {
  label: string;
  sample: { count: number; medianReturn: number | null };
}) {
  return (
    <div>
      <span>{label} outcome</span>
      <strong>
        {sample.medianReturn === null
          ? "—"
          : `${sample.medianReturn >= 0 ? "+" : ""}${(
              sample.medianReturn * 100
            ).toFixed(0)}%`}
      </strong>
      <small>{sample.count ? `median · n=${sample.count}` : "needs more reads"}</small>
    </div>
  );
}

function localCalibration(history: ScanMemory[]) {
  return {
    h1: calibrationWindow(history, 3_600, 7_200),
    h6: calibrationWindow(history, 21_600, 32_400),
    h24: calibrationWindow(history, 86_400, 129_600),
  };
}

function calibrationWindow(
  history: ScanMemory[],
  minSeconds: number,
  maxSeconds: number,
) {
  const returns: number[] = [];

  for (let index = 0; index < history.length; index += 1) {
    const start = history[index];
    if (start.priceUsd === null || start.priceUsd <= 0) continue;

    const later = history.find(
      (candidate, candidateIndex) =>
        candidateIndex > index &&
        candidate.chain === start.chain &&
        candidate.token === start.token &&
        candidate.priceUsd !== null &&
        candidate.at - start.at >= minSeconds &&
        candidate.at - start.at <= maxSeconds,
    );

    if (later?.priceUsd !== null && later?.priceUsd !== undefined) {
      returns.push((later.priceUsd - start.priceUsd) / start.priceUsd);
    }
  }

  returns.sort((left, right) => left - right);
  const middle = Math.floor(returns.length / 2);
  const medianReturn = returns.length
    ? returns.length % 2
      ? returns[middle]
      : (returns[middle - 1] + returns[middle]) / 2
    : null;

  return { count: returns.length, medianReturn };
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
