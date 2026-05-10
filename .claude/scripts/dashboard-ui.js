'use strict';

// Mustard Dashboard UI — Linear/Supabase-inspired SaaS dashboard
// Type: Geist Sans (body) + Geist Mono (data/code). Brand: mustard.

function escapeHtml(s) {
  return String(s == null ? '' : s).replace(/[<>&"']/g, c => (
    { '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;', "'": '&#39;' }[c]
  ));
}

const CSS = `
*, *::before, *::after { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
:root {
  --bg: #08080a;
  --surface: #0e0e10;
  --surface-2: #16161a;
  --surface-3: #1c1c22;
  --border: #1f1f24;
  --border-2: #2a2a32;
  --ink: #ededf0;
  --ink-mute: #a4a4ae;
  --ink-dim: #6e6e7a;
  --brand: #e2a93b;
  --brand-2: #f5c542;
  --brand-soft: rgba(226,169,59,0.10);
  --success: #10b981;
  --success-soft: rgba(16,185,129,0.12);
  --success-soft-2: rgba(16,185,129,0.06);
  --warning: #f59e0b;
  --warning-soft: rgba(245,158,11,0.12);
  --danger: #ef4444;
  --danger-soft: rgba(239,68,68,0.12);
  --info: #6366f1;
  --info-soft: rgba(99,102,241,0.12);
  --plum: #a78bfa;
  --plum-soft: rgba(167,139,250,0.12);
  --shadow-1: 0 1px 0 rgba(255,255,255,0.03), 0 0 0 1px var(--border);
  --shadow-2: 0 4px 20px -4px rgba(0,0,0,0.4), 0 0 0 1px var(--border);
  --shadow-pop: 0 24px 48px -12px rgba(0,0,0,0.6), 0 0 0 1px var(--border-2);
  --rail-w: 232px;
  --content-max: 1240px;
  --side-panel-w: min(760px, 92vw);
  --radius-sm: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;
}
[data-theme="light"] {
  --bg: #fafafa;
  --surface: #ffffff;
  --surface-2: #f4f4f5;
  --surface-3: #e9e9ec;
  --border: #e4e4e7;
  --border-2: #d4d4d8;
  --ink: #18181b;
  --ink-mute: #52525b;
  --ink-dim: #a1a1aa;
  --brand: #b8890e;
  --brand-2: #8a6608;
  --brand-soft: rgba(184,137,14,0.10);
  --success: #059669;
  --success-soft: rgba(5,150,105,0.10);
  --success-soft-2: rgba(5,150,105,0.05);
  --warning: #d97706;
  --warning-soft: rgba(217,119,6,0.10);
  --danger: #dc2626;
  --danger-soft: rgba(220,38,38,0.10);
  --info: #4f46e5;
  --info-soft: rgba(79,70,229,0.10);
  --plum: #7c3aed;
  --plum-soft: rgba(124,58,237,0.10);
}

html { color-scheme: dark; background: var(--bg); }
[data-theme="light"] html, html[data-theme="light"] { color-scheme: light; }

body {
  font-family: 'Geist', ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  font-size: 14px; line-height: 1.55;
  color: var(--ink); background: var(--bg);
  font-feature-settings: 'cv11', 'ss01', 'ss03';
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  min-height: 100vh;
}
.mono, code, kbd, pre { font-family: 'Geist Mono', ui-monospace, SFMono-Regular, 'SF Mono', monospace; font-feature-settings: 'tnum'; }
::selection { background: var(--brand-soft); color: var(--brand-2); }
button, input, select, textarea { font-family: inherit; }

/* App shell ------------------------------------------------------------ */
.app {
  display: block; min-height: 100vh;
  padding-left: var(--rail-w);
  transition: padding 240ms cubic-bezier(0.2,0.8,0.2,1);
}

.rail {
  position: fixed; top: 0; left: 0; bottom: 0; width: var(--rail-w);
  background: var(--bg); border-right: 1px solid var(--border);
  padding: 18px 14px 14px;
  display: flex; flex-direction: column; gap: 24px;
  z-index: 30;
  overflow-y: auto;
}
.rail .brand-row { display: flex; align-items: center; gap: 10px; padding: 6px 8px; }
.rail .logo {
  width: 28px; height: 28px; border-radius: 7px;
  background: linear-gradient(135deg, var(--brand) 0%, var(--brand-2) 100%);
  display: grid; place-items: center;
  font-weight: 600; color: #1a1208; font-size: 15px; letter-spacing: -0.02em;
}
.rail .brand-name { font-weight: 600; font-size: 14px; letter-spacing: -0.01em; }
.rail .brand-meta { font-size: 11px; color: var(--ink-dim); margin-left: auto; font-family: 'Geist Mono', monospace; }
.rail .nav-section-label {
  font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em;
  color: var(--ink-dim); padding: 0 8px; margin-bottom: 6px; font-weight: 500;
}
.rail nav { display: flex; flex-direction: column; gap: 2px; }
.rail nav a {
  display: flex; align-items: center; gap: 10px;
  padding: 7px 10px; border-radius: var(--radius-sm); cursor: pointer;
  font-size: 13px; color: var(--ink-mute);
  transition: all 120ms ease; user-select: none; position: relative;
}
.rail nav a .ic { width: 16px; height: 16px; display: grid; place-items: center; }
.rail nav a .ic svg { width: 16px; height: 16px; stroke: currentColor; }
.rail nav a:hover { background: var(--surface-2); color: var(--ink); }
.rail nav a.on { background: var(--surface-2); color: var(--ink); }
.rail nav a.on .ic { color: var(--brand); }
.rail .footer-actions { margin-top: auto; display: flex; gap: 6px; padding: 4px; }
.rail .footer-actions button {
  flex: 1; background: transparent; border: 1px solid var(--border);
  border-radius: var(--radius-sm); color: var(--ink-mute);
  font-size: 11px; padding: 7px 10px; cursor: pointer; transition: all 120ms;
  display: inline-flex; align-items: center; justify-content: center; gap: 6px;
}
.rail .footer-actions button:hover { color: var(--ink); border-color: var(--border-2); background: var(--surface-2); }
.rail .footer-actions button svg { width: 13px; height: 13px; stroke: currentColor; fill: none; stroke-width: 1.7; }

/* Main */
.main { padding: 0; min-width: 0; }

.topbar {
  position: sticky; top: 0; z-index: 20; padding: 0; height: 56px;
  background: rgba(8,8,10,0.85); backdrop-filter: blur(8px);
  border-bottom: 1px solid var(--border);
}
[data-theme="light"] .topbar { background: rgba(250,250,250,0.85); }
.topbar-inner {
  height: 100%; padding: 0 32px;
  display: flex; align-items: center; gap: 14px;
}
.topbar h1 { margin: 0; font-size: 14px; font-weight: 600; letter-spacing: -0.01em; }
.topbar .crumb { font-size: 12px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; }
.topbar .crumb b { color: var(--ink-mute); font-weight: 500; }
.topbar .right { margin-left: auto; }

.menu-btn {
  display: none; background: var(--surface-2); border: 1px solid var(--border);
  border-radius: var(--radius-sm); color: var(--ink); width: 34px; height: 34px;
  align-items: center; justify-content: center; cursor: pointer; padding: 0;
}
.menu-btn svg { width: 18px; height: 18px; stroke: currentColor; fill: none; stroke-width: 1.8; }

/* Live banner (sticky below topbar) */
.live-banner {
  position: sticky; top: 56px; z-index: 19;
  background: var(--success-soft); border-bottom: 1px solid var(--success);
  padding: 8px 32px; display: flex; align-items: center; gap: 10px;
  font-size: 13px; color: var(--success); font-weight: 500;
}
.live-banner[hidden] { display: none; }
.live-banner .summary { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: 'Geist', sans-serif; }
.live-banner .summary b { color: var(--ink); font-weight: 600; }
[data-theme="dark"] .live-banner .summary b { color: var(--ink); }
.live-banner .btn { padding: 4px 10px; }

.panel { display: none; padding: 28px 32px 64px; }
.panel.on { display: block; animation: fadein 200ms ease both; }
@keyframes fadein { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }

/* Section heads */
.h-section {
  font-size: 13px; font-weight: 500; color: var(--ink-mute);
  text-transform: uppercase; letter-spacing: 0.06em;
  margin: 28px 0 12px; display: flex; align-items: center; gap: 10px;
}
.h-section::after { content: ''; flex: 1; height: 1px; background: var(--border); }
.help-line { font-size: 12.5px; color: var(--ink-mute); margin-top: -6px; margin-bottom: 14px; line-height: 1.6; }

/* Filter chips */
.filter-bar { display: flex; align-items: center; gap: 8px; margin-bottom: 14px; flex-wrap: wrap; }
.filter-bar .label { font-size: 11px; color: var(--ink-dim); text-transform: uppercase; letter-spacing: 0.08em; font-weight: 500; }
.chip {
  background: transparent; border: 1px solid var(--border); border-radius: 999px;
  color: var(--ink-mute); padding: 4px 12px; font-size: 12px; cursor: pointer;
  transition: all 120ms; font-family: 'Geist Mono', monospace;
}
.chip:hover { color: var(--ink); border-color: var(--border-2); }
.chip.on { background: var(--brand-soft); color: var(--brand); border-color: transparent; }

/* Cards */
.card { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 18px; }
.card-h { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
.card-h h3 { margin: 0; font-size: 13px; font-weight: 600; letter-spacing: -0.005em; }
.card-h .crumb { font-size: 11px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; margin-left: auto; }

/* KPI grid */
.kpi-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
.kpi-grid.cols-3 { grid-template-columns: repeat(3, 1fr); }
.kpi-grid.cols-2 { grid-template-columns: repeat(2, 1fr); }
.kpi { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 16px 18px; display: flex; flex-direction: column; gap: 4px; min-width: 0; transition: border-color 200ms; }
.kpi:hover { border-color: var(--border-2); }
.kpi .label { font-size: 11px; color: var(--ink-dim); text-transform: uppercase; letter-spacing: 0.06em; font-weight: 500; }
.kpi .val { font-family: 'Geist Mono', monospace; font-size: 28px; font-weight: 500; color: var(--ink); letter-spacing: -0.02em; line-height: 1.1; word-break: break-all; }
.kpi .delta { font-size: 12px; color: var(--ink-mute); display: flex; align-items: center; gap: 6px; }
.kpi .delta .dot { width: 6px; height: 6px; border-radius: 50%; }
.kpi .delta.ok .dot { background: var(--success); }
.kpi .delta.warn .dot { background: var(--warning); }
.kpi .delta.dim .dot { background: var(--ink-dim); }

/* Tags */
.tag {
  display: inline-flex; align-items: center; gap: 5px;
  padding: 3px 8px; border-radius: var(--radius-sm);
  font-size: 11px; font-weight: 500; letter-spacing: 0.01em;
  background: var(--surface-2); color: var(--ink-mute); border: 1px solid var(--border);
  font-family: 'Geist Mono', monospace; text-transform: lowercase;
}
.tag.brand { background: var(--brand-soft); color: var(--brand); border-color: transparent; }
.tag.success { background: var(--success-soft); color: var(--success); border-color: transparent; }
.tag.warn { background: var(--warning-soft); color: var(--warning); border-color: transparent; }
.tag.danger { background: var(--danger-soft); color: var(--danger); border-color: transparent; }
.tag.info { background: var(--info-soft); color: var(--info); border-color: transparent; }
.tag.plum { background: var(--plum-soft); color: var(--plum); border-color: transparent; }
.tag.ph-analyze { background: var(--plum-soft); color: var(--plum); border-color: transparent; }
.tag.ph-plan { background: var(--brand-soft); color: var(--brand); border-color: transparent; }
.tag.ph-execute { background: var(--info-soft); color: var(--info); border-color: transparent; }
.tag.ph-qa { background: var(--success-soft); color: var(--success); border-color: transparent; }
.tag.ph-close { background: var(--surface-2); color: var(--ink-mute); border-color: transparent; }

/* Live indicator */
.live-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: var(--success); position: relative; flex: 0 0 auto; }
.live-dot::after { content: ''; position: absolute; inset: -4px; border-radius: 50%; border: 2px solid var(--success); opacity: 0.5; animation: live-pulse 1.6s ease-out infinite; }
@keyframes live-pulse {
  0% { transform: scale(0.8); opacity: 0.7; }
  100% { transform: scale(1.6); opacity: 0; }
}
.live-pill {
  display: inline-flex; align-items: center; gap: 6px;
  padding: 2px 8px 2px 6px; border-radius: 999px;
  background: var(--success-soft); color: var(--success);
  font-family: 'Geist Mono', monospace; font-size: 10px; font-weight: 600;
  letter-spacing: 0.06em; text-transform: uppercase;
}

/* Spec card */
.spec-card {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); padding: 18px 20px;
  display: flex; flex-direction: column; gap: 12px; margin-bottom: 10px;
  transition: border-color 200ms;
}
.spec-card.live { border-color: var(--success); box-shadow: 0 0 0 3px var(--success-soft-2); }
.spec-card:hover { border-color: var(--border-2); }
.spec-card.live:hover { border-color: var(--success); }
.spec-card .head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.spec-card .ttl { font-size: 15px; font-weight: 600; color: var(--ink); letter-spacing: -0.01em; flex: 1; min-width: 0; }
.spec-card .nm { font-size: 12px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; word-break: break-all; }
.spec-card .summary { font-size: 13.5px; color: var(--ink-mute); line-height: 1.65; width: 100%; }
.spec-card .progress { display: grid; grid-template-columns: auto 1fr auto; gap: 10px; align-items: center; }
.spec-card .progress .pct { font-family: 'Geist Mono', monospace; font-size: 12px; font-weight: 500; color: var(--ink); }
.spec-card .progress .frac { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-dim); }
.spec-card .progress .track { height: 6px; background: var(--surface-2); border-radius: 3px; position: relative; overflow: hidden; }
.spec-card .progress .fill { position: absolute; left: 0; top: 0; bottom: 0; background: var(--brand); transition: width 600ms cubic-bezier(0.2,0.8,0.2,1); border-radius: 3px; }
.spec-card.live .progress .fill { background: var(--success); }
.spec-card .meta-row { display: flex; gap: 16px; flex-wrap: wrap; font-size: 12px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; }
.spec-card .meta-row b { color: var(--ink-mute); font-weight: 500; }
.spec-card .actions { display: flex; gap: 6px; flex-wrap: wrap; }

/* Epic card with sub-waves */
.epic-card { padding: 18px 20px 14px; }
.epic-card .epic-summary { font-size: 13.5px; color: var(--ink-mute); line-height: 1.65; width: 100%; margin-bottom: 4px; }
.epic-card .epic-progress-line { display: flex; align-items: center; gap: 10px; padding: 8px 0; border-top: 1px dashed var(--border); margin-top: 10px; }
.epic-card .epic-progress-line .lbl { font-family: 'Geist Mono', monospace; font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--ink-dim); }
.epic-card .waves-list { display: flex; flex-direction: column; gap: 4px; padding-top: 6px; }
.wave-row {
  display: grid; grid-template-columns: 28px 1fr auto auto auto auto; gap: 14px; align-items: center;
  padding: 10px 12px; border-radius: var(--radius-sm); background: var(--surface-2);
  border: 1px solid transparent; transition: all 120ms; cursor: pointer;
}
.wave-row:hover { background: var(--surface-3); border-color: var(--border); }
.wave-row.live { border-color: var(--success); background: var(--success-soft-2); }
.wave-row .ix { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-dim); text-align: center; }
.wave-row .name { font-size: 13px; font-weight: 500; color: var(--ink); display: flex; align-items: center; gap: 8px; min-width: 0; }
.wave-row .name .lbl { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.wave-row .progress-mini { width: 110px; height: 4px; background: var(--surface-3); border-radius: 2px; position: relative; overflow: hidden; }
.wave-row .progress-mini .fill { position: absolute; left: 0; top: 0; bottom: 0; background: var(--brand); transition: width 600ms; border-radius: 2px; }
.wave-row.live .progress-mini .fill { background: var(--success); }
.wave-row .frac { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-mute); min-width: 42px; text-align: right; }
.wave-row .stamp { font-family: 'Geist Mono', monospace; font-size: 10px; color: var(--ink-dim); min-width: 70px; text-align: right; }

/* Checklist */
.checklist { margin-top: 10px; padding: 12px 14px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); }
.checklist[hidden] { display: none; }
.checklist .item { display: grid; grid-template-columns: 16px 1fr; gap: 8px; align-items: baseline; padding: 4px 0; font-size: 13px; }
.checklist .item .mark { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-dim); }
.checklist .item.done .mark { color: var(--success); }
.checklist .item.done .text { color: var(--ink-dim); text-decoration: line-through; }
.checklist .item .pfx { display: inline-block; font-family: 'Geist Mono', monospace; font-size: 10px; color: var(--brand); margin-right: 6px; padding: 1px 6px; background: var(--brand-soft); border-radius: 3px; }

/* Index list (completed specs) */
.idx-month { font-size: 11px; color: var(--ink-dim); text-transform: uppercase; letter-spacing: 0.08em; margin: 22px 0 8px; padding-bottom: 6px; border-bottom: 1px solid var(--border); font-weight: 500; }
.idx-row { display: grid; grid-template-columns: 1fr auto auto; gap: 14px; align-items: center; padding: 9px 12px; font-size: 13px; border-radius: var(--radius-sm); transition: background 120ms; }
.idx-row:hover { background: var(--surface-2); cursor: pointer; }
.idx-row .nm { font-family: 'Geist Mono', monospace; color: var(--ink); font-size: 12.5px; }
.idx-row .meta { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-dim); }
.idx-row .stat { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-mute); }

/* Buttons */
.btn { display: inline-flex; align-items: center; gap: 6px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); color: var(--ink); padding: 6px 11px; font-size: 12px; font-weight: 500; cursor: pointer; transition: all 120ms ease; }
.btn:hover { border-color: var(--border-2); background: var(--surface-3); }
.btn.primary { background: var(--brand); color: #1a1208; border-color: var(--brand); }
.btn.primary:hover { background: var(--brand-2); border-color: var(--brand-2); }
.btn.ghost { background: transparent; }
.btn.live { background: var(--success-soft); color: var(--success); border-color: transparent; font-weight: 600; }
.btn.live:hover { background: var(--success); color: white; }
.btn:disabled { opacity: 0.5; cursor: not-allowed; }
.btn svg { width: 14px; height: 14px; stroke: currentColor; fill: none; stroke-width: 1.7; }

/* Tables */
.tbl { width: 100%; border-collapse: collapse; }
.tbl thead th { font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--ink-dim); font-weight: 500; text-align: left; padding: 10px 12px; border-bottom: 1px solid var(--border-2); }
.tbl tbody td { padding: 10px 12px; font-size: 13px; border-bottom: 1px solid var(--border); color: var(--ink); font-family: 'Geist Mono', monospace; vertical-align: top; }
.tbl tbody tr:hover td { background: var(--surface-2); }
.tbl td.muted { color: var(--ink-mute); }
.tbl td.num { text-align: right; }
.tbl td.help { font-family: 'Geist', sans-serif; font-size: 12px; color: var(--ink-mute); line-height: 1.5; }

/* Telemetry chart */
.chart-wrap { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 18px 20px 14px; margin-top: 12px; position: relative; }
.chart-wrap .chart { width: 100%; height: 240px; display: block; }
.chart-wrap svg { width: 100%; height: 100%; overflow: visible; }
.chart-wrap .grid { stroke: var(--border); stroke-width: 1; stroke-dasharray: 2 4; }
.chart-wrap .axis { stroke: var(--border-2); stroke-width: 1; }
.chart-wrap .area { fill: url(#chart-gradient); }
.chart-wrap .line { fill: none; stroke: var(--brand); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
.chart-wrap .pt { fill: var(--bg); stroke: var(--brand); stroke-width: 2; transition: r 200ms; }
.chart-wrap .pt:hover { r: 6; }
.chart-wrap .pt.zero { stroke: var(--ink-dim); }
.chart-wrap .x-label { font-family: 'Geist Mono', monospace; font-size: 11px; fill: var(--ink-mute); }
.chart-wrap .y-label { font-family: 'Geist Mono', monospace; font-size: 10px; fill: var(--ink-dim); }
.chart-wrap .pt-value { font-family: 'Geist Mono', monospace; font-size: 11px; font-weight: 500; fill: var(--ink); }
.chart-wrap .legend { display: flex; align-items: center; justify-content: space-between; margin-top: 8px; font-size: 11px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; padding-top: 8px; border-top: 1px solid var(--border); }
.chart-wrap .legend .swatch { display: inline-block; width: 12px; height: 2px; background: var(--brand); vertical-align: middle; margin-right: 6px; border-radius: 1px; }

/* Phase distribution bar */
.phase-bar { display: flex; height: 32px; border-radius: var(--radius-sm); overflow: hidden; border: 1px solid var(--border); margin: 12px 0; }
.phase-bar .seg { display: flex; align-items: center; justify-content: center; font-size: 11px; font-weight: 500; font-family: 'Geist Mono', monospace; color: white; padding: 0 8px; min-width: 40px; }
.phase-legend { display: flex; gap: 16px; flex-wrap: wrap; font-size: 11px; color: var(--ink-mute); font-family: 'Geist Mono', monospace; }
.phase-legend span { display: inline-flex; align-items: center; gap: 6px; }
.phase-legend .dot { width: 8px; height: 8px; border-radius: 2px; }

/* PRD layout */
.prd-layout { display: grid; grid-template-columns: minmax(0,1fr) minmax(0,1fr); gap: 18px; }
.prd-layout .card { padding: 18px 20px; }
.prd-layout .card-h h3 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--ink-mute); }
.prd-layout .row { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.prd-layout .field { margin-bottom: 12px; }
.prd-layout .field label { display: flex; align-items: baseline; font-size: 12px; font-weight: 500; color: var(--ink); margin-bottom: 5px; }
.prd-layout .field label .hint { color: var(--ink-dim); font-weight: 400; font-size: 11px; margin-left: 8px; font-family: 'Geist Mono', monospace; }
.prd-layout input[type="text"], .prd-layout textarea, .prd-layout select { width: 100%; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 7px 10px; color: var(--ink); font-size: 13px; transition: border-color 120ms; outline: none; }
.prd-layout textarea { resize: vertical; min-height: 64px; font-family: 'Geist Mono', monospace; font-size: 12px; line-height: 1.55; }
.prd-layout input[type="text"]:focus, .prd-layout textarea:focus, .prd-layout select:focus { border-color: var(--brand); box-shadow: 0 0 0 3px var(--brand-soft); }
.checkbox-group { display: flex; flex-wrap: wrap; gap: 6px; }
.checkbox-group label { display: inline-flex; align-items: center; gap: 6px; padding: 5px 9px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: 12px; color: var(--ink-mute); cursor: pointer; transition: all 120ms; user-select: none; }
.checkbox-group label:has(input:checked) { background: var(--brand-soft); color: var(--brand); border-color: transparent; }
.checkbox-group input { margin: 0; accent-color: var(--brand); }
.prd-actions { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border); }
.prd-output { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 14px 16px; font-family: 'Geist Mono', monospace; font-size: 12px; white-space: pre-wrap; word-break: break-word; color: var(--ink-mute); line-height: 1.55; min-height: 480px; max-height: 70vh; overflow: auto; }
.prd-meta-line { display: flex; align-items: center; justify-content: space-between; font-size: 11px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; margin-bottom: 8px; }

/* Settings */
.set-group { margin-bottom: 28px; }
.set-group .gh { display: flex; align-items: baseline; gap: 12px; margin-bottom: 4px; flex-wrap: wrap; }
.set-group .gh h3 { margin: 0; font-size: 16px; font-weight: 600; letter-spacing: -0.01em; }
.set-group .gd { font-size: 13px; color: var(--ink-mute); margin: 0 0 14px; line-height: 1.55; }
.set-list { display: flex; flex-direction: column; gap: 12px; }
.set-card { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 16px 18px 14px; }
.set-card .head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 8px; }
.set-card .key {
  font-family: 'Geist Mono', monospace; font-size: 14px; font-weight: 600; color: var(--ink);
  background: var(--surface-2); padding: 5px 10px; border-radius: var(--radius-sm); border: 1px solid var(--border);
}
.set-card .desc { font-size: 13.5px; color: var(--ink-mute); line-height: 1.6; margin: 0 0 14px; }
.set-card .opt-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 8px; }
.set-card .opt {
  border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 10px 12px;
  cursor: pointer; transition: all 140ms; user-select: none; background: var(--surface-2);
  display: flex; flex-direction: column; gap: 4px;
}
.set-card .opt:hover { border-color: var(--border-2); background: var(--surface-3); }
.set-card .opt.on { border-color: var(--brand); background: var(--brand-soft); box-shadow: 0 0 0 3px var(--brand-soft); }
.set-card .opt input { display: none; }
.set-card .opt .name { font-family: 'Geist Mono', monospace; font-size: 12px; font-weight: 600; color: var(--ink); display: flex; align-items: center; gap: 6px; }
.set-card .opt.on .name { color: var(--brand); }
.set-card .opt .name .star { color: var(--ink-dim); font-size: 9px; font-weight: 400; }
.set-card .opt .doc { font-size: 12px; color: var(--ink-mute); line-height: 1.5; }
.set-bar {
  position: sticky; bottom: 16px; margin: 28px 0 0;
  background: var(--surface); border: 1px solid var(--border-2); border-radius: var(--radius-md);
  padding: 12px 16px; display: flex; align-items: center; gap: 12px;
  box-shadow: var(--shadow-2);
}
.set-bar .summary { font-size: 12px; color: var(--ink-mute); flex: 1; font-family: 'Geist Mono', monospace; }
.set-bar.dirty { border-color: var(--brand); }

/* Glossary tooltip via abbr */
abbr.gloss {
  text-decoration: underline dotted;
  text-decoration-color: var(--ink-dim);
  text-underline-offset: 2px;
  cursor: help;
}
.gloss-card {
  background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md);
  padding: 12px 14px; margin-bottom: 8px;
}
.gloss-card .term { font-family: 'Geist Mono', monospace; font-size: 13px; font-weight: 600; color: var(--ink); }
.gloss-card .def { font-size: 13px; color: var(--ink-mute); margin-top: 4px; line-height: 1.55; }

/* Commands tab */
.cmd-filters { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-bottom: 18px; }
.cmd-filters .label { font-size: 11px; color: var(--ink-dim); text-transform: uppercase; letter-spacing: 0.08em; font-weight: 500; }
.cmd-search { flex: 1; min-width: 220px; max-width: 320px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 7px 10px; color: var(--ink); font-size: 13px; outline: none; transition: border-color 120ms; }
.cmd-search:focus { border-color: var(--brand); box-shadow: 0 0 0 3px var(--brand-soft); }
.cmd-card {
  background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md);
  padding: 18px 20px 16px; margin-bottom: 12px;
}
.cmd-card .title-row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 4px; }
.cmd-card .cmd {
  font-family: 'Geist Mono', monospace; font-size: 16px; font-weight: 600; color: var(--ink);
  background: var(--brand-soft); padding: 4px 10px; border-radius: var(--radius-sm);
}
.cmd-card .syntax { font-family: 'Geist Mono', monospace; font-size: 12px; color: var(--ink-dim); }
.cmd-card .short { font-size: 14px; color: var(--ink); margin: 4px 0 12px; line-height: 1.5; font-weight: 500; }
.cmd-card .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 14px 24px; margin-top: 8px; }
.cmd-card .block .lk {
  font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--ink-dim);
  font-weight: 600; margin-bottom: 4px; display: flex; align-items: center; gap: 6px;
}
.cmd-card .block .lk .pill { font-size: 9px; padding: 1px 6px; background: var(--surface-2); border-radius: 3px; color: var(--ink-mute); border: 1px solid var(--border); text-transform: lowercase; letter-spacing: 0.02em; font-weight: 500; }
.cmd-card .block .lk .pill.ok { background: var(--success-soft); color: var(--success); border-color: transparent; }
.cmd-card .block .lk .pill.tech { background: var(--info-soft); color: var(--info); border-color: transparent; }
.cmd-card .block .v { font-size: 13px; color: var(--ink-mute); line-height: 1.6; }
.cmd-card .examples { margin-top: 12px; padding-top: 12px; border-top: 1px dashed var(--border); }
.cmd-card .examples .lk { font-size: 10px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--ink-dim); font-weight: 600; margin-bottom: 6px; }
.cmd-card .ex {
  display: block; font-family: 'Geist Mono', monospace; font-size: 12px;
  background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm);
  padding: 6px 10px; color: var(--ink); margin-bottom: 4px; cursor: pointer; transition: all 120ms;
}
.cmd-card .ex:hover { background: var(--surface-3); border-color: var(--brand); color: var(--brand); }
.cmd-card .seealso { margin-top: 10px; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; font-size: 11px; color: var(--ink-dim); font-family: 'Geist Mono', monospace; }
.cmd-card .seealso .ref {
  padding: 2px 7px; background: var(--surface-2); border-radius: 3px; cursor: pointer;
  color: var(--ink-mute); transition: all 120ms; border: 1px solid var(--border);
}
.cmd-card .seealso .ref:hover { color: var(--brand); border-color: var(--brand); }
.cmd-card.dim { display: none; }

.cmd-cat-head {
  display: flex; align-items: baseline; gap: 12px; margin: 24px 0 8px;
}
.cmd-cat-head h3 { margin: 0; font-size: 15px; font-weight: 600; letter-spacing: -0.01em; }
.cmd-cat-head .ct { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-dim); }

/* Side panel (right slide-in) ----------------------------------- */
.side-panel {
  position: fixed; top: 0; right: 0; bottom: 0; width: var(--side-panel-w);
  background: var(--surface); border-left: 1px solid var(--border-2);
  z-index: 200; display: flex; flex-direction: column;
  transform: translateX(100%); transition: transform 240ms cubic-bezier(0.2,0.8,0.2,1);
  box-shadow: var(--shadow-pop);
}
.side-panel.open { transform: translateX(0); }
.side-panel.pinned { box-shadow: var(--shadow-md); }
/* When pinned, shrink only the .app content (rail is fixed and unaffected) */
body.panel-pinned .app { padding-right: var(--side-panel-w); }
@media (max-width: 1100px) { body.panel-pinned .app { padding-right: 0; } }
/* Resize handle (left edge of side-panel) */
.sp-resize {
  position: absolute; top: 0; bottom: 0; left: 0; width: 6px;
  cursor: col-resize; z-index: 5;
  transition: background 120ms;
}
.sp-resize:hover, .sp-resize.dragging { background: var(--brand-soft); }
.sp-resize::after {
  content: ''; position: absolute; left: 2px; top: 0; bottom: 0; width: 2px;
  background: transparent; transition: background 120ms;
}
.sp-resize:hover::after, .sp-resize.dragging::after { background: var(--brand); }
@media (max-width: 1100px) { .sp-resize { display: none; } }
/* While dragging: kill animations + force cursor everywhere */
body.resizing-panel { cursor: col-resize !important; user-select: none; }
body.resizing-panel .side-panel,
body.resizing-panel.panel-pinned .app { transition: none !important; }
.side-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,0.35); backdrop-filter: blur(2px);
  z-index: 199; opacity: 0; pointer-events: none; transition: opacity 200ms;
}
[data-theme="light"] .side-overlay { background: rgba(0,0,0,0.2); }
.side-overlay.open { opacity: 1; pointer-events: auto; }
.side-overlay.pinned { opacity: 0; pointer-events: none; }
.sp-pin {
  width: 30px; height: 30px; border: 1px solid var(--border); border-radius: var(--radius-sm);
  background: transparent; color: var(--ink-mute); cursor: pointer;
  display: grid; place-items: center; transition: all 120ms;
  font-size: 14px;
}
.sp-pin:hover { background: var(--surface-2); color: var(--ink); }
.sp-pin.active { background: var(--brand); color: white; border-color: var(--brand); }
.sp-header {
  padding: 16px 22px 14px; border-bottom: 1px solid var(--border);
  display: flex; align-items: center; gap: 12px; flex-wrap: wrap;
}
.sp-header h2 { margin: 0; font-size: 16px; font-weight: 600; letter-spacing: -0.01em; flex: 1; min-width: 0; }
.sp-header .nm { font-family: 'Geist Mono', monospace; font-size: 11px; color: var(--ink-dim); width: 100%; }
.sp-close {
  width: 30px; height: 30px; border: 1px solid var(--border); border-radius: var(--radius-sm);
  background: transparent; color: var(--ink-mute); cursor: pointer;
  display: grid; place-items: center; transition: all 120ms;
}
.sp-close:hover { background: var(--surface-2); color: var(--ink); }
.sp-body { flex: 1; overflow: auto; padding: 22px 28px 32px; }
.sp-body h1, .sp-body h2, .sp-body h3 { font-weight: 600; letter-spacing: -0.015em; color: var(--ink); margin: 22px 0 10px; line-height: 1.3; }
.sp-body h1 { font-size: 22px; margin-top: 0; }
.sp-body h2 { font-size: 17px; }
.sp-body h3 { font-size: 14.5px; }
.sp-body p { color: var(--ink-mute); margin: 6px 0; line-height: 1.65; }
.sp-body ul, .sp-body ol { color: var(--ink-mute); padding-left: 22px; margin: 6px 0; }
.sp-body li { margin: 3px 0; }
.sp-body code { font-family: 'Geist Mono', monospace; font-size: 12px; background: var(--surface-2); padding: 1px 6px; border: 1px solid var(--border); border-radius: 3px; color: var(--ink); }
.sp-body pre { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 12px 14px; overflow: auto; }
.sp-body pre code { background: none; border: none; padding: 0; font-size: 12px; }
.sp-body a { color: var(--brand); text-decoration: none; }
.sp-body a:hover { text-decoration: underline; }
.sp-body strong { color: var(--ink); font-weight: 600; }
.sp-body hr { border: none; border-top: 1px solid var(--border); margin: 20px 0; }

/* Live monitor inside side-panel */
.lm-stats { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; margin-bottom: 14px; }
.lm-stats .one { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 10px 12px; }
.lm-stats .one .lk { font-size: 10px; color: var(--ink-dim); text-transform: uppercase; letter-spacing: 0.06em; font-weight: 500; }
.lm-stats .one .lv { font-family: 'Geist Mono', monospace; font-size: 18px; font-weight: 500; color: var(--ink); margin-top: 4px; line-height: 1.1; }
.event-stream { font-family: 'Geist Mono', monospace; font-size: 12px; max-height: 380px; overflow: auto; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 4px 0; }
.event-stream .ev { display: grid; grid-template-columns: 80px 160px 1fr; gap: 10px; padding: 6px 14px; border-bottom: 1px dashed var(--border); align-items: baseline; }
.event-stream .ev:last-child { border-bottom: none; }
.event-stream .ev:hover { background: var(--surface-3); }
.event-stream .ev .ts { color: var(--ink-dim); font-size: 11px; }
.event-stream .ev .ev-name { color: var(--brand); font-weight: 600; }
.event-stream .ev .pl { color: var(--ink-mute); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.empty-stream { padding: 32px 0; text-align: center; color: var(--ink-dim); font-size: 13px; font-family: 'Geist', sans-serif; }

/* Skeletons / empty / err */
.skel { background: linear-gradient(90deg, var(--surface), var(--surface-2), var(--surface)); background-size: 200% 100%; animation: shimmer 1.4s infinite; height: 14px; border-radius: 4px; }
@keyframes shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
.empty { text-align: center; padding: 36px 0; color: var(--ink-dim); font-size: 13px; }
.err { background: var(--danger-soft); color: var(--danger); border: 1px solid transparent; padding: 12px 16px; border-radius: var(--radius-sm); font-size: 13px; }

/* Toast */
.toast { position: fixed; bottom: 24px; right: 24px; z-index: 200; background: var(--ink); color: var(--bg); border-radius: var(--radius-md); padding: 10px 16px; font-size: 13px; font-weight: 500; box-shadow: var(--shadow-pop); opacity: 0; transform: translateY(8px); pointer-events: none; transition: all 200ms ease; }
.toast.show { opacity: 1; transform: translateY(0); }
.toast.ok { background: var(--success); color: white; }
.toast.err { background: var(--danger); color: white; }

/* Responsive */
@media (max-width: 1100px) {
  .kpi-grid, .kpi-grid.cols-3 { grid-template-columns: repeat(2, 1fr); }
  .prd-layout { grid-template-columns: 1fr; }
  .lm-stats { grid-template-columns: repeat(2, 1fr); }
  .topbar-inner, .panel { padding-left: 22px; padding-right: 22px; }
}
@media (max-width: 720px) {
  .app { padding-left: 0; }
  .rail {
    width: 264px;
    z-index: 50; transform: translateX(-100%); transition: transform 220ms cubic-bezier(0.2,0.8,0.2,1);
    border-right: 1px solid var(--border-2);
  }
  .rail.open { transform: translateX(0); }
  .rail-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.35); z-index: 40; opacity: 0; pointer-events: none; transition: opacity 200ms; }
  .rail-overlay.open { opacity: 1; pointer-events: auto; }
  .menu-btn { display: inline-flex; }
  .topbar-inner, .panel { padding-left: 16px; padding-right: 16px; }
  .kpi-grid, .kpi-grid.cols-3 { grid-template-columns: 1fr; }
  .prd-layout .row { grid-template-columns: 1fr; }
  .wave-row { grid-template-columns: 1fr; gap: 6px; }
  .wave-row .progress-mini { width: 100%; }
  .lm-stats { grid-template-columns: 1fr; }
  .event-stream .ev { grid-template-columns: 60px 1fr; }
  .event-stream .ev .pl { grid-column: 1 / -1; }
  .live-banner { padding-left: 16px; padding-right: 16px; }
}
`;

