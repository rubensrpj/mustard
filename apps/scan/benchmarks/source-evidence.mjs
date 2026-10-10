// Grade the source actually delivered, including non-contiguous AST excerpts.
// Labels and source verification stay outside the retrieval request.
export function visibleReference(card,view) {
  let file=null;const reference=`${card.name} ${card.source.line}-${card.source.end_line}`;
  for(const line of String(view).split('\n')) {
    if(line.startsWith('@ '))file=line.slice(2);
    if(file===card.source.file && (line.startsWith(`# ${reference} [`)
      ||line.startsWith('# References (expand source/responsibility): ')
        &&line.slice('# References (expand source/responsibility): '.length).split('; ').includes(reference)))return true;
    const prefix=`# Static targets in ${card.source.file}: `;
    if(line.startsWith(prefix)&&line.slice(prefix.length).split('; ').some(value=>value.startsWith(reference+': ')))return true;
  }
  return false;
}
export function deliveredLines(view) {
  const files=new Map();let file=null;
  for(const line of String(view).split('\n')) {
    if(line.startsWith('@ ')) {file=line.slice(2);continue;}
    const match=/^(\d+) \| (.*)$/.exec(line);
    if(!match||!file)continue;
    const lines=files.get(file)||new Map();
    lines.set(Number(match[1]),match[2]);files.set(file,lines);
  }
  return files;
}
export function completeBody(card,view) {
  if(!card.initial_source_excerpt||card.source_excerpt?.truncated!==false)return false;
  const required=String(card.source_excerpt.text||'').split('\n').filter(Boolean);
  const delivered=deliveredLines(view).get(card.source?.file);
  return required.length>0&&required.every(line=>{
    const match=/^(\d+) \| (.*)$/.exec(line);
    return match&&delivered?.get(Number(match[1]))===match[2];
  });
}
