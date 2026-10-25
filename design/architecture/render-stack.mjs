import fs from 'node:fs/promises';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const sharp=require('sharp');
const out=new URL('./output',import.meta.url).pathname;
const a=[];const esc=s=>s.replaceAll('&','&amp;').replaceAll('<','&lt;');
function text(x,y,s,size=20,color='#222',weight='normal',anchor='middle'){a.push(`<text x="${x}" y="${y}" font-family="Arial, Helvetica, sans-serif" font-size="${size}" fill="${color}" font-weight="${weight}" text-anchor="${anchor}">${esc(s)}</text>`);}
function label(x,y,s,size=17){const w=s.length*size*.54+12;a.push(`<rect x="${x-w/2}" y="${y-size}" width="${w}" height="${size+5}" fill="white"/>`);text(x,y,s,size);}
function box(x,y,w,h,title,meta,lines,color='#078218',bodySize=21){a.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="white" stroke="${color}" stroke-width="1.6"/><path d="M${x} ${y+47}h${w}" stroke="${color}" stroke-width="1.4"/>`);text(x+w/2,y+32,title,25,color,'bold');text(x+w/2,y+73,meta,17,'#666');lines.forEach((s,i)=>text(x+w/2,y+105+i*28,s,bodySize));}
function path(d,both=false){a.push(`<path d="${d}" fill="none" stroke="#444" stroke-width="1.5" marker-end="url(#arrow)" ${both?'marker-start="url(#arrowStart)"':''}/>`);}
function boundary(x,y,w,h,label){a.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="none" stroke="#999" stroke-width="1.5" stroke-dasharray="7 6"/>`);if(label)text(x+14,y-12,label,20,'#555','normal','start');}
a.push('<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 1000"><defs><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 1L9 5L0 9Z" fill="#444"/></marker><marker id="arrowStart" viewBox="0 0 10 10" refX="1" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M9 1L1 5L9 9Z" fill="#444"/></marker></defs><rect width="1600" height="1000" fill="white"/>');
text(40,58,'Wright software architecture',43,'#181818','bold','start');
text(40,94,'AI hardware engineering',25,'#666','normal','start');
text(40,127,'Desktop orchestration and local engineering tools, with private Azure persistence',19,'#666','normal','start');
box(400,167,330,125,'Model providers','[External software system]',['Claude · OpenAI Codex · Foundry'],'#078218',18);
box(770,167,330,125,'CREAO','[External software system]',['Parts sourcing and quotes']);
boundary(380,365,740,286,'Wright system');
box(400,386,700,244,'Wright desktop application','[Container: Tauri 2 · Rust + React / TypeScript]',[
'Agent harness: Rust + Tokio',
'Coordinates tools, design revisions and evidence',
'Checks: Rust · Haskell oracle · Lean specifications',
'Local state: checkpoints, events and lessons'],'#3021F5');
box(40,410,210,95,'Engineer','[Person]',[],'#ed2024');
path('M250 457H400');text(323,422,'Requirements',17);text(323,444,'and reviews',17);
path('M565 386V292');label(565,322,'Model requests / CLI or HTTPS',17);
path('M935 386V292');label(935,322,'BOM + quotes / HTTPS',17);
box(1160,386,400,244,'Azure storage','[Private cloud services]',[
'Cosmos DB: run history and results',
'Blob Storage: PCB, CAD, build files',
'HTTPS REST · Microsoft Entra ID',
'Automatic save after a run ends']);
path('M1100 500H1160',true);text(1130,482,'HTTPS',13);
text(40,700,'Local engineering tools',20,'#555','normal','start');
boundary(25,749,1550,176,'');
const tools=[
['Mechanical Design','[FreeCAD · Python worker]','Native CAD, STEP and meshes','CAD / JSON-RPC'],
['Electronics Design','[Rust PCB engine · KiCad CLI]','Schematic, placement and routing','PCB integration in progress'],
['Physics Simulation','[SU2 · AVL / OptVL]','Aerodynamics and flight analysis','Solver inputs'],
['Structural Analysis','[Gmsh + CalculiX]','Meshing, loads and deformation','Meshes + loads'],
['Print Preparation','[PrusaSlicer · C++ application]','Slicing and manufacturing output','STL + settings']];
// The common line denotes tool invocation, not data flowing between tools.
a.push('<path d="M750 630V711H180M750 711H1400" fill="none" stroke="#444" stroke-width="1.5"/>');
tools.forEach((t,i)=>{const x=40+i*305,c=x+140;path(`M${c} 711V778`);label(c,738,t[3],17);box(x,778,280,128,t[0],t[1],[t[2]],'#078218',18);});
text(40,965,'Legend',18,'#222','bold','start');
[['#ed2024','Person',130],['#3021F5','Wright container',270],['#078218','Other software / service',505]].forEach(([c,t,x])=>{a.push(`<rect x="${x}" y="950" width="15" height="15" fill="white" stroke="${c}" stroke-width="1.5"/>`);text(x+23,965,t,18,'#222','normal','start');});
path('M810 958H850');text(864,965,'Uses / exchanges data',18,'#222','normal','start');
a.push('<rect x="1110" y="949" width="22" height="18" fill="none" stroke="#999" stroke-dasharray="5 4"/>');text(1142,965,'System / process boundary',18,'#222','normal','start');
a.push('</svg>');const svg=a.join('\n');await fs.writeFile(out+'/wright-architecture-stack.svg',svg);await sharp(Buffer.from(svg)).resize(3200,1800).flatten({background:"#ffffff"}).png().toFile(out+'/wright-architecture-stack.png');
