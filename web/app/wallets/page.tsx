import { WalletTracker } from "@/components/wallet-tracker";

export default function Wallets() {
  return (
    <main className="shell">
      <header className="topbar">
        <a className="wordmark" href="/" aria-label="Water home">
          water
        </a>
        <nav className="water-nav" aria-label="Water tools">
          <a href="/">Token scan</a>
          <a href="/wallets" aria-current="page">
            Wallets
          </a>
        </nav>
      </header>
      <WalletTracker />
      <footer className="footer">
        Water shows evidence and derived diagnostics, not trading instructions.
      </footer>
    </main>
  );
}
