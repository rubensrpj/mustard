//! Minimal wire-compatible SCIP messages. Unknown Protobuf fields are skipped.
//! Tags follow scip-code/scip/scip.proto; no indexer or language runtime linked.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Index {
    #[prost(message, optional, tag = "1")]
    pub metadata: Option<Metadata>,
    #[prost(message, repeated, tag = "2")]
    pub documents: Vec<Document>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Metadata {
    #[prost(message, optional, tag = "2")]
    pub tool_info: Option<ToolInfo>,
    #[prost(string, tag = "3")]
    pub project_root: String,
    #[prost(int32, tag = "4")]
    pub text_document_encoding: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct ToolInfo {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub version: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Document {
    #[prost(string, tag = "1")]
    pub relative_path: String,
    #[prost(message, repeated, tag = "2")]
    pub occurrences: Vec<Occurrence>,
    #[prost(message, repeated, tag = "3")]
    pub symbols: Vec<Symbol>,
    #[prost(string, tag = "5")]
    pub text: String,
    #[prost(int32, tag = "6")]
    pub position_encoding: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Symbol {
    #[prost(string, tag = "1")]
    pub symbol: String,
    #[prost(message, repeated, tag = "4")]
    pub relationships: Vec<Relationship>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Relationship {
    #[prost(string, tag = "1")]
    pub symbol: String,
    #[prost(bool, tag = "2")]
    pub is_reference: bool,
    #[prost(bool, tag = "3")]
    pub is_implementation: bool,
    #[prost(bool, tag = "5")]
    pub is_definition: bool,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Occurrence {
    #[prost(int32, repeated, packed = "true", tag = "1")]
    pub range: Vec<i32>,
    #[prost(string, tag = "2")]
    pub symbol: String,
    #[prost(int32, tag = "3")]
    pub roles: i32,
    #[prost(oneof = "TypedRange", tags = "8,9")]
    pub typed_range: Option<TypedRange>,
}
#[derive(Clone, PartialEq, prost::Oneof)]
pub enum TypedRange {
    #[prost(message, tag = "8")]
    Single(SingleLine),
    #[prost(message, tag = "9")]
    Multi(MultiLine),
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct SingleLine {
    #[prost(int32, tag = "1")]
    pub line: i32,
    #[prost(int32, tag = "2")]
    pub start: i32,
    #[prost(int32, tag = "3")]
    pub end: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct MultiLine {
    #[prost(int32, tag = "1")]
    pub start_line: i32,
    #[prost(int32, tag = "2")]
    pub start: i32,
    #[prost(int32, tag = "3")]
    pub end_line: i32,
    #[prost(int32, tag = "4")]
    pub end: i32,
}
