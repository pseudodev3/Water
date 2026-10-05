import { WalletTracker } from "@/components/wallet-tracker";
import { ResearchHeader } from "@/components/research-ui";

export default function Wallets() {
  return (
    <main className="shell">
      <ResearchHeader current="wallets" />
      <WalletTracker />
      <footer className="footer">
        Water shows evidence and derived diagnostics, not trading instructions.
      </footer>
    </main>
  );
}
