//! Stream past unrelated scan fields and unrequested declarations. The stored
//! analysis is still validated JSON; catalogue positions never bypass identity
//! checks in the caller. No additional persistent copy or cache is needed.
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::{collections::{BTreeMap, BTreeSet}, fmt};

struct Pack<'a> { depth:u8, wanted:&'a BTreeSet<usize> }
impl<'de> DeserializeSeed<'de> for Pack<'_> {
    type Value=BTreeMap<usize,Value>;
    fn deserialize<D:serde::Deserializer<'de>>(self,deserializer:D)->Result<Self::Value,D::Error> {
        if self.depth==0 {deserializer.deserialize_seq(self)}else{deserializer.deserialize_map(self)}
    }
}
impl<'de> Visitor<'de> for Pack<'_> {
    type Value=BTreeMap<usize,Value>;
    fn expecting(&self,f:&mut fmt::Formatter)->fmt::Result {f.write_str("a scan evidence pack or its declaration array")}
    fn visit_map<M:MapAccess<'de>>(self,mut map:M)->Result<Self::Value,M::Error> {
        let mut selected=BTreeMap::new();
        let field=if self.depth==2 {"knowledge"}else{"cards"};
        while let Some(key)=map.next_key::<String>()? {
            if key==field {selected=map.next_value_seed(Pack{depth:self.depth-1,wanted:self.wanted})?;}
            else {map.next_value::<IgnoredAny>()?;}
        }
        Ok(selected)
    }
    fn visit_seq<S:SeqAccess<'de>>(self,mut seq:S)->Result<Self::Value,S::Error> {
        let mut selected=BTreeMap::new();let mut at=0;
        loop {
            if self.wanted.contains(&at) {
                let Some(value)=seq.next_element::<Value>()? else{break};selected.insert(at,value);
            }else if seq.next_element::<IgnoredAny>()?.is_none(){break;}
            at+=1;
        }
        Ok(selected)
    }
}
pub(super) fn selected(text:&str,wanted:&BTreeSet<usize>)->Result<BTreeMap<usize,Value>,serde_json::Error> {
    let mut deserializer=serde_json::Deserializer::from_str(text);
    let pack=Pack{depth:2,wanted}.deserialize(&mut deserializer)?;
    deserializer.end()?;Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_declarations_keep_identity_and_position_guards_without_decoding_other_cards() {
        let conn=rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE knowledge_symbols(path TEXT,position INTEGER,id TEXT);CREATE TABLE texts(path TEXT,analysis TEXT);").unwrap();
        let card=|line:u64,name:&str|serde_json::json!({"id":format!("a.rs:{line}:{name}"),"name":name,"kind":"function",
            "source":{"file":"a.rs","line":line,"end_line":line,"sha256":"h"}});
        let analysis=serde_json::json!({"unused":{"large_declarations":vec!["unrelated";1000]},
            "knowledge":{"cards":[card(1,"entry"),{"id":null},card(3,"tail")]}}).to_string();
        conn.execute("INSERT INTO texts VALUES('a.rs',?1)",[&analysis]).unwrap();
        conn.execute_batch("INSERT INTO knowledge_symbols VALUES('a.rs',2,'a.rs:3:tail'),('a.rs',0,'a.rs:1:entry');").unwrap();
        let ids=BTreeSet::from(["a.rs:3:tail".into(),"a.rs:1:entry".into()]);
        let cards=super::super::hydrate(&conn,&ids).unwrap();
        assert_eq!(cards.iter().map(|c|c.name.as_str()).collect::<Vec<_>>(),["entry","tail"]);
        conn.execute("UPDATE knowledge_symbols SET position=0 WHERE id='a.rs:3:tail'",[]).unwrap();
        assert!(super::super::hydrate(&conn,&ids).is_err());
        conn.execute("UPDATE knowledge_symbols SET position=4 WHERE id='a.rs:3:tail'",[]).unwrap();
        assert!(super::super::hydrate(&conn,&ids).is_err());
        assert!(selected(&(analysis+" trailing"),&BTreeSet::from([0])).is_err());
        assert!(selected(r#"{"unused":[invalid],"knowledge":{"cards":[]}}"#,&BTreeSet::new()).is_err());
    }
}
