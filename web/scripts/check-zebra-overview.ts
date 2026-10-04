import assert from "node:assert/strict";
import { clearOverviewMemo, compactOverviewText, fetchOverview, loadOverview, normalizeOverview, overviewHref, syncOverviewAccountRevision, type OverviewEntity } from "../lib/zebra/overview.ts";

const gene = { id: "HGNC:11444", label: "STXBP1", kind: "gene" as const };
const disease = { id: "MONDO:0012812", label: "Developmental and epileptic encephalopathy, 4", kind: "disease" as const };
const source = (entity: OverviewEntity = gene, extra: Record<string, unknown> = {}) => ({ entity, text: "Syntaxin binding protein 1.", mode: "source", language: "en", requested_language: "en", evidence: [{ source: "HGNC", url: "https://www.genenames.org/data/gene-symbol-report/#!/hgnc_id/HGNC:11444", record: gene.id }], source_ids: ["hgnc"], can_enhance: false, ...extra });
const ai = (extra: Record<string, unknown> = {}) => source(gene, { mode: "ai", model: { connection: "fixture", id: "model-fixture" }, text: "A bounded explanation. A second sourced sentence.", ...extra });
assert.equal(normalizeOverview(source(disease), gene, "en"), null, "Never turn a linked disease definition into a gene explanation");
assert.equal(normalizeOverview(source(), gene, "de"), null, "A previous interface-language request cannot overwrite the current scope");
assert.equal(normalizeOverview(source(gene, { requested_language: "de" }), gene, "de")?.language, "en", "Original English source retains its actual language");
assert.equal(normalizeOverview(ai({ requested_language: "de" }), gene, "de"), null, "AI output must use the requested interface language");
assert.equal(normalizeOverview(ai({ model: null }), gene, "en"), null, "AI must disclose its actual model");
assert.equal(normalizeOverview(source(gene, { evidence: [], source_ids: [] }), gene, "en"), null, "Unsupported source text is hidden");
assert.equal(normalizeOverview(ai(), gene, "en")?.mode, "ai");
assert.equal(normalizeOverview(source({...gene,official_name:"syntaxin binding protein 1"}),gene,"en")?.entity.official_name,"syntaxin binding protein 1","Localised factual identity retains the actual official name");
assert.equal(overviewHref("javascript:alert(1)"), undefined);
assert.equal(overviewHref("https://name:password@example.org"), undefined);
assert.equal(overviewHref("cache/clinical.json"), undefined);
assert.equal(overviewHref("https://example.org/source"), "https://example.org/source");
assert.equal(compactOverviewText("First sentence. Second sentence. Third sentence.", "en"), "First sentence. Second sentence.");
assert.ok(compactOverviewText(Array.from({length:100},()=>"word").join(" "), "en").split(/\s+/u).length <= 90);
assert.equal(compactOverviewText("第一句话。第二句话。第三句话。", "zh-Hans"), "第一句话。第二句话。");

const controller = new AbortController();
let calls: boolean[] = [];
let updates: { mode: string; pending: boolean }[] = [];
let finishAi!: (value: unknown) => void;
const deferredAi = new Promise(resolve => { finishAi = resolve; });
const pending = loadOverview(gene, "en", controller.signal, async (_entity,_lang, enhance) => { calls.push(enhance); return enhance ? deferredAi : source(gene, {can_enhance:true}); }, (value,pending) => updates.push({mode:value.mode,pending}));
await Promise.resolve(); await Promise.resolve();
assert.deepEqual(updates, [{mode:"source",pending:true}], "Source is visible while optional enhancement is still pending");
assert.deepEqual(calls,[false,true]);
finishAi(ai()); await pending;
assert.deepEqual(updates,[{mode:"source",pending:true},{mode:"ai",pending:false}]);

calls=[]; updates=[];
await loadOverview(gene,"en",new AbortController().signal,async (_entity,_lang,enhance)=>{ calls.push(enhance); return source(); },(value,pending)=>updates.push({mode:value.mode,pending}));
assert.deepEqual(calls,[false],"No automatic AI request unless source response explicitly confirms eligibility");
assert.deepEqual(updates,[{mode:"source",pending:false}]);

updates=[];
await loadOverview(gene,"en",new AbortController().signal,async (_entity,_lang,enhance)=>{ if(enhance)throw new Error("unavailable"); return source(gene,{can_enhance:true}); },(value,pending)=>updates.push({mode:value.mode,pending}));
assert.deepEqual(updates,[{mode:"source",pending:true},{mode:"source",pending:false}],"Model failure preserves factual text");
updates=[];
await loadOverview(gene,"de",new AbortController().signal,async (_entity,_lang,enhance)=>enhance?ai({requested_language:"de"}):source(gene,{requested_language:"de",can_enhance:true}),(value,pending)=>updates.push({mode:value.mode,pending}));
assert.equal(updates.at(-1)?.mode,"source","Wrong-language AI preserves the labeled original source");