const ICONS = {
  refresh: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" stroke-linejoin="round" d="M3 12a9 9 0 0115.5-6.36L21 8M21 3v5h-5M21 12a9 9 0 01-15.5 6.36L3 16M3 21v-5h5"/></svg>',
  sun: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><circle cx="12" cy="12" r="4"/><path stroke-linecap="round" d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>',
  moon: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" stroke-linejoin="round" d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/></svg>',
  home: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" stroke-linejoin="round" d="M3 9l9-7 9 7v11a2 2 0 01-2 2h-4v-7H10v7H6a2 2 0 01-2-2z"/></svg>',
  doc: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" stroke-linejoin="round" d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path stroke-linecap="round" stroke-linejoin="round" d="M14 2v6h6M9 13h6M9 17h6"/></svg>',
  chart: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" d="M3 3v18h18M7 14l4-4 4 4 5-5"/></svg>',
  plus: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" d="M12 5v14M5 12h14"/></svg>',
  cog: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><circle cx="12" cy="12" r="3"/><path stroke-linecap="round" stroke-linejoin="round" d="M19.4 15a1.6 1.6 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.6 1.6 0 00-1.8-.3 1.6 1.6 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.6 1.6 0 00-1-1.5 1.6 1.6 0 00-1.8.3l-.1.1A2 2 0 114.4 17l.1-.1a1.6 1.6 0 00.3-1.8 1.6 1.6 0 00-1.5-1H3a2 2 0 110-4h.1a1.6 1.6 0 001.5-1 1.6 1.6 0 00-.3-1.8l-.1-.1A2 2 0 117 4.4l.1.1a1.6 1.6 0 001.8.3h.1a1.6 1.6 0 001-1.5V3a2 2 0 114 0v.1a1.6 1.6 0 001 1.5 1.6 1.6 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.6 1.6 0 00-.3 1.8v.1a1.6 1.6 0 001.5 1H21a2 2 0 110 4h-.1a1.6 1.6 0 00-1.5 1z"/></svg>',
  book: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" stroke-linejoin="round" d="M4 19.5A2.5 2.5 0 016.5 17H20M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/></svg>',
  terminal: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" stroke-linejoin="round" d="M4 17l6-6-6-6M12 19h8"/></svg>',
  menu: '<svg viewBox="0 0 24 24" fill="none" stroke-width="1.7"><path stroke-linecap="round" d="M4 6h16M4 12h16M4 18h16"/></svg>',
};

