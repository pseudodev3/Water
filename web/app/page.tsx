import { Scanner } from "@/components/scanner";
import { ResearchHeader } from "@/components/research-ui";

export default function Home() {
  return (
    <main className="shell">
      <ResearchHeader current="scanner" />

      <Scanner />

      <footer className="footer">
        Water shows evidence and derived diagnostics, not trading instructions.
      </footer>
    </main>
  );
}
