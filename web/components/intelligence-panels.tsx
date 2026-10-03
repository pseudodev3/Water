"use client";

import {
  Activity,
  ArrowRight,
  Fingerprint,
  ExternalLink,
  History,
  MoveUpRight,
  Save,
  Scale,
  UsersRound,
} from "lucide-react";
import { useEffect, useState } from "react";
import launchpadMarks from "@/lib/launchpad-marks.json";
import {
  Chain,
  EarlyHolderMap,
  fetchEarlyHolders,
  fetchOrigin,
  fetchTokenInfo,
  OriginEvidence,
  ScanResult,
  TokenInfo,
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
          <div className="eyebrow">Large holder map</div>
          <h3>Where the big wallets stand</h3>
          <p className="section-subcopy">
            Current large wallet-controlled holders. Movement and entry numbers
            only appear when Water can support them.
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
          Holder candidates were unavailable from the current providers.
        </div>
      ) : data && data.holders.length ? (
        <>
          <div className="early-summary">
            <div>
              <span>Still holding</span>
              <strong>{formatPercent(data.cohort_retained_from_peak)}</strong>
              <small>
                {data.cohort_retained_from_peak === null
                  ? "needs all movement histories"
                  : "of combined verified peak"}
              </small>
            </div>
            <div>
              <span>Distributed</span>
              <strong>{formatPercent(data.cohort_distributed_fraction)}</strong>
              <small>
                {data.cohort_distributed_fraction === null
                  ? "needs all movement histories"
                  : "of verified acquired tokens"}
              </small>
            </div>
            <p>
              {data.wallets_listed} top wallets shown ·{" "}
              {data.complete_movement_histories} complete movement{" "}
              {data.complete_movement_histories === 1 ? "history" : "histories"}
            </p>
          </div>

          <div className="holder-list">
            {data.holders.map((holder) => (
              <div className="holder-row" key={holder.wallet}>
                <div className="holder-identity">
                  <span>{String(holder.rank).padStart(2, "0")}</span>
                  <div>
                    <strong>{shortenAddress(holder.wallet)}</strong>
                    <small>
                      {formatFirstSeen(
                        holder.first_acquired_at,
                        data.observed_at_unix,
                      )}
                    </small>
                  </div>
                </div>

                <div className="holder-retention">
                  <div>
                    <span>Still holding</span>
                    <strong>{formatPercent(holder.retained_from_peak)}</strong>
                  </div>
                  {holder.retained_from_peak !== null ? (
                    <div className="holder-track" aria-hidden="true">
                      <span
                        style={{
                          width: `${Math.max(
                            2,
                            holder.retained_from_peak * 100,
                          )}%`,
                        }}
                      />
                    </div>
                  ) : (
                    <div className="holder-track unknown" aria-hidden="true" />
                  )}
                  <small>
                    {formatQuantity(holder.current_quantity)} now ·{" "}
                    {holder.peak_quantity === null
                      ? "peak unknown"
                      : `${formatQuantity(holder.peak_quantity)} peak`}
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

                <div className="holder-coverage">
                  <span
                    className={`basis-tag ${
                      holder.basis_status
                        ? holder.basis_status.replace("_", "-")
                        : "unavailable"
                    }`}
                  >
                    {basisLabel(holder.basis_status, holder.basis_coverage)}
                  </span>
                  <small>{holderStatusLine(holder)}</small>
                </div>
              </div>
            ))}
          </div>
        </>
      ) : (
        <div className="early-state muted">
          No wallet-controlled holder candidates could be verified.
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
  if (timestamp === null || timestamp <= 0) return "Entry time unavailable";

  const seconds = Math.max(0, now - timestamp);
  if (seconds < 60) return "Earliest observed <1m ago";
  if (seconds < 3_600) {
    return `Earliest observed ${Math.floor(seconds / 60)}m ago`;
  }
  if (seconds < 86_400) {
    return `Earliest observed ${Math.floor(seconds / 3_600)}h ago`;
  }
  return `Earliest observed ${Math.floor(seconds / 86_400)}d ago`;
}

