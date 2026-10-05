"use client";

import { Check, Copy, Droplets, ScanLine, Users } from "lucide-react";
import { KeyboardEvent, useId, useRef, useState } from "react";

export function ResearchHeader({
  current,
}: {
  current: "scanner" | "wallets";
}) {
  return (
    <header className="topbar research-topbar">
      <a className="wordmark" href="/" aria-label="Water home">
        <Droplets size={22} strokeWidth={1.5} aria-hidden="true" /> water
        <span className="brand-caption">onchain research</span>
      </a>
      <nav className="water-nav" aria-label="Water tools">
        <a href="/" aria-current={current === "scanner" ? "page" : undefined}>
          <ScanLine size={16} aria-hidden="true" /> <span>Token scanner</span>
        </a>
        <a
          href="/wallets"
          aria-current={current === "wallets" ? "page" : undefined}
        >
          <Users size={16} aria-hidden="true" /> <span>Wallet tracker</span>
        </a>
      </nav>
    </header>
  );
}

export function CopyAddress({
  value,
  label = "address",
}: {
  value: string;
  label?: string;
}) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setState("copied");
    } catch {
      setState("failed");
    }
  }
  return (
    <span className="copy-address">
      <button
        type="button"
        className="icon-action"
        aria-label={`Copy ${label}`}
        onClick={() => void copy()}
      >
        {state === "copied" ? (
          <Check size={15} aria-hidden="true" />
        ) : (
          <Copy size={15} aria-hidden="true" />
        )}
      </button>
      <span className="copy-feedback" role="status">
        {state === "copied"
          ? "Copied"
          : state === "failed"
            ? "Copy failed. Select the address instead."
            : ""}
      </span>
    </span>
  );
}

export function useResearchTabs<T extends string>(initial: T) {
  const [active, setActive] = useState<T>(initial);
  const [visited, setVisited] = useState<T[]>([initial]);
  const id = useId();
  function select(next: T) {
    setActive(next);
    setVisited((current) =>
      current.includes(next) ? current : [...current, next],
    );
  }
  function panel(value: T) {
    return {
      id: `${id}-panel-${value}`,
      role: "tabpanel",
      "aria-labelledby": `${id}-tab-${value}`,
      hidden: active !== value,
      tabIndex: 0,
    };
  }
  return { active, select, visited, id, panel };
}

export function ResearchTabs<T extends string>({
  tabs,
  active,
  onChange,
  id,
  label,
}: {
  tabs: ReadonlyArray<{ id: T; label: string; count?: number }>;
  active: T;
  onChange: (value: T) => void;
  id: string;
  label: string;
}) {
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  function move(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let next = index;
    if (event.key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (event.key === "ArrowLeft")
      next = (index - 1 + tabs.length) % tabs.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = tabs.length - 1;
    else return;
    event.preventDefault();
    onChange(tabs[next].id);
    buttons.current[next]?.focus();
  }
  return (
    <div className="research-tabs" role="tablist" aria-label={label}>
      {tabs.map((tab, index) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          id={`${id}-tab-${tab.id}`}
          aria-controls={`${id}-panel-${tab.id}`}
          aria-selected={active === tab.id}
          tabIndex={active === tab.id ? 0 : -1}
          ref={(node) => {
            buttons.current[index] = node;
          }}
          onClick={() => onChange(tab.id)}
          onKeyDown={(event) => move(event, index)}
        >
          {tab.label}
          {tab.count !== undefined && (
            <span className="tab-count">{tab.count}</span>
          )}
        </button>
      ))}
    </div>
  );
}
