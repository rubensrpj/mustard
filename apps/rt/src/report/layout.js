// O script das páginas do Mustard, aprovado em 17/09 (versão 9): o menu
// lateral que acompanha a rolagem, os atalhos, a busca (por texto, sem acento
// e sem caixa, ou pelo código de um item) e os botões de abrir e fechar tudo.
// Os textos vêm da própria página, nos atributos da moldura (data-of,
// data-one, data-many), no idioma dela.
(function(){
  var shell=document.querySelector('.shell');
  var box=document.getElementById('content');
  var crumb=document.querySelector('.crumb');
  var q=document.getElementById('q'),hits=document.getElementById('hits'),empty=document.getElementById('empty');
  var searchBox=document.getElementById('searchBox');
  var navLis=[].slice.call(document.querySelectorAll('ol.nav>li'));
  var buttons=[].slice.call(document.querySelectorAll('ol.nav button[data-go]'));
  var sections=[].slice.call(document.querySelectorAll('section.block'));
  var groups=[].slice.call(document.querySelectorAll('details.group'));
  var items=[].slice.call(document.querySelectorAll('details.item'));
  var overview=document.querySelector('.overview');
  var reduce=window.matchMedia&&matchMedia('(prefers-reduced-motion: reduce)').matches;
  var marks=sections.concat(groups);

  function say(pattern,n,total){return pattern.replace('{n}',n).replace('{total}',total);}
  function of(n,total){return say(shell.getAttribute('data-of'),n,total);}
  function counted(n){return say(shell.getAttribute(n===1?'data-one':'data-many'),n,n);}

  function top(el){return el.getBoundingClientRect().top-box.getBoundingClientRect().top;}
  function openUp(el){for(var p=el;p;p=p.parentElement){if(p.tagName==='DETAILS')p.open=true;}}
  function scrollTo(el){box.scrollTo({top:Math.max(0,box.scrollTop+top(el)-68),behavior:reduce?'auto':'smooth'});}
  function flash(el){el.classList.remove('flash');void el.offsetWidth;el.classList.add('flash');}

  function mark(id){
    var el=document.getElementById(id);if(!el)return;
    var sec=el.closest('section.block');var sid=sec?sec.id:id;
    buttons.forEach(function(b){var g=b.getAttribute('data-go');b.classList.toggle('here',g===id||g===sid);});
    navLis.forEach(function(li){li.classList.toggle('open',li.getAttribute('data-sec')===sid);});
    var parts=(el.getAttribute('data-crumb')||'').split(' / ');
    crumb.innerHTML='';
    parts.forEach(function(p,i){
      if(i)crumb.appendChild(document.createTextNode('  /  '));
      var n=document.createElement(i===parts.length-1?'b':'span');n.textContent=p;crumb.appendChild(n);
    });
  }
  var lock=0,ticking=false;
  function spy(){
    ticking=false;
    if(Date.now()<lock)return;
    var current=null;
    for(var i=0;i<marks.length;i++){var m=marks[i];if(m.hidden||m.offsetParent===null)continue;if(top(m)<=72)current=m;}
    if(current)mark(current.id);else{crumb.textContent='';navLis.forEach(function(li,i){li.classList.toggle('open',i===0);});}
  }
  box.addEventListener('scroll',function(){if(!ticking){ticking=true;requestAnimationFrame(spy);}},{passive:true});

  function goTo(id){
    var el=document.getElementById(id);if(!el)return;
    if(el.tagName==='DETAILS')el.open=true;
    openUp(el.parentElement);
    lock=Date.now()+900;mark(id);scrollTo(el);
    document.body.classList.remove('nav-open');
  }
  document.addEventListener('click',function(e){
    var b=e.target.closest('[data-go]');
    if(b){goTo(b.getAttribute('data-go'));return;}
    var a=e.target.closest('a[href^="#"]');
    if(a){
      var id=decodeURIComponent(a.getAttribute('href').slice(1));
      var el=document.getElementById(id);if(!el)return;
      e.preventDefault();
      if(el.closest('[hidden]')){q.value='';run();}
      openUp(el);scrollTo(el);flash(el);
      try{history.replaceState(null,'','#'+id);}catch(_){}
    }
  });
  document.getElementById('menuBtn').addEventListener('click',function(){
    var on=document.body.classList.toggle('nav-open');this.setAttribute('aria-expanded',on?'true':'false');
  });

  // busca
  var index=null,saved=null,byRun=[],cur=-1,counts=null;
  function fold(s){return s.normalize('NFD').replace(/[\u0300-\u036f]/g,'').toLowerCase();}
  function clearMarks(){
    [].slice.call(document.querySelectorAll('mark.hl')).forEach(function(m){
      var p=m.parentNode;p.replaceChild(document.createTextNode(m.textContent),m);p.normalize();
    });
  }
  function highlight(root,terms){
    if(!root||!terms.length)return;
    var walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT,null),nodes=[],n;
    while((n=walker.nextNode()))nodes.push(n);
    nodes.forEach(function(node){
      var text=node.nodeValue,low=fold(text);
      if(low.length!==text.length)return;
      var ranges=[];
      terms.forEach(function(t){var i=low.indexOf(t);while(i>=0){ranges.push([i,i+t.length]);i=low.indexOf(t,i+t.length);}});
      if(!ranges.length)return;
      ranges.sort(function(a,b){return a[0]-b[0];});
      var frag=document.createDocumentFragment(),pos=0;
      ranges.forEach(function(r){
        if(r[0]<pos)return;
        frag.appendChild(document.createTextNode(text.slice(pos,r[0])));
        var m=document.createElement('mark');m.className='hl';m.textContent=text.slice(r[0],r[1]);frag.appendChild(m);
        pos=r[1];
      });
      frag.appendChild(document.createTextNode(text.slice(pos)));
      node.parentNode.replaceChild(frag,node);
    });
  }
  function terms(){return fold(q.value.trim()).split(/\s+/).filter(Boolean);}
  // Uma busca que é o código de um item (MSTD-DEC-0142, ou só DEC-0142)
  // deixa só o item dono do código, e não os que o citam.
  function byCode(ts){
    if(ts.length!==1||!/[a-z]-\d/.test(ts[0]))return null;
    var t=ts[0];
    var hit=items.filter(function(d){var id=fold(d.id);return id!==''&&(id===t||id.slice(-t.length-1)==='-'+t);});
    return hit.length?hit:null;
  }
  function navCounts(on){
    if(!counts)counts=buttons.map(function(b){var i=b.querySelector('i');return i?i.textContent:'';});
    buttons.forEach(function(b,k){
      var el=document.getElementById(b.getAttribute('data-go'));var i=b.querySelector('i');
      var li=b.parentElement;
      if(!on){if(i)i.textContent=counts[k];li.hidden=false;return;}
      var n=el?el.querySelectorAll('details.item:not([hidden])').length:0;
      if(i)i.textContent=n||'';
      li.hidden=!n;
    });
  }
  var countEls=[].slice.call(document.querySelectorAll('.count'));
  countEls.forEach(function(c){c.setAttribute('data-n',c.textContent);});
  function recount(on){
    countEls.forEach(function(c){
      if(!on){c.textContent=c.getAttribute('data-n');return;}
      var host=c.closest('details.group')||c.closest('section.block');
      var total=host.querySelectorAll('details.item').length;
      var vis=host.querySelectorAll('details.item:not([hidden])').length;
      c.textContent=total?of(vis,total):'';
    });
  }
  function reset(){
    items.forEach(function(d){d.hidden=false;});
    groups.forEach(function(g,i){g.hidden=false;if(saved)g.open=saved[i];});
    sections.forEach(function(s){s.hidden=false;});
    if(overview)overview.hidden=false;
    byRun.forEach(function(d){d.open=false;});byRun=[];saved=null;cur=-1;
    hits.textContent='';empty.hidden=true;searchBox.classList.remove('on');
    navCounts(false);recount(false);spy();
  }
  function run(){
    clearMarks();
    var ts=terms();
    if(!ts.length){reset();return;}
    if(!index)index=items.map(function(d){return fold(d.textContent);});
    if(!saved)saved=groups.map(function(g){return g.open;});
    byRun.forEach(function(d){d.open=false;});byRun=[];cur=-1;
    var found=[],owners=byCode(ts);
    items.forEach(function(d,i){
      var ok=true;
      if(owners)ok=owners.indexOf(d)>=0;
      else for(var k=0;k<ts.length;k++){if(index[i].indexOf(ts[k])<0){ok=false;break;}}
      d.hidden=!ok;if(ok)found.push(d);
    });
    groups.forEach(function(g){var any=g.querySelector('details.item:not([hidden])');g.hidden=!any;g.open=!!any;});
    sections.forEach(function(s){s.hidden=!s.querySelector('details.group:not([hidden])');});
    if(overview)overview.hidden=true;
    if(found.length<=5)found.forEach(function(d){if(!d.open){d.open=true;byRun.push(d);}});
    found.slice(0,300).forEach(function(d){highlight(d.querySelector('summary .t'),ts);highlight(d.querySelector('summary .c'),ts);});
    found.forEach(function(d){if(d.open)highlight(d.querySelector('.body'),ts);});
    hits.textContent=counted(found.length);
    empty.hidden=found.length>0;
    searchBox.classList.add('on');
    navCounts(true);recount(true);
    box.scrollTop=0;spy();
  }
  var timer=0;
  q.addEventListener('input',function(){clearTimeout(timer);timer=setTimeout(run,160);});
  q.addEventListener('keydown',function(e){
    if(e.key==='Escape'){q.value='';run();q.blur();}
    if(e.key==='Enter'){
      e.preventDefault();
      var vis=items.filter(function(d){return !d.hidden;});if(!vis.length)return;
      cur=(cur+1)%vis.length;var d=vis[cur];
      if(!d.open){d.open=true;byRun.push(d);}
      openUp(d);scrollTo(d);flash(d);
      hits.textContent=of(cur+1,vis.length);
    }
  });
  document.addEventListener('toggle',function(e){
    var d=e.target;
    if(d.classList&&d.classList.contains('item')&&d.open&&q.value.trim()){
      if(!d.querySelector('.body mark.hl'))highlight(d.querySelector('.body'),terms());
    }
  },true);
  document.addEventListener('keydown',function(e){
    var t=document.activeElement&&document.activeElement.tagName;
    if(e.key==='/'&&t!=='INPUT'&&t!=='TEXTAREA'){e.preventDefault();q.focus();q.select();}
  });

  document.getElementById('openAll').addEventListener('click',function(){
    groups.forEach(function(g){if(!g.hidden)g.open=true;});
    items.forEach(function(d){if(!d.hidden)d.open=true;});
  });
  document.getElementById('closeAll').addEventListener('click',function(){
    items.forEach(function(d){d.open=false;});
    groups.forEach(function(g){g.open=false;});
    byRun=[];spy();
  });

  if(location.hash){var el=document.getElementById(decodeURIComponent(location.hash.slice(1)));if(el){openUp(el);setTimeout(function(){scrollTo(el);flash(el);},50);}}
  spy();
})();