function basisLabel(
  status: "verified" | "partial_history" | "incomplete" | null,
  coverage: number | null,
) {
  if (status === "verified") return "entry verified";
  if (status === "partial_history" && coverage !== null) {
    return `${Math.round(coverage * 100)}% entry basis known`;
  }
  if (status === "incomplete") return "entry cost unknown";
  return "history unavailable";
}

function holderStatusLine(holder: EarlyHolderMap["holders"][number]) {
  if (holder.movement_history === "unavailable") {
    return "Current balance verified; wallet history unavailable.";
  }
  if (holder.movement_history === "partial") {
    return "Current balance known; peak/distribution withheld.";
  }
  if (holder.basis_status === "incomplete") {
    return "Movement history reconciles; economic entry could not be proven.";
  }
  return "Movement history reconciles.";
}


export function AssetLinks({
  chain,
  token,
}: {
  chain: Chain;
  token: string;
}) {
  const [info, setInfo] = useState<TokenInfo | null>(null);

  useEffect(() => {
    let active = true;
    setInfo(null);

    fetchTokenInfo(chain, token)
      .then((value) => {
        if (active) setInfo(value);
      })
      .catch(() => {
        if (active) setInfo(null);
      });

    return () => {
      active = false;
    };
  }, [chain, token]);

  if (!info) return null;

  const links = [
    info.websites[0] ? { label: "Website", url: info.websites[0] } : null,
    info.twitter_url ? { label: "X", url: info.twitter_url } : null,
    info.telegram_url ? { label: "Telegram", url: info.telegram_url } : null,
    info.discord_url ? { label: "Discord", url: info.discord_url } : null,
    info.farcaster_url ? { label: "Farcaster", url: info.farcaster_url } : null,
    info.zora_url ? { label: "Zora", url: info.zora_url } : null,
  ].filter((item): item is { label: string; url: string } => Boolean(item));

  if (!links.length) return null;

  return (
    <div className="asset-links" aria-label="Token links">
      {links.slice(0, 5).map((link) => (
        <a
          key={link.label}
          href={link.url}
          target="_blank"
          rel="noreferrer"
        >
          {link.label}
          <ExternalLink size={10} strokeWidth={1.5} aria-hidden="true" />
        </a>
      ))}
    </div>
  );
}

// Only verified, locally vendored marks belong here. Unknown slugs keep initials.
const launchpadLogos: Readonly<Record<string, { path: string }>> = launchpadMarks;

