use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const ENDPOINTS_FILE: &str = "x32_osc_endpoints.json";
const FULL_EXTRACT_FILE: &str = "x32_osc_full_extract.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceFiles {
    pub root: PathBuf,
}

impl ReferenceFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn endpoints_path(&self) -> PathBuf {
        self.root.join(ENDPOINTS_FILE)
    }

    pub fn full_extract_path(&self) -> PathBuf {
        self.root.join(FULL_EXTRACT_FILE)
    }

    pub fn load_endpoints(&self) -> Result<Vec<Endpoint>, ReferenceError> {
        read_json(&self.endpoints_path())
    }

    pub fn load_full_extract(&self) -> Result<FullExtract, ReferenceError> {
        read_json(&self.full_extract_path())
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub path: String,
    #[serde(rename = "type")]
    pub value_type: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FullExtract {
    pub source_file: String,
    pub method: String,
    pub counts: FullExtractCounts,
    #[serde(default)]
    pub root_patterns: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<FullExtractPattern>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FullExtractCounts {
    pub patterns: usize,
    pub concrete_paths: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FullExtractPattern {
    pub pattern: String,
    pub line: usize,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub expanded_count: Option<usize>,
}

#[derive(Debug)]
pub enum ReferenceError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReferenceError {}

impl From<std::io::Error> for ReferenceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ReferenceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

fn read_json<T>(path: &Path) -> Result<T, ReferenceError>
where
    T: for<'de> Deserialize<'de>,
{
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}
