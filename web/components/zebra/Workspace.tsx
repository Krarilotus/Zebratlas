"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";


import dynamic from "next/dynamic";
import Link from "next/link";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { community, condition, explore, runSparqlQuery, saveItem, savedItems } from "@/lib/zebra/client";
import type { ConditionDetail, ConditionSection, ExplorePlan, ExploreResponse, ExploreResult, SearchGraph } from "@/lib/zebra/types";
import { useZebraCopy, useZebraLocale, useZebraKindLabel } from "./Locale";
import { getZebraCopy, resolveZebraLocale, zebraHref } from "@/lib/zebra/locale";
import { hasRenderableAnswer } from "@/lib/zebra/query-answer";
import { contributionHref } from "@/lib/zebra/contribution-form";
import { Icon } from "./Icon";
import { ZebraLoader } from "./ZebraLoader";
import { Brand, BrandText } from "./Brand";
import { AccountControl } from "./AccountControl";
import { installMenuDismissal } from "@/lib/zebra/menu-dismissal";
import { dismissHeaderPopover, toggleHeaderPopover, type HeaderPopover } from "@/lib/zebra/header-popover";
import { LanguagePicker } from "./LanguagePicker";
import { NetworkDirectory } from "./NetworkDirectory";
import { publicSearchUrl } from "@/lib/zebra/search-privacy";
import { PrivacyNotice } from "./Privacy";
import { SearchBox } from "./SearchBox";
import { acceptNavigationResponse, decodeNavigation, heroProgress, infoNavigation, isCurrentInfo, restoreNavigation, type NavigationView } from "./navigation";
import { compactResultLabel } from "./result-label";
import { queryHistoryState, type QueryRunSettings } from "./query-workspace";
import queryStyles from "./QueryWorkspace.module.css";
import ContactFilters from "./ContactFilters";
import { contactPlan, contactRole, type ContactRole } from "./contact-filters";
import { classifyExploreError } from "@/lib/zebra/explore-error";
import { isShortIndexedName, lookupIndexedNames, type IndexedLookupMatch } from "@/lib/zebra/indexed-lookup";

function GraphPending() { const copy = useZebraCopy(); return <div className="z-graph-loading" role="status"><ZebraLoader /><span>{copy.loading}</span></div>; }
const Graph = dynamic(() => import("./Graph"), { ssr: false, loading: GraphPending });
const Sources = dynamic(() => import("./Sources"));
const ResearchBrief = dynamic(() => import("./ResearchBrief"));
const QueryPlan = dynamic(() => import("./QueryPlan"));
const QueryAnswer = dynamic(() => import("./QueryAnswer"));
const ResultOverview = dynamic(() => import("./ResultOverview"));
const ProgrammeRoutes = dynamic(() => import("./ProgrammeRoutes"));
const ConditionPanel = dynamic(() => import("./ConditionPanel").then((module) => module.ConditionPanel));
const Utility = dynamic(() => import("./Utility").then((module) => module.Utility));
const ExploreError = dynamic(() => import("./ExploreError").then(module => module.ExploreError));
type View = "home" | "community" | "contribute" | "saved" | "account" | "about" | "privacy" | "request-removal" | "imprint";
const queryInfoSymbol = "i";
const viewWords = { en: { expand: "Explore graph", collapse: "Show findings", back: "Back to results", query: "Inspect query", bookmarks: "Search results", nodes: "nodes", edges: "connections", limited: "Limited view" }, de: { expand: "Graph erkunden", collapse: "Ergebnisse anzeigen", back: "Zurück zu den Ergebnissen", query: "Abfrage ansehen", bookmarks: "Suchergebnisse", nodes: "Knoten", edges: "Verbindungen", limited: "Begrenzte Ansicht" } };
type Panel = "overview" | "evidence" | "request" | "plan";
type GraphVisit = { data: ExploreResponse; selectedId: string | null };
type SearchSnapshot = { data: ExploreResponse; query: string; text: string; filter: string; scroll: number; visits: GraphVisit[]; extra: Map<string, { result: ExploreResult; context?: string }> };
type HistoryView = NavigationView;
function historyView(): HistoryView | null {
  return decodeNavigation(window.history.state);
}
function navigationGraph(next: SearchGraph, previous: SearchGraph): SearchGraph {
  const nodes = new Map(next.nodes.map((node) => [node.id, node]));
  for (const node of previous.nodes) if (!nodes.has(node.id) && nodes.size < 160) nodes.set(node.id, { ...node, matched: false });
  const edges = new Map(next.edges.map((edge) => [edge.id, edge]));
  for (const edge of previous.edges) if (!edges.has(edge.id) && nodes.has(edge.source) && nodes.has(edge.target) && edges.size < 800) edges.set(edge.id, { ...edge, highlighted: false });
  return { ...next, nodes: [...nodes.values()], edges: [...edges.values()] };
}