function LaunchpadBrand({ slug, name }: { slug: string; name: string }) {
  const logo = Object.hasOwn(launchpadLogos, slug) ? launchpadLogos[slug].path : undefined;
  const [failedLogo, setFailedLogo] = useState<string | undefined>();

  return (
    <div className="launchpad-brand">
      {logo && failedLogo !== logo ? (
        <img
          className="launchpad-logo"
          src={logo}
          width={34}
          height={34}
          alt=""
          aria-hidden="true"
          onError={() => setFailedLogo(logo)}
        />
      ) : (
        <span className="launchpad-logo-fallback" aria-hidden="true">
          {name.slice(0, 1).toUpperCase()}
        </span>
      )}
      <div>
        <span>Launchpad recognized</span>
        <strong>{name}</strong>
      </div>
    </div>
  );
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
          {data.launchpad ? (
            <div className="launchpad-match">
              <LaunchpadBrand
                slug={data.launchpad.slug}
                name={data.launchpad.name}
              />
              <small>{data.launchpad.evidence}</small>
            </div>
          ) : (
            <div className="launchpad-match quiet">
              <div className="launchpad-brand">
                <span className="launchpad-logo-fallback" aria-hidden="true">
                  ?
                </span>
                <div>
                  <span>Launchpad</span>
                  <strong>Not identified</strong>
                </div>
              </div>
              <small>
                Water did not find a high-confidence launchpad fingerprint.
              </small>
            </div>
          )}

          <div className="origin-addresses">
            <div>
              <span>{data.primary_label}</span>
              <strong>
                {data.primary_address ? shortenAddress(data.primary_address) : "revoked / unknown"}
              </strong>
              {data.creator_label ? (
                <small>{data.creator_label}</small>
              ) : null}
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


export function CounterCasePanel({ result }: { result: ScanResult }) {
  const continuation: string[] = [];
  const fragility: string[] = [];

  if (result.demand.buy_share_h1 !== null) {
    const buyShare = result.demand.buy_share_h1;
    const sentence = `${Math.round(buyShare * 100)}% of last-hour top-pool transactions were buys.`;
    (buyShare >= 0.5 ? continuation : fragility).push(sentence);
  }

  if (result.demand.buyer_arrival_vs_h24_hourly !== null) {
    const pace = result.demand.buyer_arrival_vs_h24_hourly;
    const sentence = `Unique-buyer arrival is ${pace.toFixed(1)}× the token's 24h hourly average.`;
    (pace >= 1 ? continuation : fragility).push(sentence);
  }

  const concentration = result.pressure.components.find(
    (component) => component.key === "top_holder_concentration",
  );
  if (concentration) {
    const sentence = `Top-wallet concentration is ${concentration.observed} of supply.`;
    (concentration.pressure >= 0.5 ? fragility : continuation).push(sentence);
  }

  const coverage = result.pressure.components.find(
    (component) => component.key === "liquidity_coverage",
  );
  if (coverage) {
    const sentence = `Observed DEX liquidity covers ${coverage.observed} of the valuation reference.`;
    (coverage.pressure >= 0.5 ? fragility : continuation).push(sentence);
  }

  return (
    <article className="countercase-panel">
      <div className="section-heading compact countercase-heading">
        <div>
          <div className="eyebrow">Countercase</div>
          <h3>Build both sides before you decide.</h3>
          <p className="section-subcopy">
            Same evidence, argued in opposite directions. Placement uses the
            same neutral midpoints as Water&apos;s visible diagnostics.
          </p>
        </div>
        <Scale size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>

      <div className="countercase-body">
        <EvidenceCase
          label="Continuation case"
          items={continuation}
          empty="No current buyer-side evidence clears its neutral comparison."
        />
        <EvidenceCase
          label="Fragility case"
          items={fragility}
          empty="No current holder/liquidity constraint was available."
        />
      </div>
    </article>
  );
}

function EvidenceCase({
  label,
  items,
  empty,
}: {
  label: string;
  items: string[];
  empty: string;
}) {
  return (
    <div className="evidence-case">
      <span>{label}</span>
      {items.length ? (
        items.slice(0, 3).map((item) => <p key={item}>{item}</p>)
      ) : (
        <p className="case-empty">{empty}</p>
      )}
    </div>
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
  const [saveError, setSaveError] = useState("");
  const current = thesisSnapshot(result);
  const changeFlags = saved ? thesisChangeFlags(saved, current) : [];

  useEffect(() => {
    setSaveError("");
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
      setSaveError("");
    } catch {
      setSaveError("This browser could not save the baseline. Check storage permissions and try again.");
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
          {changeFlags.length ? (
            <div className="water-alerts">
              <span>Change flags</span>
              <div>
                {changeFlags.map((flag) => (
                  <p key={flag}>{flag}</p>
                ))}
              </div>
              <small>
                Sensitivity thresholds only — these are not buy/sell signals.
              </small>
            </div>
          ) : null}

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
            Save &amp; watch this setup
          </button>
        </div>
      )}
      {saveError ? <p className="error-line" role="alert">{saveError}</p> : null}
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


function thesisChangeFlags(before: ThesisSnapshot, now: ThesisSnapshot) {
  const flags: string[] = [];

  if (
    before.holderConcentration !== null &&
    now.holderConcentration !== null
  ) {
    const delta = now.holderConcentration - before.holderConcentration;
    if (Math.abs(delta) >= 0.05) {
      flags.push(
        `Top-wallet concentration ${delta >= 0 ? "+" : ""}${(
          delta * 100
        ).toFixed(1)}pp`,
      );
    }
  }

  if (before.buyShare !== null && now.buyShare !== null) {
    const delta = now.buyShare - before.buyShare;
    if (Math.abs(delta) >= 0.1) {
      flags.push(
        `Buy share ${delta >= 0 ? "+" : ""}${(delta * 100).toFixed(1)}pp`,
      );
    }
  }

  for (const [label, previous, current, threshold] of [
    ["Liquidity", before.liquidityUsd, now.liquidityUsd, 0.2],
    ["Market cap", before.marketCapUsd, now.marketCapUsd, 0.25],
  ] as const) {
    if (previous !== null && current !== null && previous !== 0) {
      const delta = (current - previous) / Math.abs(previous);
      if (Math.abs(delta) >= threshold) {
        flags.push(
          `${label} ${delta >= 0 ? "+" : ""}${(delta * 100).toFixed(0)}%`,
        );
      }
    }
  }

  return flags;
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
  const calibration = localCalibration(history, result.pressure.index);

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
              <MemoryDelta
                label="Top wallets"
                before={previous.holderConcentration}
                now={current.holderConcentration}
                kind="points"
              />
            </div>
          </div>
        ) : (
          <p className="memory-first">
            First saved read for this token. The next read creates a comparison.
          </p>
        )}

        <HolderConcentrationPath history={tokenHistory} />

        <div className="calibration-strip">
          <div>
            <span>{calibration.label}</span>
            <strong>{calibration.samples}</strong>
            <small>matching saved starting reads</small>
          </div>
          <CalibrationCell label="~1h" sample={calibration.h1} />
          <CalibrationCell label="~6h" sample={calibration.h6} />
          <CalibrationCell label="~24h" sample={calibration.h24} />
        </div>

        <p className="memory-note">
          Outcome samples compare later reads from the same token and the same
          25-point Exit Pressure band. They are calibration evidence, not a
          forecast.
        </p>
      </div>
    </article>
  );
}

