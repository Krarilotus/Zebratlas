import { Workspace } from "@/components/zebra/Workspace";

export default async function ZebraPage({ searchParams }: { searchParams: Promise<Record<string, string | string[] | undefined>> }) {
  const params = await searchParams;
  return <Workspace initialQuery={typeof params.q === "string" ? params.q : ""} initialNode={typeof params.node === "string" ? params.node : undefined} initialSavedId={typeof params.saved === "string" ? params.saved : undefined} />;
}