export function Workspace({ initialQuery = "", initialView = "home", initialNode, initialSavedId }: { initialQuery?: string; initialView?: View; initialNode?: string; initialSavedId?: string }) {
  const copy = useZebraCopy();
  const locale = useZebraLocale();
  const kindLabel = useZebraKindLabel();
  const views = viewWords[zebraCopyLocale(locale)];
  const href = (path: string) => zebraHref(path, locale);
  const graphLabels = { region: copy.graphRegion, zoomIn: copy.zoomIn, zoomOut: copy.zoomOut, fit: copy.fit, selectNode: copy.selectNode, nodeList: copy.nodeList, connections: copy.connections, moreNodes: copy.moreNodes, back: copy.graphBack, reset: copy.graphReset, previousNeighbors: copy.graphPrevious, nextNeighbors: copy.graphNext, moreNeighbors: copy.graphMoreNeighbors, noNeighbors: copy.graphNoNeighbors, loading: copy.loading, kinds: copy.kinds, edgeProperties: copy.graphRelations, property: copy.graphProperty, noEdgeEvidence: copy.graphNoEdgeEvidence, close: copy.close, evidence: copy.evidence, inferred: copy.graphInferred, hypothesis: copy.graphHypothesis };
  const [query, setQuery] = useState(initialQuery);
  const [searchRevision, setSearchRevision] = useState(0);
  const [data, setData] = useState<ExploreResponse | null>(null);
  const [rootData, setRootData] = useState<ExploreResponse | null>(null);
  const [page, setPage] = useState<"results" | "info">(initialNode ? "info" : "results");
  const [graphExpanded, setGraphExpanded] = useState(initialView === "community");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [errorCause, setErrorCause] = useState<unknown>(null);
  const [indexedMatches, setIndexedMatches] = useState<IndexedLookupMatch[]>([]);
  const [indexedCorrection, setIndexedCorrection] = useState<string | undefined>();
  const [indexedBusy, setIndexedBusy] = useState(false);
  const indexedRequest = useRef<AbortController | null>(null);
  const [filter, setFilter] = useState("all");
  const [selectedId, setSelectedId] = useState<string | null>(initialNode ?? null);
  const [selectedExtra, setSelectedExtra] = useState<ExploreResult | null>(null);
  const [selectedContext, setSelectedContext] = useState<string | undefined>(undefined);
  const [detail, setDetail] = useState<ConditionDetail | null>(null);
  const [detailBusy, setDetailBusy] = useState(false);
  const [openPanel, setOpenPanel] = useState<Panel | null>(initialNode ? "overview" : null);
  const [headerPopover, setHeaderPopover] = useState<HeaderPopover>(null);
  const menu = headerPopover === "account";
  const closeNetworks = useCallback(() => setHeaderPopover(current => dismissHeaderPopover(current, "networks")), []);
  const [phone, setPhone] = useState(false);
  const [notice, setNotice] = useState("");
  const [navigating, setNavigating] = useState(false);
  const [navigationDepth, setNavigationDepth] = useState(0);
  const [sectionLoading, setSectionLoading] = useState(false);
  const searchRequest = useRef<AbortController | null>(null);
  const navigationRequest = useRef<AbortController | null>(null);
  const sectionRequest = useRef<AbortController | null>(null);
  const graphHistory = useRef<GraphVisit[]>([]);
  const graphHistoryIndex = useRef(0);
  const actualQuery = useRef(initialQuery);
  const headerMenu = useRef<HTMLDivElement>(null);
  const menuButton = useRef<HTMLButtonElement>(null);
  const panelElement = useRef<HTMLElement>(null);
  const readingScroll = useRef<HTMLDivElement>(null);
  const heroElement = useRef<HTMLElement>(null);
  const heroDistance = useRef(0);
  const heroInitialized = useRef(false);
  const heroStartExpanded = useRef(initialView === "community");
  const heroFrame = useRef<number | null>(null);
  const heroExpandedRef = useRef(initialView === "community");
  const pageRef = useRef<"results" | "info">(initialNode ? "info" : "results");
  const activeSearch = useRef("");
  const snapshots = useRef(new Map<string, SearchSnapshot>());
  const pendingScroll = useRef<number | null>(null);
  const communityView = initialView === "community";
  const workspace = initialView === "home" && (!!query || busy || !!data || !!error) || communityView;
  const utility = !["home", "community"].includes(initialView);
  const drawerShown = openPanel === "plan";
  const queryOpened = useRef(false);
  const queryReturnPanel = useRef<Panel | null>(null);
  function openQuery() {
    if (queryOpened.current) return;
    queryOpened.current = true; queryReturnPanel.current = openPanel;
    window.history.pushState(queryHistoryState(window.history.state, activeSearch.current), "", window.location.href);
    setOpenPanel("plan");
  }
  const closeQuery = useCallback(() => {
    if (queryOpened.current && window.history.state?.zebraQuery) { window.history.back(); return; }
    queryOpened.current = false; setOpenPanel(queryReturnPanel.current);
  }, []);

  useLayoutEffect(() => {
    if (!workspace || !readingScroll.current || !heroElement.current) return;
    const scroll = readingScroll.current;
    const hero = heroElement.current;
    function paint() {
      heroFrame.current = null;
      const progress = heroProgress(scroll.scrollTop, heroDistance.current, 0);
      hero.style.setProperty("--z-hero-offset", `${progress.offset}px`);
      const expanded = progress.expanded;
      if (expanded !== heroExpandedRef.current) { heroExpandedRef.current = expanded; setGraphExpanded(expanded); }
      const snapshot = snapshots.current.get(activeSearch.current);
      if (snapshot && pageRef.current === "results") snapshot.scroll = scroll.scrollTop;
    }
    function onScroll() { if (heroFrame.current === null) heroFrame.current = requestAnimationFrame(paint); }
    function measure() {
      heroDistance.current = Math.max(0, hero.clientHeight - parseFloat(getComputedStyle(hero).getPropertyValue("--z-hero-compact")));
      scroll.style.setProperty("--z-reading-height", `${scroll.clientHeight}px`);
      if (pendingScroll.current !== null) { scroll.scrollTop = pendingScroll.current; pendingScroll.current = null; }
      else if (!heroInitialized.current && hero.clientHeight > 0) { scroll.scrollTop = heroStartExpanded.current ? 0 : heroDistance.current; heroInitialized.current = true; }
      onScroll();
    }
    const observer = new ResizeObserver(measure);
    observer.observe(hero); observer.observe(scroll);
    scroll.addEventListener("scroll", onScroll, { passive: true });
    measure();
    return () => { observer.disconnect(); scroll.removeEventListener("scroll", onScroll); if (heroFrame.current !== null) cancelAnimationFrame(heroFrame.current); heroFrame.current = null; };
  }, [workspace, communityView]);
  function revealGraph(expand = true) {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    readingScroll.current?.scrollTo({ top: expand ? 0 : heroDistance.current, behavior: reduced ? "auto" : "smooth" });
  }

  const runSearch = useCallback(async (text: string, display = text, updateUrl = true, focusId?: string, plan?: ExplorePlan, inspectPlan = true) => {
    searchRequest.current?.abort();
    navigationRequest.current?.abort(); sectionRequest.current?.abort(); setNavigating(false); setNavigationDepth(0);
    graphHistory.current = []; graphHistoryIndex.current = 0;
    const controller = new AbortController(); searchRequest.current = controller;
    actualQuery.current = text;
    indexedRequest.current?.abort(); setIndexedMatches([]); setIndexedCorrection(undefined); setIndexedBusy(false); setErrorCause(null);
    queryOpened.current = false;
    if (!plan) { heroInitialized.current = false; heroStartExpanded.current = communityView && !text; }
    if (updateUrl || !activeSearch.current) { snapshots.current.clear(); activeSearch.current = crypto.randomUUID(); }
    setQuery(display); setBusy(true); setData(null); setRootData(null); setError(""); setFilter("all"); setSelectedId(null); setSelectedExtra(null); setSelectedContext(undefined); setDetail(null); setOpenPanel(plan && inspectPlan ? "plan" : null); if (!focusId) { setPage("results"); pageRef.current = "results"; }
    const searchUrl = updateUrl && !communityView ? zebraHref("/zebra", resolveZebraLocale(document.documentElement.lang)) : window.location.href;
    window.history.replaceState({ ...queryHistoryState(window.history.state), zebra: { search: activeSearch.current, page: "results" } }, "", publicSearchUrl(searchUrl, "", []));
    try {
      const response = await explore(text, { mode: plan && !inspectPlan ? "knowledge" : communityView ? "community" : "knowledge", signal: controller.signal, plan }); if (controller.signal.aborted) return;
      setData(response); setRootData(response); graphHistory.current = [{ data: response, selectedId: focusId ?? null }];
      snapshots.current.set(activeSearch.current, { data: response, query: display, text, filter: "all", scroll: heroDistance.current, visits: graphHistory.current, extra: new Map() });
      if (snapshots.current.size > 8) snapshots.current.delete(snapshots.current.keys().next().value!);
      window.history.replaceState({ ...queryHistoryState(window.history.state), zebra: { search: activeSearch.current, page: focusId ? "info" : "results", node: focusId } }, "", publicSearchUrl(window.location.href, display, response.graph.nodes.map((node) => node.id), focusId));
      if (focusId && response.graph.nodes.some((node) => node.id === focusId)) { setSelectedId(focusId); setPage("info"); pageRef.current = "info"; if (!(document.activeElement instanceof HTMLElement && document.activeElement.closest(".z-search"))) setOpenPanel("overview"); }
      if (!plan && readingScroll.current) { readingScroll.current.scrollTop = communityView && !text ? 0 : heroDistance.current; }
    }
    catch (e) { if (!controller.signal.aborted) { setData(null); setErrorCause(e); setError(e instanceof Error ? e.message : getZebraCopy(resolveZebraLocale(document.documentElement.lang)).offline); if (!plan && text === display && classifyExploreError(e).kind === "unsupported" && isShortIndexedName(text)) { const lookupController = new AbortController(); indexedRequest.current = lookupController; setIndexedBusy(true); void lookupIndexedNames(text, lookupController.signal).then(response => { if (!lookupController.signal.aborted && !controller.signal.aborted) { setIndexedMatches(response.matches); setIndexedCorrection(response.corrected_query); } }).catch(() => {}).finally(() => { if (!lookupController.signal.aborted && !controller.signal.aborted) setIndexedBusy(false); }); } } }
    finally { if (!controller.signal.aborted) setBusy(false); }
  }, [communityView]);

  async function runSparql(sparql: string, settings: QueryRunSettings) {
    searchRequest.current?.abort(); navigationRequest.current?.abort(); sectionRequest.current?.abort();
    const controller = new AbortController(); searchRequest.current = controller;
    const caption = query; const text = actualQuery.current;
    const requestedSearch = activeSearch.current;
    setNavigating(false); setSectionLoading(false); setBusy(true); setError(""); setErrorCause(null); setIndexedMatches([]);
    try {
      const response = await runSparqlQuery(sparql, caption, controller.signal, settings);
      if (controller.signal.aborted || activeSearch.current !== requestedSearch) return;
      // Replace the complete workspace only after execution succeeds. An invalid
      // query keeps the editable draft mounted, so the user can correct it.
      const search = crypto.randomUUID(); activeSearch.current = search;
      snapshots.current.clear(); graphHistoryIndex.current = 0;
      setNavigationDepth(0); setFilter("all"); setSelectedId(null); setSelectedExtra(null);
      setSelectedContext(undefined); setDetail(null); setPage("results"); pageRef.current = "results";
      queryOpened.current = false; setOpenPanel(null); heroStartExpanded.current = false; heroInitialized.current = false;
      const url = new URL(window.location.href); url.searchParams.delete("node");
      window.history.replaceState({ ...queryHistoryState(window.history.state), zebra: { search, page: "results" } }, "", `${url.pathname}${url.search}${url.hash}`);
      setData(response); setRootData(response);
      graphHistory.current = [{ data: response, selectedId: null }];
      snapshots.current.set(search, { data: response, query: caption, text, filter: "all", scroll: heroDistance.current, visits: graphHistory.current, extra: new Map() });
      readingScroll.current?.scrollTo({ top: heroDistance.current, behavior: "auto" });
    } catch (cause) {
      if (!controller.signal.aborted) { setErrorCause(cause); setError(cause instanceof Error ? cause.message : copy.offline); }
    } finally { if (!controller.signal.aborted) setBusy(false); }
  }

  async function indexedLookup() {
    indexedRequest.current?.abort(); const controller = new AbortController(); indexedRequest.current = controller;
    setIndexedBusy(true);
    try { const response = await lookupIndexedNames(query, controller.signal); if (!controller.signal.aborted) { setIndexedMatches(response.matches); setIndexedCorrection(response.corrected_query); } }
    catch { if (!controller.signal.aborted) { setIndexedMatches([]); setIndexedCorrection(undefined); } }
    finally { if (!controller.signal.aborted) setIndexedBusy(false); }
  }

  useEffect(() => {
    let mounted = true;
    if (restoreNavigation(window.history.state, snapshots.current)?.view.search === activeSearch.current) return;
    if (initialSavedId) {
      const controller = new AbortController(); searchRequest.current = controller;
      void Promise.resolve().then(() => { if (mounted) setBusy(true); });
      void savedItems(controller.signal).then((items) => {
        if (!mounted) return;
        const item = items.find((saved) => saved.id === initialSavedId);
        const text = typeof item?.payload.query === "string" ? item.payload.query : item?.refs[0];
        if (text) void runSearch(text, typeof item?.payload.displayQuery === "string" ? item.payload.displayQuery : text, false);
        else { setBusy(false); setError(getZebraCopy(resolveZebraLocale(document.documentElement.lang)).noData); }
      }).catch((e: unknown) => { if (mounted) { setBusy(false); setError(e instanceof Error ? e.message : getZebraCopy(resolveZebraLocale(document.documentElement.lang)).accountError); } });
    }
    else if (initialQuery || initialNode) void Promise.resolve().then(() => { if (mounted) void runSearch(initialQuery || initialNode || "", initialQuery || initialNode || "", false, initialNode); });
    else if (communityView) {
      const controller = new AbortController(); searchRequest.current = controller;
      void Promise.resolve().then(() => { if (mounted) setBusy(true); });
      void community({ signal: controller.signal }).then((response) => { if (!controller.signal.aborted) { setData(response); setRootData(response); graphHistory.current = [{ data: response, selectedId: null }]; activeSearch.current ||= crypto.randomUUID(); snapshots.current.set(activeSearch.current, { data: response, query: "", text: "", filter: "all", scroll: 0, visits: graphHistory.current, extra: new Map() }); window.history.replaceState({ ...window.history.state, zebra: { search: activeSearch.current, page: "results" } }, "", window.location.href); } }).catch((e: unknown) => { if (!controller.signal.aborted) setError(e instanceof Error ? e.message : getZebraCopy(resolveZebraLocale(document.documentElement.lang)).offline); }).finally(() => { if (!controller.signal.aborted) setBusy(false); });
    }
    return () => { mounted = false; searchRequest.current?.abort(); };
  }, [communityView, initialQuery, initialNode, initialSavedId, runSearch]);
  useEffect(() => () => { searchRequest.current?.abort(); navigationRequest.current?.abort(); sectionRequest.current?.abort(); indexedRequest.current?.abort(); }, []);
  useEffect(() => {
    const media = window.matchMedia("(max-width: 720px)");
    const update = () => setPhone(media.matches);
    void Promise.resolve().then(update);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    if (!drawerShown) return;
    const previous = document.activeElement;
    if (previous instanceof HTMLElement && previous.closest(".z-search")) return;
    const panel = panelElement.current;
    panel?.querySelector<HTMLButtonElement>("button")?.focus({ preventScroll: true });
    return () => {
      // Return only focus still owned by this drawer; the user may have resumed typing.
      const active = document.activeElement;
      if (previous instanceof HTMLElement && previous.isConnected && (panel?.contains(active) || active === document.body)) previous.focus({ preventScroll: true });
    };
  }, [phone, drawerShown]);
  useEffect(() => {
    function back() {
      if (window.history.state?.zebraQuery === activeSearch.current) { queryOpened.current = true; setOpenPanel("plan"); return; }
      if (queryOpened.current && decodeNavigation(window.history.state)?.search === activeSearch.current) { queryOpened.current = false; setOpenPanel(queryReturnPanel.current); return; }
      queryOpened.current = false;
      searchRequest.current?.abort(); navigationRequest.current?.abort(); sectionRequest.current?.abort();
      setSearchRevision((value) => value + 1); setBusy(false); setNavigating(false); setDetail(null); setError("");
      const restored = restoreNavigation(window.history.state, snapshots.current); const state = restored?.view; const snapshot = restored?.snapshot;
      if (state && snapshot) {
        activeSearch.current = state.search; actualQuery.current = snapshot.text;
        setQuery(snapshot.query); setFilter(snapshot.filter); setRootData(snapshot.data); setPage(state.page); pageRef.current = state.page;
        const visitIndex = state.page === "info" ? snapshot.visits.findLastIndex((visit) => visit.selectedId === state.node) : 0;
        const visit = snapshot.visits[Math.max(0, visitIndex)];
        setData(state.page === "info" ? visit?.data ?? snapshot.data : snapshot.data);
        graphHistory.current = snapshot.visits; graphHistoryIndex.current = Math.max(0, visitIndex); setNavigationDepth(Math.max(0, visitIndex));
        setSelectedId(state.page === "info" ? state.node ?? null : null);
        setSelectedExtra(state.node ? snapshot.extra.get(state.node)?.result ?? null : null);
        setSelectedContext(state.context); setOpenPanel(state.page === "info" ? state.panel ?? "overview" : null);
        pendingScroll.current = state.page === "results" ? snapshot.scroll : heroDistance.current;
        requestAnimationFrame(() => { if (readingScroll.current && pendingScroll.current !== null) { readingScroll.current.scrollTop = pendingScroll.current; pendingScroll.current = null; } });
        return;
      }
      const params = new URLSearchParams(window.location.search); const q = params.get("q") ?? params.get("node") ?? "";
      if (q) { activeSearch.current = ""; void runSearch(q, q, false, params.get("node") ?? undefined); }
      else { setQuery(""); setData(null); setRootData(null); setPage("results"); pageRef.current = "results"; setNavigationDepth(0); setSelectedId(null); setSelectedExtra(null); setSelectedContext(undefined); setOpenPanel(null); graphHistory.current = []; graphHistoryIndex.current = -1; heroInitialized.current = false; }
    }
    window.addEventListener("popstate", back); return () => window.removeEventListener("popstate", back);
  }, [runSearch]);
  useEffect(() => {
    function key(event: KeyboardEvent) { if (event.key === "Escape") { if (queryOpened.current) closeQuery(); else setOpenPanel(null); setHeaderPopover(null); } }
    window.addEventListener("keydown", key); return () => window.removeEventListener("keydown", key);
  }, [closeQuery]);
  useEffect(() => {
    if (!menu || !headerMenu.current || !menuButton.current) return;
    return installMenuDismissal(headerMenu.current, menuButton.current, () => setHeaderPopover(current => dismissHeaderPopover(current, "account")), document);
  }, [menu]);

  const selected = useMemo<ExploreResult | null>(() => {
    if (!data || !selectedId) return null;
    if (selectedExtra?.id === selectedId) return selectedExtra;
    const match = data.results.find((result) => result.id === selectedId);
    if (match) return match;
    const node = data.graph.nodes.find((item) => item.id === selectedId);
    return node ? { ...node, reason: data.graph.edges.filter((edge) => edge.source === node.id || edge.target === node.id).map((edge) => edge.label || edge.relation).slice(0, 3).join(" · "), score: 0, evidence: data.graph.edges.filter((edge) => edge.source === node.id || edge.target === node.id).flatMap((edge) => edge.evidence) } : null;
  }, [data, selectedId, selectedExtra]);
  const linkedDiseases = [...new Set(data?.graph.edges.filter((edge) => edge.source === selectedId || edge.target === selectedId).map((edge) => edge.source === selectedId ? edge.target : edge.source).filter((id) => data.graph.nodes.some((node) => node.id === id && node.kind === "disease")) ?? [])];
  const linkedDisease = linkedDiseases.length === 1 ? linkedDiseases[0] : undefined;
  const conditionId = selected?.kind === "disease" ? selected.id : selectedContext ?? linkedDisease;
  const panelOpen = !!openPanel;
  useEffect(() => {
    sectionRequest.current?.abort();
    const controller = new AbortController();
    // A locale refresh must keep the same record and its mounted draft editor.
    // Clear factual context only when navigation changes the condition scope.
    void Promise.resolve().then(() => { if (!controller.signal.aborted) { setDetailBusy(panelOpen && !!conditionId); setDetail((current) => current?.id === conditionId ? current : null); setSectionLoading(false); } });
    if (!panelOpen || !conditionId) return () => controller.abort();
    void condition(conditionId, locale, controller.signal).then((response) => { if (!controller.signal.aborted) setDetail(response); }).catch(() => { if (!controller.signal.aborted) setDetail(null); }).finally(() => { if (!controller.signal.aborted) setDetailBusy(false); });
    return () => controller.abort();
  }, [conditionId, panelOpen, locale]);
  const resultData = rootData ?? data;
  function chooseRole(role: ContactRole) { const plan = contactPlan(resultData, role); if (plan) void runSearch(actualQuery.current, query, false, undefined, plan, false); }
  function chooseFilter(value: string) { setFilter(value); const snapshot = snapshots.current.get(activeSearch.current); if (snapshot) snapshot.filter = value; }
  const kinds = useMemo(() => [...new Set(resultData?.results.map((result) => result.kind) ?? [])], [resultData]);
  const results = resultData?.results.filter((result) => filter === "all" || filter === result.kind) ?? [];
  const canonicalAnswer = useMemo(() => hasRenderableAnswer(resultData?.query_execution), [resultData?.query_execution]);
  const highlightedEdges = useMemo(() => selectedId ? data?.graph.edges.filter((edge) => edge.source === selectedId || edge.target === selectedId).map((edge) => edge.id) : undefined, [data, selectedId]);
  const highlightedNodes = useMemo(() => selectedId ? [...new Set([selectedId, ...(data?.graph.edges.filter((edge) => edge.source === selectedId || edge.target === selectedId).flatMap((edge) => [edge.source, edge.target]) ?? [])])] : undefined, [data, selectedId]);
  function informationUrl(id?: string) { const url = new URL(window.location.href); if (id) url.searchParams.set("node", id); else url.searchParams.delete("node"); return `${url.pathname}${url.search}${url.hash}`; }
  function openInfo(id: string, panel: Panel = "overview", result?: ExploreResult, context?: string, expand = false) {
    const snapshot = snapshots.current.get(activeSearch.current);
    if (!snapshot) return;
    const previous = historyView();
    if (pageRef.current === "info" && isCurrentInfo(previous, activeSearch.current, id, context)) {
      // A graph click is a no-op, including while this node is still loading.
      // Explicit evidence/request actions may change the panel without refetching or moving it.
      if (!expand && openPanel !== panel) {
        setOpenPanel(panel);
        window.history.replaceState({ ...window.history.state, zebra: { ...previous, panel } }, "", informationUrl(id));
      }
      return;
    }
    if (pageRef.current === "results") snapshot.scroll = readingScroll.current?.scrollTop ?? snapshot.scroll;
    if (result) snapshot.extra.set(id, { result, context });
    const next = infoNavigation(previous, activeSearch.current, id, panel, context);
    window.history[next.method]({ ...window.history.state, zebra: next.view }, "", informationUrl(id));
    setPage("info"); pageRef.current = "info";
    void focusNode(id, true);
    setSelectedId(id); setSelectedExtra(result ?? null); setSelectedContext(context); setOpenPanel(panel);
    const target = expand || heroExpandedRef.current ? 0 : heroDistance.current;
    requestAnimationFrame(() => { readingScroll.current?.scrollTo({ top: target, behavior: "auto" }); });
  }
  function returnToResults() {
    if (historyView()?.returnsToResults) { window.history.back(); return; }
    const snapshot = snapshots.current.get(activeSearch.current);
    navigationRequest.current?.abort(); sectionRequest.current?.abort(); setNavigating(false);
    setPage("results"); pageRef.current = "results"; setOpenPanel(null); setSelectedId(null); setSelectedExtra(null); setSelectedContext(undefined); setNavigationDepth(0);
    if (snapshot) { setData(snapshot.data); setRootData(snapshot.data); graphHistory.current = [{ data: snapshot.data, selectedId: null }]; graphHistoryIndex.current = 0; requestAnimationFrame(() => readingScroll.current?.scrollTo({ top: snapshot.scroll, behavior: "auto" })); }
    window.history.replaceState({ ...window.history.state, zebra: { search: activeSearch.current, page: "results" } }, "", informationUrl());
  }
  function select(id: string, result?: ExploreResult) { openInfo(id, "overview", result, result ? conditionId : undefined); }
  async function focusNode(id: string, preservePanel = false) {
    if (!data || (id === selectedId && navigationDepth > 0)) return;
    navigationRequest.current?.abort(); sectionRequest.current?.abort(); setSectionLoading(false);
    const controller = new AbortController(); navigationRequest.current = controller;
    const requestedSearch = activeSearch.current;
    setSelectedId(id); if (!preservePanel) { setSelectedExtra(null); setSelectedContext(undefined); setOpenPanel(null); } setNavigating(true);
    try {
      const response = await explore("", { mode: communityView ? "community" : "knowledge", limit: 40, plan: { focus: [id], intent: "all", filters: {} }, signal: controller.signal });
      if (!acceptNavigationResponse(controller.signal, requestedSearch, activeSearch.current, id, response.graph.nodes)) { if (!controller.signal.aborted && requestedSearch === activeSearch.current) setNotice(copy.graphNoNeighbors); return; }
      const next = { ...response, graph: navigationGraph(response.graph, data.graph) };
      const visits = graphHistory.current.slice(0, graphHistoryIndex.current + 1);
      visits.push({ data: next, selectedId: id });
      if (visits.length > 20) visits.splice(1, visits.length - 20);
      graphHistory.current = visits; graphHistoryIndex.current = visits.length - 1;
      const snapshot = snapshots.current.get(activeSearch.current); if (snapshot) snapshot.visits = visits;
      setData(next); setNavigationDepth(visits.length - 1);
    } catch (e) { if (!controller.signal.aborted) setNotice(e instanceof Error ? e.message : copy.offline); }
    finally { if (!controller.signal.aborted) setNavigating(false); }
  }
  async function loadSections(requested: ConditionSection[]) {
    if (!detail || !conditionId) return;
    const missing = requested.filter((item) => !detail.loadedSections?.includes(item));
    if (!missing.length) return;
    sectionRequest.current?.abort(); const controller = new AbortController(); sectionRequest.current = controller;
    setSectionLoading(true);
    try {
      const response = await condition(conditionId, locale, controller.signal, missing);
      if (!controller.signal.aborted) setDetail((current) => current?.id === response.id ? { ...current, ...Object.fromEntries((response.loadedSections ?? missing).map((item) => [item, response[item]])), loadedSections: [...new Set([...(current.loadedSections ?? []), ...(response.loadedSections ?? [])])], unavailable: [...new Set([...current.unavailable.filter((item) => !response.loadedSections?.includes(item as ConditionSection)), ...response.unavailable])] } : current);
    } catch (e) { if (!controller.signal.aborted) setNotice(e instanceof Error ? e.message : copy.offline); }
    finally { if (!controller.signal.aborted) setSectionLoading(false); }
  }
  function loadSection(section: string) {
    const requested: ConditionSection[] = section === "related" || section === "people" ? ["related"] : section === "assets" ? ["jobs", "related"] : section === "questions" ? ["questions"] : ["therapy_programmes", "free_papers", "funding", "outcome_measures"].includes(section) ? ["jobs"] : [];
    return loadSections(requested);
  }
  async function share() { const url = new URL("/zebra", window.location.origin); const id = selectedId || data?.interpretation.entities[0]?.id; if (id) { url.searchParams.set("q", id); url.searchParams.set("node", id); } else if (communityView) url.pathname = "/zebra/community"; url.searchParams.set("lang", locale); try { await navigator.clipboard.writeText(url.toString()); setNotice(copy.copied); } catch { setNotice(url.toString()); } }
  async function save() {
    try { await saveItem({ kind: selected ? "node" : "search", title: selected?.label || query || copy.communityTitle, refs: selected ? [selected.id] : [], payload: { href: selected ? href(`/zebra?q=${encodeURIComponent(selected.id)}&node=${encodeURIComponent(selected.id)}`) : "/zebra", query: selected ? selected.id : actualQuery.current, displayQuery: query } }); setNotice(copy.savedItem); }
    catch (e) { setNotice(e instanceof Error ? e.message : copy.accountError); }
  }
  const edgeIds = data?.graph.edges.filter((edge) => edge.source === selectedId || edge.target === selectedId).map((edge) => edge.id) ?? [];
  const selectedConnection = [...(detail?.connections?.exact ?? []), ...(detail?.connections?.related ?? []), ...(detail?.related?.communities.flatMap((item) => [item.group, item.partner, ...(item.cards ?? [])]).filter((item) => !!item) ?? [])].find((item) => item?.id === selectedId);
  const resolvedNames = [...new Set(resultData?.interpretation.entities.map((entity) => entity.label).filter(Boolean) ?? [])];
  const resolvedTitle = resolvedNames.slice(0, 3).join(" \u00b7 ");
  const resultTitle = resolvedTitle && resolvedTitle.length <= 100 ? resolvedTitle : query.length <= 70 && query ? query : communityView ? copy.communityTitle : copy.results;
  const keywordRouting = (resultData?.query_execution?.routing ?? resultData?.interpretation.routing)?.route === "keyword";
  const bookmarks = selectedExtra && !resultData?.results.some((result) => result.id === selectedExtra.id) ? [selectedExtra, ...(resultData?.results ?? [])] : resultData?.results ?? [];
  return <div className={`z-shell ${workspace ? "z-shell-workspace" : ""}`}>
    <a href="#z-main" className="z-skip">{copy.skip}</a>
    <header className="z-header">
      <Link href={href("/zebra")} className="z-wordmark" aria-label={copy.app}><Brand name={copy.app} decorative optical="regular" /></Link>
      {workspace && <div className="z-header-search"><SearchBox initial={query} resetVersion={searchRevision} compact busy={busy} onSearch={(text, display) => void runSearch(text, display)} /></div>}
      <div className="z-header-nav">{workspace && <Link href={href("/zebra/community")} className={communityView ? "z-nav-link z-active" : "z-nav-link"}>{copy.community}</Link>}<LanguagePicker open={headerPopover === "language"} onToggle={() => setHeaderPopover(current => toggleHeaderPopover(current, "language"))} onClose={() => setHeaderPopover(current => dismissHeaderPopover(current, "language"))} /><NetworkDirectory open={headerPopover === "networks"} onToggle={() => setHeaderPopover(current => toggleHeaderPopover(current, "networks"))} onClose={closeNetworks} /><div ref={headerMenu} className="z-menu"><AccountControl buttonRef={menuButton} expanded={menu} controls="z-global-menu" onClick={() => setHeaderPopover(current => toggleHeaderPopover(current, "account"))} /><nav id="z-global-menu" aria-label={copy.menu} hidden={!menu}>
        {[{ href: "/zebra/saved", text: copy.saved }, { href: "/zebra/contribute", text: copy.contribute }, { href: "/zebra/account", text: copy.account }, { href: "/zebra/about", text: copy.about }, { href: "/zebra/privacy", text: copy.privacy }, { href: "/zebra/imprint", text: copy.imprint }].map((item) => <Link key={item.href} href={href(item.href)} onClick={() => setHeaderPopover(null)}><BrandText text={item.text} /></Link>)}
</nav></div></div>
    </header>
    <main id="z-main" className={workspace ? "z-workspace" : "z-main"}>
      {!workspace && !utility && <section className="z-home" aria-label={copy.welcome}><p className="z-welcome"><BrandText text={copy.welcome} /></p><SearchBox onSearch={(text, display) => void runSearch(text, display)} /><Link href={href("/zebra/community")} className="z-community-entry"><Icon name="graph" size={17} />{copy.communityLink}<Icon name="arrow" size={16} /></Link></section>}
      {utility && <Utility view={initialView as "contribute" | "saved" | "account" | "about" | "privacy" | "request-removal" | "imprint"} />}
      {workspace && <div className="z-research-workspace">
        {bookmarks.length > 1 && <nav className="z-result-bookmarks" aria-label={views.bookmarks} inert={drawerShown} aria-hidden={drawerShown || undefined}>
          <button type="button" className="z-bookmark-overview" aria-current={page === "results" ? "page" : undefined} onClick={returnToResults}>{copy.results}<span>{resultData?.results.length}</span></button>
          {bookmarks.map((result) => <button type="button" key={result.id} title={result.label} aria-label={`${kindLabel(result.kind)}: ${result.label}`} aria-current={page === "info" && selectedId === result.id ? "page" : undefined} onClick={() => openInfo(result.id, "overview", result, snapshots.current.get(activeSearch.current)?.extra.get(result.id)?.context)}><span>{kindLabel(result.kind)}</span><strong>{compactResultLabel(result.label, 72)}</strong></button>)}
        </nav>}
        <div ref={readingScroll} className="z-reading-scroll" data-page={page} inert={drawerShown} aria-hidden={drawerShown || undefined}>
          <section ref={heroElement} className="z-graph-hero" data-has-graph={Boolean(data?.graph.nodes.length)} data-expanded={graphExpanded} aria-label={copy.graphRegion}>
            <div className="z-hero-scene" inert={phone && drawerShown}>
              {data?.graph.nodes.length ? <Graph graph={data.graph} resultNodeIds={resultData?.results.map((result) => result.id)} selectedNodeId={selectedId} highlightedNodeIds={highlightedNodes} highlightedEdgeIds={highlightedEdges} onSelectNode={(id) => openInfo(id, "overview", undefined, undefined, true)} navigating={navigating} labels={graphLabels} mode={communityView ? "community" : "search"} /> : null}
            </div>
            {!!data?.graph.nodes.length && <>
              <div className="z-hero-controls"><span>{data.graph.nodes.length} {views.nodes}{" · "}{data.graph.edges.length} {views.edges}{(data.graph.community || data.execution.truncated) && <> · {views.limited}</>}</span><button type="button" aria-expanded={graphExpanded} onClick={() => revealGraph(!graphExpanded)}><Icon name={graphExpanded ? "arrow" : "graph"} size={16} style={graphExpanded ? { transform: "rotate(90deg)" } : undefined} />{graphExpanded ? views.collapse : views.expand}</button></div>
            </>}
          </section>
          <div className={`z-findings-sheet ${page === "info" ? "z-slide-info" : "z-slide-results"}`} inert={phone && drawerShown}>
            {page === "results" ? <section className="z-results z-results-page" aria-label={copy.results}>
              <div className="z-results-heading"><div><p className="z-eyebrow">{communityView ? copy.community : copy.results}</p><h1>{resultTitle}</h1>{query && query !== resultTitle && <details className="z-original-query"><summary>{query.length > 110 ? `${query.slice(0, 110)}...` : query}</summary><p>{query}</p></details>}</div><button type="button" className="z-query-info" title={views.query} aria-label={views.query} onClick={openQuery}>{queryInfoSymbol}</button></div>
              {keywordRouting && <p className="z-eyebrow" role="status"><strong>{copy.exploreError.keyword}</strong>{" · "}{copy.exploreError.keywordScope}</p>}
              {resultData && <ContactFilters value={contactRole(resultData, communityView)} disabled={busy || !contactPlan(resultData, "all")} onChange={chooseRole} />}
              {kinds.length > 1 && <div className="z-filters" aria-label={copy.results}><button aria-pressed={filter === "all"} onClick={() => chooseFilter("all")}>{copy.all}<span>{resultData?.results.length}</span></button>{kinds.map((kind) => <button key={kind} aria-pressed={filter === kind} onClick={() => chooseFilter(kind)}>{kindLabel(kind)}<span>{resultData?.results.filter((item) => item.kind === kind).length}</span></button>)}</div>}
              <div className="z-results-content" aria-busy={busy}>
                {(communityView || results.some((result) => result.kind === "person" || result.kind === "organisation")) && <PrivacyNotice />}
                {busy && <div className="z-result-status" role="status"><ZebraLoader /><p>{copy.loading}</p></div>}
                {!busy && error && <ExploreError error={errorCause || error} onRetry={() => void runSearch(actualQuery.current || query || "research community", query)} onIndexedLookup={isShortIndexedName(query) ? () => void indexedLookup() : undefined} matches={indexedMatches} correctedQuery={indexedCorrection} indexedBusy={indexedBusy} onSelectMatch={match => void runSearch("", match.label, true, undefined, { focus: [match.id], intent: "all", filters: {} })} />}
                {!busy && !error && resultData?.query_execution && <QueryAnswer execution={resultData.query_execution} graph={resultData.graph} onSelectNode={(id) => openInfo(id, "overview", undefined, undefined, true)} />}
                {!busy && !error && !results.length && !canonicalAnswer && <div className="z-result-status"><h2>{copy.empty}</h2><p>{copy.emptyHint}</p></div>}
                {!busy && !error && resultData && <ResultOverview data={resultData} query={query} busy={busy} results={results} selectedId={selectedId} onFocus={(id) => openInfo(id, "overview", undefined, undefined, true)} onDetails={(id, result, context) => openInfo(id, "overview", result, context)} onRequest={(id, result, context) => openInfo(id, "request", result, context)} onEvidence={(id, result, context) => openInfo(id, "evidence", result, context)} />}
                {!busy && !error && resultData && <ProgrammeRoutes />}
              </div>
              <div className="z-results-footer"><button onClick={() => void share()}><Icon name="share" size={15} />{copy.share}</button><button onClick={() => void save()}><Icon name="bookmark" size={15} />{copy.save}</button><button onClick={() => window.print()}>{copy.print}</button><Link href={contributionHref({}, locale)} className="z-help-improve">{copy.contribute}</Link></div>
            </section> : <section className="z-information-page" aria-label={selected?.label ?? copy.details}>
              <div className="z-information-nav"><button className="z-text-button" onClick={returnToResults}><Icon name="back" size={16} />{views.back}</button><button type="button" className="z-query-info" title={views.query} aria-label={views.query} onClick={openQuery}>{queryInfoSymbol}</button></div>
              <aside ref={openPanel === "plan" ? undefined : panelElement} className="z-detail z-info-page">
                <div className="z-detail-top"><div className="z-detail-actions"><button className="z-icon-button" aria-label={copy.share} title={copy.share} onClick={() => void share()}><Icon name="share" size={17} /></button><button className="z-icon-button" aria-label={copy.save} title={copy.save} onClick={() => void save()}><Icon name="bookmark" size={17} /></button></div></div>
                <div className="z-detail-heading"><p className="z-eyebrow">{selected ? kindLabel(selected.kind) : copy.connection}</p><h2>{selected?.label}</h2><p>{selected?.reason}</p></div>
                <div className="z-detail-tabs"><button aria-pressed={openPanel === "overview" || !openPanel} onClick={() => setOpenPanel("overview")}>{copy.summary}</button><button aria-pressed={openPanel === "evidence"} onClick={() => setOpenPanel("evidence")}>{copy.sources}</button><button aria-pressed={openPanel === "request"} onClick={() => setOpenPanel("request")}>{copy.request}</button></div>
                <div className="z-detail-body">{(selected?.kind === "person" || selected?.kind === "organisation") && <PrivacyNotice />}{openPanel === "evidence" ? <Sources evidence={selected?.evidence ?? []} entityId={selected?.id} edgeIds={selectedConnection?.edge_ids ?? edgeIds} sources={selectedConnection?.sources ?? (selected?.kind === "disease" ? detail?.summary?.sources : undefined)} edges={selectedConnection?.edges} validator={selectedConnection?.validator} llm={selectedConnection?.llm} conditionId={conditionId} /> : openPanel === "request" ? <>
                  {detailBusy && <div className="z-inline-loading" role="status"><ZebraLoader />{copy.loading}</div>}
                  <ResearchBrief query={query} selected={selectedConnection ?? selected} detail={detail} onLoadSections={detailBusy ? undefined : loadSections} />
                </> : <ConditionPanel key={selected?.id} detail={detail} selected={selected} busy={detailBusy} sectionLoading={sectionLoading} onLoadSection={(section) => void loadSection(section)} onSelect={select} onEvidence={() => setOpenPanel("evidence")} onRequest={() => setOpenPanel("request")} />}<Link className="z-help-improve" href={contributionHref({ subject: selected ? { id: selected.id, label: selected.label } : undefined, kind: "correction" }, locale)}>{copy.contribute}</Link></div>
              </aside>
            </section>}
          </div>
        </div>
        {openPanel === "plan" && <section ref={panelElement} className={queryStyles.page} aria-label={views.query}>
          {error && <ExploreError error={errorCause || error} compact />}
          <QueryPlan data={resultData} query={query} busy={busy} onClose={closeQuery} onRunSparql={(sparql, settings) => void runSparql(sparql, settings)} />
        </section>}
      </div>}

    </main>
    {notice && <div className="z-toast" role="status"><span>{notice}</span><button className="z-icon-button" aria-label={copy.dismiss} onClick={() => setNotice("")}><Icon name="close" size={16} /></button></div>}
  </div>;
}
