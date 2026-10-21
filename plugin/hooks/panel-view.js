// The panel's screens as one tree for every surface. The binary owns every
// number; this module only lays out the answer of the visible screen.
// As duas superfícies seguem as seis telas do layout aprovado: os mesmos
// blocos, na mesma ordem, com os mesmos textos, a mesma hierarquia e a mesma
// paleta. O terminal só as traduz para células: o px vira colunas e linhas, o
// cartão de borda fina vira superfície preenchida, o desenho em Svg vira
// caractere, o tamanho e o peso da letra viram negrito e cor, e cada botão
// leva a tecla aprovada. O que o layout não tem e já existia vem depois dos
// blocos dele.
import {contentPage} from './panel-content.js';
import {settingsScreen} from './panel-settings.js';

export const PAGE=8;
export const KINDS=['task','criterion','decision','rule','question'];
export const FILTERS=['all','attention','committed','uncommitted'];
export const initialPanelState=()=>({screen:'overview',spec:null,kind:'task',query:'',filter:'all',rows:PAGE,item:null,
  back:[],history:false,waveRequest:false,searching:false,pickSpec:false,delivered:false});

/** The `run panel` section and arguments of a screen; none for Settings. */
export function screenQuery(state) {
  if(state.screen==='items')return {section:'items',args:['--kind',state.kind,'--filter',state.filter,'--offset','0','--limit',String(Math.min(50,state.rows)),
    ...(state.query.trim()?['--query',state.query.trim()]:[])]};
  if(state.screen==='item')return {section:'item',args:state.item?['--item',state.item]:[]};
  return ['overview','spec','usage'].includes(state.screen)?{section:state.screen,args:[]}:undefined;
}

const phases={survey:['levantamento','survey'],plan:['planejamento','planning'],planned:['planejada','planned'],approved:['aprovada','approved'],
  running:['execução','running'],closed:['fechada','closed'],pr_open:['pull request','pull request'],delivered:['entregue','delivered'],discarded:['descartada','discarded']};
// How a merged pull request reads against the phase the spec is still in.
const still={running:['segue em execução','is still running'],approved:['segue aprovada, sem execução','is still approved, not running'],
  closed:['segue fechada, sem entrega','is still closed, not delivered'],pr_open:['segue com o PR aberto','still has the PR open']};
const waveStates={todo:['a fazer','to do'],running:['em execução','running'],'no-agent':['parada sem agente','stopped, no agent'],
  integrated:['integrada, sem commit','integrated, no commit'],committed:['comitada','committed'],rejected:['reprovada','rejected'],backlog:['no backlog','in the backlog']};
// A onda sem agente se lê como o layout a escreve.
const boardStates={'no-agent':['em execução sem agente','running without an agent']};
const kindNames={task:['Tarefas','Tasks'],criterion:['Critérios','Criteria'],decision:['Decisões','Decisions'],rule:['Regras','Rules'],question:['Perguntas','Questions']};
const filterNames={all:['Todas','All'],attention:['Pedem atenção','Need attention'],committed:['Comitadas','Committed'],uncommitted:['Sem commit','Not committed']};
const reasons={'wave-no-agent':['parada sem agente','stopped, no agent'],'wave-rejected':['reprovada','rejected'],
  'criterion-failing':['prova falhando','proof failing'],'criterion-without-proof':['sem prova','no proof'],'question-open':['aberta','open']};
const authors={assistant:['pelo condutor','by the conductor'],user:['pelo usuário','by the user'],round:['pela rodada','by the round']};
// How an item another one points to is named on screen: by its kind in words.
const itemKinds={criterion:['O critério','The criterion'],decision:['A decisão','The decision'],rule:['A regra','The rule'],request:['O pedido','The request'],
  limit:['O limite','The limit'],point:['A pergunta','The question'],task:['A tarefa','The task']};
// How a field a version changed is named on screen, in words.
const fieldNames={depends_on:['dependência','dependency'],files:['arquivo','file'],covers:['cobertura','coverage'],done_when:['término','done-when'],
  agent:['parte do agente',"agent's part"],skill:['skill','skill'],blocked_by:['bloqueio','block'],priority:['prioridade','priority'],group:['grupo','group'],
  cleanup:['limpeza','cleanup'],must_read:['leitura obrigatória','required reading'],retry_guard:['trava de repetição','retry guard'],role_pair:['par de papéis','role pair'],
  distinct:['motivo de trabalho próprio','own-work reason'],proof:['prova','proof'],why:['motivo','reason'],reason:['motivo','reason'],origin:['origem','origin'],
  effect:['efeito','effect'],status:['estado','state'],result:['resultado','result']};
const titles={overview:['Visão geral','Overview'],spec:['Spec','Spec'],items:['Itens','Items'],item:['Item','Item'],usage:['Consumo','Usage'],settings:['Configurações','Settings']};