function HolderConcentrationPath({
  history,
}: {
  history: ScanMemory[];
}) {
  const points = history
    .filter((item) => item.holderConcentration !== null)
    .slice(-5);

  if (points.length < 2) return null;

  return (
    <div className="holder-path">
      <span>Top-wallet concentration path</span>
      <div>
        {points.map((point, index) => (
          <span key={`${point.at}-${index}`}>
            <strong>{point.holderConcentration!.toFixed(1)}%</strong>
            {index < points.length - 1 ? (
              <ArrowRight size={11} strokeWidth={1.5} aria-hidden="true" />
            ) : null}
          </span>
        ))}
      </div>
      <small>Oldest → newest from your saved reads on this device.</small>
    </div>
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

function localCalibration(
  history: ScanMemory[],
  currentPressure: number | null,
) {
  const band = pressureBand(currentPressure);
  const starts = history.filter((item) => pressureBand(item.pressure) === band);

  return {
    label:
      band === null
        ? "Unscored calibration"
        : `Pressure ${band[0]}–${band[1]}`,
    samples: starts.length,
    h1: calibrationWindow(history, 3_600, 7_200, band),
    h6: calibrationWindow(history, 21_600, 32_400, band),
    h24: calibrationWindow(history, 86_400, 129_600, band),
  };
}

function pressureBand(
  pressure: number | null,
): readonly [number, number] | null {
  if (pressure === null || !Number.isFinite(pressure)) return null;
  const low = Math.min(75, Math.floor(pressure / 25) * 25);
  return [low, low + 24] as const;
}

function samePressureBand(
  pressure: number | null,
  band: readonly [number, number] | null,
) {
  const candidate = pressureBand(pressure);
  if (band === null) return candidate === null;
  return candidate !== null && candidate[0] === band[0];
}

function calibrationWindow(
  history: ScanMemory[],
  minSeconds: number,
  maxSeconds: number,
  band: readonly [number, number] | null,
) {
  const returns: number[] = [];

  for (let index = 0; index < history.length; index += 1) {
    const start = history[index];
    if (
      start.priceUsd === null ||
      start.priceUsd <= 0 ||
      !samePressureBand(start.pressure, band)
    ) {
      continue;
    }

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
