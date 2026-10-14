// UI-tree footprint only; not a terminal/desktop paint or an LLM benchmark.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import crypto from 'node:crypto';
const flags=new Map();for(let n=2;n<process.argv.length;n+=2)flags.set(process.argv[n],process.argv[n+1]);
for(const key of ['--baseline','--candidate','--out'])if(!flags.get(key))throw new Error('Missing '+key);
const elements=Object.fromEntries(['Box','Text','Button','Select','Input','Link','Markdown','Code','Svg'].map(type=>[type,props=>({type,props})]));
const data={ok:true,schema_version:1,section:'usage',at:'2026-10-13T12:00:00Z',project:{name:'example-service',branch:'feature/storage',language:'pt-BR'},
  specs:[{name:'storage-change',phase:'running',goal:'Implementar a operação de armazenamento',waves:[{wave:1,tokens:1200,status:'committed'},{wave:2,tokens:null,status:'running'}],
    usage:{known_wave_tokens:1200,waves_with_unknown_usage:1,conductor_tokens:2500},
    operations:{calls:20,refusals:4,search:{calls:1,enriched:1,fallback:0,local_model_calls:1,remote_model_calls:0,paired_calls:1,native_bytes:100,delivered_bytes:150}}}],
  rtk:{status:'measured',percent:40,saved_tokens:400,input_tokens:1000,output_tokens:600,commands:10,period_start:'2026-10-01',period_end:'2026-10-13'},
  jev:{physical_requests:17,cost_micro_usd:1428,by_purpose:{'wave-planning':11,context:6}}};
function footprint(tree) {
  const result={text_characters:0,text_blocks:0,bordered_boxes:0};
  const walk=node=>{
    if(Array.isArray(node)){node.forEach(walk);return;}
    if(!node || typeof node!=='object')return;
    if(node.type==='Text'){result.text_blocks++;result.text_characters+=Array.from(String(node.props.children ?? '')).length;}
    if(node.type==='Box' && node.props.borderStyle)result.bordered_boxes++;
    walk(node.props.children);
  };walk(tree);return result;
}
const arms={},source_hashes={};
for(const arm of ['baseline','candidate']) {
  const file=path.resolve(flags.get('--'+arm),'hooks/panel-view.js');
  source_hashes[arm]=Object.fromEntries(['panel-view.js','panel-economy.js','panel-content.js','panel-extras.js'].map(name=>[name,
    crypto.createHash('sha256').update(fs.readFileSync(path.resolve(flags.get('--'+arm),'hooks',name))).digest('hex')]));
  const {renderPanel,initialPanelView}=await import(pathToFileURL(file));
  arms[arm]=Object.fromEntries(['terminal','desktop'].map(surface=>[surface,footprint(renderPanel({elements,surface,columns:100,bodyRows:36,data,selected:'storage-change',
    view:{...initialPanelView(),tab:'Consumo',usageView:'Resumo'},actions:{change(){},selectSpec(){},refresh(){},close(){}}}))]));
}
const gain=Object.fromEntries(['terminal','desktop'].map(surface=>[surface,Object.fromEntries(Object.keys(arms.baseline[surface]).map(key=>[key,
  {before:arms.baseline[surface][key],after:arms.candidate[surface][key],reduction_percent:Math.round(10000*(1-arms.candidate[surface][key]/arms.baseline[surface][key]))/100}]))]));
const result={...arms,gain,source_hashes,limits:['Synthetic UI tree, not actual host rendering or a viewport fit measurement.','Text characters count Text nodes, excluding labels/options of controls. They are not LLM tokens or subscription savings.','The same data and viewport were used in both arms.']};
fs.mkdirSync(path.dirname(path.resolve(flags.get('--out'))),{recursive:true});fs.writeFileSync(flags.get('--out'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(gain,null,2));
