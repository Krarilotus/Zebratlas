import { Workspace } from "@/components/zebra/Workspace";
import type { Metadata } from "next";
export const metadata: Metadata = { referrer: "no-referrer" };
export default function AccountPage() { return <Workspace initialView="account" />; }