// A paleta do layout é a do tema escuro, a mesma nas duas superfícies; o tema
// claro é a variante. Os nomes dizem o papel: o fundo da página, o do
// cabeçalho, o do cartão e o do quadro de dentro dele; a linha; o texto, o
// secundário e o terciário; o mostarda, único destaque, com o texto de
// destaque, a tinta e a borda dela; o aviso laranja; o verde.
const palettes={
  dark:{page:'#0B0B0A',header:'#0F0F0E',card:'#141413',box:'#1C1B19',line:'#2A2926',track:'#2A2926',dashed:'#3A3834',empty:'#4A4843',
    ink:'#F5F3EC',soft:'#A9A69B',muted:'#77746B',mustard:'#F2B705',accent:'#FFC933',deep:'#8A6D0A',buttonInk:'#1A1500',
    tint:'#26200B',tintLine:'#5A4811',warning:'#F5A15C',warningSoft:'#26190F',warningLine:'#58391F',warningInk:'#F9C79B',attention:'#221810',success:'#5BCB8A'},
  light:{page:'#FAFAF7',header:'#FFFFFF',card:'#FFFFFF',box:'#F5F3EC',line:'#E6E4DC',track:'#F0EEE7',dashed:'#D9D6CC',empty:'#CFCBBF',
    ink:'#2B2B29',soft:'#6B6A64',muted:'#8A877D',mustard:'#E1AD01',accent:'#8A6700',deep:'#EFD27A',buttonInk:'#1A1500',
    tint:'#FBF1CF',tintLine:'#EFD27A',warning:'#B4570B',warningSoft:'#FCEFE3',warningLine:'#F0C9A4',warningInk:'#7A3B07',attention:'#FFF8F1',success:'#2F7D4F'},
};
/** A paleta do tema da pessoa, como a linha `theme` do `/config` o nomeia. */
export function panelPalette(theme) {
  return String(theme ?? '').includes('light')?palettes.light:palettes.dark;
}
// Os ícones do layout, no traço e no tamanho dele.
const stroke=(size,width,color,paths)=>`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="${color}" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;
const icons={
  warning:color=>stroke(18,1.9,color,'<path d="M12 9v4"/><path d="M12 17h.01"/><path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/>'),
  search:color=>stroke(16,2,color,'<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>'),
  down:color=>stroke(16,2,color,'<path d="M6 9l6 6 6-6"/>'),
  up:color=>stroke(16,2,color,'<path d="M6 15l6-6 6 6"/>'),
  back:color=>stroke(16,2,color,'<path d="M15 18l-6-6 6-6"/>'),
};
// Os mesmos ícones no terminal, num caractere de uma célula, e os dos botões
// de ícone, que no desktop também são caractere: o botão do aplicativo não
// aceita desenho Svg dentro dele.
const glyphs={warning:'▲',search:'⌕',down:'▾',up:'▴',back:'‹',refresh:'↻',settings:'⚙'};

export function renderPanel({elements,surface,columns,state,screens={},project,usage,settings,measuring=false,publication,problem,theme,focused,now=Date.now(),actions}) {
  const {Box:BoxOf,Text:TextOf,Button,Select,Input,Svg}=elements;
  const terminal=surface==='terminal', paint=panelPalette(theme);
  const english=project?.language==='en-US', locale=english?'en-US':'pt-BR';
  const t=(pt,en)=>english?en:pt, name=pair=>pair?t(...pair):undefined;
  const clean=value=>String(value ?? '').replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g,'');
  const upper=value=>value?value.charAt(0).toLocaleUpperCase(locale)+value.slice(1):value;
  // O layout escreve os títulos em maiúsculas pelo estilo; aqui o texto já vai assim.
  const caps=value=>clean(value).toLocaleUpperCase(locale);
  // O espaço entre as coisas, numa regra só, no px do layout. O desktop o
  // troca pelo degrau mais perto dele; no empate, o menor. Os degraus valem
  // aqui xs 4, sm 8, md 12, lg 16 e xl 24 px: suposição não conferida numa
  // tela real, porque a documentação não diz quantos px vale cada degrau no
  // desktop. O terminal leva o px até a caixa, que o conta em células.
  const steps=[[0,0],['xs',4],['sm',8],['md',12],['lg',16],['xl',24]];
  const PX=Symbol('px');
  const space=px=>terminal?{[PX]:px}:steps.reduce((near,step)=>Math.abs(step[1]-px)<Math.abs(near[1]-px)?step:near)[0];
  // No terminal o px vira células. Entre dois textos, uma coluna a cada oito
  // px a partir de quatro: o que o layout afasta 12 px ou mais fica a duas
  // colunas ou mais. A margem de dentro de uma caixa, uma coluna a cada dez
  // px, ao menos uma, porque a letra do terminal é mais larga que a do layout.
  // Na vertical, uma linha em branco entre os blocos que o layout afasta 14 px
  // ou mais e, por dentro de uma superfície sem borda, a linha dos 16 px ou
  // mais; dentro da borda, nenhuma, porque a borda já é a folga. Um degrau xs
  // ou sm não daria linha nenhuma, por isso o terminal nunca recebe um.
  const sides={padding:['paddingX','paddingY'],margin:['marginX','marginY'],gap:['columnGap','rowGap']};
  const across=['paddingX','paddingLeft','paddingRight','marginX','marginLeft','marginRight'];
  const cells=(prop,px,edged=false)=>prop==='columnGap'?(px>0?Math.max(1,Math.floor((px+4)/8)):0):across.includes(prop)?(px>0?Math.max(1,Math.round(px/10)):0)
    :prop==='rowGap'?(px>=14?1:0):edged?0:px>=16?1:0;
  const Box=terminal?props=>BoxOf(Object.fromEntries(Object.entries(props).flatMap(([prop,value])=>value?.[PX]==null?[[prop,value]]
    :(sides[prop] ?? [prop]).map(side=>[side,cells(side,value[PX],Boolean(props.borderStyle))])))):BoxOf;
  // Dentro de um cartão do terminal, as medidas que a barra fina já separa
  // ficam sem linha em branco entre elas, para o cartão ter a altura do layout.
  const snug=px=>terminal?0:space(px);
  // Duas superfícies cheias, uma embaixo da outra, ficam no terminal a uma
  // linha uma da outra: sem ela o fundo de uma emenda no da outra.
  const apart=px=>terminal?1:space(px);
  // O terminal tem um só tamanho e uma só face de letra: o tamanho, a face
  // fixa e os algarismos de largura igual ficam no desktop, e o seminegrito
  // vira negrito.
  const Text=terminal?({size,mono,tabular,weight,...props})=>TextOf({...props,...(['semibold','bold'].includes(weight)?{bold:true}:{})}):TextOf;
  const text=(value,props={})=>Text({children:clean(value),wrap:'wrap',color:paint.ink,...props});
  // O texto pequeno do layout: o secundário e o terciário.
  const soft=(value,props={})=>text(value,{size:'caption',color:paint.soft,...props});
  const label=(value,props={})=>text(value,{size:'caption',color:paint.muted,...props});
  const join=(...parts)=>parts.filter(part=>part!=null && part!=='').join(' · ');
  const joined=(parts,make=text)=>{const kept=parts.filter(part=>part!=null && part!=='');return kept.length?[make(join(...kept))]:[];};
  const int=value=>value==null?'—':Number(value).toLocaleString(locale);
  // Numbers keep plain spaces: a terminal draws the no-break one as any other.
  const spaced=value=>value.replace(/[  ]/g,' ');
  const compact=(value,digits=3)=>value==null?'—':spaced(new Intl.NumberFormat(locale,{notation:'compact',maximumSignificantDigits:digits}).format(value));
  // A rounded figure drops the cents only from a hundred dollars up.
  const money=(micro,round=false)=>{if(micro==null)return t('não medido','not measured');const digits=round && micro>=1e8?0:2;
    return spaced(new Intl.NumberFormat(locale,{style:'currency',currency:'USD',minimumFractionDigits:digits,maximumFractionDigits:digits}).format(micro/1e6));};
  // A date reads as it was recorded, never shifted into another zone.
  const day=value=>{const m=/^(\d{4})-(\d{2})-(\d{2})/.exec(String(value ?? ''));return m?(english?`${m[2]}/${m[3]}`:`${m[3]}/${m[2]}`):'—';};
  const hour=value=>/T(\d{2}:\d{2})/.exec(String(value ?? ''))?.[1] ?? '';
  const stamp=value=>[day(value),hour(value)].filter(Boolean).join(', ');
  // Um instante que o host mede em UTC se lê no relógio de quem olha.
  const clock=value=>{const at=new Date(value);return Number.isNaN(at.getTime())?'':spaced(at.toLocaleTimeString(locale,{hour:'2-digit',minute:'2-digit'}));};
  const calendar=value=>{const at=new Date(value);return Number.isNaN(at.getTime())?'—':at.toLocaleDateString(locale,{day:'2-digit',month:'2-digit'});};
  // Dois instantes no mesmo dia do relógio de quem olha, cada um no fuso dele.
  const today=(value,at)=>{const a=new Date(value),b=new Date(at);return !Number.isNaN(a.getTime()) && !Number.isNaN(b.getTime()) && a.toDateString()===b.toDateString();};
  const percent=value=>value==null?'—':`${Number(value).toLocaleString(locale,{maximumFractionDigits:1})}%`;
  const plural=(n,one,many)=>`${int(n)} ${n===1?one:many}`;
  const row=(children,props={})=>Box({flexDirection:'row',flexWrap:'wrap',columnGap:space(8),...props,children});
  const column=(children,props={})=>Box({flexDirection:'column',...props,children});
  // O que ocupa o resto da linha e empurra o seguinte para a direita.
  const fill=(...children)=>Box({flexGrow:1,flexShrink:1,children});
  // A tecla aprovada de cada botão: só no terminal, que a desenha antes do
  // rótulo; no desktop ela viraria uma tecla ao lado do botão, que o layout não tem.
  const hot=letter=>terminal?{hotkey:letter}:{};
  // As superfícies das etiquetas e dos botões do layout: o fundo, a borda e o texto.
  const tones={solid:[paint.mustard,paint.mustard,paint.buttonInk],mustard:[paint.tint,paint.tintLine,paint.accent],
    warning:[paint.warningSoft,paint.warningLine,paint.warningInk],card:[paint.card,paint.line,paint.soft],outline:[paint.card,paint.dashed,paint.ink],
    box:[paint.box,paint.line,paint.ink]};
  // A borda fina e o canto redondo de uma superfície. Os cartões a levam nas
  // duas superfícies, como no layout; os quadros de dentro deles, os avisos e
  // os controles de uma linha, só no desktop: no terminal cada borda custa
  // duas linhas, e eles são o fundo deles.
  const frame=color=>({borderStyle:'round',borderColor:color});
  const edge=color=>terminal?{}:frame(color);
  // Um texto que se aperta, como os links do layout: o botão sem moldura com
  // o texto na cor, no tamanho e no peso do desenho.
  const link=(key,caption,onPress,face={},extra={})=>Button({key,label:clean(caption),plain:true,onPress,...extra,
    children:[Text({children:clean(caption),color:paint.accent,size:'caption',weight:'semibold',...face})]});
  // Um botão com superfície. No terminal, o rótulo sobre o fundo do tom,
  // depois da tecla aprovada; no desktop, o botão nativo do aplicativo: o
  // principal no lugar do mostarda cheio, os outros com o rótulo na cor do tom.
  const action=(key,caption,onPress,tone='solid',extra={})=>{const [ground,,ink]=tones[tone];
    if(!terminal)return Button({key,label:clean(caption),plain:true,onPress,...extra,
      ...(tone==='solid'?{variant:'primary'}:{children:[Text({children:clean(caption),size:'footnote',weight:'semibold',color:ink})]})});
    return Button({key,label:clean(caption),plain:true,onPress,...extra,children:[TextOf({children:` ${clean(caption)} `,backgroundColor:ground,color:ink,bold:true})]});};
  // Os botões de ícone do cabeçalho: o ícone apagado sobre o fundo do cartão.
  const glyphButton=(key,glyph,alt,onPress,extra={})=>Button({key,label:alt,plain:true,onPress,...extra,
    children:[terminal?TextOf({children:` ${glyph} `,color:paint.soft,backgroundColor:paint.card}):Text({children:glyph,color:paint.soft})]});
  // Uma etiqueta do layout: a de estado no mostarda, com o ponto, e a de aviso no laranja.
  const pill=(caption,tone,dot)=>{const [ground,line,ink]=tones[tone];
    if(terminal)return TextOf({backgroundColor:ground,children:[' ',...(dot?[TextOf({children:'● ',color:dot,backgroundColor:ground})]:[]),
      TextOf({children:clean(caption),color:ink,bold:true,backgroundColor:ground}),' ']});
    return Box({flexDirection:'row',flexWrap:'nowrap',alignItems:'center',columnGap:space(6),paddingX:space(10),backgroundColor:ground,...edge(line),
      children:[...(dot?[Text({children:'●',size:'caption',color:dot})]:[]),text(caption,{size:'caption',weight:'semibold',color:ink})]});};
  // Uma tecla sem botão, na pílula, e o que ela faz.
  const hint=(keyText,caption)=>row([TextOf({children:` ${keyText} `,backgroundColor:paint.box,color:paint.accent}),text(caption,{color:paint.muted})],{columnGap:1,flexWrap:'nowrap'});
  // Um traço que enche a largura da caixa: a linha cortada nela.
  const run=(glyph,color,props={})=>Box({height:1,overflow:'hidden',...props,children:[TextOf({children:glyph.repeat(240),color})]});
  // An item by its own text, never by its code: `A regra “…”`.
  const named=item=>[name(itemKinds[item.kind]),`“${item.title || t('sem título','untitled')}”`].filter(Boolean).join(' ');
  // O título pequeno dos blocos, em maiúsculas e no texto terciário.
  const heading=title=>text(caps(title),{size:'caption',weight:'semibold',color:paint.muted});
  // O cartão do layout: o fundo do cartão e a borda fina; o tracejado não tem
  // fundo, e o de aviso leva o laranja.
  const card=(children,{dashed,tone,...props}={})=>Box({flexDirection:'column',
    ...(dashed?{borderStyle:'dashed',borderColor:paint.dashed}:tone==='warning'?{backgroundColor:paint.warningSoft,...frame(paint.warningLine)}:{backgroundColor:paint.card,...frame(paint.line)}),
    paddingX:space(18),paddingY:space(18),rowGap:space(14),...props,children});
  // O quadro de dentro de um cartão: o fundo mais claro, sem borda à vista.
  const inset=(children,props={})=>Box({flexDirection:'column',backgroundColor:paint.box,...edge(paint.box),padding:space(12),...props,children});
  // A seção: o título e o conteúdo a 10 px um do outro, o título com o que fica à direita dele.
  const section=(title,content,aside)=>column([...(title?[aside?pair(heading(title),aside,{alignItems:'flex-end'}):heading(title)]:[]),...content],{rowGap:space(10)});
  // Uma divisória de 1 px que ocupa a largura toda, na cor da linha.
  const rule=(color=paint.line)=>terminal?run('─',color):Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 1" width="1000" height="1" preserveAspectRatio="none"><rect width="1000" height="1" fill="${color}"/></svg>`,
    alt:t('divisória','divider'),height:1});
  // A lista do layout: as linhas no cartão, uma divisória entre elas.
  const rows=items=>card(items.flatMap((item,n)=>n?[rule(),item]:[item]),{paddingX:0,paddingY:0,rowGap:0});
  // Uma linha de lista: 13 px por 18 px.
  const entry=(children,props={})=>column(children,{paddingX:space(18),paddingY:space(13),rowGap:space(2),...props});
  // Um bloco: o título e as linhas num cartão.
  const block=(title,lines,{aside,...look}={})=>section(title,[card(lines,look)],aside);
  // Uma barra do layout: o trilho e a parte cheia, com o que ela diz. Ela
  // enche a coluna, como a divisória: sem largura no elemento, o desenho é mais
  // largo que a coluna e, sem viewBox, as partes em porcentagem acompanham a
  // largura sem esticar as pontas redondas. No terminal a barra é fina: meio
  // bloco de altura, a parte cheia na porcentagem inteira dela e o trilho no resto.
  const portion=value=>value>0?`${Math.max(1,Math.round(value))}%`:undefined;
  const bar=(fraction,caption,{height=6,color=paint.mustard,track=paint.track}={})=>{
    const full=Math.round(Math.max(0,Math.min(1,Number(fraction) || 0))*1000)/10,round=height/2;
    if(terminal)return row([...(full?[run('▄',color,{width:portion(full)})]:[]),...(full<100?[run('▄',track,{flexGrow:1,width:0})]:[])],{flexWrap:'nowrap',columnGap:0});
    return Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="${height}"><rect width="100%" height="${height}" rx="${round}" fill="${track}"/>${full
      ?`<rect width="${full}%" height="${height}" rx="${round}" fill="${color}"/>`:''}</svg>`,alt:caption,height});
  };
  const icon=(kind,color,alt)=>{if(terminal)return text(glyphs[kind],{color});const size=kind==='warning'?18:16;return Svg({source:icons[kind](color),alt,width:size,height:size});};
  // O aviso do layout: o laranja suave com a borda, o ícone e o texto.
  const alert=(value,extra=[],{align='flex-start',padX=13}={})=>Box({flexDirection:'row',flexWrap:'nowrap',alignItems:align,columnGap:space(10),backgroundColor:paint.warningSoft,
    ...edge(paint.warningLine),paddingX:space(padX),paddingY:space(11),
    children:[icon('warning',paint.warning,t('aviso','warning')),fill(text(value,{size:'footnote',color:paint.warningInk})),...extra]});
  // Um botão, um ponto ou um rótulo que não encolhe: num painel estreito o
  // texto ao lado quebra, ele fica inteiro.
  const whole=node=>Box({flexShrink:0,children:[node]});
  const dot=color=>whole(text('●',{size:'body',color}));
  // Duas coisas numa linha, a primeira à esquerda e a segunda à direita;
  // quando não cabem juntas, a segunda desce inteira para a linha de baixo.
  const pair=(left,right,props={})=>row([fill(left),Box({flexGrow:1,flexShrink:0,justifyContent:'flex-end',children:[right]})],{alignItems:'center',...props});
  // Um texto longo do terminal quebra nas pausas dele, depois de uma vírgula,
  // de um ponto ou de um "·": cada pedaço é um texto que leva o espaço do fim,
  // e o que não cabe desce inteiro. No desktop é um texto só.
  // O que fica à direita, como o estado da revisão, alinha cada linha à
  // direita, e o espaço vai no começo do pedaço seguinte, para a linha que
  // quebra terminar rente à borda.
  const phrases=(value,props={},justify='flex-start')=>{const parts=clean(value).split(/(?<=[,.·]) (?=\S)/),right=justify==='flex-end';
    return terminal && parts.length>1?row(parts.map((part,n)=>text(right?(n?` ${part}`:part):n<parts.length-1?`${part} `:part,{wrap:'wrap',...props})),
      {columnGap:0,flexGrow:1,flexShrink:1,justifyContent:justify}):text(value,props);};
  // A largura que sobra para o texto de um cartão no terminal: a tela menos a
  // margem da tela, a borda e a margem do cartão.
  const inner=()=>(width ?? 100)-2*cells('paddingX',20)-2-2*cells('paddingX',18);
  // Um caminho, uma branch ou o nome de uma spec, sem espaço onde quebrar: no
  // terminal, quando não cabe na largura que tem, ele quebra depois de uma
  // barra ou de um hífen, nunca no meio de um nome.
  const broken=(value,room=inner())=>{value=String(value ?? '');if(!terminal || value.length<=room)return value;
    return value.split(/(?<=[\/-])/).reduce((lines,part)=>{const last=lines.at(-1);if(last!=null && (last+part).length<=room)lines[lines.length-1]=last+part;else lines.push(part);return lines;},[]).join('\n');};
  // Quantos quadros iguais cabem numa linha sem partir a palavra mais longa deles.
  const perRow=(count,longest,room,pad)=>{if(!terminal)return count;let n=count;while(n>1 && Math.floor((room-(n-1)*cells('columnGap',10))/n)-2*pad<longest)n--;return n;};
  // Um número com o total ao lado, menor e apagado: "584/584".
  const ratio=(done,total)=>Text({children:[Text({children:int(done),size:'heading',weight:'bold',tabular:true,color:paint.accent}),
    Text({children:`/${int(total)}`,size:'footnote',weight:'medium',color:paint.muted})]});
  // No terminal a tela ocupa a largura do painel até 110 colunas, para a linha
  // não ficar longa demais. As grades que o layout põe lado a lado (as medidas
  // do uso, os dados da spec e os cartões do consumo) se empilham abaixo da
  // largura que cada uma pede, para nenhuma palavra se partir no meio: as
  // medidas do uso a partir de 54 colunas, os cartões a partir de 44, os dados
  // da spec enquanto o valor mais longo cabe na metade; o resto quebra nas
  // palavras. Abaixo de 60 colunas o custo e o rodapé descem para a linha de baixo.
  const width=terminal && columns?Math.min(columns,110):undefined;
  const tight=Boolean(width) && width<60;
  const narrow=least=>Boolean(width) && width<least;
  const grid=(children,props={},least=60)=>narrow(least)?column(children,{rowGap:apart(10)}):row(children,props);
  const span=(part,least=60)=>narrow(least)?{}:{width:part};
  const state_=state;
  const screen=state_.screen;
  const answer=screens[screen];
  const body=[];

  function airLine(air) {
    return [air.running?`${int(air.running)} ${t('no ar','running')}`:t('nenhuma no ar','none running'),
      `${int(air.free_slots)} ${t(air.free_slots===1?'vaga livre':'vagas livres',air.free_slots===1?'free slot':'free slots')}`];
  }
  const queued=air=>`${int(air.backlog)} ${t('tarefas na fila','tasks queued')}`;
  const host=()=>{const limits=Object.fromEntries((usage?.rateLimits || []).map(limit=>[limit.kind,limit]));
    return {context:usage?.context?.percent,five:limits.five_hour?.percentUsed,week:limits.seven_day?.percentUsed};};
  const delivery=item=>[day(item.delivered_at),item.pr!=null?`PR #${item.pr}`:'',item.rules?`${int(item.rules)} ${t(item.rules===1?'regra':'regras',item.rules===1?'rule':'rules')}`:''];
  const phaseOf=phase=>upper(name(phases[phase]) ?? phase);

  // Tela Visão geral: Agora, Ondas no ar, Outras specs, Uso e o rodapé.
  function overview(d) {
    const s=d.spec,current=[];
    if(!s) current.push(text(t('Nenhuma spec aberta neste projeto.','No open spec in this project.')));
    else {
      // Na spec ainda no levantamento as tarefas e as ondas não nasceram: as
      // caixas dizem "—" no lugar de zero, e uma linha diz quando elas chegam.
      const early=['survey','plan','planned'].includes(s.phase) && !s.tasks.total;
      // Três quadros de colunas iguais: o feito sobre o total, o que ele conta e
      // a barra fina, que desce para o pé do quadro: as três ficam na mesma linha
      // quando uma legenda quebra e as outras não.
      const tile=(done,total,caption)=>inset([early?text('—',{size:'heading',weight:'bold',color:paint.muted}):ratio(done,total),soft(caption),
        ...(early?[]:[Box({flexGrow:1,children:[]}),bar(total?done/total:0,`${caption} ${int(done)}/${int(total)}`,{height:4})])],{flexGrow:1,width:'30%',rowGap:space(4),paddingX:space(terminal?8:12)});
      const tiles=early?[tile(0,0,t('tarefas','tasks')),tile(0,0,t('critérios','criteria')),tile(0,0,t('ondas','waves'))]
        :[tile(s.tasks.finished,s.tasks.total,t('tarefas feitas','tasks done')),tile(s.criteria.passed,s.criteria.total,t('critérios passando','criteria passing')),
          tile(s.waves.finished,s.waves.planned,t('ondas na base','waves in the base'))];
      // Três por linha, como o layout; menos no terminal estreito, para nenhuma palavra se partir.
      const per=perRow(3,t('critérios','criteria').length,inner(),1);
      // O nome e, à direita dele, a etiqueta da fase. No desktop o nome longo se
      // corta com reticências e a etiqueta não sai do lado dele; no terminal o
      // nome quebra no hífen e, sem lugar, a etiqueta desce à direita.
      const title=terminal?pair(text(broken(s.name),{size:'heading',weight:'bold'}),pill(phaseOf(s.phase),'mustard',paint.mustard),{columnGap:space(10)})
        :row([Box({flexGrow:1,flexShrink:1,minWidth:0,children:[text(s.name,{size:'heading',weight:'bold',wrap:'truncate-end'})]}),whole(pill(phaseOf(s.phase),'mustard',paint.mustard))],
          {flexWrap:'nowrap',alignItems:'center',columnGap:space(10)});
      current.push(column([title,
        text(s.goal,{size:'footnote',color:paint.soft})],{rowGap:space(3)}),
        column(Array.from({length:Math.ceil(3/per)},(_,n)=>row(tiles.slice(n*per,n*per+per),{flexWrap:'nowrap',columnGap:space(10)})),{rowGap:apart(10)}),
        ...(early?[label(t('As tarefas e as ondas nascem depois do levantamento.','Tasks and waves are born after the survey.'))]:[]));
      // Os avisos na ordem do layout: o PR que entrou na base antes da onda parada.
      const order=['pr-merged','wave-no-agent'];
      const warnings=[...d.warnings].sort((a,b)=>order.indexOf(a.kind)-order.indexOf(b.kind)).map(warning).filter(Boolean).flat();
      if(warnings.length)current.push(column(warnings,{rowGap:apart(8)}));
      // The binary's sentence for the step; its command stays off the screen.
      if(s.next?.text)current.push(Box({flexDirection:'row',flexWrap:'nowrap',alignItems:'center',columnGap:space(12),backgroundColor:paint.tint,...edge(paint.tintLine),
        paddingX:space(14),paddingY:space(12),children:[fill(column([text(caps(t('Próximo passo','Next step')),{size:'caption',weight:'semibold',color:paint.accent}),
          text(s.next.text,{size:'footnote'})],{rowGap:space(2)})),whole(action('next-see',t('Ver','See'),()=>actions.go('spec')))]}));
    }
    body.push(section(t('Agora','Now'),[card(current,{rowGap:space(16)})]));
    if(s) {
      const air=s.air,agents=d.usage?.session_agents;
      // O ponto e o texto juntos, a uma coluna, e o agente da sessão a duas, como os 12 px do layout.
      // Sem lugar para os dois, o agente desce para a linha de baixo, à direita.
      const working=row([dot(air.running?paint.mustard:paint.dashed),
        text(air.running?plural(air.running,t('onda trabalhando','wave working'),t('ondas trabalhando','waves working')):t('Nenhuma onda trabalhando','No wave working'),
          {color:air.running?paint.ink:paint.soft})],{flexWrap:'nowrap',alignItems:'center',columnGap:space(terminal?8:12)});
      body.push(section(t('Ondas no ar','Waves in the air'),[card([agents!=null?pair(working,label(plural(agents,t('agente na sessão','agent in the session'),t('agentes na sessão','agents in the session'))),
        {columnGap:space(12)}):working],
        {dashed:!air.running,paddingY:space(15)})],label(join(airLine(air)[1],air.backlog?queued(air):t('fila vazia','queue empty')))));
    }
    if(d.others?.shown?.length)body.push(section(t('Outras specs que pedem atenção','Other specs that need attention'),[rows(d.others.shown.map(other=>entry([
      pair(text(broken(other.name),{size:'body',weight:'semibold'}),label(join(phaseOf(other.phase),today(other.updated,d.at)?`${t('hoje','today')} ${hour(other.updated)}`:day(other.updated)))),
      ...(other.warning?.kind==='pr-merged'?[text(`PR #${other.warning.pr ?? '?'} ${t('entrou na base em','entered the base on')} ${day(other.warning.at)}`,{size:'caption',color:paint.warning})]:[])])))],
      link('see-specs',`${t('ver as','see all')} ${int(d.others.total)}`,()=>actions.go('spec'))));
    const {context,five,week}=host();
    const gauge=(caption,value)=>column([row([fill(soft(caption)),text(percent(value),{size:'caption',weight:'semibold',tabular:true})],{flexWrap:'nowrap'}),
      bar((value ?? 0)/100,`${caption} ${percent(value)}`)],{flexGrow:1,...span('30%',54),rowGap:space(6)});
    body.push(section(t('Uso','Usage'),[card([grid([gauge(t('Contexto','Context'),context),gauge(t('Cota 5 h','5 h quota'),five),gauge(t('Semana','Week'),week)],{flexWrap:'nowrap',columnGap:space(14)},54),
      rule(),row([text(money(d.usage?.cost_micro_usd,true),{size:'title',weight:'bold',tabular:true}),
        fill(soft(join(t('no projeto','in the project'),`${int(d.usage?.sessions)} ${t('sessões','sessions')}`))),label(`${compact(d.usage?.tokens)} tokens`)],
        {flexWrap:tight?'wrap':'nowrap',alignItems:'flex-end',columnGap:space(8)})],{paddingY:space(16),rowGap:snug(14)})],
      link('usage-all',t('consumo completo','full usage'),()=>actions.go('usage'))));
    // The project summary as the binary gave it: the newest deliveries first.
    if(d.deliveries?.shown?.length)body.push(section(t('Entregas do projeto','Project deliveries'),[rows(d.deliveries.shown.map(item=>entry([text(item.title,{weight:'semibold'}),
      label(join(...delivery(item))),...(item.delivered?[text(item.delivered,{size:'footnote',color:paint.soft})]:[])])))],
      label(`${int(d.deliveries.shown.length)} ${t('de','of')} ${int(d.deliveries.total)} ${t('entregas','deliveries')}`)));
    if(publication?.text)body.push(text(publication.text,{size:'footnote'}));
  }

  function warning(item) {
    if(item.kind==='pr-merged') {
      const tail=`${t('mas a spec','but the spec')} ${name(still[item.phase]) ?? name(phases[item.phase]) ?? item.phase}`;
      return [alert(`${t('O PR','PR')} #${item.pr ?? '?'} ${t('entrou na base em','entered the base on')} ${day(item.at)}, ${tail}.`)];
    }
    // A revisão reaberta não entra aqui: o layout a mostra só no Fechamento da spec.
    if(item.kind==='wave-no-agent') {
      const one=item.waves.length===1,list=item.waves.join(', ');
      return [alert(one?t(`A onda ${list} consta em execução, sem agente nem commit. Ela segura a entrega.`,`Wave ${list} is still running, with no agent and no commit. It holds the delivery.`)
        :t(`As ondas ${list} constam em execução, sem agente nem commit. Elas seguram a entrega.`,`Waves ${list} are still running, with no agent and no commit. They hold the delivery.`))];
    }
    return undefined;
  }

  const counts=c=>[
    // Sem nenhuma tarefa ou nenhum critério ainda, a caixa não diz estado:
    // zero de zero não é "todas feitas" nem "todos com prova".
    {key:'items-task',kind:'task',figure:c.tasks.total,caption:t('tarefas','tasks'),status:!c.tasks.total?undefined:c.tasks.finished===c.tasks.total?[t('todas feitas','all done'),paint.success]
      :[`${int(c.tasks.finished)} ${t('feitas','done')}`,paint.muted]},
    {key:'items-criterion',kind:'criterion',figure:c.criteria.total,caption:t('critérios','criteria'),status:!c.criteria.total?undefined:c.criteria.failed?[`${c.criteria.failed} ${t('falhando','failing')}`,paint.warning]
      :c.criteria.without_proof?[`${c.criteria.without_proof} ${t('sem prova','without proof')}`,paint.warning]:[t('todos com prova','all proven'),paint.success]},
    // As perguntas contam as que seguem marcadas abertas, como o layout.
    {key:'items-question',kind:'question',figure:c.questions.open,caption:t('perguntas','questions'),status:[t('marcadas abertas','marked open'),paint.muted]},
    {key:'items-decision',kind:'decision',figure:c.decisions,caption:t('decisões','decisions')},
    {key:'items-rule',kind:'rule',figure:c.rules,caption:t('regras','rules')}];
  const review=r=>r.valid?[`${t('aprovada em','approved on')} ${day(r.approved_at)}`,paint.success]
    :r.reopened_at?[`${t('aprovada em','approved on')} ${day(r.approved_at)}, ${t('reaberta','reopened')} ${day(r.reopened_at)===day(r.approved_at)?`${t('às','at')} ${hour(r.reopened_at)}`:`${t('em','on')} ${day(r.reopened_at)}`}`,paint.warning]
    :[t('ainda não aprovada','not approved yet'),paint.muted];

  // Tela Spec: a spec em uso, os dados, o combinado, as ondas, o fechamento e a troca.
  function spec(d) {
    const s=d.spec;
    // O seletor do layout: o nome, a fase e a seta; tocado, a lista de troca
    // sobe para logo abaixo dele, e a seta vira para cima.
    body.push(pair(Button({key:'spec-picker',label:clean(s.name),plain:true,onPress:actions.toggleSpecs,...hot('o'),
        children:[Text({children:broken(s.name,(width ?? 100)-2*cells('paddingX',20)-2*cells('paddingX',16)-3),size:'body',weight:'bold',color:paint.ink})]}),
      row([text(phaseOf(s.phase),{size:'caption',weight:'semibold',color:paint.accent}),
        icon(state_.pickSpec?'up':'down',paint.muted,state_.pickSpec?t('esconder a troca de spec','hide the spec switch'):t('trocar de spec','switch spec'))],{flexWrap:'nowrap',alignItems:'center',columnGap:space(10)}),
      {columnGap:space(10),backgroundColor:paint.card,...edge(paint.line),paddingX:space(16),paddingY:space(12)}));
    if(state_.pickSpec)body.push(specList(d));
    // Os quatro dados em duas colunas, enquanto a palavra mais longa deles,
    // contada até a barra ou o hífen onde a branch quebra, cabe na metade do
    // cartão; abaixo disso, um embaixo do outro. A branch quebra no hífen.
    const facts=[[t('Branch','Branch'),s.branch ?? '—',true],[t('Sai de','From'),s.base ?? '—',true],[t('Aberta','Opened'),stamp(s.opened_at)],
      [t('Último commit','Last commit'),s.last_commit?join(stamp(s.last_commit.at),`${int(s.commits)} commits`):t('sem commit','no commit')]];
    const word=Math.max(...facts.flatMap(([caption,value])=>[caption,value].flatMap(part=>part.split(/\s+|(?<=[\/-])/))).map(part=>part.length));
    const least=2*word+2*cells('paddingX',20)+2+2*cells('paddingX',18)+cells('columnGap',16);
    const half=narrow(least)?inner():Math.floor((inner()-cells('columnGap',16))/2);
    // No terminal as duas colunas dividem a linha por igual, sem largura própria
    // que precise encolher: cada uma tem exatamente a metade que a branch usa para quebrar.
    const fact=([caption,value,mono])=>column([label(caption),mono?text(broken(value,half),{mono:true,size:'caption'}):phrases(value,{size:'footnote'})],
      {flexGrow:1,...(terminal && !narrow(least)?{width:0}:span('50%',least))});
    // O objetivo a 14 px da grade, e as duas linhas da grade a 12 px uma da outra.
    body.push(card([text(s.goal,{size:'body',weight:'medium'}),column([grid(facts.slice(0,2).map(fact),{flexWrap:'nowrap',columnGap:space(16)},least),
      grid(facts.slice(2).map(fact),{flexWrap:'nowrap',columnGap:space(16)},least)],{rowGap:space(12)})]));
    // Seis cartões iguais, de 13 px de padding: o número grande, o tipo e o
    // estado. O número de cada um se aperta, porque o aplicativo só deixa
    // apertar um botão, e o botão leva a moldura dele: as cinco primeiras abrem
    // a lista, a das tarefas também na tecla t, e a das ondas abre o Consumo,
    // onde estão os agentes das ondas.
    const tile=children=>column(children,{flexGrow:1,width:'30%',backgroundColor:paint.card,...frame(paint.line),padding:space(13)});
    const figure=value=>Text({children:int(value),size:'title',weight:'bold',tabular:true,color:paint.ink});
    const press=(key,caption,value,onPress,extra={})=>Button({key,label:`${int(value)} ${caption}`,plain:true,onPress,...extra,children:[figure(value)]});
    const tiles=counts(s.counts).map(line=>tile([press(line.kind==='task'?'go-items':line.key,line.caption,line.figure,()=>actions.go('items',{kind:line.kind}),line.kind==='task'?hot('t'):{}),
      soft(line.caption),...(line.status?[text(line.status[0],{size:'caption',color:line.status[1]})]:[])]));
    tiles.push(tile([press('go-waves',t('ondas','waves'),s.counts.waves,()=>actions.go('usage')),soft(t('ondas','waves'))]));
    // Três por linha, como o layout; menos no terminal estreito, para nenhuma palavra se partir.
    const per=perRow(3,t('perguntas','questions').length,(width ?? 100)-2*cells('paddingX',20),1+cells('paddingX',13));
    body.push(section(t('O que foi combinado','What was agreed'),[column(Array.from({length:Math.ceil(tiles.length/per)},(_,n)=>row(tiles.slice(n*per,n*per+per),{flexWrap:'nowrap',columnGap:space(10)})),
      {rowGap:apart(10)})]));
    // As ondas numa barra só e a legenda de cada parte.
    const w=s.waves,wavesOf=list=>`${t(list.length===1?'onda':'ondas',list.length===1?'wave':'waves')} ${list.join(', ')}`;
    const parts=[{value:w.committed,color:paint.ink,caption:`${int(w.committed)} ${t('comitadas','committed')}`},{value:w.integrated,color:paint.mustard,caption:`${int(w.integrated)} ${t('integradas','integrated')}`},
      {value:w.running,color:paint.tintLine,caption:`${int(w.running)} ${t('em execução','running')}`},{value:w.todo,color:paint.dashed,caption:`${int(w.todo)} ${t('a fazer','to do')}`},
      {value:w.no_agent.length,color:paint.warning,warning:true,caption:`${int(w.no_agent.length)} ${t('em execução sem agente','running without an agent')} (${wavesOf(w.no_agent)})`},
      {value:w.rejected.length,color:paint.warning,warning:true,caption:`${plural(w.rejected.length,t('reprovada','rejected'),t('reprovadas','rejected'))} (${wavesOf(w.rejected)})`}].filter(part=>part.value);
    // Sem onda nenhuma ainda, como no levantamento, o bloco não aparece vazio.
    if(parts.length)body.push(section(t('Ondas','Waves'),[card([stack(parts,Math.max(w.planned,parts.reduce((sum,part)=>sum+part.value,0))),
      row(parts.map(part=>row([text('■',{size:'caption',color:part.color}),text(part.caption,{size:'caption',...(part.warning?{color:paint.warning}:{})})],{columnGap:space(6),flexWrap:'nowrap'})),
        {columnGap:space(16),rowGap:space(6)})],{paddingY:space(16),rowGap:space(12)})]));
    // O nome à esquerda, inteiro, e o estado à direita, como no layout; quando
    // não cabe, ele quebra depois da vírgula e cada linha segue à direita.
    const closing=(caption,[status,color])=>entry([row([whole(text(caption)),row([phrases(status,{size:'footnote',color},'flex-end')],{flexGrow:1,flexShrink:1,justifyContent:'flex-end'})],
      {flexWrap:'wrap',columnGap:space(10)})]);
    const pr=s.pr?.merged_at?[`${t('entrou na base em','entered the base on')} ${day(s.pr.merged_at)}`,paint.success]:s.pr?.number?[t('aberto','open'),paint.ink]:[t('sem PR','no PR'),paint.muted];
    body.push(section(t('Fechamento','Closing'),[rows([closing(t('Revisão final','Final review'),review(s.review)),closing(`Pull request${s.pr?.number?` #${s.pr.number}`:''}`,pr)])]));
    if(!state_.pickSpec)body.push(specList(d));
  }

  // Várias partes numa barra só, arredondada nas pontas, com o que cada uma
  // diz; ela enche a coluna como a barra simples, as partes em porcentagem.
  function stack(parts,total,height=10) {
    if(terminal)return row([...parts.map(part=>run('▄',part.color,{width:portion(part.value/(total || 1)*100)})),run('▄',paint.track,{flexGrow:1,width:0})],
      {flexWrap:'nowrap',columnGap:0});
    let x=0;
    const marks=parts.map(part=>{const size=part.value/(total || 1)*100,mark=`<rect x="${x.toFixed(1)}%" width="${size.toFixed(1)}%" height="${height}" fill="${part.color}"/>`;x+=size;return mark;}).join('');
    return Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="${height}"><clipPath id="ends"><rect width="100%" height="${height}" rx="${height/2}"/></clipPath><g clip-path="url(#ends)"><rect width="100%" height="${height}" fill="${paint.track}"/>${marks}</g></svg>`,
      alt:parts.map(part=>part.caption).join(', '),height});
  }

  function specList(d) {
    const groups=[[t('Em andamento','In progress'),['approved','running','closed','pr_open']],[t('Levantamento','Survey'),['survey','plan','planned',null,undefined]]];
    const done=d.specs.filter(item=>item.phase==='delivered'),dropped=Math.max(d.discarded ?? 0,d.specs.filter(item=>item.phase==='discarded').length);
    const since=item=>['survey','plan','planned'].includes(item.phase)?`${t('desde','since')} ${day(item.created)}`:undefined;
    const lines=[];
    const shut=join(plural(done.length,t('entregue','delivered'),t('entregues','delivered')),dropped?plural(dropped,t('descartada','discarded'),t('descartadas','discarded')):'');
    const closed=d.specs.filter(item=>['delivered','discarded'].includes(item.phase));
    // A spec em uso se destaca na tinta mostarda, como no layout; as linhas
    // têm 9 px por 18 px, e só os grupos levam a divisória.
    const line=item=>{const on=item.name===d.selected_spec,survey=Boolean(since(item));
      return pair(Button({key:`spec-${item.name}`,label:clean(item.name),plain:true,onPress:()=>actions.selectSpec(item.name),
          children:[Text({children:broken(item.name),size:'footnote',weight:on?'bold':survey?'regular':'medium',color:paint.ink})]}),
        text(since(item) ?? phaseOf(item.phase),{size:'footnote',color:on?paint.accent:paint.muted}),
        {paddingX:space(18),paddingY:space(9),...(on?{backgroundColor:paint.tint}:{})});};
    groups.forEach(([title,members],n)=>{const found=d.specs.filter(item=>members.includes(item.phase));
      if(found.length)lines.push(...(n && lines.length?[rule()]:[]),column([heading(title)],{paddingX:space(18),paddingTop:space(10),paddingBottom:space(4)}),...found.map(line));});
    if(done.length || dropped)lines.push(rule(),row([fill(link('delivered',shut,actions.toggleDelivered,{size:'footnote',weight:'regular'})),text(state_.delivered?'⌄':'›',{color:paint.accent})],
      {flexWrap:'nowrap',alignItems:'center',paddingX:space(18),paddingY:space(12)}));
    if(state_.delivered)lines.push(...closed.map(line));
    return section(`${t('Trocar de spec','Switch spec')} · ${d.specs.length}`,[card(lines,{paddingX:0,paddingY:0,rowGap:0})]);
  }

  function rowDetail(item,kind) {
    const reason=(item.attention==='wave-no-agent'?name(boardStates['no-agent']):undefined) ?? name(reasons[item.attention]);
    const wave=item.wave!=null?`${t('Onda','Wave')} ${item.wave}`:'';
    if(kind==='task') {
      if(item.wave==null)return t('no backlog','in the backlog');
      const stateText=reason ?? `${name(waveStates[item.state]) ?? item.state}${item.state==='committed' && item.at?` ${t('em','on')} ${day(item.at)}`:''}`;
      // A linha que pede atenção diz o motivo e os reenvios, sem os arquivos e
      // o que cobre, como o layout.
      const resent=item.resends?`${t('reenviada','resent')} ${item.resends} ${t(item.resends===1?'vez':'vezes',item.resends===1?'time':'times')}`:'';
      if(item.attention)return join(wave,stateText,resent);
      return join(wave,stateText,resent,
        item.unmet?t('combinado não atendido','agreement not met'):'',item.files==null?'':item.files?plural(item.files,t('arquivo','file'),t('arquivos','files')):t('nenhum arquivo','no file'),
        item.covers?`${t('cobre','covers')} ${plural(item.covers,t('item','item'),t('itens','items'))}`:'');
    }
    return reason ?? wave;
  }
  const countLine=(d,shown)=>join(`${int(shown)} ${t('de','of')} ${int(d.total)}`,
    !state_.query && d.filter==='all'?t('as que pedem atenção vêm primeiro','the ones needing attention come first'):'');

  // Tela Tarefas: o caminho de volta, as abas do tipo, a busca, os filtros e a lista.
  function items(d) {
    const kind=d.kind,more=['rule','question'];
    // No terminal as teclas da lista, que não têm botão, ficam ao lado do caminho de volta.
    // Num painel estreito elas descem juntas para a linha de baixo.
    if(terminal)body.push(row([...backLink(d),fill(),row([hint('↑↓',t('escolher','pick')),hint('enter',t('abrir','open'))],{columnGap:space(16),flexWrap:'nowrap'})],
      {alignItems:'center'}));
    else body.push(...backLink(d));
    // As abas do tipo no cartão: cada uma inteira, numa linha, crescendo para
    // dividir o que sobra; a aberta no mostarda cheio, as outras no texto
    // secundário, e o resto em Mais. No terminal cada aba leva uma coluna de
    // ar de cada lado, e num painel estreito elas descem para a linha de baixo.
    const tab=value=>{const on=value===kind,caption=`${name(kindNames[value])} ${int(d.counts[value])}`,go=()=>actions.setKind(value);
      if(!terminal)return Box({flexGrow:1,flexShrink:0,children:[on?action(`kind-${value}`,caption,go):link(`kind-${value}`,caption,go,{size:'footnote',weight:'medium',color:paint.soft})]});
      return Box({flexGrow:1,flexShrink:0,justifyContent:'center',paddingX:space(10),...(on?{backgroundColor:paint.mustard}:{}),
        children:[link(`kind-${value}`,caption,go,on?{weight:'bold',color:paint.buttonInk}:{weight:'medium',color:paint.soft})]});};
    // Mais é o seletor com o próprio nome na primeira opção: ele se lê "Mais ▾"
    // até a pessoa escolher Regras ou Perguntas, sem rótulo antes do valor.
    // Nada nessa linha encolhe abaixo do próprio texto: quando a largura não
    // dá para as quatro, o que não cabe desce inteiro para a linha de baixo,
    // em vez de uma aba quebrar em duas linhas ou o seletor se cortar.
    const pick=value=>{if(more.includes(value))actions.setKind(value);};
    const picker=Select?Select({key:'kind-more',value:more.includes(kind)?kind:'more',
      options:[{value:'more',label:t('Mais','More')},...more.map(value=>({value,label:`${name(kindNames[value])} ${int(d.counts[value])}`}))],onSelect:pick}):undefined;
    body.push(row([...['task','criterion','decision'].map(tab),...(picker?[whole(picker)]:more.map(tab))],
      {flexWrap:'wrap',alignItems:'center',columnGap:space(4),rowGap:space(4),backgroundColor:paint.card,...edge(paint.line),padding:space(4)}));
    // A busca do layout. No terminal ela abre na tecla b: até lá, o campo é o
    // botão que a abre, com o texto de espera dela.
    const searching=state_.searching || state_.query;
    const field=terminal && !searching?link('search-open',t('Buscar pelo título ou arquivo','Search by title or file'),actions.openSearch,{size:'body',weight:'regular',color:paint.muted},hot('b'))
      :Input?Input({key:'search',placeholder:t('Buscar pelo título ou arquivo','Search by title or file'),value:state_.query,...(terminal && state_.searching?{autoFocus:true}:{}),onSubmit:actions.search})
      :text(state_.query);
    body.push(row([icon('search',paint.muted,t('buscar','search')),fill(field)],
      {flexWrap:'nowrap',alignItems:'center',columnGap:space(10),backgroundColor:paint.card,...edge(paint.line),paddingX:space(14),paddingY:space(10)}));
    if(state_.query)body.push(row([label(`${t('Busca','Search')}: ${state_.query}`),link('search-clear',t('limpar','clear'),()=>actions.search(''))],{alignItems:'center'}));
    // Os filtros em pílulas: só Todas e Pedem atenção levam o número; a
    // escolhida na tinta mostarda, a de atenção no laranja. No terminal a tecla
    // f os troca; no desktop a escolhida é o botão principal do aplicativo.
    const shown=kind==='task'?FILTERS:['all','attention'];
    body.push(row([...shown.map(value=>{const caption=['all','attention'].includes(value)?`${name(filterNames[value])} ${int(d.filters[value])}`:name(filterNames[value]),go=()=>actions.setFilter(value);
      return action(`filter-${value}`,caption,go,value===d.filter?terminal?'mustard':'solid':value==='attention'?'warning':'card');}),
      ...(terminal?[link('filter',t('filtrar','filter'),actions.cycleFilter,{size:'body',weight:'regular',color:paint.muted},hot('f'))]:[])],{columnGap:space(8),rowGap:space(8)}));
    // Uma linha por item, com 15 px por 18 px: o ponto da cor do estado, o
    // título e o detalhe. A que pede atenção leva o fundo dela; no terminal a
    // linha que tem o foco leva a tinta mostarda e a barra na primeira coluna.
    const color=item=>item.attention?paint.warning:item.state==='committed'?paint.success:item.state==='integrated'?paint.mustard:paint.muted;
    const line=(item,n)=>{const detail=rowDetail(item,kind),title=item.title || t('Sem título','Untitled'),key=`item-${item.code}`,on=terminal && focused===key;
      return row([...(terminal?[whole(TextOf({children:on?'▌':' ',color:paint.mustard}))]:[]),dot(color(item)),column([link(key,title,()=>actions.openItem(item.code),
        {size:'body',weight:item.attention?'semibold':'medium',color:paint.ink},terminal && !n?{autoFocus:true}:{}),
        ...(detail?[phrases(detail,{size:'caption',color:item.attention?paint.warning:paint.muted})]:[])],{flexGrow:1,flexShrink:1,rowGap:space(2)})],
        {flexWrap:'nowrap',alignItems:'flex-start',columnGap:space(12),paddingX:space(18),paddingY:space(15),...(terminal?{paddingLeft:0}:{}),
          ...(on?{backgroundColor:paint.tint}:item.attention?{backgroundColor:paint.attention}:{})});};
    body.push(d.rows.length?rows(d.rows.map(line)):card([label(t('Nenhum item encontrado.','No item found.'))]));
    if(d.rows.length)body.push(row([fill(phrases(countLine(d,d.rows.length),{size:'footnote',color:paint.muted})),
      ...(d.rows.length<d.total?[whole(action('more',t('Mostrar mais','Show more'),actions.more,'outline',hot('m')))]:[])],
      {flexWrap:'nowrap',alignItems:'center',columnGap:space(10)}));
  }

  // O caminho de volta, em mostarda, com a seta e o nome da tela de onde se veio.
  function backLink(d) {
    const back=state_.back.at(-1);
    if(!back)return [];
    const where=back.screen==='spec'?d.selected_spec ?? name(titles.spec):back.screen==='items'?name(kindNames[back.kind]) ?? name(titles.items):name(titles[back.screen]) ?? '';
    const face={size:'footnote',weight:'semibold'};
    if(terminal)return [Button({key:'back',label:clean(`‹ ${where}`),plain:true,onPress:actions.back,...hot('v'),
      children:[TextOf({children:'‹ ',color:paint.accent,bold:true}),Text({children:broken(where,(width ?? 100)-2*cells('paddingX',20)-5),color:paint.accent,...face})]})];
    return [row([icon('back',paint.accent,t('voltar','back')),link('back',where,actions.back,face)],{flexWrap:'nowrap',alignItems:'center',columnGap:space(6)})];
  }

  // Tela Uma tarefa: o título e a etiqueta do estado, o pedido, o que o agente
  // fez, o resultado, a ficha e o histórico; depois os testes, o veredito do
  // revisor e o pedido da onda, que o layout não tem.
  function itemScreen(d) {
    const item=d.item;
    if(!item){body.push(...backLink(d),text(t('Item não encontrado.','Item not found.')));return;}
    const task=item.kind==='task',title=item.title || t('Sem título','Untitled');
    const long=value=>value?[prose(value.text),...(value.cut?[label(t(`texto cortado: ${int(value.chars)} caracteres no total`,`text cut: ${int(value.chars)} characters in all`))]:[])]:[];
    const files=item.files || [],history=item.history || [];
    const versions=`${history.length} ${t(history.length===1?'versão':'versões',history.length===1?'version':'versions')}`;
    const spend=item.wave==null?undefined:item.wave_tokens==null?t('não medido','not measured'):`${compact(item.wave_tokens)} tokens ${t('na onda','in wave')} ${item.wave}`;
    const resent=item.resends?`${t('reenviada','resent')} ${item.resends} ${t(item.resends===1?'vez':'vezes',item.resends===1?'time':'times')}`:'';
    // O que a tarefa cobre numa frase só, como o layout: cada item e, sem critério entre eles, o aviso.
    const covered=item.covers?.length?[item.covers.map(cover=>`${named(cover)}${/[.!?]$/.test(cover.title ?? '')?'':'.'}`).join(' '),
      ...(item.covers.some(cover=>cover.kind==='criterion')?[]:[t('Nenhum critério ligado.','No criterion linked.')])].join(' '):'';
    const until=item.done_when?contentPage(item.done_when,0,600):undefined;
    const finish=until?until.text+(until.pages>1?' …':''):'';
    const agreed=item.result?.agreed?.length?item.result.agreed.flatMap(answer=>[text(join(named(answer),answer.met?t('atendido','met'):t('não atendido','not met')),{size:'footnote',color:paint.soft}),
      ...(answer.note?[label(answer.note)]:[])]):[];
    const unmet=Boolean(item.result?.unmet);
    // The tests run on what the task covers: the proof and its newest run.
    const results={pass:['passou','passed'],fail:['falhou','failed']};
    const ran=run=>`${run.result?name(results[run.result]) ?? run.result:t('não rodado','not run')}${run.at?` ${t('em','on')} ${day(run.at)}`:''}`;
    const tone=run=>run.result==='fail'?paint.warning:run.result==='pass'?paint.success:paint.muted;
    // O resultado e a data do teste se leem juntos, ao lado do critério: "passou em 24/09".
    const tests=(item.tests || []).flatMap(run=>[text(join(named({kind:'criterion',title:run.title}),ran(run)),{size:'footnote',color:tone(run)}),...(run.proof?[label(run.proof)]:[])]);
    // The wave reviewer's newest verdict: approved, or rejected with its reason and the files it points at.
    const approved=item.review?.result==='approved';
    const verdict=item.review?[text(approved?t('Revisão: aprovada','Review: approved')
      :`${t('Revisão: reprovada','Review: rejected')}${item.review.reason?` — ${item.review.reason}`:''}`,{color:approved?paint.success:paint.warningInk}),
      ...(approved?[]:(item.review.files || []).map(file=>label(file,{mono:true})))]:[];
    const timeline=[...history].reverse().map((version,index)=>({change:change(version,history.length-1-index,item.kind),when:join(stamp(version.at),name(authors[version.author]) ?? version.author)}));
    body.push(...backLink(d));
    // O estado da onda na etiqueta: no mostarda, e no laranja quando ele é um aviso.
    const alarming=['no-agent','rejected'].includes(item.state);
    const when=!item.at?'':item.state==='committed'?`${t('Commit em','Committed on')} ${stamp(item.at)}`:['integrated','rejected'].includes(item.state)?`${t('Entregue em','Delivered on')} ${stamp(item.at)}`
      :`${t('Desde','Since')} ${stamp(item.at)}`;
    const meta=item.wave==null?[pill(t('No backlog','In the backlog'),'card')]
      :[pill(upper(name(boardStates[item.state]) ?? name(waveStates[item.state]) ?? item.state),alarming?'warning':'mustard',alarming?paint.warning:paint.mustard),
        label(`${t('Onda','Wave')} ${item.wave}`),...(when?[label(when)]:[])];
    body.push(column([text(title,{size:'heading',weight:'bold'}),...(task?[row(meta,{columnGap:space(12),rowGap:space(8),alignItems:'center'})]:[])],{rowGap:space(10)}));
    if(item.request)body.push(block(t('O que foi pedido','What was asked'),long(item.request),{paddingY:space(16)}));
    if(item.done)body.push(block(t('O que o agente fez','What the agent did'),long(item.done),{paddingY:space(16)}));
    if(agreed.length)body.push(block(t('Resultado','Result'),[row([pill(unmet?t('combinado não atendido','agreement not met'):t('combinado atendido','agreement met'),unmet?'warning':'mustard')]),
      column(agreed,{rowGap:space(8)})],{paddingY:space(16)}));
    // A ficha: o rótulo pequeno em cima e o valor embaixo, uma divisória entre as linhas.
    const detail=(caption,lines)=>entry([label(caption),...lines]);
    const sheet=[...(files.length?[detail(files.length===1?t('Arquivo','File'):t('Arquivos','Files'),files.map(file=>text(broken(file),{mono:true,size:'caption'})))]:[]),
      ...(finish?[detail(t('Término','Done when'),[text(finish)])]:[]),...(covered?[detail(t('Cobre','Covers'),[text(covered)])]:[]),
      ...(spend?[detail(t('Gasto','Spend'),[text(spend),...(resent?[label(resent)]:[])])]:[])];
    if(sheet.length)body.push(rows(sheet));
    if(timeline.length)body.push(block(`${t('Histórico','History')} · ${versions}`,[column(timeline.map((version,index)=>row([dot(index?paint.dashed:paint.mustard),
      column([text(version.change,{weight:index?'medium':'semibold'}),label(version.when)])],{flexWrap:'nowrap',columnGap:space(12),alignItems:'flex-start'})),{rowGap:space(14)})],{paddingY:space(16)}));
    if(tests.length)body.push(block(t('Testes','Tests'),tests,{paddingY:space(16),rowGap:space(4)}));
    if(verdict.length)body.push(card(verdict,{tone:approved?undefined:'warning',paddingY:space(16),rowGap:space(4)}));
    // The wave's request goes whole, as the agent got it, only on its key.
    if(item.wave_request!=null) {
      body.push(row([action('wave-request',`${state_.waveRequest?t('esconder pedido da onda','hide wave request'):t('pedido da onda','wave request')} (${int(item.wave_request.length)} ${t('caracteres','characters')})`,
        actions.toggleWaveRequest,'outline',{hotkey:'p'})]));
      if(state_.waveRequest)body.push(block(t('Pedido da onda','Wave request'),[text(item.wave_request)]));
    }
  }

  // Um trecho entre crases vira código, como no layout: a face fixa no quadro
  // escuro e no texto de destaque; o resto é texto corrido.
  function prose(value) {
    const parts=String(value ?? '').split('`');
    if(parts.length<3 || parts.length%2===0)return text(value);
    return Text({wrap:'wrap',color:paint.ink,children:parts.map((part,n)=>n%2?Text({children:clean(part),mono:true,size:'caption',color:paint.accent,backgroundColor:paint.box}):clean(part))});
  }

  // O que uma versão mudou, em palavras, como o layout: "Reescrita com
  // dependência e arquivo". O nome interno de um campo nunca chega à tela.
  function change(version,index,kind) {
    if(index===0 || !version.changed)return t('Primeira versão','First version');
    const changed=version.changed,fields=Object.keys(changed);
    if(changed.wave)return changed.wave.to==null?t('Tirada da onda','Taken out of its wave'):`${t('Posta na onda','Put in wave')} ${changed.wave.to}`;
    if(fields.includes('title'))return t('Título reescrito','Title rewritten');
    if(fields.includes('text'))return t('Pedido reescrito','Request rewritten');
    const words=[...new Set(fields.map(field=>name(fieldNames[field])).filter(Boolean))];
    const rewritten=kind==='criterion'?t('Reescrito','Rewritten'):t('Reescrita','Rewritten');
    return words.length?`${rewritten} ${t('com','with')} ${new Intl.ListFormat(locale,{type:'conjunction'}).format(words)}`:rewritten;
  }

  // O quanto o último dia passou da média da semana, como o layout escreve.
  const rise=trend=>{const ratio_=trend?.average_tokens?trend.tokens/trend.average_tokens:trend?.ratio;
    if(ratio_==null)return '';const change_=Math.round((ratio_-1)*100);
    return `${change_>=0?'+':''}${int(change_)}% ${t('na última semana','in the last week')}`;};
  const sessionFacts=d=>{
    const limits=Object.fromEntries((usage?.rateLimits || []).map(limit=>[limit.kind,limit]));
    const elapsed=usage?.startedAt!=null?Math.max(0,now-usage.startedAt):null;
    return {limits,duration:elapsed==null?'':`${Math.floor(elapsed/3600000)} h ${Math.floor(elapsed/60000)%60} min`,agents:d.session?.agents!=null?[`${d.session.agents} ${t('agentes observados','agents observed')}`,
      `${d.session.working ?? 0} ${t('trabalhando agora','working now')}`]:[]};};
  const disk=d=>d.copies_disk?text(join(`${(d.copies_disk.bytes/1e9).toLocaleString(locale,{maximumFractionDigits:d.copies_disk.bytes<1e10?1:0})} GB`,
    `${t('medido em','measured on')} ${day(d.copies_disk.measured_at)} ${hour(d.copies_disk.measured_at)}`)):label(t('não medido; use Medir agora','not measured; use Measure now'));

  // Tela Consumo: o aviso da medida, o total, o custo, os dias, a spec, as
  // ferramentas e a sessão.
  function usageScreen(d) {
    body.push(alert(d.measured_through?`${t('Medido até','Measured through')} ${day(d.measured_through)}.${d.measured_at?` ${t('O histórico não é atualizado desde','The history has not been updated since')} ${day(d.measured_at)}.`:''}`
      :t('Ainda não medido.','Not measured yet.'),[whole(measureButton())],{align:terminal?'flex-start':'center',padX:14}));
    if(d.available===false)body.push(card([label(t('O histórico deste projeto ainda não foi medido. Use Medir agora.','This project history was not measured yet. Use Measure now.'))]));
    // Dois cartões de colunas iguais, a 10 px um do outro: o rótulo, o número
    // grande e a nota, que quebra nas palavras; lado a lado a partir de 44 colunas.
    const stat=(caption,figure,foot,{size='title',color=paint.ink,footColor,...look}={})=>card([soft(caption),text(figure,{size,weight:'bold',tabular:true,color}),...(foot?[label(foot,footColor?{color:footColor}:{})]:[])],
      {flexGrow:1,...span('40%',44),rowGap:space(2),paddingX:space(16),paddingY:space(16),...look});
    const up=rise(d.trend);
    body.push(section(`${t('Total','Total')} · ${day(d.period?.first)} ${t('a','to')} ${day(d.period?.last)}`,[grid([stat(t('Este projeto','This project'),compact(d.project_tokens),'tokens',{color:paint.accent}),
      stat(t('A máquina inteira','The whole machine'),compact(d.machine_tokens),up || 'tokens',up?{footColor:paint.warning}:{})],{flexWrap:'nowrap',columnGap:space(10)},44)]));
    const cost=d.cost,total=cost.total_micro_usd;
    const share=(caption,micro,color)=>column([pair(text(broken(caption),{size:'footnote'}),text(money(micro),{size:'footnote',weight:'semibold',tabular:true})),
      bar(total?(micro ?? 0)/total:0,`${caption} ${money(micro)}`,{height:8,color})],{rowGap:space(5)});
    body.push(section(t('Custo estimado do Claude','Estimated Claude cost'),[card([row([text(money(total),{size:'title',weight:'bold',tabular:true}),
      label(`${t('em','in')} ${int(cost.sessions)} ${t('sessões','sessions')}`)],{alignItems:'flex-end',columnGap:space(10)}),
      ...cost.by_spec.map(item=>share(item.name,item.micro_usd,paint.mustard)),...(cost.outside_spec_micro_usd!=null?[share(t('fora de spec','outside a spec'),cost.outside_spec_micro_usd,paint.deep)]:[]),
      ...(cost.specs_without_cost?[label(t(`As outras ${cost.specs_without_cost} specs não têm custo atribuído.`,`The other ${cost.specs_without_cost} specs have no attributed cost.`))]:[])],{paddingY:space(16)})]));
    const days=d.days || [];
    if(days.length) {
      const top=days.reduce((best,item)=>item.tokens!=null && (best==null || item.tokens>best.tokens)?item:best,null);
      const legend=row([terminal?text('╌╌',{color:paint.empty}):Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 4" width="10" height="4"><line x1="0" x2="10" y1="2" y2="2" stroke="${paint.empty}" stroke-width="2" stroke-dasharray="3 2"/></svg>`,
        alt:t('tracejado','dashed'),width:10,height:4}),label(t('sem medida','no measure'))],{columnGap:space(4),flexWrap:'nowrap',alignItems:'center'});
      body.push(section(t('Tokens por dia · este projeto','Tokens per day · this project'),[card([row([fill(top?label(`${t('máximo','highest')} ${compact(top.tokens)} ${t('em','on')} ${day(top.day)}`):label('')),legend],
        {flexWrap:'nowrap',alignItems:'center'}),terminal?dayChart(days):chartSvg(days),row([label(day(days[0].day)),label(day(days[7]?.day)),label(day(days.at(-1).day))],{justifyContent:'space-between',flexWrap:'nowrap'})],
        {paddingY:space(16),rowGap:space(8)})]));
    }
    // Os da spec, menores: 14 px por 16 px.
    const small={size:'heading',paddingX:space(16),paddingY:space(14)};
    if(d.spec)body.push(section(`${t('Esta spec','This spec')} · ${broken(d.spec.name,(width ?? 100)-2*cells('paddingX',20))}`,[grid([stat(t('Ondas','Waves'),compact(d.spec.wave_tokens),
      d.spec.waves_without_measure?`${d.spec.waves_without_measure} ${t('ondas sem medida','waves without measure')}`:'',small),
      stat(t('Condutor','Conductor'),d.spec.conductor_tokens==null?'—':compact(d.spec.conductor_tokens),d.spec.conductor_tokens==null?t('não medido','not measured'):t('custo não medido','cost not measured'),small)],
      {flexWrap:'nowrap',columnGap:space(10)},44)]));
    const tools=d.tools,jev=tools.jev,purposes={'wave-planning':['montagem','planning'],context:['contexto','context'],search:['busca','search'],'spec-search':['busca na spec','spec search']};
    // Cada ferramenta: o nome e o número na mesma linha, e o detalhe que quebra
    // nas pausas dele, o número sempre junto da unidade.
    const tool=(title,value,color,detail)=>entry([pair(text(title,{weight:'semibold'}),text(value,{weight:'bold',tabular:true,...(color?{color}:{})}),{columnGap:space(10)}),
      ...(detail?[phrases(detail,{size:'caption',color:paint.muted})]:[])]);
    body.push(section(t('Ferramentas','Tools'),[rows([
      tool(t('Filtro de saída (RTK)','Output filter (RTK)'),!tools.rtk.configured?t('desligado','off'):tools.rtk.percent!=null?`−${percent(tools.rtk.percent)}`:t('não medido','not measured'),
        tools.rtk.configured && tools.rtk.percent!=null?paint.success:paint.muted,tools.rtk.commands!=null?join(`${int(tools.rtk.commands)} ${t('comandos','commands')}`,`${compact(tools.rtk.saved_tokens)} ${t('tokens poupados','tokens saved')}`):''),
      // Os tokens do Jev em dois algarismos, como o layout os escreve: "5,4 mi".
      tool('Jev',`${int(jev.calls)} ${t('chamadas','calls')}`,undefined,join(...Object.entries(jev.by_purpose || {}).sort((a,b)=>b[1]-a[1]).map(([purpose,count])=>`${int(count)} ${name(purposes[purpose]) ?? purpose}`),
        jev.input_tokens!=null?`${compact(jev.input_tokens,2)} tokens`:'',jev.cost_micro_usd==null?t('custo não medido','cost not measured'):money(jev.cost_micro_usd))),
      tool(t('Busca de código','Code search'),`${int(tools.search.total)} ${t('buscas','searches')}`,undefined,tools.search.in_spec?`${int(tools.search.in_spec)} ${t('nesta spec','in this spec')}`:t('nenhuma nesta spec','none in this spec'))])]));
    const {limits,duration,agents}=sessionFacts(d);
    // Cada medida da sessão: o que ela é à esquerda, a porcentagem à direita e a barra fina.
    const gauge=(caption,value,alt)=>column([row([fill(text(caption,{size:'footnote',color:paint.soft})),text(percent(value),{size:'footnote',weight:'semibold',tabular:true})],{flexWrap:'nowrap'}),
      bar((value ?? 0)/100,alt)],{rowGap:space(5)});
    const quota=(kind,caption,resets)=>limits[kind]?[gauge(`${caption}${limits[kind].resetsAt?` · ${resets(limits[kind].resetsAt)}`:''}`,limits[kind].percentUsed,
      `${caption} ${percent(limits[kind].percentUsed)}`)]:[];
    // As medidas juntas, que a barra fina já separa, e a nota dos agentes a uma linha da última barra.
    const gauges=[...(usage?.context?[gauge(`${t('Contexto','Context')} · ${int(usage.context.tokens)} ${t('de','of')} ${compact(usage.context.window)}`,usage.context.percent,
        `${t('Contexto','Context')} ${int(usage.context.tokens)} ${t('de','of')} ${compact(usage.context.window)}`)]:[]),
      ...quota('five_hour',t('Cota de 5 h','5 h quota'),at=>`${t('renova às','resets at')} ${clock(at)}`),...quota('seven_day',t('Cota da semana','Weekly quota'),at=>`${t('renova em','resets on')} ${calendar(at)}`)];
    body.push(section(`${t('Esta sessão','This session')}${duration?` · ${duration}`:''}`,[card([column(gauges,{rowGap:snug(14)}),
      ...(agents.length?[label(join(...agents))]:[])],{paddingY:space(16)})]));
    usageExtras(d);
  }

  // O que a tela Consumo do layout não tem: os agentes das ondas, as skills e as cópias.
  function usageExtras(d) {
    if(d.spec?.waves)waveAgents(d.spec);
    if(d.mustard_skills?.length)mustardSkills(d.mustard_skills);
    body.push(block(t('Cópias das ondas','Wave copies'),[disk(d)],{paddingY:space(16)}));
  }

  // What each wave's agent got and what went wrong, as the sends recorded
  // it: the model it used (measured) or, unmeasured, the one asked; the
  // effort that held at the launch or, unrecorded, the one asked; the
  // skills; the summary it continued. Then the failures.
  function waveAgents(s) {
    const reasons={no_credit:['sem crédito','no credit'],missing_key:['sem chave','no key'],key_refused:['chave recusada','key refused'],
      key_in_git:['chave no git','key in git'],network:['rede','network'],busy:['ocupado','busy'],refused:['recusado','refused'],
      timeout:['tempo esgotado','timed out'],unreadable:['resposta ilegível','unreadable answer'],too_large:['pergunta grande demais','request too large']};
    const counted=(map,names={})=>Object.entries(map || {}).sort((a,b)=>b[1]-a[1]).map(([key,count])=>`${name(names[key]) ?? key} (${int(count)})`).join(', ');
    const refused=n=>n?plural(n,t('volta recusada','refused return'),t('voltas recusadas','refused returns')):'';
    const rejected=n=>n?plural(n,t('reprovação da revisão','review rejection'),t('reprovações da revisão','review rejections')):'';
    // The effort the send asked; a wave shows the one that held at the launch instead, when the send recorded it.
    const effort=value=>value?`${t('esforço pedido','effort asked')} ${value}`:'';
    const held=value=>`${t('esforço','effort')} ${value}`;
    const reuse=s.reuse || {};
    const look={paddingY:space(16),rowGap:space(8)};
    body.push(block(t('Agentes das ondas','Wave agents'),[
      ...(s.models || []).flatMap(m=>joined([m.model,effort(m.effort),plural(m.waves,t('onda','wave'),t('ondas','waves'))])),
      ...(s.skills?.length?[label(`${t('skills entregues','skills delivered')}: ${s.skills.map(k=>`${k.name} (${int(k.waves)})`).join(', ')}`)]:[]),
      ...joined([plural(reuse.summaries ?? 0,t('onda continuou um resumo','wave continued a summary'),t('ondas continuaram um resumo','waves continued a summary')),
        reuse.knowledge_calls?`${int(reuse.knowledge_cached)} ${t('de','of')} ${int(reuse.knowledge_calls)} ${t('consultas de conhecimento reaproveitadas','knowledge lookups reused')}`:''])],look));
    if(s.waves.length)body.push(block(`${t('Por onda','By wave')} · ${t('últimas','last')} ${s.waves.length} ${t('de','of')} ${int(s.waves_sent)} ${t('ondas enviadas','waves sent')}`,
      s.waves.map(w=>{
        const used=w.model_used ?? (w.model_asked?`${w.model_asked} ${t('(pedido)','(asked)')}`:t('modelo não medido','model not measured'));
        const extra=[(w.skills || []).join(', '),w.summary_reused?t('continuou um resumo','continued a summary'):'',refused(w.refused_returns),rejected(w.review_rejections)];
        return column([...joined([`${t('onda','wave')} ${w.wave}`,used,w.effort_used?held(w.effort_used):effort(w.effort_asked),w.tokens!=null?`${compact(w.tokens)} tokens`:'']),...joined(extra,label)]);
      }),look));
    const f=s.failures || {},lines=[];
    if(f.jev?.calls)lines.push(text(`Jev ${t('falhou','failed')} ${plural(f.jev.calls,t('vez','time'),t('vezes','times'))}: ${counted(f.jev.reasons,reasons)}`));
    if(f.hook_blocks?.total)lines.push(text(`${plural(f.hook_blocks.total,t('bloqueio de gancho','hook block'),t('bloqueios de gancho','hook blocks'))}: ${counted(f.hook_blocks.by_hook)}`));
    const returns=join(refused(f.refused_returns),rejected(f.review_rejections));
    if(returns)lines.push(text(returns));
    body.push(block(t('Falhas','Failures'),lines.length?lines:[label(t('Nenhuma falha gravada.','No failure recorded.'))],look));
  }

  // The skills Mustard made: the state, how many times each went with a
  // task, the tokens of the wave that made it and the last measured gain.
  function mustardSkills(skills) {
    const states={active:['ativa','active'],discarded:['descartada','discarded'],retired:['retirada','retired']};
    body.push(block(t('Skills do Mustard','Mustard skills'),skills.map(s=>column([
      ...joined([s.name,name(states[s.state]) ?? s.state,`${int(s.tasks)} ${s.tasks===1?t('tarefa','task'):t('tarefas','tasks')}`,
        s.wave_tokens!=null?`${compact(s.wave_tokens)} ${t('tokens na onda que a criou','tokens in the wave that made it')}`:'']),
      ...(s.gain?[label(s.gain)]:[])])),{paddingY:space(16),rowGap:space(8)}));
  }

  // O botão do aviso, no mostarda cheio do layout; no terminal com a tecla m.
  function measureButton() {
    if(measuring)return label(t('Medindo…','Measuring…'),{color:paint.warningInk});
    return action('measure',t('Medir agora','Measure now'),actions.measure,'solid',hot('m'));
  }

  // O gráfico do layout: barras mostarda do mesmo tamanho, arredondadas só em
  // cima, o dia sem medida tracejado na base e a linha da base embaixo.
  function chartSvg(days) {
    const width=360,height=120,gap=5,base=height-1,step=(width+gap)/days.length,size=step-gap,max=Math.max(1,...days.map(item=>item.tokens ?? 0));
    const n=value=>value.toFixed(1);
    const marks=days.map((item,index)=>{const x=index*step;
      if(item.tokens==null)return `<line x1="${n(x)}" x2="${n(x+size)}" y1="${base-2}" y2="${base-2}" stroke="${paint.empty}" stroke-width="2" stroke-dasharray="3 2"/>`;
      const tall=Math.max(2,item.tokens/max*(base-2)),top=base-tall,r=Math.min(4,tall/2,size/2);
      return `<path d="M${n(x)} ${base}V${n(top+r)}A${n(r)} ${n(r)} 0 0 1 ${n(x+r)} ${n(top)}H${n(x+size-r)}A${n(r)} ${n(r)} 0 0 1 ${n(x+size)} ${n(top+r)}V${base}Z" fill="${paint.mustard}"/>`;}).join('');
    return Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}">${marks}<rect y="${base}" width="${width}" height="1" fill="${paint.line}"/></svg>`,
      alt:`${t('Tokens por dia','Tokens per day')}: ${days.map(item=>`${day(item.day)}: ${item.tokens==null?t('sem medida','no measure'):compact(item.tokens)}`).join('; ')}`,width,height});
  }

  // O mesmo gráfico no terminal: uma coluna por dia, de colunas iguais, quatro
  // linhas de altura, o topo de cada barra em oitavos de bloco; o dia sem
  // medida tracejado na base, e a linha da base embaixo.
  function dayChart(days) {
    const tall=4,max=Math.max(1,...days.map(item=>item.tokens ?? 0)),empty=()=>Box({height:1,children:[]});
    const bars=item=>{
      if(item.tokens==null)return column([...Array.from({length:tall-1},empty),run('╌',paint.empty)],{flexGrow:1,width:0});
      const eighths=item.tokens?Math.max(1,Math.round(item.tokens/max*tall*8)):0;
      return column(Array.from({length:tall},(_,n)=>{const left=Math.max(0,Math.min(8,eighths-(tall-1-n)*8));
        return left?run(left===8?'█':'▁▂▃▄▅▆▇'[left-1],paint.mustard):empty();}),{flexGrow:1,width:0});
    };
    return column([row(days.map(bars),{flexWrap:'nowrap',columnGap:1}),rule()]);
  }

  if(problem)body.push(text(problem,{color:paint.warning}));
  if(screen==='settings')body.push(settingsScreen(settings,actions,{t,terminal,paint,space,apart,text,soft,label,join,row,column,fill,section,rows,entry,block,action,Box,Select,Input,Button,Text,hot}));
  else if(!answer || answer.reason==='panel-no-spec' && screen!=='overview') {
    // Enquanto a lista ou a tarefa não chega, a volta segue à vista.
    if(['items','item'].includes(screen))body.push(...backLink(answer ?? {}));
    body.push(answer?text(t('Nenhuma spec neste projeto.','No spec in this project.')):label(t('Carregando…','Loading…')));
  } else if(screen==='overview') overview(answer);
  else if(screen==='spec') spec(answer);
  else if(screen==='items') items(answer);
  else if(screen==='item') itemScreen(answer);
  else if(screen==='usage') usageScreen(answer);

  // O cabeçalho do layout, no fundo dele e com a linha de baixo: a marca, o
  // nome, a branch, atualizar, configurações e as três abas, a aberta
  // sublinhada em mostarda; nas configurações, o cabeçalho delas. No terminal
  // cada botão leva a tecla aprovada: a de configurações e as das abas, e a
  // Visão geral leva o v onde ela é a volta. O desktop guarda o fechar do
  // aplicativo, que ele desenha na borda do painel; no terminal o painel fecha
  // pela marca dele e pelo Esc.
  const close=terminal?[]:[Button({key:'quit',label:'✕',plain:true,role:'dismiss',onPress:actions.close})];
  // A marca: o quadrado mostarda com o quadrado escuro no meio. No terminal,
  // quatro colunas por duas linhas, o do meio em meios blocos.
  const brand=terminal?column([TextOf({children:' ▄▄ ',color:paint.page,backgroundColor:paint.mustard}),TextOf({children:' ▀▀ ',color:paint.page,backgroundColor:paint.mustard})])
    :Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 34 34" width="34" height="34"><rect width="34" height="34" rx="10" fill="${paint.mustard}"/><rect x="11" y="11" width="12" height="12" rx="3" fill="${paint.page}"/></svg>`,
      alt:'Mustard',width:34,height:34});
  const underline=()=>Box({backgroundColor:paint.mustard,children:[Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2" width="2" height="2"><rect width="2" height="2" fill="${paint.mustard}"/></svg>`,
    alt:t('aba aberta','open tab'),width:2,height:2})]});
  const tabs=[['go-overview','overview'],['go-spec','spec'],['go-usage','usage']].map(([id,target])=>{const on=(['items','item'].includes(screen)?'spec':screen)===target;
    return {id,target,on,caption:name(titles[target]),letter:on?undefined:{overview:['spec','usage'].includes(screen)?'v':undefined,spec:'s',usage:'c'}[target]};});
  const tabRow=row(tabs.map(tab=>{const button=link(tab.id,tab.caption,()=>actions.go(tab.target),{size:'body',weight:tab.on?'semibold':'medium',color:tab.on?paint.ink:paint.muted},tab.letter?hot(tab.letter):{});
    return tab.on && !terminal?column([button,underline()],{rowGap:space(11)}):column([button]);}),{columnGap:space(24),flexWrap:'nowrap'});
  // No terminal o sublinhado da aba aberta corre na própria linha de baixo do
  // cabeçalho: o traço mostarda embaixo dela, a linha no resto. A tecla que o
  // terminal desenha antes do rótulo ("s: ") conta na largura.
  const drawn=tab=>(tab.letter?tab.letter.length+2:0)+tab.caption.length;
  const open=tabs.findIndex(tab=>tab.on);
  const under=()=>{const before=cells('paddingX',20)+tabs.slice(0,open).reduce((sum,tab)=>sum+drawn(tab)+cells('columnGap',24),0);
    return row([run('─',paint.line,{width:before}),run('━',paint.mustard,{width:drawn(tabs[open])}),run('─',paint.line,{flexGrow:1,width:0})],{flexWrap:'nowrap',columnGap:0});};
  // A branch fica embaixo do nome, ao lado dos botões; no terminal estreito,
  // onde ela não cabe ali inteira, desce para uma linha dela, de ponta a ponta.
  const named_=project?.branch || t('sem branch','no branch');
  // Ao lado dela: a marca (4), as duas lacunas do cabeçalho e os dois botões,
  // atualizar (3) e configurações com a tecla (6), com a lacuna entre eles.
  const beside=!terminal || named_.length<=(width ?? 100)-2*cells('paddingX',20)-4-3*cells('columnGap',12)-9;
  const branch=text(beside?named_:broken(named_,(width ?? 100)-2*cells('paddingX',20)),{mono:true,size:'caption',color:paint.muted,wrap:beside?'truncate-end':'wrap'});
  const top=screen==='settings'
    ?column([row([glyphButton('back',glyphs.back,t('Voltar','Back'),actions.back,hot('v')),
      column([text(t('Configurações','Settings'),{size:'body',weight:'bold'}),label(t('salvo no mustard.json','saved in mustard.json'))],{flexGrow:1,flexShrink:1}),...close],
      {flexWrap:'nowrap',alignItems:'center',columnGap:space(12),paddingX:space(20),paddingY:space(18)}),rule()],{backgroundColor:paint.header})
    :column([column([column([row([brand,column([text(project?.name || 'Mustard',{size:'body',weight:'bold'}),...(beside?[branch]:[])],{flexGrow:1,flexShrink:1}),
      // Os botões juntos e inteiros: num painel estreito encolhe a branch, não eles.
      row([glyphButton('refresh',glyphs.refresh,t('Atualizar','Refresh'),actions.refresh),glyphButton('go-settings',glyphs.settings,t('Configurações','Settings'),()=>actions.go('settings'),hot('a')),...close],
        {flexWrap:'nowrap',flexShrink:0,alignItems:'center',columnGap:space(12)})],
      {flexWrap:'nowrap',alignItems:'center',columnGap:space(12)}),...(beside?[]:[branch])]),tabRow],
      {rowGap:space(14),paddingX:space(20),paddingTop:space(18)}),terminal?under():rule()],{backgroundColor:paint.header});
  // O rodapé da Visão geral, no fundo do cabeçalho e com a linha de cima:
  // publicar, no mostarda cheio e na tecla p, e a hora da leitura.
  const publish=publication?.busy?label(t('Publicando…','Publishing…')):whole(action('publish',t('Publicar no Cloudflare','Publish to Cloudflare'),actions.publish,'solid',hot('p')));
  const foot=screen==='overview'?[column([rule(),row([publish,fill(...(answer?.at?[row([label(`${t('atualizado às','updated at')} ${clock(answer.at)}`)],{flexGrow:1,justifyContent:'flex-end'})]:[]))],
    {flexWrap:tight?'wrap':'nowrap',alignItems:'center',columnGap:space(12),paddingX:space(20),paddingY:space(14)})],{backgroundColor:paint.header})]:[];
  // O espaço entre os blocos de cada tela: 24 px na Visão geral, 16 nas
  // Tarefas, 20 na tarefa e 22 nas outras.
  const gaps={overview:24,items:16,item:20};
  return Box({flexDirection:'column',backgroundColor:paint.page,...(width?{width}:{}),children:[top,column(body,{rowGap:space(gaps[screen] ?? 22),paddingX:space(20),paddingY:space(22)}),...foot]});
}
