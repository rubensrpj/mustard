const fs = require('fs');

let ok = true;

// AC-W9.1
{
  const t = fs.readFileSync('packages/core/src/model/contract.rs', 'utf8');
  for (const v of ['Stop', 'Notification']) {
    if (!new RegExp('\\b' + v + '\\b').test(t)) {
      console.error('AC-W9.1 FAIL: missing ' + v);
      ok = false;
    }
  }
  if (ok) console.log('AC-W9.1 PASS');
}

// AC-W9.2
{
  let pass = true;
  const t = fs.readFileSync('apps/rt/src/registry.rs', 'utf8');
  for (const h of ['stop', 'notification']) {
    if (!new RegExp('"' + h + '"').test(t)) {
      console.error('AC-W9.2 FAIL: missing "' + h + '"');
      pass = false;
      ok = false;
    }
  }
  if (pass) console.log('AC-W9.2 PASS');
}

// AC-W9.3
{
  let pass = true;
  const j = JSON.parse(fs.readFileSync('apps/cli/templates/settings.json', 'utf8'));
  const txt = JSON.stringify(j);
  for (const k of ['on Stop', 'on Notification']) {
    if (!txt.includes(k)) {
      console.error('AC-W9.3 FAIL: missing "' + k + '"');
      pass = false;
      ok = false;
    }
  }
  if (pass) console.log('AC-W9.3 PASS');
}

process.exit(ok ? 0 : 1);
