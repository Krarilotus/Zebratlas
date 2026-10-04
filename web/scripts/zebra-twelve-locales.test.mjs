import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { languagePrelude } from './zebra-language-fixture.mjs';
import { formatMessage, messageVars } from '../lib/i18n/format.ts';

const runtime=await import('data:text/javascript;base64,'+Buffer.from(languagePrelude(true)).toString('base64'));
const locales=runtime.ZEBRA_LOCALES;
const flatten=(value,p='') => typeof value==='string' ? {[p]:value}
 : Object.fromEntries(Object.entries(value).flatMap(([k,v])=>Object.entries(flatten(v,p?p+'.'+k:k))));
const read=(name)=>readFileSync(new URL('../'+name,import.meta.url),'utf8');

test('all twelve catalogs preserve ICU variables, complete frames and localized enum labels',()=>{
 assert.equal(locales.length,12);
 const source=flatten(runtime.getZebraCatalog('en'));
 for(const locale of locales){
  const catalog=runtime.getZebraCatalog(locale); const target=flatten(catalog);
  assert.deepEqual(Object.keys(target).sort(),Object.keys(source).sort());
  for(const [key,value] of Object.entries(target))assert.deepEqual(messageVars(value),messageVars(source[key]),locale+' '+key);
  assert.deepEqual(Object.keys(catalog.copy.savedKinds),['condition','connection_card','connection_map','message_draft','node','search']);
  for(const count of [0,1,2,12]){
   const words=formatMessage(catalog.answerWords.rowCount,{shown:count,total:12},locale);
   assert.ok(!words.includes('{shown}')&&!words.includes('{total}'),locale);
  }
  assert.deepEqual(messageVars(catalog.proofWords.conclusion),['object','subject']);
 }
});

test('locale aliases and links preserve all supported language choices',()=>{
 for(const locale of locales){
  assert.equal(runtime.resolveZebraLocale(locale.toUpperCase()),locale);
  assert.equal(new URL(runtime.zebraHref('/zebra/about?node=STXBP1',locale),'https://fixture.invalid').searchParams.get('lang'),locale);
 }
 for(const [input,expected] of [['fr-FR','fr'],['pt_BR','pt'],['zh-CN','zh-Hans'],['ja-JP','ja'],['xx','en']])assert.equal(runtime.resolveZebraLocale(input),expected);
});

test('local validation and account errors keep original diagnostics and stable meaning in twelve languages',()=>{
 for(const locale of locales){
  const request=new Request('https://fixture.invalid',{headers:{'x-zebra-locale':locale}});
  const catalog=runtime.getZebraCatalog(locale);
  const invalid=runtime.localizedError(request,'validField:identifier');
  assert.equal(invalid.detail_lang,locale);
  assert.ok(invalid.detail.includes(catalog.apiFields.identifier));
  assert.equal(invalid.original_detail,'Please provide a valid identifier.');
  const upstream=runtime.localizedError(request,'token expired','invalid_token');
  assert.equal(upstream.detail,catalog.copy.accountLinkInvalid);
  assert.equal(upstream.original_detail,'token expired');
  assert.equal(runtime.localizedError(request,'private diagnostic').detail,catalog.apiErrors.retry);
 }
});

test('first-link client requests keep the visible language, private body and localized failures',async()=>{
 const source=stripTypeScriptTypes(read('lib/zebra/client.ts').replace(/^import .*;\r?$/gm,''),{mode:'transform'});
 const prelude=languagePrelude()+`\nconst accountChanged=()=>{}; const getAccountRevision=()=>0; const isAccountIdentity=()=>true; const rememberAccountIdentity=()=>{}; const lookupRequest=()=>{}; const documentRequest=()=>{};`;
 const client=await import('data:text/javascript;base64,'+Buffer.from(prelude+'\n'+source).toString('base64'));
 const originalFetch=globalThis.fetch; const originalDocument=globalThis.document;
 try{
  for(const locale of locales){
   globalThis.document={documentElement:{lang:locale}};
   let seen;
   globalThis.fetch=async(url,init)=>{seen={url,init};return new Response(JSON.stringify({code:'forbidden',detail:'not allowed'}),{status:403});};
   await assert.rejects(client.explore('PRIVATE PATIENT WORDS'),error=>error.status===403&&error.code==='forbidden'&&error.message===runtime.getZebraCatalog(locale).apiErrors.forbidden);
   assert.equal(new URL(seen.url,'https://fixture.invalid').searchParams.get('lang'),locale);
   assert.ok(!seen.url.includes('PRIVATE')); assert.ok(seen.init.body.includes('PRIVATE PATIENT WORDS'));
   globalThis.fetch=async()=>{throw new Error('raw network diagnostic');};
   await assert.rejects(client.account(),error=>error.status===0&&error.message===runtime.getZebraCatalog(locale).apiErrors.unavailable);
  }
 }finally{globalThis.fetch=originalFetch;globalThis.document=originalDocument;}
});
