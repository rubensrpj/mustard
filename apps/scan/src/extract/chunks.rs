//! AST boundaries, with sibling merging. Ranges reference the original source;
//! no generated explanation or duplicate body text enters the catalogue.
use tree_sitter::{Node,Point};
const BYTES:usize=2048;
#[derive(Clone,Copy)]
struct Span {start:usize,end:usize,first:Point,last:Point}
fn append(out:&mut Vec<Span>,span:Span) {
    if span.start==span.end {return;}
    if let Some(previous)=out.last_mut().filter(|p|p.end==span.start && span.end-p.start<=BYTES) {
        previous.end=span.end;previous.last=span.last;
    } else {out.push(span);}
}
fn walk(node:Node,out:&mut Vec<Span>) {
    let whole=Span{start:node.start_byte(),end:node.end_byte(),first:node.start_position(),last:node.end_position()};
    if node.byte_range().len()<=BYTES || node.named_child_count()==0 {append(out,whole);return;}
    let mut at=whole.start;let mut point=whole.first;let mut cursor=node.walk();
    for child in node.named_children(&mut cursor) {
        append(out,Span{start:at,end:child.start_byte(),first:point,last:child.start_position()});
        walk(child,out);at=child.end_byte();point=child.end_position();
    }
    append(out,Span{start:at,end:whole.end,first:point,last:whole.last});
}
pub(super) fn of(node:Node)->serde_json::Value {
    let mut spans=Vec::new();walk(node,&mut spans);
    serde_json::json!(spans.into_iter().map(|s|serde_json::json!({"start_byte":s.start,"end_byte":s.end,
        "line":s.first.row+1,"end_line":if s.last.column==0 && s.last.row>s.first.row{s.last.row}else{s.last.row+1},
        "oversized_leaf":s.end-s.start>BYTES})).collect::<Vec<_>>())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_ranges_cover_unicode_source_once_and_keep_large_statements_explicit() {
        let source=format!("fn evaluate() {{\n{}\n}}",(0..100).map(|i|format!("let café_{i} = \"{}\";","coração".repeat(8))).collect::<Vec<_>>().join("\n"));
        let analyzer=super::super::Analyzer::declarations_only("rust").unwrap();
        let mut parser=tree_sitter::Parser::new();parser.set_language(&analyzer.language).unwrap();
        let tree=parser.parse(&source,None).unwrap();let node=tree.root_node().named_child(0).unwrap();
        let chunks=of(node);let mut offset=node.start_byte();
        for chunk in chunks.as_array().unwrap() {
            let start=chunk["start_byte"].as_u64().unwrap() as usize;let end=chunk["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(start,offset);assert!(source.is_char_boundary(start)&&source.is_char_boundary(end));
            assert!(end>start);assert!(end-start<=BYTES);offset=end;
        }
        assert_eq!(offset,node.end_byte());assert!(chunks.as_array().unwrap().len()>1);
    }
}
