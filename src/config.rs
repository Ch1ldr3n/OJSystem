use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: Bind,
    pub problems: Vec<Problem>,
    pub languages: Vec<Language>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Bind {
    pub bind_address: String,
    pub bind_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Problem {
    pub id: u32,
    pub name: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub misc: Misc,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Misc {
    packing: Option<Vec<Vec<usize>>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Case {
    pub score: f64,
    pub input_file: String,
    pub answer_file: String,
    pub time_limit: u64,
    pub memory_limit: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Language {
    pub name: String,
    pub file_name: String,
    pub command: Vec<String>,
}
