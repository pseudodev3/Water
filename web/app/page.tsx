import { Scanner } from "@/components/scanner";

export default function Home() {
  return (
    <main className="shell">
      <header className="topbar">
        <a className="wordmark" href="/" aria-label="Water home">
          water
        </a>
        <div className="topbar-note">position intelligence</div>
      </header>

      <Scanner />

      <footer className="footer">
        Water shows evidence and derived diagnostics, not trading instructions.
      </footer>
    </main>
  );
}
