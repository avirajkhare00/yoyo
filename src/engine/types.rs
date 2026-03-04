use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Core index structs ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub(crate) struct BakeIndex {
    pub(crate) version: String,
    pub(crate) project_root: PathBuf,
    pub(crate) languages: BTreeSet<String>,
    pub(crate) files: Vec<BakeFile>,
    #[serde(default)]
    pub(crate) functions: Vec<crate::lang::IndexedFunction>,
    #[serde(default)]
    pub(crate) endpoints: Vec<crate::lang::IndexedEndpoint>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BakeFile {
    pub(crate) path: PathBuf,
    pub(crate) language: String,
    pub(crate) bytes: u64,
}

// ── Consolidated shared structs ───────────────────────────────────────────────

/// Shared function summary used by shake, api_surface, package_summary.
#[derive(Serialize)]
pub(crate) struct FunctionSummary {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
}

/// Shared endpoint summary used by shake, all_endpoints, api_trace, package_summary.
#[derive(Serialize)]
pub(crate) struct EndpointSummary {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) file: String,
    pub(crate) handler_name: Option<String>,
}

// ── Per-tool payload structs ──────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct LlmInstructionsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) languages: Vec<String>,
    pub(crate) files_indexed: usize,
    pub(crate) tools: Vec<ToolDescription>,
    pub(crate) workflows: Vec<Workflow>,
}

#[derive(Serialize)]
pub(crate) struct ToolDescription {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) requires_bake: bool,
}

#[derive(Serialize)]
pub(crate) struct Workflow {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) steps: Vec<WorkflowStep>,
}

#[derive(Serialize)]
pub(crate) struct WorkflowStep {
    pub(crate) tool: &'static str,
    pub(crate) hint: &'static str,
}

#[derive(Serialize)]
pub(crate) struct ShakePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) languages: Vec<String>,
    pub(crate) files_indexed: usize,
    pub(crate) notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_functions: Option<Vec<FunctionSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) express_endpoints: Option<Vec<EndpointSummary>>,
}

#[derive(Serialize)]
pub(crate) struct BakeSummary {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) bake_path: PathBuf,
    pub(crate) files_indexed: usize,
    pub(crate) languages: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct SearchPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) query: String,
    pub(crate) limit: usize,
    pub(crate) function_hits: Vec<SearchFunctionHit>,
    pub(crate) file_hits: Vec<SearchFileHit>,
}

#[derive(Serialize)]
pub(crate) struct SearchFunctionHit {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
    pub(crate) score: f32,
}

#[derive(Serialize)]
pub(crate) struct SearchFileHit {
    pub(crate) path: String,
    pub(crate) language: String,
    pub(crate) bytes: u64,
    pub(crate) score: f32,
}

#[derive(Serialize)]
pub(crate) struct SymbolPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    pub(crate) matches: Vec<SymbolMatch>,
}

#[derive(Serialize)]
pub(crate) struct SymbolMatch {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AllEndpointsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) endpoints: Vec<EndpointSummary>,
}

#[derive(Serialize)]
pub(crate) struct SupersearchPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) query: String,
    pub(crate) context: String,
    pub(crate) pattern: String,
    pub(crate) exclude_tests: bool,
    pub(crate) matches: Vec<SupersearchMatch>,
}

#[derive(Serialize)]
pub(crate) struct SupersearchMatch {
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) snippet: String,
}

#[derive(Serialize)]
pub(crate) struct PackageSummaryPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) package: String,
    pub(crate) files: Vec<PackageFileSummary>,
    pub(crate) functions: Vec<FunctionSummary>,
    pub(crate) endpoints: Vec<EndpointSummary>,
}

#[derive(Serialize)]
pub(crate) struct PackageFileSummary {
    pub(crate) path: String,
    pub(crate) language: String,
    pub(crate) bytes: u64,
}

#[derive(Serialize)]
pub(crate) struct ArchitectureMapPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) intent: String,
    pub(crate) directories: Vec<ArchitectureDir>,
    pub(crate) suggestions: Vec<ArchitectureSuggestion>,
}

#[derive(Serialize)]
pub(crate) struct ArchitectureDir {
    pub(crate) path: String,
    pub(crate) file_count: u32,
    pub(crate) languages: BTreeSet<String>,
    pub(crate) roles: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ArchitectureSuggestion {
    pub(crate) directory: String,
    pub(crate) score: u32,
    pub(crate) rationale: String,
}

#[derive(Serialize)]
pub(crate) struct SuggestPlacementPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) function_name: String,
    pub(crate) function_type: String,
    pub(crate) related_to: Option<String>,
    pub(crate) suggestions: Vec<PlacementSuggestion>,
}

#[derive(Serialize)]
pub(crate) struct PlacementSuggestion {
    pub(crate) file: String,
    pub(crate) score: u32,
    pub(crate) rationale: String,
}

#[derive(Serialize)]
pub(crate) struct CrudOperationsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) entity: Option<String>,
    pub(crate) entities: Vec<CrudEntitySummary>,
}

#[derive(Serialize)]
pub(crate) struct CrudEntitySummary {
    pub(crate) entity: String,
    pub(crate) operations: Vec<CrudOperation>,
}

#[derive(Serialize)]
pub(crate) struct CrudOperation {
    pub(crate) operation: String,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) file: String,
}

#[derive(Serialize)]
pub(crate) struct ApiTracePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) endpoint: String,
    pub(crate) method: Option<String>,
    pub(crate) traces: Vec<EndpointSummary>,
}

#[derive(Serialize)]
pub(crate) struct FindDocsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) doc_type: String,
    pub(crate) matches: Vec<DocMatch>,
}

#[derive(Serialize)]
pub(crate) struct DocMatch {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snippet: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PatchPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) file: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) total_lines: u32,
}

#[derive(Serialize)]
pub(crate) struct SlicePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) file: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) total_lines: u32,
    pub(crate) lines: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ApiSurfacePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) package: Option<String>,
    pub(crate) limit: usize,
    pub(crate) modules: Vec<ApiSurfaceModule>,
}

#[derive(Serialize)]
pub(crate) struct ApiSurfaceModule {
    pub(crate) module: String,
    pub(crate) functions: Vec<FunctionSummary>,
}

#[derive(Serialize)]
pub(crate) struct FileFunctionsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) file: String,
    pub(crate) include_summaries: bool,
    pub(crate) functions: Vec<FileFunctionSummary>,
}

#[derive(Serialize)]
pub(crate) struct FileFunctionSummary {
    pub(crate) name: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
}
