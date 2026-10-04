import path from "node:path";

import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // web/ is its own app; don't let Turbopack pick up workspace files from parent folders.
  turbopack: { root: path.resolve(".") },
  experimental: { cpus: 2 },
  // Metadata (the page <title>) blocks instead of streaming, so every page has its title in <head>
  // for screen readers and the axe check (WCAG 2.4.2); the summary call is shared with the page.
  htmlLimitedBots: /.*/,
};

export default nextConfig;
