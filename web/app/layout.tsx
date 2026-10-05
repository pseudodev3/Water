import type { Metadata } from "next";
import "./globals.css";
import "./research.css";

export const metadata: Metadata = {
  title: "Water",
  description: "Read the other side of the trade.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
