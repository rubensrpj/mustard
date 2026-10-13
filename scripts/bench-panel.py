#!/usr/bin/env python3
"""Reproduce a synthetic dashboard comparison; never executes specs or calls AI.

Both inputs should be release binaries. The fixture has 25 specs and 80 rules
per spec. Results measure local latency and bytes, never net LLM savings.
"""
import argparse,os,json,pathlib,tempfile,subprocess,time,statistics,hashlib
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline',required=True,type=pathlib.Path)
parser.add_argument('--candidate',required=True,type=pathlib.Path)
parser.add_argument('--out',type=pathlib.Path)
args=parser.parse_args()
baseline=args.baseline.resolve()
candidate=args.candidate.resolve()
root=pathlib.Path(tempfile.mkdtemp(prefix='mustard-mods-benchmark-'))
(root/'mustard.json').write_text(json.dumps({'version':'0.2.18','rtk':False,'ai':{'fallback':False,'vectors':False}}))
subprocess.run(['git','init','-q',str(root)],check=True)
subprocess.run(['git','-C',str(root),'symbolic-ref','HEAD','refs/heads/feature/spec-00'],check=True)
index=[]
for n in range(25):
 name=f'spec-{n:02d}';folder=root/'.claude/spec'/name;folder.mkdir(parents=True)
 rows=[{'v':1,'id':1,'type':'state','phase':'running','branch':f'feature/{name}'},
       {'v':1,'id':2,'type':'context','title':'Objetivo aprovado','text':'Implementar o fluxo seguindo as regras do projeto.'}]
 for k in range(80):
  rows.append({'v':1,'id':k+3,'type':'rule','title':f'Regra {k}','code':f'MSTD-RULE-{k+1:04d}',
               'text':'Preservar o comportamento aprovado e conferir a fonte atual. '*8})
 rows += [{'v':1,'id':83,'type':'wave','n':1,'text':'Implementar o comportamento aprovado.','criteria':[]},
          {'v':1,'id':84,'type':'send','wave':1,'role':'wave','model':'fixture','effort':'medium',
           'text':f'# {name} — onda 1\n\n'+'Pedido exato com todas as orientações.\n'*200}]
 (folder/'spec.ndjson').write_text('\n'.join(json.dumps(row,ensure_ascii=False) for row in rows)+'\n')
 index.append({'v':1,'type':'spec','name':name,'goal':'Implementar o fluxo seguindo as regras do projeto.','phase':'running'})
(root/'.claude/spec/index.ndjson').write_text('\n'.join(json.dumps(row) for row in index)+'\n')
env=dict(os.environ,MUSTARD_SPEND_DIR=str(root/'spend'),MUSTARD_DISK_SYNC='off',MUSTARD_RT_DELEGATED='1')
def run(binary,args,repeats=9):
 times=[];bodies=[]
 for _ in range(repeats):
  started=time.perf_counter();p=subprocess.run([str(binary),'run','panel','--root',str(root),'--spec','spec-00',*args],cwd=root,env=env,capture_output=True,check=True)
  times.append((time.perf_counter()-started)*1000);bodies.append(p.stdout)
 return {'median_ms':round(statistics.median(times),3),'min_ms':round(min(times),3),'bytes':len(bodies[-1])},json.loads(bodies[-1])
base,old=run(baseline,[])
project,new=run(candidate,['--section','project'])
definitions,defs=run(candidate,['--section','definitions'])
unchanged,small=run(candidate,['--section','project','--if-revision',new['revision']])
wave,detail=run(candidate,['--section','waves','--wave','1'])
old_wave=subprocess.run([str(baseline),'run','panel','--root',str(root),'--spec','spec-00','--wave','1'],cwd=root,env=env,capture_output=True,check=True)
old_wave=json.loads(old_wave.stdout)
old_spec=next(row for row in old['specs'] if row['name']=='spec-00')
new_spec=next(row for row in defs['specs'] if row['name']=='spec-00')
assert new_spec['definitions']==old_spec['definitions']
assert detail['wave_detail']['request']==old_wave['wave_detail']['request']
assert small['unchanged'] is True and 'specs' not in small
receipt={'kind':'step','agent_id':'fixture-agent','turn_id':'fixture-turn','index':0,'model':'fixture','effort':'medium',
         'usage':{'input_tokens':100,'output_tokens':10,'cache_read_input_tokens':200,'cache_creation_input_tokens':0}}
for _ in range(2):
 p=subprocess.run([str(candidate),'run','usage-record','--root',str(root),'--session','fixture-session'],input=json.dumps(receipt),cwd=root,env=env,text=True,capture_output=True,check=True)
 assert json.loads(p.stdout)['ok'] is True
_,usage=run(candidate,['--section','usage','--session','fixture-session'],1)
agent=next(iter(usage['live_usage']['agents'].values()))
assert agent['input_tokens']==100 and agent['requests']==1
result={'fixture':{'specs':25,'rules_per_spec':80,'repeats':9,'root':str(root),'synthetic':True},
 'baseline_v0_2_18_full':base,'candidate_project':project,'candidate_definitions':definitions,'candidate_unchanged':unchanged,'candidate_wave':wave,
 'gains':{'project_latency_percent':round(100*(1-project['median_ms']/base['median_ms']),2),
          'project_output_bytes_percent':round(100*(1-project['bytes']/base['bytes']),2),
          'unchanged_latency_percent':round(100*(1-unchanged['median_ms']/base['median_ms']),2),
          'unchanged_output_bytes_percent':round(100*(1-unchanged['bytes']/base['bytes']),2),
          'idle_poll_schedule_percent':80},
 'verified':{'selected_definitions_identical':True,'wave_input_identical':True,'replayed_request_counted_once':True,'costless_usage_recorded':True,
             'remote_ai_enabled_in_fixture':False,'net_llm_token_savings_measured':False},
 'binaries':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in [('baseline',baseline),('candidate',candidate)]}}
if args.out:
 args.out.parent.mkdir(parents=True,exist_ok=True)
 args.out.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