const stale = new AbortController();
let finishSource!: (value:unknown)=>void;
const lateSource=new Promise(resolve=>{finishSource=resolve;});
let staleUpdates=0;
const staleRun=loadOverview(gene,"en",stale.signal,async ()=>lateSource,()=>{staleUpdates++;});
stale.abort(); finishSource(source(gene,{can_enhance:true})); await staleRun;
assert.equal(staleUpdates,0,"A source arriving after navigation/language cancellation cannot update UI or initiate AI");
const lateAiController=new AbortController();
let lateAiResolve!: (value:unknown)=>void;
const lateAi=new Promise(resolve=>{lateAiResolve=resolve;});
updates=[];
const lateAiRun=loadOverview(gene,"en",lateAiController.signal,async(_entity,_lang,enhance)=>enhance?lateAi:source(gene,{can_enhance:true}),(value,pending)=>updates.push({mode:value.mode,pending}));
await Promise.resolve();await Promise.resolve();lateAiController.abort();lateAiResolve(ai());await lateAiRun;
assert.deepEqual(updates,[{mode:"source",pending:true}],"An AI reply after navigation/language cancellation cannot overwrite the current overview");
const preCanceled=new AbortController();preCanceled.abort();
await loadOverview(gene,"en",preCanceled.signal,async()=>{throw new Error("must not request");},()=>assert.fail("Canceled update"));

const originalFetch=globalThis.fetch;
let fetched: {input: unknown; init?:RequestInit} | undefined;
try {
 globalThis.fetch=(async(input:unknown,init?:RequestInit)=>{fetched={input,init};return new Response(JSON.stringify(source()),{status:200,headers:{"content-type":"application/json"}});}) as typeof fetch;
 const signal=new AbortController().signal;
 await fetchOverview(gene,"en",false,signal);
 assert.equal(fetched?.input,"/zebra/api/overview");
 assert.deepEqual(JSON.parse(String(fetched?.init?.body)),{id:gene.id,lang:"en",enhance:false});
 assert.equal(fetched?.init?.signal,signal);assert.equal(fetched?.init?.credentials,"same-origin");
} finally {globalThis.fetch=originalFetch;}

// Each remount reverifies source. Successful AI reuse is exact-source/model/account bound.
clearOverviewMemo();
let accountRevision=7;
let clock=1000;
let sourceChange="first";
let modelChange="model-fixture";
let aiCalls=0;
let sourceCalls=0;
let permitted=true;
let modelFails=false;
const memoLoader = async (_entity:OverviewEntity,lang:string,enhance:boolean) => {
 const model={connection:"fixture",id:modelChange};
 if(enhance){aiCalls++;if(modelFails)throw new Error("unavailable");return ai({entity:_entity,requested_language:lang,language:lang,model});}
 sourceCalls++;return source(_entity,{requested_language:lang,can_enhance:permitted,model,facts:[{id:"fact",text:sourceChange}],evidence:[{url:"https://example.org/source",record:"row1",sha256:sourceChange}]});
};
const runMemo=(lang="en",entity:OverviewEntity=gene)=>loadOverview(entity,lang,new AbortController().signal,memoLoader,()=>{},undefined,{accountRevision:()=>accountRevision,now:()=>clock});
await runMemo();await runMemo();assert.equal(aiCalls,1,"Successful remount uses one AI call");assert.equal(sourceCalls,2,"Every remount reverifies source/eligibility");
sourceChange="second";await runMemo();assert.equal(aiCalls,2,"Changed source facts/evidence invalidate memo");
modelChange="new-model";await runMemo();assert.equal(aiCalls,3,"Changed eligible model invalidates memo");
await runMemo("de");assert.equal(aiCalls,4,"Changed language invalidates memo");
accountRevision++;syncOverviewAccountRevision(accountRevision);await runMemo("de");assert.equal(aiCalls,5,"Changed account revision invalidates memo");
clock+=10*60*1000+1;await runMemo("de");assert.equal(aiCalls,6,"Ten-minute expiry invalidates memo");
permitted=false;await runMemo("de");assert.equal(aiCalls,6,"Current ineligibility prevents reuse and generation");permitted=true;
clearOverviewMemo();modelFails=true;await runMemo();await runMemo();assert.equal(aiCalls,8,"Failed responses are never cached");modelFails=false;
clearOverviewMemo();
const beforeEviction=aiCalls;
for(let n=0;n<25;n++)await runMemo("en",{...gene,id:`HGNC:fixture-${n}`});
await runMemo("en",{...gene,id:"HGNC:fixture-0"});assert.equal(aiCalls,beforeEviction+26,"At most24 successful entries are retained");
clearOverviewMemo();
console.log("PASS: compact sourced overview, exact identity/language, source-first eligibility, cancellation, truthful fallback and safe transport");
