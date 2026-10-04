import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import sharp from "sharp";
import ts from "typescript";
import { fileURLToPath } from "node:url";
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),"..");
const source=fs.readFileSync(path.join(root,"components/zebra/Brand.tsx"),"utf8");
const css=fs.readFileSync(path.join(root,"components/zebra/Brand.module.css"),"utf8");
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX}}).outputText;
const geometrySource=fs.readFileSync(path.join(root,"lib/zebra/brand-geometry.ts"),"utf8");
const geometryJs=ts.transpileModule(geometrySource,{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const geometry=new Function("exports",geometryJs+";return exports;")({});
const grids=new Function("exports","require",compiled+";return exports.ZEBRA_GRIDS;")({},name=>name==="@/lib/zebra/brand-geometry"?geometry:{});
assert(!css.includes("1cap")&&!css.includes("em; height"),"No fractional font-unit scaling");
assert(css.includes("align-items:baseline"));
const panels=[],samples=[];
function checkDiagonal(rectangles,grid,dpr=1){
  const extent=grid.height-grid.stroke,travel=grid.width-grid.middleWidth;
  for(const row of rectangles.slice(1,-1)){
    const x=row.x/dpr,y=row.y/dpr;
    const ideal=travel*(extent-y)/extent;
    const expected=y===extent/2?travel/2:y<extent/2?Math.round(ideal):travel-Math.round(travel*y/extent);
    assert.equal(x,expected,`${grid.name}: horizontal position derives from its actual vertical level`);
    assert(Math.abs(x-ideal)<=0.5+Number.EPSILON*32,`${grid.name}: native diagonal error is at most half a pixel`);
  }
}
assert(!geometrySource.includes("stepX"),"Horizontal placement has no independent step parameter");
assert.deepEqual(geometry.markRectangles(geometry.NATIVE_MARKS.find(grid=>grid.name==="caption")).slice(1,-1).map(row=>row.x),[5,2]);
assert.deepEqual(geometry.markRectangles(geometry.NATIVE_MARKS.find(grid=>grid.name==="body")).slice(1,-1).map(row=>row.x),[4,2]);
assert.deepEqual(geometry.markRectangles(geometry.NATIVE_MARKS.find(grid=>grid.name==="header")).slice(1,-1).map(row=>row.x),[8,5,2]);
for(const [theme,ink,paper] of [["light","#17675f","#f8fafb"],["dark","#e1f1eb","#182b36"]]) for(const [index,grid] of grids.entries()) for(const optical of ["regular","compact"]) for(const dpr of [1,2,3]) {
  const width=grid.width*dpr,height=grid.height*dpr;
  const svg=`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${grid.width} ${grid.height}"><path fill="${ink}" d="${grid[optical]}"/></svg>`;
  const pixels=await sharp(Buffer.from(svg)).ensureAlpha().raw().toBuffer();
  const active=(x,y)=>pixels[(y*width+x)*4+3]===255;
  for(let i=3;i<pixels.length;i+=4) assert(pixels[i]===0||pixels[i]===255,`No partial alpha / blurred edges ${grid.name}/${optical}/${dpr}`);
  assert(Array.from({length:width},(_,x)=>active(x,0)&&active(x,height-1)).every(Boolean),"Full-width top/bottom bars");
  const seen=new Set(),rectangles=[];let components=0;
  for(let y=0;y<height;y++)for(let x=0;x<width;x++){
    if(!active(x,y)||seen.has(`${x},${y}`))continue;
    components++;const queue=[[x,y]];seen.add(`${x},${y}`);
    for(let i=0;i<queue.length;i++){const [px,py]=queue[i];for(const [dx,dy] of [[0,1],[1,0],[0,-1],[-1,0]]){const a=px+dx,b=py+dy,key=`${a},${b}`;if(a>=0&&b>=0&&a<width&&b<height&&active(a,b)&&!seen.has(key)){seen.add(key);queue.push([a,b]);}}}
    const xs=queue.map(p=>p[0]),ys=queue.map(p=>p[1]);
    const box={x:Math.min(...xs),y:Math.min(...ys),width:Math.max(...xs)-Math.min(...xs)+1,height:Math.max(...ys)-Math.min(...ys)+1};
    assert.equal(queue.length,box.width*box.height,"Each stroke is a pure filled rectangle");
    rectangles.push(box);
  }
  assert.equal(components,grid.name==="tiny"?3:["body","caption"].includes(grid.name)?4:5,"Reference stroke levels remain disconnected");
  assert.equal(new Set(rectangles.map(r=>r.height)).size,1,"Stroke thickness is uniform within each native grid");
  rectangles.sort((a,b)=>a.y-b.y);
  checkDiagonal(rectangles,grid,dpr);
  const middle=rectangles.slice(1,-1);
  assert.equal(new Set(middle.map(r=>r.width)).size,1,"Intermediate rectangles have equal width");
  if(middle.length===3) assert.equal(middle[1].y-middle[0].y,middle[2].y-middle[1].y,"Vertical intermediate steps are equal");
  for(let y=0;y<height;y++)for(let x=0;x<width;x++)assert.equal(active(x,y),active(width-1-x,height-1-y),"The complete mark is 180-degree rotation symmetric");
  if(dpr===1){
    const native=await sharp(Buffer.from(svg)).flatten({background:paper}).png().toBuffer();
    const preview=await sharp(native).resize(grid.width*5,grid.height*5,{kernel:"nearest"}).png().toBuffer();
    const top=12+(theme==="light"?0:2)*160+(optical==="regular"?0:1)*160;
    panels.push({input:preview,left:12+index*140,top});
    panels.push({input:native,left:12+index*140,top:top+130});
  }
  samples.push({grid:grid.name,optical,theme,dpr,components,crisp:true});
}
const faviconPaths=Object.fromEntries(geometry.FAVICON_MARKS.map(grid=>[grid.width,geometry.markPath(grid)]));
if(process.argv.includes("--write-icons")){
  fs.mkdirSync(path.join(root,"public/zebra"),{recursive:true});
  for(const [size,glyph]of Object.entries(faviconPaths))for(const [suffix,ink]of [["","#17675f"],["-dark","#e1f1eb"]]){
    await sharp(Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}"><path fill="${ink}" d="${glyph}"/></svg>`)).png().toFile(path.join(root,`public/zebra/mark-${size}${suffix}.png`));
  }
  fs.writeFileSync(path.join(root,"public/zebra/mark.svg"),`<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32" shape-rendering="crispEdges"><style>path{fill:#17675f}@media(prefers-color-scheme:dark){path{fill:#e1f1eb}}</style><path d="${faviconPaths[32]}"/></svg>\n`);
}
for(const size of [16,32,48])for(const suffix of ["","-dark"]){
  const filename=path.join(root,`public/zebra/mark-${size}${suffix}.png`);
  const meta=await sharp(filename).metadata();assert.equal(meta.width,size);assert.equal(meta.height,size);
  const raw=await sharp(filename).ensureAlpha().raw().toBuffer();for(let i=3;i<raw.length;i+=4)assert(raw[i]===0||raw[i]===255);
  const active=(x,y)=>raw[(y*size+x)*4+3]===255;
  const bars=[];
  for(let y=0;y<size;y++) {
    const xs=Array.from({length:size},(_,x)=>x).filter(x=>active(x,y));
    if(!xs.length) continue;
    assert.equal(xs.length,xs.at(-1)-xs[0]+1,"Favicon rows are filled horizontal rectangles");
    const previous=bars.at(-1);
    if(previous&&previous.y+previous.height===y&&previous.x===xs[0]&&previous.width===xs.length)previous.height++;
    else bars.push({x:xs[0],y,width:xs.length,height:1});
  }
  assert.equal(bars.length,5,"Every native favicon contains exactly five rectangular levels");
  assert.equal(new Set(bars.map(bar=>bar.height)).size,1,"Favicon stroke height is uniform");
  assert.equal(new Set(bars.slice(1,-1).map(bar=>bar.width)).size,1,"Favicon middle widths are equal");
  checkDiagonal(bars,geometry.FAVICON_MARKS.find(grid=>grid.width===size));
  assert.equal(bars[2].y-bars[1].y,bars[3].y-bars[2].y,"Favicon vertical steps are equal");
  for(let y=0;y<size;y++)for(let x=0;x<size;x++)assert.equal(active(x,y),active(size-1-x,size-1-y),"Native favicon is rotation symmetric");
}
const output=process.argv.find(value=>value.endsWith(".png"));
if(output)await sharp({create:{width:860,height:650,channels:4,background:"#ced9dc"}}).composite(panels).png().toFile(output);
console.log(JSON.stringify({passed:true,nativeGrids:grids.map(({name,width,height})=>({name,width,height})),rasterChecks:samples.length,faviconSizes:[16,32,48],themes:["light","dark"],devicePixelRatios:[1,2,3]},null,2));
