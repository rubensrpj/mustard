---
name: mustard:dashboard
description: Inicia ou exibe URL do dashboard local (specs, métricas, criação de PRD). Use quando o usuário pedir para ver specs em uma página, abrir dashboard, criar PRD via UI.
---

# /mustard:dashboard - Local Dashboard

## Trigger
`/mustard:dashboard [start|stop|status]` (default: `start`)

## What it does
Inicia (ou exibe status de) um servidor HTTP local em `http://localhost:7878` que expõe quatro tabs — Visão, Specs, Métricas e Novo PRD — lendo specs de `.claude/spec/{active,completed}` e métricas de `.claude/.metrics/` e `.claude/.harness/events.jsonl` em tempo real. Tab "Novo PRD" gera spec.md em `.claude/spec/active/<data>-<slug>/`.

## Action

### `start` (default)

```bash
node -e "const fs=require('fs'),path=require('path'),{spawn}=require('child_process'),http=require('http');const pidFile=path.join('.claude','.dashboard.pid');function alive(p){try{process.kill(p,0);return true;}catch(_){return false;}}function probe(cb){const r=http.request({host:'127.0.0.1',port:7878,method:'HEAD',path:'/',timeout:1500},res=>{cb(true);res.resume();});r.on('error',()=>cb(false));r.on('timeout',()=>{r.destroy();cb(false);});r.end();}if(fs.existsSync(pidFile)){const pid=parseInt(fs.readFileSync(pidFile,'utf8'),10);if(alive(pid)){console.log('Already running. URL: http://localhost:7878 (pid '+pid+')');process.exit(0);}else{try{fs.unlinkSync(pidFile);}catch(_){}}}const child=spawn('node',['.claude/scripts/dashboard.js'],{detached:true,stdio:'ignore',windowsHide:true});child.unref();setTimeout(()=>probe(ok=>{if(ok)console.log('Started. URL: http://localhost:7878 (pid '+child.pid+')');else console.log('Spawn issued (pid '+child.pid+') but probe failed — check logs or port 7878.');process.exit(0);}),1200);"
```

### `stop`

```bash
node -e "const fs=require('fs'),path=require('path');const pidFile=path.join('.claude','.dashboard.pid');if(!fs.existsSync(pidFile)){console.log('Not running.');process.exit(0);}const pid=parseInt(fs.readFileSync(pidFile,'utf8'),10);try{process.kill(pid);console.log('Stopped (pid '+pid+').');}catch(e){console.log('Process not found: '+e.message);}try{fs.unlinkSync(pidFile);}catch(_){}"
```

### `status`

```bash
node -e "const fs=require('fs'),path=require('path');const pidFile=path.join('.claude','.dashboard.pid');if(!fs.existsSync(pidFile)){console.log('stopped');process.exit(0);}const pid=parseInt(fs.readFileSync(pidFile,'utf8'),10);try{process.kill(pid,0);console.log('running (pid '+pid+') — http://localhost:7878');}catch(_){console.log('stopped (stale pid file)');}"
```

## Rules
- Server hardcoded to port `7878`. If occupied, the spawned process exits with a clear message — re-run `stop` or kill the conflicting process.
- All endpoints localhost-only; no auth, no CORS.
- POST /api/prd refuses overwrites (409 if `<date>-<slug>/` already exists).
- Logs to stdout when not detached; detached mode silences logs.
- PID file `.claude/.dashboard.pid` is gitignored.