const CLIENT_JS = `
(function(){
  'use strict';
  var STATE = {
    tab: 'overview', specs: null, metrics: null, extra: null, projects: null, events: null,
    settings: null, dirtySettings: {}, panelTimer: null, liveTimer: null, lastLiveCheck: 0,
    specsPeriod: '30', commands: null, cmdFilter: 'all', cmdQuery: '', panelPinned: false,
  };
  var POLL_MS = 12000;
  var LIVE_POLL_MS = 8000;
  var pollTimer = null;
  var liveBgTimer = null;

  function $(sel, root){ return (root||document).querySelector(sel); }
  function $$(sel, root){ return Array.prototype.slice.call((root||document).querySelectorAll(sel)); }
  function el(tag, attrs, children){
    var n = document.createElement(tag);
    if (attrs) for (var k in attrs){
      if (k === 'class') n.className = attrs[k];
      else if (k === 'html') n.innerHTML = attrs[k];
      else if (k === 'text') n.textContent = attrs[k];
      else if (k.indexOf('on') === 0) n.addEventListener(k.slice(2), attrs[k]);
      else n.setAttribute(k, attrs[k]);
    }
    if (children) (Array.isArray(children) ? children : [children]).forEach(function(c){
      if (c == null) return;
      n.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    });
    return n;
  }
  function esc(s){ var d = document.createElement('div'); d.textContent = String(s == null ? '' : s); return d.innerHTML; }
  function fetchJson(url, opts){
    return fetch(url, opts).then(function(r){
      return r.json().then(function(j){ return { ok: r.ok, status: r.status, body: j }; });
    });
  }
  function timeAgo(iso){
    if (!iso) return '—';
    var t = Date.parse(iso); if (isNaN(t)) return '—';
    var s = Math.floor((Date.now() - t)/1000);
    if (s < 60) return s + 's atrás';
    if (s < 3600) return Math.floor(s/60) + ' min atrás';
    if (s < 86400) return Math.floor(s/3600) + ' h atrás';
    return Math.floor(s/86400) + ' d atrás';
  }
  function isLiveTs(iso, mins){
    if (!iso) return false;
    var t = Date.parse(iso); if (isNaN(t)) return false;
    return (Date.now() - t) < (mins || 5) * 60 * 1000;
  }
  function fmtNum(n){ return (n||0).toLocaleString('pt-BR'); }
  function fmtTokens(n){ if (!n) return '0'; if (n >= 1e6) return (n/1e6).toFixed(1)+'M'; if (n >= 1e3) return (n/1e3).toFixed(0)+'k'; return ''+n; }
  function fmtBytes(n){ if (!n) return '0'; if (n >= 1024*1024) return (n/(1024*1024)).toFixed(1)+'MB'; if (n >= 1024) return (n/1024).toFixed(1)+'KB'; return n+'B'; }
  function pad2(n){ return n < 10 ? '0' + n : '' + n; }
  function today(){ return new Date().toISOString().slice(0, 10); }
  function slugify(t){ return String(t||'').toLowerCase().normalize('NFD').replace(/[\\u0300-\\u036f]/g,'').replace(/[^a-z0-9]+/g,'-').replace(/^-+|-+$/g,'').replace(/-+/g,'-'); }

  function phaseClassFor(p){
    if (!p) return '';
    var k = String(p).toLowerCase();
    if (k.indexOf('analy') === 0) return 'ph-analyze';
    if (k.indexOf('plan') === 0) return 'ph-plan';
    if (k.indexOf('exec') === 0) return 'ph-execute';
    if (k.indexOf('qa') === 0) return 'ph-qa';
    if (k.indexOf('clos') === 0) return 'ph-close';
    return '';
  }
  function phaseColor(p){
    var k = String(p || '').toLowerCase();
    if (k.indexOf('analy') === 0) return 'var(--plum)';
    if (k.indexOf('plan') === 0) return 'var(--brand)';
    if (k.indexOf('exec') === 0) return 'var(--info)';
    if (k.indexOf('qa') === 0) return 'var(--success)';
    if (k.indexOf('clos') === 0) return 'var(--ink-dim)';
    return 'var(--ink-mute)';
  }
  function displayTitle(name){
    var n = String(name || ''); n = n.replace(/^\\d{4}-\\d{2}-\\d{2}-/, ''); n = n.split('/').pop(); n = n.replace(/-/g, ' ');
    return n.charAt(0).toUpperCase() + n.slice(1);
  }

  // ── Glossary ────────────────────────────────────────────────────
  var GLOSSARY = {
    // Acrônimos técnicos gerais
    'AC': 'Acceptance Criteria — critérios objetivos que provam que a spec entregou o que prometeu. Cada AC tem um comando executável que vira teste na fase QA.',
    'API': 'Application Programming Interface — superfície que um sistema expõe para outro consumir.',
    'CI': 'Continuous Integration — processo automatizado que roda build/lint/test a cada push. No Mustard, o pipeline local emula isso na fase CLOSE.',
    'CI/CD': 'Continuous Integration / Continuous Delivery — pipeline automatizado de build, teste e deploy.',
    'CLI': 'Command Line Interface — ferramenta operada por terminal.',
    'CRUD': 'Create, Read, Update, Delete — operações básicas de manipulação de dados.',
    'DI': 'Dependency Injection — padrão onde dependências são fornecidas externamente em vez de instanciadas internamente.',
    'DORA': 'DevOps Research & Assessment — métricas que medem performance de entrega (lead time, deploy frequency, change failure rate, MTTR).',
    'IDE': 'Integrated Development Environment — VS Code, JetBrains, etc.',
    'JSON': 'JavaScript Object Notation — formato de dados estruturados leve.',
    'JSONL': 'JSON Lines — um JSON por linha. Usado pelo harness log do Mustard.',
    'MTTR': 'Mean Time To Recovery — tempo médio para se recuperar de uma falha.',
    'MVC': 'Model-View-Controller — padrão arquitetural separando dados, apresentação e controle.',
    'MVVM': 'Model-View-ViewModel — variação do MVC comum em apps mobile/desktop.',
    'OOP': 'Object-Oriented Programming — paradigma baseado em classes/objetos.',
    'ORM': 'Object-Relational Mapping — mapeia tabelas SQL em objetos da linguagem (Drizzle, Prisma, EF Core).',
    'PR': 'Pull Request — proposta de mudança em um repositório, sujeita a review.',
    'PRD': 'Product Requirements Document — documento que descreve o que precisa ser construído. Vira spec depois de aprovado.',
    'QA': 'Quality Assurance — fase do pipeline Mustard que executa cada Acceptance Criteria como comando e bloqueia CLOSE se algum falhar.',
    'REST': 'Representational State Transfer — estilo arquitetural HTTP baseado em recursos.',
    'RTK': 'Rust Token Killer — wrapper que reescreve comandos CLI (git, ls, cargo) para versões compactas, economizando 60-90% de tokens.',
    'SDK': 'Software Development Kit — biblioteca que abstrai uma API.',
    'SOLID': 'Cinco princípios de OOP: Single responsibility, Open/closed, Liskov, Interface segregation, Dependency inversion.',
    'SQL': 'Structured Query Language — linguagem padrão de banco relacional.',
    'SRP': 'Single Responsibility Principle — uma classe/módulo deve ter uma única razão para mudar.',
    'SVG': 'Scalable Vector Graphics — formato de imagem vetorial.',
    'YAML': 'Yet Another Markup Language — formato de config legível.',
    'SSE': 'Server-Sent Events — push de eventos do servidor para o browser via HTTP de longa duração.',
    'TDD': 'Test-Driven Development — escreve teste primeiro, depois código que faz passar.',
    'KPI': 'Key Performance Indicator — métrica chave para acompanhar saúde de um sistema.',
    'L0': 'Level 0 — regra de delegação universal do Mustard: o contexto principal só coordena, todo trabalho de código vai via Task.',
    'PID': 'Process ID — identificador numérico de um processo do sistema operacional.',

    // Conceitos do Mustard
    'spec': 'Documento .md em .claude/spec/ que descreve uma mudança a ser feita. O pipeline Mustard executa cada spec em fases.',
    'spec.md': 'Arquivo individual de spec — fica em .claude/spec/active/<date>-<slug>/spec.md durante o trabalho e é movido para completed/ no CLOSE.',
    'wave': 'Subdivisão de uma spec grande. Quando o escopo é "full" e tem 3+ camadas, o spec vira um epic com waves numeradas.',
    'wave-plan.md': 'Documento mestre de um epic que lista as waves filhas e suas dependências. Fica em .claude/spec/active/<epic>/wave-plan.md.',
    'epic': 'Spec grande dividida em waves. O wave-plan.md descreve as waves; cada wave tem seu próprio spec.md.',
    'hook': 'Script JS em .claude/hooks/ que roda em eventos (PreToolUse, PostToolUse, etc) para validar, transformar ou registrar.',
    'gate': 'Hook que bloqueia uma ação se condições não são atendidas (ex: close-gate, qa-gate, model-routing-gate).',
    'agent': 'Subprocess de IA despachado via Task tool com escopo isolado (Explore, Plan, general-purpose, Bash).',
    'pipeline': 'Sequência de fases (ANALYZE → PLAN → EXECUTE → QA → CLOSE) que o Mustard executa para entregar uma spec.',
    'pipeline-state': 'JSON em .claude/.pipeline-states/<spec>.metrics.json com fase atual, apiCalls, retries, tool breakdown e timestamps de uma spec.',
    'harness': 'Sistema de logging do Mustard. Eventos JSONL em .claude/.harness/events.jsonl gravados pelos hooks.',
    'knowledge': 'Base de conhecimento em .claude/knowledge.json com padrões/convenções/decisões aprendidas das specs.',
    'knowledge.json': 'Arquivo da knowledge base do projeto. Atualizado por session-knowledge hook.',
    'registry': 'entity-registry.json — mapa de entidades (tables, classes, modelos) detectadas no projeto pelo sync-registry.',
    'monorepo': 'Repositório que contém vários subprojetos. Mustard detecta automaticamente via .detect-cache.json.',
    'subproject': 'Pasta dentro de um monorepo com seu próprio stack (ex: backend Node + frontend React em pastas diferentes).',
    'recipe': 'Skeleton estruturado em .claude/recipes/<operation>.json — template 90% pronto para uma operação comum (add-field, add-endpoint, etc).',
    'skill': 'Pacote de instrução em SKILL.md com YAML frontmatter. Carregado automaticamente pelo Claude quando a description bate com o trigger.',
    'progressive disclosure': 'Padrão de skill onde o body fica curto (≤200 linhas) e detalhes vão para arquivos refs/ carregados sob demanda.',
    'fail-open': 'Política do Mustard: se um hook crasha, ele sai com exit 0 (sucesso) para não bloquear o user. Erros são logados, não fatais.',
    'budget': 'Limite de tamanho do prompt despachado para um agente. Cada role (Explore, Plan, general) tem seu budget calibrado.',
    'pass@1': 'Métrica: % de pipelines que terminaram sem nenhum retry. Quanto mais alto, mais "first time right" o seu pipeline está.',

    // Fases do pipeline
    'ANALYZE': 'Primeira fase do pipeline — exploração mecânica do código relevante para entender o contexto.',
    'PLAN': 'Fase de planejamento — define spec, boundaries, AC e checklist antes de mexer em código.',
    'EXECUTE': 'Fase de implementação — agentes editam código seguindo a spec aprovada.',
    'CLOSE': 'Fase final — roda build/lint/test e arquiva a spec em completed/.',

    // Comandos slash do Mustard
    '/mustard:feature': 'Inicia o pipeline para nova feature/enhancement (ANALYZE → PLAN → /approve → EXECUTE → QA → CLOSE).',
    '/mustard:bugfix': 'Pipeline focado em correção de defeitos com test-first reproduce.',
    '/mustard:approve': 'Aprova a spec atual e libera transição PLAN → EXECUTE.',
    '/mustard:complete': 'Finaliza o pipeline ativo: roda close-gate e arquiva spec em completed/.',
    '/mustard:resume': 'Retoma um pipeline interrompido (após erro de dispatch ou timeout).',
    '/mustard:qa': 'Executa todos os Acceptance Criteria da spec ativa e reporta pass/fail.',
    '/mustard:status': 'Status consolidado: git, pipeline, build, entity-registry.',
    '/mustard:stats': 'Estatísticas detalhadas: pipelines + hooks + RTK savings.',
    '/mustard:metrics': 'Métricas focadas em hook events com filtros --since/--event/--compare/--pr.',
    '/mustard:knowledge': 'Gerencia knowledge base do projeto (list/search/audit/add).',
    '/mustard:task': 'Despacha agente pontual sem spec. Ideal para análise/spike/refactor pequeno.',
    '/mustard:scan': 'Análise agnóstica de código (clusters, convenções, problemas).',
    '/mustard:scan-format': 'Regras de formatação para output do /mustard:scan.',
    '/mustard:review': 'Code review completo de um PR (lê diff, comenta, prioriza findings).',
    '/mustard:skill': 'Gerencia skills (list/validate/new/edit) com checks de YAML/size.',
    '/mustard:git': 'Wrapper de git seguindo flow do mustard.json (sync/commit/push/merge).',
    '/mustard:dashboard': 'Inicia/para/checa o servidor local do dashboard (esta UI).',
    '/mustard:maint': 'Utilitários: limpa órfãos, valida estado, recompila registry.',
  };

  function applyGlossary(rootSel){
    var roots = $$(rootSel || '.gloss-target, .help-line, .desc, .summary, .epic-summary, .gd, .meta-row');
    if (!roots.length) return;
    var keys = Object.keys(GLOSSARY).sort(function(a,b){ return b.length - a.length; });
    var rx = new RegExp('\\\\b(' + keys.map(function(k){ return k.replace(/[/]/g,'\\\\/'); }).join('|') + ')\\\\b', 'g');
    roots.forEach(function(node){
      walkText(node, function(t){
        if (!t.nodeValue || !rx.test(t.nodeValue)) return;
        rx.lastIndex = 0;
        var html = t.nodeValue.replace(rx, function(m){
          return '<abbr class="gloss" title="' + (GLOSSARY[m] || '').replace(/"/g,'&quot;') + '">' + m + '</abbr>';
        });
        var span = document.createElement('span');
        span.innerHTML = html;
        t.parentNode.replaceChild(span, t);
      });
    });
  }
  function walkText(node, fn){
    if (node.nodeType === 3) { fn(node); return; }
    if (node.nodeType !== 1) return;
    if (/^(SCRIPT|STYLE|CODE|PRE|ABBR)$/.test(node.tagName)) return;
    var c = node.firstChild;
    while (c) { var next = c.nextSibling; walkText(c, fn); c = next; }
  }

  // ── Theme & toast ───────────────────────────────────────────────
  function initTheme(){
    var saved = localStorage.getItem('mustard.theme');
    var sysDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    var theme = saved || (sysDark ? 'dark' : 'light');
    document.documentElement.setAttribute('data-theme', theme);
    syncThemeBtn();
  }
  function toggleTheme(){
    var cur = document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light';
    var next = cur === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    localStorage.setItem('mustard.theme', next);
    syncThemeBtn();
  }
  function syncThemeBtn(){
    var btn = $('#theme-btn'); if (!btn) return;
    var isDark = document.documentElement.getAttribute('data-theme') === 'dark';
    btn.innerHTML = (isDark ? window.MICONS.sun : window.MICONS.moon);
    btn.title = isDark ? 'Modo claro' : 'Modo escuro';
  }
  function toast(msg, kind){
    var t = $('#toast'); t.textContent = msg;
    t.className = 'toast show' + (kind ? ' ' + kind : '');
    setTimeout(function(){ t.className = 'toast'; }, 1800);
  }

  // ── Mobile sidebar drawer ──────────────────────────────────────
  function toggleRail(){ $('.rail').classList.toggle('open'); $('#rail-overlay').classList.toggle('open'); }
  function closeRail(){ $('.rail').classList.remove('open'); $('#rail-overlay').classList.remove('open'); }

  // ── Tabs ────────────────────────────────────────────────────────
  var TAB_TITLES = { overview:'Overview', specs:'Specs', telemetry:'Telemetry', compose:'Compose PRD', settings:'Settings', glossary:'Glossário', commands:'Comandos' };
  var TAB_CRUMBS = {
    overview: 'Visão geral · poll <b>12s</b>',
    specs: 'Active &amp; completed',
    telemetry: 'Hooks · pipelines · storage',
    compose: 'Gerador de PRD · padrão Mustard',
    settings: 'Mustard env · grava em <code>.claude/settings.json</code>',
    glossary: 'Termos e siglas usadas no Mustard',
    commands: 'Todos os <code>/mustard:*</code> com explicação leiga e técnica',
  };
  function setTab(name){
    STATE.tab = name;
    $$('[data-tab]').forEach(function(a){ a.classList.toggle('on', a.dataset.tab === name); });
    $$('.panel').forEach(function(p){ p.classList.toggle('on', p.id === 'panel-' + name); });
    var titleEl = $('#tab-title'); if (titleEl) titleEl.textContent = TAB_TITLES[name] || name;
    var crumbEl = $('#tab-crumb'); if (crumbEl) crumbEl.innerHTML = TAB_CRUMBS[name] || '';
    closeRail();
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    if (name === 'overview') { loadOverview(); pollTimer = setInterval(loadOverview, POLL_MS); }
    else if (name === 'specs') { loadSpecs(); }
    else if (name === 'telemetry') { loadMetrics(); pollTimer = setInterval(loadMetrics, POLL_MS); }
    else if (name === 'compose') { loadProjects(); }
    else if (name === 'settings') { loadSettings(); }
    else if (name === 'glossary') { renderGlossary(); }
    else if (name === 'commands') { loadCommands(); }
  }

  // ── Live banner (background poll) ──────────────────────────────
  function startLiveBgPoll(){
    if (liveBgTimer) clearInterval(liveBgTimer);
    checkLiveBanner();
    liveBgTimer = setInterval(checkLiveBanner, LIVE_POLL_MS);
  }
  function checkLiveBanner(){
    fetchJson('/api/telemetry-extra').then(function(r){
      var ex = r.body || {};
      STATE.extra = ex;
      var live = (ex.activeNow || []);
      var banner = $('#live-banner');
      if (!banner) return;
      if (!live.length) { banner.setAttribute('hidden', ''); return; }
      var first = live[0];
      var label = '<span class="live-dot"></span><span class="summary">Processando: <b>' + esc(displayTitle(first.spec)) + '</b>'
        + (first.wave ? ' · wave <b>' + esc(first.wave.replace(/^wave-?/, '')) + '</b>' : '')
        + ' · última atividade ' + esc(timeAgo(first.lastActivity)) + '</span>'
        + '<button class="btn live" data-live="' + esc(first.spec + (first.wave ? '/' + first.wave : '')) + '">Acompanhar</button>';
      banner.innerHTML = label;
      banner.removeAttribute('hidden');
    }).catch(function(){});
  }

  // ── Overview ─────────────────────────────────────────────────────
  function loadOverview(){
    var pane = $('#panel-overview .mount');
    if (!STATE.specs) pane.innerHTML = skelKpis() + '<div class="h-section">Em produção</div><div class="skel" style="height:180px;border-radius:8px;"></div>';
    Promise.all([fetchJson('/api/specs'), fetchJson('/api/metrics'), fetchJson('/api/events?n=200'), fetchJson('/api/telemetry-extra')])
      .then(function(rs){
        STATE.specs = (rs[0].body.specs) || [];
        STATE.metrics = rs[1].body || {};
        STATE.events = (rs[2].body.events) || [];
        STATE.extra = rs[3].body || {};
        renderOverview();
        applyGlossary();
      })
      .catch(function(e){ pane.innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }

  function skelKpis(){ return '<div class="kpi-grid">' + Array(4).fill(0).map(function(){ return '<div class="kpi"><div class="skel" style="width:40%"></div><div class="skel" style="width:60%;height:30px;margin-top:6px"></div><div class="skel" style="width:50%;height:10px;margin-top:6px"></div></div>'; }).join('') + '</div>'; }

  function renderOverview(){
    var specs = STATE.specs, m = STATE.metrics, evs = STATE.events, ex = STATE.extra || {};
    var actives = specs.filter(function(s){ return s.state === 'active'; });
    var completed = specs.filter(function(s){ return s.state === 'completed'; });
    var rtkTokens = (m.rtkSavings && m.rtkSavings.tokens) || 0;
    var hookEvents = (m.hookEvents || []).reduce(function(a,h){ return a + (h.count||0); }, 0);
    var savedSum = (m.hookEvents || []).reduce(function(a,h){ return a + (h.tokensSaved||0); }, 0) + rtkTokens;
    var todayStr = today();
    var todayCount = (m.last7Days || []).filter(function(d){ return d.day === todayStr; }).reduce(function(a,d){ return a + d.events; }, 0);
    var liveCount = (ex.activeNow || []).length;

    var kpis = '<div class="kpi-grid">'
      + kpi('Specs ativas', actives.length, liveCount ? liveCount + ' processando agora' : (actives[0] ? actives[0].phase || 'sem fase' : 'nenhuma'), liveCount ? 'ok' : 'dim')
      + kpi('Concluídas', completed.length, 'no histórico', 'ok')
      + kpi('Tokens poupados', fmtTokens(savedSum), 'RTK + hooks', 'ok')
      + kpi('Eventos hoje', fmtNum(todayCount), 'de ' + fmtNum(hookEvents) + ' totais', 'dim')
      + '</div>';

    var feature = actives[0];
    var html = kpis;
    html += '<div class="h-section">Em produção</div>';
    if (feature) html += renderSpecCard(feature);
    else html += '<div class="empty">Nenhuma spec ativa.</div>';

    html += '<div class="h-section">Atividade recente</div>';
    if (evs.length) html += renderEventsList(evs.slice(-12).reverse());
    else html += '<div class="empty">Sem eventos no harness log.</div>';

    $('#panel-overview .mount').innerHTML = html;
  }
  function kpi(label, val, delta, kind){
    return '<div class="kpi">'
      + '<div class="label">' + esc(label) + '</div>'
      + '<div class="val">' + esc(val) + '</div>'
      + '<div class="delta ' + (kind || '') + '"><span class="dot"></span>' + esc(delta || '') + '</div>'
      + '</div>';
  }
  function renderEventsList(events){
    return '<div class="card">' + events.map(function(e){
      var when = timeAgo(e.ts || e.timestamp);
      var ev = e.event || e.type || '—';
      var label = (e.payload && (e.payload.spec || e.payload.event || e.payload.command)) || '';
      return '<div class="idx-row">'
        + '<div class="nm">' + esc(ev) + '</div>'
        + '<div class="meta">' + esc(String(label).slice(0, 80)) + '</div>'
        + '<div class="stat">' + esc(when) + '</div>'
        + '</div>';
    }).join('') + '</div>';
  }

  // ── Specs ───────────────────────────────────────────────────────
  function loadSpecs(){
    var mount = $('#panel-specs .mount');
    if (!STATE.specs) mount.innerHTML = '<div class="skel" style="height:180px;border-radius:8px;margin-bottom:12px"></div><div class="skel" style="height:180px;border-radius:8px"></div>';
    fetchJson('/api/specs').then(function(r){ STATE.specs = r.body.specs || []; renderSpecs(); applyGlossary(); })
      .catch(function(e){ mount.innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }
  function renderSpecs(){
    var specs = STATE.specs;
    var actives = specs.filter(function(s){ return s.state === 'active'; });
    var completed = filterByPeriod(specs.filter(function(s){ return s.state === 'completed'; }), STATE.specsPeriod);

    var filterBar = '<div class="filter-bar">'
      + '<span class="label">Concluídas no período:</span>'
      + ['7','15','30','60','90','all'].map(function(p){
          var on = STATE.specsPeriod === p; var lbl = p === 'all' ? 'tudo' : p + 'd';
          return '<button class="chip' + (on ? ' on' : '') + '" data-period="' + p + '">' + lbl + '</button>';
        }).join('')
      + '</div>';

    var html = '';
    html += '<div class="h-section">Active · ' + actives.length + '</div>';
    if (!actives.length) html += '<div class="empty">Nenhuma spec ativa.</div>';
    else html += actives.map(renderSpecCard).join('');

    html += '<div class="h-section">Completed · ' + completed.length + '</div>';
    html += filterBar;
    if (!completed.length) html += '<div class="empty">Nenhuma spec no período selecionado.</div>';
    else html += renderCompletedIndex(completed);

    $('#panel-specs .mount').innerHTML = html;

    $$('#panel-specs .chip[data-period]').forEach(function(b){
      b.addEventListener('click', function(){ STATE.specsPeriod = b.dataset.period; renderSpecs(); applyGlossary(); });
    });
  }
  function filterByPeriod(items, period){
    if (period === 'all') return items;
    var days = parseInt(period, 10) || 30;
    var cutoff = Date.now() - days * 86400000;
    return items.filter(function(s){
      var t = Date.parse(s.checkpoint || s.name.slice(0,10));
      if (isNaN(t)) return true;
      return t >= cutoff;
    });
  }

  function renderSpecCard(s){
    if (s.isEpic && s.waves && s.waves.length) return renderEpicCard(s);
    return renderSingleCard(s);
  }
  function specStamps(s){
    var phaseTag = s.phase ? '<span class="tag ' + phaseClassFor(s.phase) + '">' + esc(s.phase) + '</span>' : '';
    var scopeTag = s.scope ? '<span class="tag">' + esc(s.scope) + '</span>' : '';
    var waveTag = '';
    if (s.currentWave && s.wave) waveTag = '<span class="tag brand">wave ' + esc(s.currentWave) + '/' + esc(String(s.wave).split('/')[1] || s.wave) + '</span>';
    else if (s.currentWave) waveTag = '<span class="tag brand">wave ' + esc(s.currentWave) + '</span>';
    return phaseTag + scopeTag + waveTag;
  }
  function renderSingleCard(s){
    var c = s.checklist || { total:0, done:0, percent:0, items:[] };
    var live = isLiveTs(s.lastActivity);
    var clId = 'cl-' + Math.random().toString(36).slice(2,8);
    var ttl = displayTitle(s.name);
    var summary = esc(s.summary || '');
    var meta = '<div class="meta-row">'
      + '<span><b>' + esc(timeAgo(s.lastActivity)) + '</b></span>'
      + (s.apiCalls != null ? '<span>API calls: <b>' + fmtNum(s.apiCalls) + '</b></span>' : '')
      + (s.retries != null ? '<span>Retries: <b>' + fmtNum(s.retries) + '</b></span>' : '')
      + (s.checkpoint ? '<span>Checkpoint: <b>' + esc(s.checkpoint) + '</b></span>' : '')
      + '</div>';
    var actions = '<div class="actions">'
      + (s.state === 'active' ? '<button class="btn live" data-live="' + esc(s.name) + '"><span class="live-dot" style="margin-right:2px"></span>Acompanhar</button>' : '')
      + (c.items && c.items.length ? '<button class="btn ghost" data-toggle="' + clId + '">Checklist (' + c.total + ')</button>' : '')
      + '<button class="btn ghost" data-open="' + esc(s.path) + '">Ver spec.md</button>'
      + '</div>';
    var checklist = '';
    if (c.items && c.items.length) {
      checklist = '<div class="checklist" id="' + clId + '" hidden>'
        + c.items.map(function(it){
            var t = it.text || ''; var pfx = '';
            var pm = t.match(/^\\[([^\\]]+)\\]\\s*(.*)/);
            if (pm) { pfx = '<span class="pfx">' + esc(pm[1]) + '</span>'; t = pm[2]; }
            return '<div class="item ' + (it.done ? 'done' : '') + '"><div class="mark">' + (it.done ? '✓' : '○') + '</div><div class="text">' + pfx + esc(t) + '</div></div>';
          }).join('')
        + '</div>';
    }
    return '<div class="spec-card' + (live ? ' live' : '') + '">'
      + '<div class="head">'
        + (live ? '<span class="live-pill"><span class="live-dot"></span>ao vivo</span>' : '')
        + specStamps(s)
        + '<div class="ttl">' + esc(ttl) + '</div>'
      + '</div>'
      + '<div class="nm">' + esc(s.name) + '</div>'
      + (summary ? '<div class="summary">' + summary + '</div>' : '')
      + '<div class="progress"><div class="pct">' + (c.percent||0) + '%</div><div class="track"><div class="fill" style="width:' + (c.percent||0) + '%"></div></div><div class="frac">' + c.done + '/' + c.total + '</div></div>'
      + meta + actions + checklist
      + '</div>';
  }
  function renderEpicCard(s){
    var c = s.checklist || { total:0, done:0, percent:0 };
    var anyLive = (s.waves || []).some(function(w){ return isLiveTs(w.lastActivity); }) || isLiveTs(s.lastActivity);
    var ttl = displayTitle(s.name);
    var summary = esc(s.summary || '');
    var meta = '<div class="meta-row">'
      + '<span><b>' + esc(timeAgo(s.lastActivity)) + '</b></span>'
      + (s.apiCalls != null ? '<span>API calls: <b>' + fmtNum(s.apiCalls) + '</b></span>' : '')
      + (s.retries != null ? '<span>Retries: <b>' + fmtNum(s.retries) + '</b></span>' : '')
      + '<span>Checkpoint: <b>' + esc(s.checkpoint || '—') + '</b></span>'
      + '</div>';
    var actions = '<div class="actions">'
      + (anyLive ? '<button class="btn live" data-live="' + esc(s.name) + '"><span class="live-dot" style="margin-right:2px"></span>Acompanhar epic</button>' : '')
      + '<button class="btn ghost" data-open="' + esc(s.path) + '">Ver wave-plan.md</button>'
      + '</div>';
    var head = '<div class="head">'
      + (anyLive ? '<span class="live-pill"><span class="live-dot"></span>ao vivo</span>' : '')
      + specStamps(s)
      + '<span class="tag info">epic · ' + s.waves.length + ' waves</span>'
      + '<div class="ttl">' + esc(ttl) + '</div>'
      + '</div>';
    var wavesHtml = (s.waves || []).map(function(w, i){ return renderWaveRow(w, i+1, s.name); }).join('');
    return '<div class="spec-card epic-card' + (anyLive ? ' live' : '') + '">'
      + head
      + '<div class="nm">' + esc(s.name) + '</div>'
      + (summary ? '<div class="epic-summary">' + summary + '</div>' : '')
      + '<div class="epic-progress-line">'
        + '<span class="lbl">progresso geral</span>'
        + '<div class="progress" style="flex:1;display:grid;grid-template-columns:auto 1fr auto;gap:10px;align-items:center;">'
          + '<div class="pct">' + (c.percent||0) + '%</div>'
          + '<div class="track" style="height:6px;background:var(--surface-2);border-radius:3px;position:relative;overflow:hidden;"><div class="fill" style="position:absolute;left:0;top:0;bottom:0;background:var(--brand);width:' + (c.percent||0) + '%;border-radius:3px"></div></div>'
          + '<div class="frac">' + c.done + '/' + c.total + '</div>'
        + '</div>'
      + '</div>'
      + meta + actions
      + '<div class="waves-list">' + wavesHtml + '</div>'
      + '</div>';
  }
  function renderWaveRow(w, idx, parentName){
    var c = w.checklist || { total:0, done:0, percent:0 };
    var live = isLiveTs(w.lastActivity);
    var phaseTag = w.phase ? '<span class="tag ' + phaseClassFor(w.phase) + '">' + esc(w.phase) + '</span>' : '';
    var liveTag = live ? '<span class="live-pill"><span class="live-dot"></span>live</span>' : '';
    var fullPath = parentName + '/' + w.name;
    return '<div class="wave-row' + (live ? ' live' : '') + '" data-live="' + esc(fullPath) + '">'
      + '<div class="ix">' + pad2(idx) + '</div>'
      + '<div class="name">' + liveTag + phaseTag + '<span class="lbl">' + esc(w.name) + '</span></div>'
      + '<div class="frac">' + (c.percent||0) + '%</div>'
      + '<div class="progress-mini"><div class="fill" style="width:' + (c.percent||0) + '%"></div></div>'
      + '<div class="frac">' + c.done + '/' + c.total + '</div>'
      + '<div class="stamp">' + esc(timeAgo(w.lastActivity)) + '</div>'
      + '</div>';
  }
  function renderCompletedIndex(items){
    items = items.slice().sort(function(a, b){ return (b.checkpoint || b.name).localeCompare(a.checkpoint || a.name); });
    var byMonth = {};
    items.forEach(function(s){ var k = (s.checkpoint || s.name || '').slice(0, 7) || 'unknown'; (byMonth[k] = byMonth[k] || []).push(s); });
    var months = ['Jan','Fev','Mar','Abr','Mai','Jun','Jul','Ago','Set','Out','Nov','Dez'];
    function monthLabel(yyyymm){ if (!/^\\d{4}-\\d{2}$/.test(yyyymm)) return yyyymm; var p = yyyymm.split('-'); return months[parseInt(p[1],10)-1] + ' ' + p[0]; }
    var out = '';
    Object.keys(byMonth).sort().reverse().forEach(function(month){
      out += '<div class="idx-month">' + esc(monthLabel(month)) + ' · ' + byMonth[month].length + '</div>';
      out += '<div class="card" style="padding:6px;margin-bottom:6px;">';
      byMonth[month].forEach(function(s){
        var c = s.checklist || {};
        out += '<div class="idx-row" data-open="' + esc(s.path) + '">'
          + '<div class="nm">' + esc(s.name) + '</div>'
          + '<div class="meta">' + esc(s.scope || '—') + '</div>'
          + '<div class="stat">' + (c.total ? c.done + '/' + c.total : '—') + '</div>'
          + '</div>';
      });
      out += '</div>';
    });
    return out;
  }

  // ── Side panel (slide from right) ──────────────────────────────
  function openPanel(opts){
    if (STATE.panelTimer) { clearInterval(STATE.panelTimer); STATE.panelTimer = null; }
    $('#sp-title').textContent = opts.title || '';
    $('#sp-name').textContent = opts.subtitle || '';
    $('#sp-body').innerHTML = opts.body || '';
    $('#side-panel').classList.add('open');
    $('#side-overlay').classList.add('open');
    applyPinState();
    if (opts.poll) {
      var fn = opts.poll;
      fn();
      STATE.panelTimer = setInterval(fn, 3000);
    }
  }
  function closePanel(force){
    if (STATE.panelPinned && !force) return;
    $('#side-panel').classList.remove('open');
    $('#side-overlay').classList.remove('open');
    document.body.classList.remove('panel-pinned');
    if (STATE.panelTimer) { clearInterval(STATE.panelTimer); STATE.panelTimer = null; }
  }
  function togglePin(){
    STATE.panelPinned = !STATE.panelPinned;
    applyPinState();
  }
  // ── Side panel resize (drag handle on left edge) ───────────────
  var PANEL_W_KEY = 'mustard-dashboard-panel-w';
  var PANEL_MIN = 360;
  function panelMaxPx(){ return Math.floor(window.innerWidth * 0.92); }
  function setPanelWidth(px){
    var min = PANEL_MIN, max = panelMaxPx();
    var w = Math.max(min, Math.min(max, px));
    document.documentElement.style.setProperty('--side-panel-w', w + 'px');
    return w;
  }
  function restorePanelWidth(){
    try {
      var saved = parseInt(localStorage.getItem(PANEL_W_KEY), 10);
      if (saved && !isNaN(saved)) setPanelWidth(saved);
    } catch (e) { /* ignore */ }
  }
  function bindPanelResize(){
    var handle = $('#sp-resize');
    if (!handle) return;
    var dragging = false;
    handle.addEventListener('mousedown', function(e){
      e.preventDefault();
      dragging = true;
      handle.classList.add('dragging');
      document.body.classList.add('resizing-panel');
    });
    document.addEventListener('mousemove', function(e){
      if (!dragging) return;
      var w = window.innerWidth - e.clientX;
      setPanelWidth(w);
    });
    document.addEventListener('mouseup', function(){
      if (!dragging) return;
      dragging = false;
      handle.classList.remove('dragging');
      document.body.classList.remove('resizing-panel');
      // Persist current width
      var cur = getComputedStyle(document.documentElement).getPropertyValue('--side-panel-w').trim();
      var px = parseInt(cur, 10);
      if (px && !isNaN(px)) {
        try { localStorage.setItem(PANEL_W_KEY, String(px)); } catch (e) { /* ignore */ }
      }
    });
    // Re-clamp when window resizes (so panel doesn't overflow)
    window.addEventListener('resize', function(){
      var cur = parseInt(getComputedStyle(document.documentElement).getPropertyValue('--side-panel-w'), 10);
      if (cur && !isNaN(cur)) setPanelWidth(cur);
    });
  }
  function applyPinState(){
    var sp = $('#side-panel'), so = $('#side-overlay'), btn = $('#sp-pin');
    if (!sp) return;
    var pinned = !!STATE.panelPinned;
    var open = sp.classList.contains('open');
    sp.classList.toggle('pinned', pinned);
    if (so) so.classList.toggle('pinned', pinned);
    document.body.classList.toggle('panel-pinned', pinned && open);
    if (btn) {
      btn.classList.toggle('active', pinned);
      btn.title = pinned
        ? 'Despinar painel (volta a fechar quando clicar fora)'
        : 'Pinar painel (mantém aberto e troca conteúdo ao clicar)';
    }
  }

  // Spec markdown viewer (uses side panel)
  function openSpec(specPath){
    openPanel({ title: 'Spec', subtitle: specPath, body: '<div class="skel" style="width:60%;height:24px"></div><div class="skel" style="width:100%;height:14px;margin-top:14px"></div>' });
    fetchJson('/api/spec?path=' + encodeURIComponent(specPath))
      .then(function(r){
        var name = specPath.split('/').slice(-2).join('/');
        $('#sp-title').textContent = displayTitle(name);
        $('#sp-name').textContent = specPath;
        $('#sp-body').innerHTML = renderMarkdown(r.body.markdown || '');
        applyGlossary('#sp-body');
      })
      .catch(function(e){ $('#sp-body').innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }

  // Live monitor (uses side panel, polls 3s)
  function openLiveMonitor(specName){
    openPanel({
      title: displayTitle(specName), subtitle: specName,
      body: '<div class="skel" style="height:60px;border-radius:8px"></div><div class="skel" style="height:200px;border-radius:8px;margin-top:14px"></div>',
      poll: function(){ pollLive(specName); },
    });
  }
  function pollLive(specName){
    fetchJson('/api/spec/live?spec=' + encodeURIComponent(specName))
      .then(function(r){ renderLive(specName, r.body); })
      .catch(function(e){ $('#sp-body').innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }
  function renderLive(specName, data){
    var liveTag = data.isLive ? '<span class="live-pill"><span class="live-dot"></span>ao vivo · poll 3s</span>' : '<span class="tag">inativa</span>';
    var phaseTag = data.phase ? '<span class="tag ' + phaseClassFor(data.phase) + '">' + esc(data.phase) + '</span>' : '';
    var html = '';
    html += '<div style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:14px;">' + liveTag + phaseTag + '</div>';

    if (data.summary) {
      html += '<p class="summary" style="margin-top:0;font-size:14px;color:var(--ink-mute);line-height:1.65;">' + esc(data.summary).slice(0, 600) + (data.summary.length > 600 ? '…' : '') + '</p>';
    }

    html += '<div class="lm-stats">'
      + statBox('Última atividade', timeAgo(data.lastActivity))
      + statBox('Fase atual', data.phase || '—')
      + statBox('API calls', data.apiCalls != null ? fmtNum(data.apiCalls) : '—')
      + statBox('Retries', data.retries != null ? fmtNum(data.retries) : '—')
      + '</div>';

    if (data.checklist && data.checklist.items && data.checklist.items.length) {
      html += '<div class="h-section" style="margin-top:14px;">Checklist · ' + data.checklist.done + '/' + data.checklist.total + ' (' + data.checklist.percent + '%)</div>';
      html += '<div class="checklist" style="display:block">'
        + data.checklist.items.map(function(it){
            var t = it.text || ''; var pfx = '';
            var pm = t.match(/^\\[([^\\]]+)\\]\\s*(.*)/);
            if (pm) { pfx = '<span class="pfx">' + esc(pm[1]) + '</span>'; t = pm[2]; }
            return '<div class="item ' + (it.done ? 'done' : '') + '"><div class="mark">' + (it.done ? '✓' : '○') + '</div><div class="text">' + pfx + esc(t) + '</div></div>';
          }).join('')
        + '</div>';
    }

    if (data.agentAttempts && Object.keys(data.agentAttempts).length) {
      html += '<div class="h-section" style="margin-top:14px;">Tentativas por fase</div>';
      html += '<div class="lm-stats">' + Object.keys(data.agentAttempts).map(function(k){ return statBox(k, fmtNum(data.agentAttempts[k])); }).join('') + '</div>';
    }
    if (data.toolBreakdown && Object.keys(data.toolBreakdown).length) {
      html += '<div class="h-section" style="margin-top:14px;">Uso de tools</div>';
      html += '<div class="lm-stats">' + Object.keys(data.toolBreakdown).map(function(k){ return statBox(k, fmtNum(data.toolBreakdown[k])); }).join('') + '</div>';
    }

    html += '<div class="h-section" style="margin-top:14px;">Stream de eventos · ' + (data.events ? data.events.length : 0) + '</div>';
    if (!data.events || !data.events.length) {
      html += '<div class="empty-stream">Sem eventos registrados para esta spec ainda. Os eventos aparecem quando algum agente despacha tool calls vinculados a este nome.</div>';
    } else {
      html += '<div class="event-stream">' + data.events.slice().reverse().map(function(ev){
        var ts = (ev.ts || ev.timestamp || '').slice(11, 19);
        var name = ev.event || ev.type || '—';
        var pl = '';
        if (ev.payload) {
          if (typeof ev.payload === 'string') pl = ev.payload;
          else if (ev.payload.command) pl = ev.payload.command;
          else if (ev.payload.event) pl = ev.payload.event;
          else if (ev.payload.message) pl = ev.payload.message;
          else pl = JSON.stringify(ev.payload).slice(0, 200);
        }
        return '<div class="ev"><span class="ts">' + esc(ts) + '</span><span class="ev-name">' + esc(name) + '</span><span class="pl">' + esc(pl) + '</span></div>';
      }).join('') + '</div>';
    }

    if (data.specPath) {
      html += '<div style="margin-top:18px;display:flex;gap:8px;flex-wrap:wrap;">'
        + '<button class="btn ghost" data-open="' + esc(data.specPath) + '">Ver spec.md completa</button>'
        + '</div>';
    }

    $('#sp-body').innerHTML = html;
    applyGlossary('#sp-body');
  }
  function statBox(k, v){ return '<div class="one"><div class="lk">' + esc(k) + '</div><div class="lv">' + esc(v) + '</div></div>'; }

  // ── Telemetry ───────────────────────────────────────────────────
  var HOOK_LEGEND = {
    'bash-native-redirect': 'Redireciona comandos Bash genéricos (grep/cat/find/etc) para tools nativas (Grep/Read/Glob), economizando tokens.',
    'budget-check': 'Mede o tamanho do prompt de uma Task antes de despachar e bloqueia se exceder o budget configurado para o role.',
    'model-routing-gate': 'Bloqueia upgrades involuntários de modelo (sonnet→opus). Downgrades passam para economia opt-in.',
    'rtk-gain': 'Marca uma agregação periódica do RTK (Rust Token Killer) — mede tokens economizados em CLI rewrites.',
    'rtk-rewrite': 'Conta cada comando Bash que foi reescrito via RTK (transparente, sem mudar comportamento).',
    'tool-use-counter': 'Conta tool uses por agente. Bloqueia Explore agents acima do limite (warn em 12, block em 15-20).',
    'spec-size-gate': 'Avisa quando spec.md ultrapassa 500 linhas (modo strict bloqueia).',
    'skill-size-gate': 'Avisa quando SKILL.md está acima do body limit recomendado pela Anthropic.',
    'duplication-check': 'Detecta blocos duplicados em arquivos editados.',
    'convention-check': 'Verifica naming/estrutura segundo convenções do projeto.',
    'review-gate': 'Avisa sobre secrets staged ou build broken antes de git commit.',
    'close-gate': 'Bloqueia transição para CLOSE se build/lint/test/QA falham ou checklist incompleto.',
    'checklist-auto-mark': 'Auto-marca itens da checklist quando o arquivo correspondente é editado.',
    'auto-format': 'Formata arquivo automaticamente após Edit/Write (Prettier/Black/etc por extensão).',
    'memory-auto-extract': 'Extrai decisões não-óbvias de specs ativas para memória de longo prazo.',
    'session-knowledge': 'Extrai padrões da pipeline-states/* para a knowledge base do projeto.',
    'session-memory': 'Injeta knowledge.json + cross-session timeline no início de cada sessão.',
  };

  function loadMetrics(){
    var pane = $('#panel-telemetry .mount');
    if (!STATE.metrics || !STATE.extra) pane.innerHTML = skelKpis() + '<div class="h-section">Hooks</div><div class="skel" style="height:280px;border-radius:8px"></div>';
    Promise.all([fetchJson('/api/metrics'), fetchJson('/api/telemetry-extra')])
      .then(function(rs){ STATE.metrics = rs[0].body || {}; STATE.extra = rs[1].body || {}; renderMetrics(); applyGlossary(); })
      .catch(function(e){ pane.innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }

  function renderMetrics(){
    var m = STATE.metrics, ex = STATE.extra || {};
    if (m.error) { $('#panel-telemetry .mount').innerHTML = '<div class="err">' + esc(m.error) + '</div>'; return; }
    var hooks = m.hookEvents || [];
    var rtk = m.rtkSavings || {};
    var days = m.last7Days || [];
    var pa = ex.pipelineAggregates || {};
    var phaseDist = ex.phaseDistribution || {};
    var aging = ex.activeAging || {};
    var storage = ex.storageBreakdown || {};

    var totalCount = hooks.reduce(function(a,h){ return a + (h.count||0); }, 0);
    var totalSaved = hooks.reduce(function(a,h){ return a + (h.tokensSaved||0); }, 0);
    var totalAffected = hooks.reduce(function(a,h){ return a + (h.tokensAffected||0); }, 0);

    var html = '';

    // Tier 1: tokens
    html += '<div class="h-section">Tokens</div>';
    html += '<div class="help-line">Soma de tokens economizados pelo Mustard via RTK + hooks. RTK reescreve comandos CLI; hooks bloqueiam upgrades de modelo desnecessários e tool uses excessivos.</div>';
    html += '<div class="kpi-grid">'
      + kpi('Total poupado', fmtTokens(totalSaved + (rtk.tokens || 0)), 'RTK + hooks somados', 'ok')
      + kpi('RTK · taxa', (rtk.rate || 0) + '%', fmtNum(rtk.commands || 0) + ' comandos reescritos', 'ok')
      + kpi('Tokens afetados', fmtTokens(totalAffected), 'inspecionados pelos hooks', 'dim')
      + kpi('Eventos de hook', fmtNum(totalCount), 'desde início do log', 'dim')
      + '</div>';

    // Tier 2: pipelines
    html += '<div class="h-section">Pipelines</div>';
    html += '<div class="help-line">Estatísticas agregadas de todos os pipelines (specs ativas + concluídas) lidas de <code class="mono">.claude/.pipeline-states/*.metrics.json</code>.</div>';
    var pass1Pct = pa.runs ? Math.round((pa.pass1 / pa.runs) * 100) : 0;
    html += '<div class="kpi-grid">'
      + kpi('Total de runs', fmtNum(pa.runs || 0), 'pipelines registrados', 'dim')
      + kpi('Pass@1', pass1Pct + '%', fmtNum(pa.pass1 || 0) + ' sem retry', pass1Pct >= 70 ? 'ok' : 'warn')
      + kpi('API calls (acum.)', fmtNum(pa.totalApiCalls || 0), 'soma de todos os runs', 'dim')
      + kpi('Retries (acum.)', fmtNum(pa.totalRetries || 0), 'reexecuções totais', pa.totalRetries > 50 ? 'warn' : 'dim')
      + '</div>';

    // Phase distribution (active specs)
    if (Object.keys(phaseDist).length) {
      html += '<div class="h-section">Distribuição de fase · specs ativas</div>';
      html += '<div class="help-line">Em qual fase estão as specs ativas agora. Picos em PLAN ou EXECUTE indicam trabalho em andamento; CLOSE significa quase pronto.</div>';
      var phaseTotal = Object.keys(phaseDist).reduce(function(a,k){ return a + phaseDist[k]; }, 0);
      var orderedPhases = ['ANALYZE','PLAN','EXECUTE','QA','CLOSE'].filter(function(k){ return phaseDist[k]; });
      var others = Object.keys(phaseDist).filter(function(k){ return orderedPhases.indexOf(k) < 0; });
      var allPhases = orderedPhases.concat(others);
      var phaseBar = '<div class="phase-bar">' + allPhases.map(function(k){
        var w = (phaseDist[k] / phaseTotal) * 100;
        return '<div class="seg" style="width:' + w + '%;background:' + phaseColor(k) + ';">' + k + ' ' + phaseDist[k] + '</div>';
      }).join('') + '</div>';
      var phaseLegend = '<div class="phase-legend">' + allPhases.map(function(k){ return '<span><span class="dot" style="background:' + phaseColor(k) + '"></span>' + k + ' · ' + phaseDist[k] + '</span>'; }).join('') + '</div>';
      html += phaseBar + phaseLegend;
    }

    // Active aging
    html += '<div class="h-section">Specs ativas · idade</div>';
    html += '<div class="help-line">Specs paradas por muitos dias geralmente indicam trabalho abandonado. Idealmente nada deveria ficar > 30d em "active".</div>';
    html += '<div class="kpi-grid cols-3">'
      + kpi('< 7 dias', fmtNum(aging.lt7d || 0), 'recentes ou em curso', 'ok')
      + kpi('7–30 dias', fmtNum(aging.d7_30 || 0), 'precisa de atenção', (aging.d7_30 || 0) > 0 ? 'warn' : 'dim')
      + kpi('> 30 dias', fmtNum(aging.gt30d || 0), 'considerar arquivar', (aging.gt30d || 0) > 0 ? 'warn' : 'dim')
      + '</div>';

    // Hooks table
    html += '<div class="h-section">Hooks · o que rodou em segundo plano</div>';
    html += '<div class="help-line">Cada linha é um hook que rodou. A coluna "Por que existe" explica em uma frase qual fricção ele reduz. <b>Tokens poupados</b> só aparecem para hooks que medem economia (RTK, model-routing, tool-use-counter).</div>';
    html += '<div class="card" style="padding:0;overflow:hidden;">'
      + '<table class="tbl"><thead><tr><th>Hook</th><th class="num">Disparou</th><th>Por que existe</th><th class="num">Tokens afetados</th><th class="num">Tokens poupados</th></tr></thead><tbody>'
      + hooks.map(function(h){
          return '<tr>'
            + '<td>' + esc(h.event) + '</td>'
            + '<td class="num muted">' + fmtNum(h.count) + '</td>'
            + '<td class="help">' + esc(HOOK_LEGEND[h.event] || '—') + '</td>'
            + '<td class="num muted">' + (h.tokensAffected ? fmtNum(h.tokensAffected) : '—') + '</td>'
            + '<td class="num">' + (h.tokensSaved ? fmtNum(h.tokensSaved) : '—') + '</td>'
            + '</tr>';
        }).join('')
      + '<tr style="border-top:2px solid var(--border-2);"><td><b>Total</b></td><td class="num"><b>' + fmtNum(totalCount) + '</b></td><td class="muted help">soma de todos os disparos</td><td class="num muted">' + fmtNum(totalAffected) + '</td><td class="num"><b>' + fmtNum(totalSaved) + '</b></td></tr>'
      + '</tbody></table></div>';

    // Tool breakdown
    if (pa.toolBreakdown && Object.keys(pa.toolBreakdown).length) {
      html += '<div class="h-section">Tools mais usadas (acumulado)</div>';
      html += '<div class="help-line">Quais ferramentas (Edit, Bash, Write, Agent…) os agentes mais chamaram em todos os runs. Picos em Bash + low Edit indicam exploração; picos em Edit indicam implementação.</div>';
      var keys = Object.keys(pa.toolBreakdown).sort(function(a,b){ return pa.toolBreakdown[b] - pa.toolBreakdown[a]; });
      html += '<div class="card" style="padding:0;overflow:hidden;"><table class="tbl"><thead><tr><th>Tool</th><th class="num">Chamadas</th></tr></thead><tbody>'
        + keys.map(function(k){ return '<tr><td>' + esc(k) + '</td><td class="num">' + fmtNum(pa.toolBreakdown[k]) + '</td></tr>'; }).join('')
        + '</tbody></table></div>';
    }

    // 7-day chart
    html += '<div class="h-section">Eventos por dia · últimos 7 dias</div>';
    html += '<div class="help-line">Cada ponto agrega <b>todos os events</b> escritos em <code class="mono">.claude/.metrics/*.jsonl</code> nesse dia. Picos = sessão ativa; vales = sem trabalho ou hooks desligados.</div>';
    html += '<div class="chart-wrap">' + renderLineChart(days) + '</div>';

    // Storage + knowledge
    html += '<div class="h-section">Storage e knowledge</div>';
    html += '<div class="help-line">Espaço em disco usado pelo Mustard em diretórios principais e tamanho da knowledge base.</div>';
    html += '<div class="kpi-grid">'
      + kpi('Knowledge entries', fmtNum(ex.knowledgeEntries || 0), 'em knowledge.json', 'dim')
      + kpi('Specs (tudo)', fmtBytes(storage.spec || 0), 'pasta spec/', 'dim')
      + kpi('Pipeline states', fmtBytes(storage.pipelineStates || 0), '.pipeline-states/', 'dim')
      + kpi('Harness log', fmtBytes(storage.harness || 0), '.harness/', 'dim')
      + '</div>';

    $('#panel-telemetry .mount').innerHTML = html;
  }

  function renderLineChart(days){
    if (!days || !days.length) return '<div class="empty">Sem dados.</div>';
    var W = 1100, H = 240, padL = 48, padR = 16, padT = 28, padB = 36;
    var max = Math.max.apply(null, days.map(function(d){ return d.events; }).concat([1]));
    var step;
    if (max <= 10) step = 2;
    else if (max <= 50) step = 10;
    else if (max <= 200) step = 50;
    else if (max <= 500) step = 100;
    else step = Math.ceil(max / 5 / 50) * 50;
    var topY = Math.ceil(max / step) * step;
    var n = days.length, innerW = W - padL - padR, innerH = H - padT - padB;
    function x(i){ return padL + (n === 1 ? innerW/2 : (innerW * i) / (n - 1)); }
    function y(v){ return padT + innerH - (innerH * v / topY); }
    var pts = days.map(function(d, i){ return { x: x(i), y: y(d.events), v: d.events, label: d.day.slice(5) }; });
    var line = pts.map(function(p, i){ return (i === 0 ? 'M' : 'L') + p.x.toFixed(1) + ',' + p.y.toFixed(1); }).join(' ');
    var area = line + ' L' + pts[pts.length-1].x.toFixed(1) + ',' + (padT + innerH) + ' L' + pts[0].x.toFixed(1) + ',' + (padT + innerH) + ' Z';
    var yLines = '', yLabels = '';
    for (var v = 0; v <= topY; v += step) {
      var yy = y(v);
      yLines += '<line class="grid" x1="' + padL + '" x2="' + (W - padR) + '" y1="' + yy + '" y2="' + yy + '"/>';
      yLabels += '<text class="y-label" x="' + (padL - 8) + '" y="' + (yy + 3) + '" text-anchor="end">' + v + '</text>';
    }
    var xLabels = pts.map(function(p, i){
      var anchor = i === 0 ? 'start' : (i === n - 1 ? 'end' : 'middle');
      return '<text class="x-label" x="' + p.x.toFixed(1) + '" y="' + (H - 14) + '" text-anchor="' + anchor + '">' + p.label + '</text>';
    }).join('');
    var dots = pts.map(function(p){
      return '<circle class="pt' + (p.v === 0 ? ' zero' : '') + '" cx="' + p.x.toFixed(1) + '" cy="' + p.y.toFixed(1) + '" r="' + (p.v === 0 ? 3 : 4) + '"><title>' + p.label + ': ' + p.v + ' eventos</title></circle>';
    }).join('');
    var valueLabels = pts.map(function(p){
      if (p.v === 0) return '';
      return '<text class="pt-value" x="' + p.x.toFixed(1) + '" y="' + (p.y - 10).toFixed(1) + '" text-anchor="middle">' + p.v + '</text>';
    }).join('');
    var totalEvents = days.reduce(function(a,d){ return a + d.events; }, 0);
    var avg = (totalEvents / n).toFixed(1);
    return '<svg class="chart" viewBox="0 0 ' + W + ' ' + H + '" preserveAspectRatio="none">'
      + '<defs><linearGradient id="chart-gradient" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="var(--brand)" stop-opacity="0.35"/><stop offset="100%" stop-color="var(--brand)" stop-opacity="0"/></linearGradient></defs>'
      + yLines + '<path class="area" d="' + area + '"/><path class="line" d="' + line + '"/>'
      + dots + valueLabels + yLabels + xLabels
      + '<line class="axis" x1="' + padL + '" y1="' + (H - padB) + '" x2="' + (W - padR) + '" y2="' + (H - padB) + '"/>'
      + '</svg>'
      + '<div class="legend"><span><span class="swatch"></span>events/dia</span><span>total: ' + fmtNum(totalEvents) + ' · média: ' + avg + '/dia</span></div>';
  }

  // ── Commands tab ─────────────────────────────────────────────────
  function loadCommands(){
    var pane = $('#panel-commands .mount');
    if (!STATE.commands) pane.innerHTML = '<div class="skel" style="height:60px;border-radius:8px;margin-bottom:12px"></div><div class="skel" style="height:200px;border-radius:8px"></div>';
    fetchJson('/api/commands').then(function(r){ STATE.commands = r.body; renderCommands(); applyGlossary('#panel-commands'); })
      .catch(function(e){ pane.innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }
  function renderCommands(){
    var data = STATE.commands || {}; var cmds = data.commands || []; var cats = data.categories || [];
    var html = '';
    html += '<div class="help-line">Catálogo completo dos comandos <code class="mono">/mustard:*</code>. Cada card tem uma explicação <b>em palavras simples</b> (linguagem simples) e uma <b>técnica</b> (o que acontece por dentro). Use os filtros pra encontrar rápido.</div>';
    html += '<div class="cmd-filters">'
      + '<input type="text" id="cmd-search" class="cmd-search" placeholder="Buscar por nome, categoria, descrição…" value="' + esc(STATE.cmdQuery || '') + '">'
      + '<span class="label">Categoria:</span>'
      + ['all'].concat(cats).map(function(c){
          var on = STATE.cmdFilter === c; var lbl = c === 'all' ? 'todos' : c.toLowerCase();
          return '<button class="chip' + (on ? ' on' : '') + '" data-cat="' + esc(c) + '">' + lbl + '</button>';
        }).join('')
      + '</div>';

    var query = (STATE.cmdQuery || '').toLowerCase();
    var byCategory = {};
    cmds.forEach(function(c){ (byCategory[c.category] = byCategory[c.category] || []).push(c); });

    var anyShown = 0;
    cats.forEach(function(cat){
      if (STATE.cmdFilter !== 'all' && STATE.cmdFilter !== cat) return;
      var list = byCategory[cat] || [];
      // Apply search filter
      var filtered = list.filter(function(c){
        if (!query) return true;
        var hay = (c.cmd + ' ' + c.short + ' ' + c.simples + ' ' + c.tecnico + ' ' + c.when + ' ' + (c.examples||[]).join(' ')).toLowerCase();
        return hay.indexOf(query) >= 0;
      });
      if (!filtered.length) return;
      html += '<div class="cmd-cat-head"><h3>' + esc(cat) + '</h3><span class="ct">' + filtered.length + ' comando' + (filtered.length > 1 ? 's' : '') + '</span></div>';
      filtered.forEach(function(c){
        anyShown++;
        html += renderCommandCard(c);
      });
    });
    if (!anyShown) html += '<div class="empty">Nenhum comando encontrado para esses filtros.</div>';

    $('#panel-commands .mount').innerHTML = html;

    $('#cmd-search').addEventListener('input', function(e){ STATE.cmdQuery = e.target.value; renderCommands(); applyGlossary('#panel-commands'); $('#cmd-search').focus(); });
    $$('#panel-commands .chip[data-cat]').forEach(function(b){
      b.addEventListener('click', function(){ STATE.cmdFilter = b.dataset.cat; renderCommands(); applyGlossary('#panel-commands'); });
    });
    $$('#panel-commands .cmd-card .ex').forEach(function(elx){
      elx.addEventListener('click', function(){
        navigator.clipboard.writeText(elx.textContent).then(function(){ toast('Copiado: ' + elx.textContent, 'ok'); });
      });
    });
  }
  function renderCommandCard(c){
    var examples = (c.examples || []).map(function(ex){ return '<code class="ex" title="Clique para copiar">' + esc(ex) + '</code>'; }).join('');
    var seeAlso = (c.seeAlso || []).map(function(ref){ return '<span class="ref">/mustard:' + esc(ref) + '</span>'; }).join('');
    return '<div class="cmd-card">'
      + '<div class="title-row">'
        + '<span class="cmd">' + esc(c.cmd) + '</span>'
        + '<span class="syntax">' + esc(c.syntax || '') + '</span>'
        + '<span class="tag">' + esc(c.category) + '</span>'
      + '</div>'
      + '<div class="short">' + esc(c.short) + '</div>'
      + '<div class="grid">'
        + '<div class="block">'
          + '<div class="lk">Em palavras simples <span class="pill ok">linguagem simples</span></div>'
          + '<div class="v">' + esc(c.simples) + '</div>'
        + '</div>'
        + '<div class="block">'
          + '<div class="lk">Por dentro <span class="pill tech">técnico</span></div>'
          + '<div class="v">' + esc(c.tecnico) + '</div>'
        + '</div>'
        + '<div class="block">'
          + '<div class="lk">Quando usar</div>'
          + '<div class="v">' + esc(c.when) + '</div>'
        + '</div>'
        + '<div class="block">'
          + '<div class="lk">Quando NÃO usar</div>'
          + '<div class="v">' + esc(c.notWhen) + '</div>'
        + '</div>'
      + '</div>'
      + (examples ? '<div class="examples"><div class="lk">Exemplos · clique para copiar</div>' + examples + '</div>' : '')
      + (seeAlso ? '<div class="seealso"><span>relacionados:</span>' + seeAlso + '</div>' : '')
      + '</div>';
  }

  // ── Glossary tab ────────────────────────────────────────────────
  function renderGlossary(){
    var keys = Object.keys(GLOSSARY).sort();
    var html = '<div class="help-line">Termos e siglas usados em todo o Mustard. Passe o mouse sobre qualquer ocorrência (texto sublinhado pontilhado) para ver o significado em contexto.</div>';
    html += keys.map(function(k){
      return '<div class="gloss-card"><div class="term">' + esc(k) + '</div><div class="def">' + esc(GLOSSARY[k]) + '</div></div>';
    }).join('');
    $('#panel-glossary .mount').innerHTML = html;
  }

  // ── Compose PRD ────────────────────────────────────────────────
  function loadProjects(){
    if (STATE.projects) return renderCompose();
    fetchJson('/api/projects').then(function(r){ STATE.projects = r.body.projects || []; renderCompose(); })
      .catch(function(e){ $('#panel-compose .mount').innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }
  function renderCompose(){
    var projOpts = (STATE.projects || []).map(function(p){
      return '<option value="' + esc(p.path) + '">' + esc(p.name) + (p.role !== 'root' ? ' · ' + esc(p.role) : '') + '</option>';
    }).join('');

    var html = '<div class="prd-layout">'
      + '<div class="card">'
        + '<div class="card-h"><h3>Entrada</h3><span class="crumb">campos do PRD</span></div>'
        + '<form id="prd-form" autocomplete="off">'
          + '<div class="field"><label>Título da Solicitação <span class="hint">slug curto, ex: add-user-login</span></label><input type="text" name="title" placeholder="ex: criar-cadastro-de-cliente" required></div>'
          + '<div class="row">'
            + '<div class="field"><label>Projeto</label><select name="project">' + projOpts + '</select></div>'
            + '<div class="field"><label>Tipo de Demanda</label><select name="type">'
              + '<option value="feature">Feature (nova funcionalidade)</option>'
              + '<option value="enhancement">Enhancement (melhoria)</option>'
              + '<option value="bugfix">Bugfix (correção)</option>'
              + '<option value="analysis">Analysis (investigação)</option>'
            + '</select></div>'
          + '</div>'
          + '<div class="row">'
            + '<div class="field"><label>Escopo Sugerido <span class="hint">auto = mustard decide</span></label><select name="scope">'
              + '<option value="auto">Auto (recomendado)</option>'
              + '<option value="light">Light (≤5 arquivos)</option>'
              + '<option value="full">Full (3+ camadas)</option>'
            + '</select></div>'
            + '<div class="field"><label>Prioridade</label><select name="priority">'
              + '<option value="medium">Média</option>'
              + '<option value="high">Alta</option>'
              + '<option value="low">Baixa</option>'
              + '<option value="urgent">Urgente</option>'
            + '</select></div>'
          + '</div>'
          + '<div class="field"><label>Rota / Endpoint <span class="hint">opcional</span></label><input type="text" name="route" placeholder="ex: POST /api/clientes"></div>'
          + '<div class="field"><label>Entidade <span class="hint">opcional</span></label><input type="text" name="entity" placeholder="ex: Cliente"></div>'
          + '<div class="field"><label>Operações CRUD</label><div class="checkbox-group">'
            + crudCb('Create') + crudCb('Read') + crudCb('Update') + crudCb('Delete') + crudCb('List')
          + '</div></div>'
          + '<div class="field"><label>Camadas Afetadas</label><div class="checkbox-group">'
            + layerCb('backend','Backend') + layerCb('frontend','Frontend') + layerCb('database','Database')
            + layerCb('design','Design') + layerCb('docs','Docs') + layerCb('tests','Testes')
          + '</div></div>'
          + '<div class="row">'
            + '<div class="field"><label>Tem Design? <span class="hint">figma, mockup</span></label><select name="design"><option value="no">Não</option><option value="yes">Sim</option><option value="partial">Parcial</option></select></div>'
            + '<div class="field"><label>É Bug?</label><select name="bug"><option value="no">Não</option><option value="yes">Sim</option></select></div>'
          + '</div>'
          + '<div class="field" id="bug-details" style="display:none"><label>Comportamento Esperado vs Atual</label><textarea name="bugRepro" rows="3" placeholder="Esperado: ...&#10;Atual: ...&#10;Passos: 1) ..."></textarea></div>'
          + '<div class="field"><label>Solicitação / Descrição</label><textarea name="request" rows="5" placeholder="Descreva a necessidade, motivação e contexto..." required></textarea></div>'
          + '<div class="field"><label>Critérios de Aceitação <span class="hint">um por linha</span></label><textarea name="ac" rows="5" placeholder="ex: npm test passa em src/clientes"></textarea></div>'
          + '<div class="field"><label>Restrições / Dependências</label><textarea name="constraints" rows="3"></textarea></div>'
          + '<div class="field"><label>Fora de Escopo</label><textarea name="oos" rows="2"></textarea></div>'
          + '<div class="prd-actions">'
            + '<button type="button" class="btn primary" id="prd-gen">Gerar PRD</button>'
            + '<button type="button" class="btn ghost" id="prd-example">Exemplo</button>'
            + '<button type="button" class="btn ghost" id="prd-reset">Limpar</button>'
          + '</div>'
        + '</form>'
      + '</div>'
      + '<div class="card">'
        + '<div class="card-h"><h3>PRD Gerado</h3><span class="crumb"><span id="prd-chars">0</span> chars</span></div>'
        + '<div class="prd-meta-line"><span>Cole no Claude e dispare <code class="mono">/mustard:feature</code> ou <code class="mono">/mustard:bugfix</code>.</span></div>'
        + '<div id="prd-output" class="prd-output">Preencha o formulário e clique em <b style="color:var(--brand);">Gerar PRD</b>.</div>'
        + '<div class="prd-actions">'
          + '<button type="button" class="btn primary" id="prd-copy">Copiar PRD</button>'
          + '<button type="button" class="btn" id="prd-copy-cmd">Copiar com /mustard</button>'
          + '<button type="button" class="btn ghost" id="prd-download">Download .md</button>'
        + '</div>'
      + '</div>'
    + '</div>';

    $('#panel-compose .mount').innerHTML = html;
    var f = $('#prd-form');
    f.querySelector('select[name="bug"]').addEventListener('change', function(e){
      $('#bug-details').style.display = e.target.value === 'yes' ? 'block' : 'none';
      if (e.target.value === 'yes' && f.type.value !== 'bugfix') f.type.value = 'bugfix';
    });
    f.querySelector('select[name="type"]').addEventListener('change', function(e){
      if (e.target.value === 'bugfix') { f.bug.value = 'yes'; $('#bug-details').style.display = 'block'; }
    });
    $('#prd-gen').addEventListener('click', generatePrd);
    $('#prd-example').addEventListener('click', loadExample);
    $('#prd-reset').addEventListener('click', resetPrd);
    $('#prd-copy').addEventListener('click', copyPrd);
    $('#prd-copy-cmd').addEventListener('click', copyPrdAsCmd);
    $('#prd-download').addEventListener('click', downloadPrd);
  }
  function crudCb(v){ return '<label><input type="checkbox" name="op" value="' + v + '">' + v + '</label>'; }
  function layerCb(v,l){ return '<label><input type="checkbox" name="layer" value="' + v + '">' + l + '</label>'; }
  function getChecked(name){ return $$('#prd-form input[name="' + name + '"]:checked').map(function(c){ return c.value; }); }
  function suggestSkills(layers, type){
    var s = [];
    if (layers.indexOf('backend') >= 0) s.push('templates-hook-protocol');
    if (layers.indexOf('database') >= 0) s.push('templates-sync-detect');
    if (type === 'feature' || type === 'enhancement' || type === 'bugfix') s.push('karpathy-guidelines');
    return s;
  }
  function suggestAgents(layers, type){
    var a = [];
    if (type === 'bugfix') a.push('bugfix');
    if (layers.indexOf('backend') >= 0) a.push('backend');
    if (layers.indexOf('frontend') >= 0) a.push('frontend');
    if (layers.indexOf('database') >= 0) a.push('database');
    a.push('review');
    return a;
  }
  function generatePrd(){
    var f = $('#prd-form');
    var title = (f.title.value || '').trim() || '(sem título)';
    var project = (f.project.value || '').trim();
    var type = f.type.value, scope = f.scope.value, priority = f.priority.value;
    var route = (f.route.value || '').trim();
    var entity = (f.entity.value || '').trim();
    var ops = getChecked('op'), layers = getChecked('layer');
    var design = f.design.value, bug = f.bug.value;
    var bugRepro = (f.bugRepro.value || '').trim();
    var request = (f.request.value || '').trim();
    var ac = (f.ac.value || '').split('\\n').map(function(s){ return s.trim(); }).filter(Boolean);
    var constraints = (f.constraints.value || '').trim();
    var oos = (f.oos.value || '').trim();

    var typeLabel = { feature:'Feature', enhancement:'Enhancement', bugfix:'Bugfix', analysis:'Analysis' }[type];
    var pipelineCmd = type === 'bugfix' ? '/mustard:bugfix' : '/mustard:feature';
    var slug = slugify(title);

    var md = '';
    md += '# PRD: ' + title + '\\n\\n';
    md += '**Projeto:** ' + (project && project !== '.' ? project : '(root)') + '\\n';
    md += '**Tipo:** ' + typeLabel + '\\n';
    md += '**Escopo:** ' + (scope === 'auto' ? 'Auto-detect' : scope.charAt(0).toUpperCase() + scope.slice(1)) + '\\n';
    md += '**Prioridade:** ' + priority + '\\n';
    md += '**Data:** ' + today() + '\\n';
    md += '**Pipeline:** \`' + pipelineCmd + ' ' + slug + '\`\\n\\n';
    md += '## Solicitação\\n\\n' + (request || '_(sem descrição)_') + '\\n\\n';
    if (bug === 'yes' && bugRepro) md += '## Reprodução do Bug\\n\\n' + bugRepro + '\\n\\n';
    md += '## Escopo Técnico\\n\\n';
    if (route) md += '- **Rota/Endpoint:** \`' + route + '\`\\n';
    if (entity) md += '- **Entidade:** \`' + entity + '\`\\n';
    if (ops.length) md += '- **Operações:** ' + ops.join(', ') + '\\n';
    md += '- **Tem Design:** ' + (design === 'yes' ? 'Sim' : design === 'partial' ? 'Parcial' : 'Não') + '\\n';
    md += '- **É Bug:** ' + (bug === 'yes' ? 'Sim' : 'Não') + '\\n';
    if (!route && !entity && !ops.length) md += '_(sem detalhes técnicos preenchidos)_\\n';
    md += '\\n';
    md += '## Camadas Afetadas\\n\\n';
    [['backend','Backend'],['frontend','Frontend'],['database','Database'],['design','Design'],['docs','Docs'],['tests','Testes']].forEach(function(p){ md += '- [' + (layers.indexOf(p[0]) >= 0 ? 'x' : ' ') + '] ' + p[1] + '\\n'; });
    md += '\\n';
    md += '## Acceptance Criteria\\n\\n';
    if (ac.length) ac.forEach(function(a, i){ md += (i+1) + '. ' + a + '\\n'; });
    else md += '_(adicionar 3-8 critérios executáveis)_\\n';
    md += '\\n';
    if (constraints) md += '## Restrições / Dependências\\n\\n' + constraints + '\\n\\n';
    if (oos) md += '## Fora de Escopo\\n\\n' + oos + '\\n\\n';
    var skills = suggestSkills(layers, type), agents = suggestAgents(layers, type);
    md += '## Sugestão de Roteamento (mustard)\\n\\n';
    md += '- **Agentes recomendados:** ' + agents.join(', ') + '\\n';
    if (skills.length) md += '- **Skills sugeridas:** ' + skills.map(function(s){ return '\`' + s + '\`'; }).join(', ') + '\\n';
    md += '- **Fases:** ' + (scope === 'light' ? 'ANALYZE → EXECUTE → QA → CLOSE' : 'ANALYZE → PLAN → /approve → EXECUTE → QA → CLOSE') + '\\n\\n';
    md += '---\\n_Gerado por PRD Builder · cole no Claude e rode \`' + pipelineCmd + '\` para iniciar a pipeline._\\n';
    $('#prd-output').textContent = md; $('#prd-chars').textContent = md.length;
  }
  function copyPrd(){ var t = $('#prd-output').textContent; if (!t || t.indexOf('Preencha') === 0) { toast('Gere o PRD primeiro', 'err'); return; } navigator.clipboard.writeText(t).then(function(){ toast('PRD copiado', 'ok'); }); }
  function copyPrdAsCmd(){ var t = $('#prd-output').textContent; if (!t || t.indexOf('Preencha') === 0) { toast('Gere o PRD primeiro', 'err'); return; } var cmd = $('#prd-form').type.value === 'bugfix' ? '/mustard:bugfix' : '/mustard:feature'; navigator.clipboard.writeText(cmd + '\\n\\n' + t).then(function(){ toast('Copiado com ' + cmd, 'ok'); }); }
  function downloadPrd(){
    var t = $('#prd-output').textContent;
    if (!t || t.indexOf('Preencha') === 0) { toast('Gere o PRD primeiro', 'err'); return; }
    var title = ($('#prd-form').title.value.trim() || 'prd');
    var blob = new Blob([t], { type: 'text/markdown' });
    var url = URL.createObjectURL(blob);
    var a = document.createElement('a'); a.href = url; a.download = 'prd-' + slugify(title) + '-' + today() + '.md';
    document.body.appendChild(a); a.click(); a.remove(); URL.revokeObjectURL(url);
    toast('Download iniciado', 'ok');
  }
  function resetPrd(){
    var f = $('#prd-form');
    $$('input[type="text"], textarea', f).forEach(function(i){ i.value = ''; });
    $$('input[type="checkbox"]', f).forEach(function(i){ i.checked = false; });
    f.type.value = 'feature'; f.scope.value = 'auto'; f.priority.value = 'medium';
    f.design.value = 'no'; f.bug.value = 'no';
    $('#bug-details').style.display = 'none';
    $('#prd-output').textContent = 'Preencha o formulário e clique em Gerar PRD.';
    $('#prd-chars').textContent = '0';
  }
  function loadExample(){
    var f = $('#prd-form');
    f.title.value = 'cadastro-de-cliente';
    f.type.value = 'feature'; f.scope.value = 'auto'; f.priority.value = 'medium';
    f.route.value = 'POST /api/clientes'; f.entity.value = 'Cliente';
    $$('input[name="op"]', f).forEach(function(c){ c.checked = ['Create','Read','List'].indexOf(c.value) >= 0; });
    $$('input[name="layer"]', f).forEach(function(c){ c.checked = ['backend','frontend','database'].indexOf(c.value) >= 0; });
    f.design.value = 'yes'; f.bug.value = 'no';
    f.request.value = 'Permitir cadastro de novos clientes via formulário web. Necessário para suportar onboarding self-service e reduzir tickets de suporte para cadastro manual.';
    f.ac.value = 'npm test passa em src/clientes\\nPOST /api/clientes com payload válido retorna 201 e cliente persistido\\nPOST /api/clientes com email duplicado retorna 409\\nTela /clientes/novo renderiza e submete sem erros\\nMigração drizzle aplica e reverte sem perda de dados';
    f.constraints.value = 'Reusar middleware de auth existente. Email único. Validar CPF/CNPJ.';
    f.oos.value = 'Edição e exclusão de cliente (próxima iteração).';
    generatePrd();
  }

  // ── Settings ───────────────────────────────────────────────────
  function loadSettings(){
    var pane = $('#panel-settings .mount');
    if (!STATE.settings) pane.innerHTML = '<div class="skel" style="height:60px;border-radius:8px;margin-bottom:12px"></div><div class="skel" style="height:240px;border-radius:8px"></div>';
    fetchJson('/api/settings').then(function(r){ STATE.settings = r.body; STATE.dirtySettings = {}; renderSettings(); applyGlossary(); })
      .catch(function(e){ pane.innerHTML = '<div class="err">' + esc(e.message) + '</div>'; });
  }
  function renderSettings(){
    var s = STATE.settings || {}; var catalog = s.catalog || []; var values = s.values || {};
    var html = '';
    html += '<div class="help-line">Cada parâmetro é uma <code class="mono">env</code> lida pelos hooks do Mustard. Selecione o valor que prefere — a explicação ao lado do valor diz exatamente o que ele faz. Salvar grava em <code class="mono">.claude/settings.json</code> e os hooks lêem na próxima execução.</div>';
    catalog.forEach(function(g){
      html += '<div class="set-group">'
        + '<div class="gh"><h3>' + esc(g.group) + '</h3></div>'
        + '<p class="gd">' + esc(g.desc) + '</p>'
        + '<div class="set-list">';
      g.keys.forEach(function(k){
        var cur = values[k.key] != null ? values[k.key] : k.default;
        html += '<div class="set-card" data-key="' + esc(k.key) + '">'
          + '<div class="head">'
            + '<span class="key">' + esc(k.key) + '</span>'
            + '<span class="tag">default: ' + esc(k.default) + '</span>'
          + '</div>'
          + '<p class="desc">' + esc(k.desc) + '</p>'
          + '<div class="opt-grid">'
          + k.options.map(function(opt){
              var isOn = String(cur) === String(opt);
              var doc = (k.valueDocs && k.valueDocs[opt]) || '';
              return '<label class="opt' + (isOn ? ' on' : '') + '">'
                + '<input type="radio" name="' + esc(k.key) + '" value="' + esc(opt) + '"' + (isOn ? ' checked' : '') + '>'
                + '<span class="name">' + esc(opt) + (opt === k.default ? ' <span class="star">· default</span>' : '') + '</span>'
                + '<span class="doc">' + esc(doc) + '</span>'
                + '</label>';
            }).join('')
          + '</div>'
          + '</div>';
      });
      html += '</div></div>';
    });
    html += '<div class="set-bar" id="set-bar"><div class="summary" id="set-summary">Sem mudanças pendentes.</div>'
      + '<button class="btn ghost" id="set-discard" disabled>Descartar</button>'
      + '<button class="btn primary" id="set-save" disabled>Salvar alterações</button>'
      + '</div>';
    $('#panel-settings .mount').innerHTML = html;
    $$('#panel-settings .opt input[type="radio"]').forEach(function(inp){
      inp.addEventListener('change', function(){
        var k = inp.name, v = inp.value;
        var orig = (STATE.settings.values || {})[k];
        if (String(v) === String(orig)) delete STATE.dirtySettings[k];
        else STATE.dirtySettings[k] = v;
        var card = inp.closest('.set-card');
        $$('.opt', card).forEach(function(o){ o.classList.toggle('on', o.querySelector('input').checked); });
        updateSetBar();
      });
    });
    $('#set-save').addEventListener('click', saveSettings);
    $('#set-discard').addEventListener('click', function(){ STATE.dirtySettings = {}; renderSettings(); });
  }
  function updateSetBar(){
    var keys = Object.keys(STATE.dirtySettings);
    var bar = $('#set-bar'); var save = $('#set-save'); var dis = $('#set-discard');
    if (!bar) return;
    if (!keys.length) {
      bar.classList.remove('dirty');
      $('#set-summary').textContent = 'Sem mudanças pendentes.';
      save.disabled = true; dis.disabled = true;
    } else {
      bar.classList.add('dirty');
      $('#set-summary').textContent = keys.length + ' alteração' + (keys.length > 1 ? 'ões' : '') + ' pendente' + (keys.length > 1 ? 's' : '');
      save.disabled = false; dis.disabled = false;
    }
  }
  function saveSettings(){
    var btn = $('#set-save'); btn.disabled = true; btn.textContent = 'Salvando…';
    fetchJson('/api/settings', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ values: STATE.dirtySettings }) })
      .then(function(r){
        btn.textContent = 'Salvar alterações';
        if (r.body.ok) { toast('Salvo em .claude/settings.json', 'ok'); STATE.settings = null; loadSettings(); }
        else { toast('Erro: ' + (r.body.error || 'unknown'), 'err'); btn.disabled = false; }
      })
      .catch(function(e){ btn.textContent = 'Salvar alterações'; toast('Falha: ' + e.message, 'err'); btn.disabled = false; });
  }

  // ── Markdown renderer ─────────────────────────────────────────
  function renderMarkdown(src){
    if (!src) return '';
    var lines = src.split(/\\r?\\n/);
    var out = []; var inCode = false; var codeBuf = []; var inList = false; var listType = '';
    function flushList(){ if (inList) { out.push('</' + listType + '>'); inList = false; listType=''; } }
    function flushCode(){ if (inCode) { out.push('<pre><code>' + esc(codeBuf.join('\\n')) + '</code></pre>'); codeBuf = []; inCode = false; } }
    for (var i = 0; i < lines.length; i++) {
      var ln = lines[i];
      if (/^\\s*\\\`\\\`\\\`/.test(ln)) {
        if (inCode) { flushCode(); } else { flushList(); inCode = true; }
        continue;
      }
      if (inCode) { codeBuf.push(ln); continue; }
      var h = ln.match(/^(#{1,6})\\s+(.*)$/);
      if (h) { flushList(); out.push('<h' + h[1].length + '>' + inline(h[2]) + '</h' + h[1].length + '>'); continue; }
      if (/^\\s*[-*]\\s+/.test(ln)) {
        if (!inList || listType !== 'ul') { flushList(); out.push('<ul>'); inList = true; listType = 'ul'; }
        out.push('<li>' + inline(ln.replace(/^\\s*[-*]\\s+/, '')) + '</li>');
        continue;
      }
      if (/^\\s*\\d+\\.\\s+/.test(ln)) {
        if (!inList || listType !== 'ol') { flushList(); out.push('<ol>'); inList = true; listType = 'ol'; }
        out.push('<li>' + inline(ln.replace(/^\\s*\\d+\\.\\s+/, '')) + '</li>');
        continue;
      }
      if (/^---+$/.test(ln.trim())) { flushList(); out.push('<hr>'); continue; }
      if (ln.trim() === '') { flushList(); out.push(''); continue; }
      flushList();
      out.push('<p>' + inline(ln) + '</p>');
    }
    flushCode(); flushList();
    return out.join('\\n');
  }
  function inline(s){
    var r = esc(s);
    r = r.replace(/\\\`([^\\\`]+)\\\`/g, '<code>$1</code>');
    r = r.replace(/\\*\\*([^*]+)\\*\\*/g, '<strong>$1</strong>');
    r = r.replace(/\\*([^*]+)\\*/g, '<em>$1</em>');
    r = r.replace(/\\[([^\\]]+)\\]\\(([^\\)]+)\\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');
    return r;
  }

  // ── Boot ───────────────────────────────────────────────────────
  function bindGlobalClicks(){
    document.addEventListener('click', function(e){
      var live = e.target.closest('[data-live]');
      if (live) { e.preventDefault(); e.stopPropagation(); openLiveMonitor(live.dataset.live); return; }
      var t = e.target.closest('[data-tab]');
      if (t) { e.preventDefault(); setTab(t.dataset.tab); return; }
      var openA = e.target.closest('[data-open]');
      if (openA) { e.preventDefault(); openSpec(openA.dataset.open); return; }
      var tog = e.target.closest('[data-toggle]');
      if (tog) {
        var node = document.getElementById(tog.dataset.toggle);
        if (node) {
          if (node.hasAttribute('hidden')) node.removeAttribute('hidden'); else node.setAttribute('hidden', '');
        }
      }
    });
    var tb = $('#theme-btn'); if (tb) tb.addEventListener('click', toggleTheme);
    var rb = $('#refresh-btn'); if (rb) rb.addEventListener('click', function(){
      if (STATE.tab === 'overview') loadOverview();
      else if (STATE.tab === 'specs') { STATE.specs = null; loadSpecs(); }
      else if (STATE.tab === 'telemetry') loadMetrics();
      else if (STATE.tab === 'settings') { STATE.settings = null; loadSettings(); }
    });
    var mb = $('#menu-btn'); if (mb) mb.addEventListener('click', toggleRail);
    var ro = $('#rail-overlay'); if (ro) ro.addEventListener('click', closeRail);
    var sc = $('#sp-close'); if (sc) sc.addEventListener('click', function(){ STATE.panelPinned = false; applyPinState(); closePanel(true); });
    var so = $('#side-overlay'); if (so) so.addEventListener('click', function(){ closePanel(false); });
    var sp = $('#sp-pin'); if (sp) sp.addEventListener('click', togglePin);
  }
  function start(){ initTheme(); restorePanelWidth(); bindGlobalClicks(); bindPanelResize(); setTab('overview'); startLiveBgPoll(); }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
`;

