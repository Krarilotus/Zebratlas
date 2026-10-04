import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {runInNewContext} from 'node:vm';
import ts from 'typescript';
const require=createRequire(import.meta.url);
function load(path,imports,globals={}) {
 const exports={};
 const code=ts.transpileModule(readFileSync(new URL(path,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 runInNewContext(code,{exports,require:id=>imports[id]??require(id),Request,Response,URL,URLSearchParams,Headers,TextEncoder,Buffer,AbortController,...globals}); return exports;
}
class HttpError extends Error {constructor(status,message){super(message);this.status=status;}}
const upstreamCalls=[];
const route=load('../app/zebra/api/[operation]/route.ts',{
 '@/lib/zebra/error-copy':{localizedError:(request,detail,code)=>({detail,code})},'@/lib/adapt':{},'@/components/account/types':{SAVED_KINDS:[]},'@/lib/zebra/types':{CONDITION_SECTIONS:[],EXPLORE_INTENTS:[]},'@/lib/zebra/normalize':{sourceDates:value=>value},
 '@/lib/zebra/server':{HttpError,bodyObject:request=>request.json(),boundedString:(value,name,max=200,required=true)=>{if(typeof value!=='string'||value.length>max||(required&&!value.trim()))throw new HttpError(400,name);return value;},jsonResponse:(body,status=200)=>Response.json(body,{status}),upstreamJson:async(request,path,init)=>{upstreamCalls.push({path,init});return {engine:'nrese',receipt:'actual'};},enc:encodeURIComponent},
});
const get=(search)=>route.GET(new Request('https://zebratlas.example/zebra/api/query-suggestions?'+search),{params:Promise.resolve({operation:'query-suggestions'})});
assert.equal((await get('node=HGNC%3A1&limit=5&offset=0')).status,200);
assert.equal(upstreamCalls.at(-1).path,'/api/query-graph/suggestions?node=HGNC%3A1&offset=0&limit=5');
let before=upstreamCalls.length;
for(const search of ['node=HGNC%3A1&limit=50','node=HGNC%3A1&offset=-1','node=HGNC%3A1&direction=sideways','node=HGNC%3A1&endpoint=https%3A%2F%2Fforeign.example'])assert.equal((await get(search)).status,400);
assert.equal(upstreamCalls.length,before,'Invalid/unbounded suggestion parameters never reach upstream');
const run=body=>route.POST(new Request('https://zebratlas.example/zebra/api/sparql',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)}),{params:Promise.resolve({operation:'sparql'})});
const focus=Array.from({length:16},(_,index)=>`HGNC:${index}`);
assert.equal((await run({sparql:'SELECT * WHERE {}',query:'reviewed-file.txt',focus,semantic_focus:['MONDO:actual'],limit:160,reasoning:false})).status,200);
assert.equal(upstreamCalls.at(-1).path,'/api/explore/sparql');
assert.equal(upstreamCalls.at(-1).init.body.reasoning,false);
assert.equal(upstreamCalls.at(-1).init.body.limit,160);
assert.equal(upstreamCalls.at(-1).init.body.query,'reviewed-file.txt');
assert.deepEqual(Array.from(upstreamCalls.at(-1).init.body.focus),focus);
assert.deepEqual(Array.from(upstreamCalls.at(-1).init.body.semantic_focus),['MONDO:actual'],'Semantic input context is distinct from query execution seeds');
before=upstreamCalls.length;
assert.equal((await run({sparql:'SELECT * WHERE {}',limit:161})).status,400);
assert.equal((await run({sparql:'é'.repeat(9000)})).status,400);
assert.equal((await run({sparql:'SELECT * WHERE {}',focus:[...focus,'HGNC:overflow']})).status,400);
for(const semantic_focus of [[...focus,'HGNC:overflow'],['HGNC:1','HGNC:1'],[''],['x'.repeat(257)]]) assert.equal((await run({sparql:'SELECT * WHERE {}',semantic_focus,limit:20,reasoning:false})).status,400);
assert.equal((await run({sparql:'SELECT * WHERE {}',limit:20})).status,400,'Missing inference cannot silently become true');
assert.equal((await run({sparql:'SELECT * WHERE {}',reasoning:false})).status,400,'Missing cap cannot silently become 100');
assert.equal((await run({sparql:'SELECT * WHERE {}',limit:20,reasoning:false,linked:[{id:'HGNC:1',label:'Untrusted label'}]})).status,400,'Display labels never enter the backend contract');
assert.equal(upstreamCalls.length,before);
const requests=[];
const originalResponse={query:'reviewed-file.txt',execution:{engine:'nrese',queries:[{sparql:'SELECT * WHERE {}',reasoning:false,row_cap:20}]},query_execution:{semantic_focus:['MONDO:actual'],answer:{results:[{query:'SELECT * WHERE {}',data:{head:{vars:['count']},results:{bindings:[{count:{type:'literal',value:'0'}}]}}}]}},graph:{nodes:[],edges:[]},results:[]};
const client=load('../lib/zebra/client.ts',{'./locale':{resolveZebraLocale:()=> 'en',getZebraCatalog:()=>({apiErrors:{unavailable:'Unavailable',retry:'Retry',forbidden:'Forbidden'}})},'./search-privacy':load('../lib/zebra/search-privacy.ts',{}),'./account-events':{accountChanged(){},getAccountRevision(){return 0;},rememberAccountIdentity(){},isAccountIdentity(){return true;}}},{fetch:async(url,init)=>{requests.push({url,init});return {ok:true,status:200,json:async()=>originalResponse};}});
const settings={limit:20,reasoning:false,focus:['HGNC:1'],semantic_focus:['MONDO:actual'],linked:[{id:'HGNC:1',label:'Actual gene'}]};
const returned=await client.runSparqlQuery('SELECT * WHERE {}','reviewed-file.txt',undefined,settings);
const sent=JSON.parse(requests[0].init.body);
assert.equal(sent.reasoning,false);assert.equal(sent.limit,20);assert.equal(sent.query,'reviewed-file.txt');
assert.deepEqual(sent.focus,['HGNC:1']);assert.deepEqual(sent.semantic_focus,['MONDO:actual']);assert.equal(sent.linked,undefined);
assert.equal(returned.execution.rerun_context,settings,'Known labels and request focus are UI tracking, never fabricated backend receipts');
assert.equal(originalResponse.execution.rerun_context,undefined,'Original backend execution remains immutable');
assert.deepEqual(returned.results,originalResponse.results);
assert.equal(returned.query_execution,originalResponse.query_execution,'Replacement answer preserves canonical rows, including zero-count answers');
assert.deepEqual(returned.query_execution.semantic_focus,['MONDO:actual']);
assert.equal(returned.graph,originalResponse.graph);
const validRequests=requests.length;
await assert.rejects(client.runSparqlQuery('SELECT * WHERE {}','reviewed-file.txt',undefined,undefined),/execution settings/);
await assert.rejects(client.runSparqlQuery('SELECT * WHERE {}','reviewed-file.txt',undefined,{...settings,reasoning:undefined}),/execution settings/);
assert.equal(requests.length,validRequests,'Unknown settings are rejected before any API request');
console.log('PASS query proxy/client: bounded five-item index, guarded rerun settings, UTF-8 limits, separate semantic focus, public caption only, no display-label forwarding, immutable original execution and no model route.');
