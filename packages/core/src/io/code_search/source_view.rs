//! Lossless source-line reuse inside a task view. Coordinates always accompany
//! reused lines; explicit reads and original tool results never pass here.
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn row(text: &str) -> Option<(u64, &str)> {
    let (number, body) = text.split_once(" | ")?;
    let line = number.parse::<u64>().ok().filter(|line| *line > 0)?;
    Some((line, body))
}

pub(super) fn ranges(lines: &BTreeSet<u64>) -> String {
    let mut out = Vec::new();
    let mut current = None;
    for &line in lines {
        match current {
            Some((start, end)) if line == end + 1 => current = Some((start, line)),
            Some((start, end)) => { out.push(format!("{start}-{end}")); current = Some((line,line)); }
            None => current = Some((line,line)),
        }
    }
    if let Some((start,end)) = current { out.push(format!("{start}-{end}")); }
    out.join(", ")
}

#[derive(Default)]
pub(super) struct View {
    lines: BTreeMap<(String, String, u64), String>,
}
impl View {
    pub fn render(&mut self, file: &str, hash: &str, source: &str) -> String {
        let mut fresh=String::new();
        let mut reused=BTreeSet::new();
        for line in source.split_inclusive('\n') {
            if let Some((number,body))=row(line.trim_end_matches('\n')) {
                let key=(file.to_string(),hash.to_string(),number);
                if self.lines.get(&key).is_some_and(|old|old==body) {
                    reused.insert(number);
                    continue;
                }
                self.lines.insert(key,body.to_string());
            }
            fresh.push_str(line);
        }
        if reused.is_empty() { return source.to_string(); }
        let note=format!("# Source lines {} already above in this file; same source hash.\n",ranges(&reused));
        if fresh.len()+note.len()>=source.len() {return source.to_string();}
        format!("{note}{fresh}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_ranges_keep_new_lines_and_never_reuse_changed_content_or_hashes() {
        let mut view=View::default();
        let first=format!("1 | {}\n2 | {}\n","a".repeat(120),"b".repeat(120));
        assert_eq!(view.render("a.rs","one",&first),first);
        let second=format!("2 | {}\n3 | new\n","b".repeat(120));
        let rendered=view.render("a.rs","one",&second);
        assert!(rendered.contains("2-2 already above"));assert!(rendered.contains("3 | new"));
        assert_eq!(view.render("a.rs","two",&first),first);
        let changed="1 | changed\n";
        assert_eq!(view.render("a.rs","one",changed),changed);
        assert_eq!(view.render("other.rs","one",&first),first);
    }
}