function renderHtml(branch) {
  const safeBranch = escapeHtml(branch);
  const head = ''
    + '<!doctype html>'
    + '<html lang="pt-BR">'
    + '<head>'
    + '<meta charset="utf-8">'
    + '<title>Mustard · Dashboard</title>'
    + '<meta name="viewport" content="width=device-width,initial-scale=1">'
    + '<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 viewBox=%220 0 32 32%22%3E%3Crect width=%2232%22 height=%2232%22 rx=%227%22 fill=%22%23e2a93b%22/%3E%3Ctext x=%2216%22 y=%2222%22 font-family=%22sans-serif%22 font-size=%2218%22 font-weight=%22600%22 fill=%22%231a1208%22 text-anchor=%22middle%22%3EM%3C/text%3E%3C/svg%3E">'
    + '<link rel="preconnect" href="https://fonts.googleapis.com">'
    + '<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>'
    + '<link href="https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700&family=Geist+Mono:wght@400;500;600&display=swap" rel="stylesheet">'
    + '<style>' + CSS + '</style>'
    + '</head>';

  const body = ''
    + '<body>'
    + '<div id="rail-overlay" class="rail-overlay"></div>'
    + '<div class="app">'
    +   '<aside class="rail">'
    +     '<div class="brand-row">'
    +       '<div class="logo">M</div>'
    +       '<div class="brand-name">Mustard</div>'
    +       '<div class="brand-meta">' + safeBranch + '</div>'
    +     '</div>'
    +     '<div>'
    +       '<div class="nav-section-label">Workspace</div>'
    +       '<nav>'
    +         '<a data-tab="overview" class="on"><span class="ic">' + ICONS.home + '</span><span class="label">Overview</span></a>'
    +         '<a data-tab="specs"><span class="ic">' + ICONS.doc + '</span><span class="label">Specs</span></a>'
    +         '<a data-tab="telemetry"><span class="ic">' + ICONS.chart + '</span><span class="label">Telemetry</span></a>'
    +       '</nav>'
    +     '</div>'
    +     '<div>'
    +       '<div class="nav-section-label">Tools</div>'
    +       '<nav>'
    +         '<a data-tab="compose"><span class="ic">' + ICONS.plus + '</span><span class="label">Compose PRD</span></a>'
    +         '<a data-tab="commands"><span class="ic">' + ICONS.terminal + '</span><span class="label">Comandos</span></a>'
    +         '<a data-tab="settings"><span class="ic">' + ICONS.cog + '</span><span class="label">Settings</span></a>'
    +         '<a data-tab="glossary"><span class="ic">' + ICONS.book + '</span><span class="label">Glossário</span></a>'
    +       '</nav>'
    +     '</div>'
    +     '<div class="footer-actions">'
    +       '<button id="refresh-btn" title="Recarregar tab atual">' + ICONS.refresh + '<span>Refresh</span></button>'
    +       '<button id="theme-btn" title="Alternar tema">' + ICONS.moon + '</button>'
    +     '</div>'
    +   '</aside>'
    +   '<main class="main">'
    +     '<div class="topbar"><div class="topbar-inner">'
    +       '<button class="menu-btn" id="menu-btn" title="Menu">' + ICONS.menu + '</button>'
    +       '<h1 id="tab-title">Overview</h1>'
    +       '<span class="crumb" id="tab-crumb">Visão geral · poll <b>12s</b></span>'
    +     '</div></div>'
    +     '<div class="live-banner" id="live-banner" hidden></div>'
    +     '<section class="panel on" id="panel-overview"><div class="mount"></div></section>'
    +     '<section class="panel" id="panel-specs"><div class="mount"></div></section>'
    +     '<section class="panel" id="panel-telemetry"><div class="mount"></div></section>'
    +     '<section class="panel" id="panel-compose"><div class="mount"></div></section>'
    +     '<section class="panel" id="panel-commands"><div class="mount"></div></section>'
    +     '<section class="panel" id="panel-settings"><div class="mount"></div></section>'
    +     '<section class="panel" id="panel-glossary"><div class="mount"></div></section>'
    +   '</main>'
    + '</div>'
    + '<div class="side-overlay" id="side-overlay"></div>'
    + '<div class="side-panel" id="side-panel">'
    +   '<div class="sp-resize" id="sp-resize" title="Arraste para redimensionar"></div>'
    +   '<div class="sp-header">'
    +     '<button class="sp-pin" id="sp-pin" title="Pinar painel (mantém aberto e troca conteúdo ao clicar)" aria-label="Pinar">⚲</button>'
    +     '<button class="sp-close" id="sp-close" title="Fechar">×</button>'
    +     '<h2 id="sp-title">—</h2>'
    +     '<div class="nm" id="sp-name"></div>'
    +   '</div>'
    +   '<div class="sp-body" id="sp-body"></div>'
    + '</div>'
    + '<div class="toast" id="toast"></div>'
    + '<script>window.MICONS = { sun: ' + JSON.stringify(ICONS.sun) + ', moon: ' + JSON.stringify(ICONS.moon) + ', refresh: ' + JSON.stringify(ICONS.refresh) + ' };</script>'
    + '<script>' + CLIENT_JS + '</script>'
    + '</body></html>';

  return head + body;
}

module.exports = { renderHtml };
