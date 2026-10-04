/** Historical artifact receipt, retained for inspection, not approved for download or installation.
 * The old public artifact failed current release validation. No replacement is verified.
 * The original receipt does not provide a verification timestamp; none is synthesized here.
 */
export const HISTORICAL_ZEBRA_RELEASE = {
  status: "withheld",
  version: "v0.1",
  url: "https://huggingface.co/datasets/Krarilotus/zebratlas-kg",
  revision: "e7de6d1b07c51e311f8c7b6feed2d1ff16ee6d9d",
  artifact: "graph.ttl",
  sha256: "990e5b027ccc4f562df1ae11026a4065f4dd3d01e3c94145e2409078f6d09f9b",
  manifestSha256: "8459a39e63343082f23eddab1e915a8d8e2369b2d4e099e7e61f059b6f4429ef",
} as const;
