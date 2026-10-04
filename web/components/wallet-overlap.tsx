"use client";
import { useEffect, useState } from "react";
import { Users } from "lucide-react";
import type { Chain } from "@/lib/api";
import { getWalletOverlap, shortAddress } from "@/lib/wallets";

export function WalletOverlap({
  chain,
  token,
}: {
  chain: Chain;
  token: string;
}) {
  const [wallets, setWallets] = useState<
    Array<{ wallet: string; status: string }>
  >([]);
  const [state, setState] = useState<"loading" | "ready" | "failed">("loading");
  useEffect(() => {
    const controller = new AbortController();
    setWallets([]);
    setState("loading");
    getWalletOverlap(chain, token, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) {
          setWallets(value.wallets);
          setState("ready");
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) setState("failed");
      });
    return () => controller.abort();
  }, [chain, token]);
  return (
    <section className="wallet-overlap">
      <div className="section-heading">
        <div>
          <div className="eyebrow">Wallet overlap</div>
          <h3>Who else holds this?</h3>
        </div>
        <Users size={18} strokeWidth={1.5} aria-hidden="true" />
      </div>
      <p className="muted-copy">
        {state === "loading"
          ? "Checking collected qualifying wallets…"
          : state === "failed"
            ? "Wallet evidence is unavailable for this read."
            : wallets.length === 0
              ? "No fresh qualifying wallet in Water’s collected cohort has a verified position here."
              : `${wallets.length} qualifying ${wallets.length === 1 ? "wallet holds" : "wallets hold"} this asset.`}
      </p>
      {wallets.map((w) => (
        <p key={w.wallet}>
          {shortAddress(w.wallet)} ·{" "}
          {w.status === "qualified_60d"
            ? "60-day consistent record"
            : "30-day record"}
        </p>
      ))}
      <a className="wallet-inline-button" href="/wallets">
        Explore wallet records
      </a>
    </section>
  );
}
