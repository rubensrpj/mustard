// Keep each view small on both hosts while preserving the complete original.
// This local paging budget is not a claim about a host's maximum tree size.
export function contentPage(value, requested=0, size=4000) {
  const points=Array.from(String(value ?? ''));
  size=Math.max(1,Math.min(4000,Math.floor(size)||4000));
  const chunks=[];
  for(let start=0;start<points.length;) {
    let end=Math.min(points.length,start+size);
    if(end<points.length) {
      // Prefer complete paragraphs/lines, then words. Oversized uninterrupted
      // content still pages losslessly at the surface limit.
      const minimum=start+Math.floor(size/2);
      const boundary=separator=>{
        for(let at=end-separator.length;at>=minimum;at--)
          if(separator.every((point,n)=>points[at+n]===point))return at+separator.length;
      };
      end=boundary(['\n','\n']) ?? boundary(['\n']) ?? boundary([' ']) ?? end;
    }
    chunks.push(points.slice(start,end).join(''));start=end;
  }
  if(!chunks.length)chunks.push('');
  const pages=chunks.length;
  const page=Math.max(0,Math.min(requested,pages-1));
  return {text:chunks[page],page,pages};
}
