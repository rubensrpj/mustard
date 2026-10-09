//! Optional precise symbol port. Producers retain their own quality/coverage;
//! a compiler reference is not evidence of runtime execution.
use super::Source;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    Definitions,
    References,
    Implementations,
}
impl Relation {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "definitions" => Ok(Self::Definitions),
            "references" => Ok(Self::References),
            "implementations" => Ok(Self::Implementations),
            _ => Err("precise-invalid-relation".into()),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Location<'a> {
    pub file: &'a str,
    pub line: u64,
    pub column_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub symbol: String,
    pub source: Source,
    pub column_bytes: u64,
    pub end_line: u64,
    pub end_column_bytes: u64,
    pub roles: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub status: String,
    pub producer: String,
    pub index_sha256: String,
    pub references: Vec<Reference>,
    pub symbols: Vec<String>,
    pub has_more: bool,
}
pub trait PreciseSymbols {
    fn resolve(
        &self,
        location: &Location<'_>,
        relation: Relation,
        limit: usize,
    ) -> Result<Resolution, String>;
}
