use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[serde(default)]
    pub(crate) types: Vec<crate::lang::IndexedType>,
    #[serde(default)]
    pub(crate) impls: Vec<crate::lang::IndexedImpl>,
}

fn default_origin() -> String {
    "user".to_string()
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BakeFile {
    pub(crate) path: PathBuf,
    pub(crate) language: String,
    pub(crate) bytes: u64,
    /// Modification time in nanoseconds since UNIX epoch. Used for incremental bake.
    /// Zero means "unknown / not tracked" — file will always be re-parsed.
    #[serde(default)]
    pub(crate) mtime_ns: i64,
    #[serde(default)]
    pub(crate) imports: Vec<String>,
    /// "user" for project files, "stdlib" for toolchain stdlib files.
    #[serde(default = "default_origin")]
    pub(crate) origin: String,
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

pub(crate) const DEFAULT_COMPACT_LIMIT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseView {
    Compact,
    Full,
    Raw,
}

impl ResponseView {
    pub(crate) fn parse(view: Option<&str>) -> Result<Self> {
        match view.unwrap_or("raw") {
            "compact" => Ok(Self::Compact),
            "full" => Ok(Self::Full),
            "raw" => Ok(Self::Raw),
            other => Err(anyhow!(
                "Unsupported view '{other}'. Expected one of: compact, full, raw."
            )),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
            Self::Raw => "raw",
        }
    }
}

pub(crate) fn parse_section_cursor(cursor: Option<&str>) -> Result<Option<(String, usize)>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };

    let (section, offset) = cursor
        .split_once(':')
        .ok_or_else(|| anyhow!("Invalid cursor '{cursor}'. Expected format <section>:<offset>."))?;
    let offset = offset.parse::<usize>().map_err(|_| {
        anyhow!("Invalid cursor '{cursor}'. Offset must be a non-negative integer.")
    })?;
    Ok(Some((section.to_string(), offset)))
}

#[derive(Serialize)]
pub(crate) struct CompactSection {
    pub(crate) section: String,
    pub(crate) total: usize,
    pub(crate) offset: usize,
    pub(crate) limit: usize,
    pub(crate) items: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_cursor: Option<String>,
}

pub(crate) fn build_compact_section(
    section: &str,
    items: Vec<Value>,
    limit: usize,
    cursor: Option<(&str, usize)>,
) -> Option<CompactSection> {
    if let Some((cursor_section, _)) = cursor {
        if cursor_section != section {
            return None;
        }
    }

    let total = items.len();
    let offset = cursor
        .and_then(|(cursor_section, offset)| (cursor_section == section).then_some(offset))
        .unwrap_or(0)
        .min(total);
    let end = offset.saturating_add(limit).min(total);
    let next_cursor = (end < total).then(|| format!("{section}:{end}"));

    Some(CompactSection {
        section: section.to_string(),
        total,
        offset,
        limit,
        items: items[offset..end].to_vec(),
        next_cursor,
    })
}

// ── Per-tool payload structs ──────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct LlmWorkflowsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) workflows: Vec<Workflow>,
    /// Maps natural-language questions to the correct yoyo tool.
    pub(crate) decision_map: Vec<DecisionEntry>,
    /// Explicit anti-patterns — things that look reasonable but produce wrong answers.
    pub(crate) antipatterns: Vec<&'static str>,
    /// High-level shapes that all workflows are instances of.
    pub(crate) metapatterns: Vec<Metapattern>,
}

#[derive(Serialize)]
pub(crate) struct LlmWorkflowsCompactPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) view: &'static str,
    pub(crate) summary: String,
    pub(crate) sections: Vec<CompactSection>,
    pub(crate) detail_hints: Vec<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct WorkflowQueryMatch {
    pub(crate) kind: &'static str,
    pub(crate) score: usize,
    pub(crate) item: serde_json::Value,
}

#[derive(Serialize)]
pub(crate) struct LlmWorkflowsQueryPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) query: String,
    pub(crate) matches: Vec<WorkflowQueryMatch>,
}

#[derive(Serialize)]
pub(crate) struct DecisionEntry {
    pub(crate) question: &'static str,
    pub(crate) wrong_tool: &'static str,
    pub(crate) wrong_because: &'static str,
    pub(crate) right_tool: &'static str,
    pub(crate) right_field: &'static str,
}

#[derive(Serialize)]
pub struct ToolDescription {
    pub name: &'static str,
    pub description: &'static str,
    pub requires_bake: bool,
    pub category: &'static str,
    pub parallelisable: bool,
    /// JSON skeleton of the top-level fields this tool returns.
    /// Used by pipeline spec authors to write correct {{step.field}} refs.
    /// None for tools with no structured output (write tools, bootstrap tools).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_shape: Option<&'static str>,
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

/// A high-level shape that all yoyo workflows are instances of.
/// Learn these five shapes first — every specific workflow is a variation.
#[derive(Serialize)]
pub(crate) struct Metapattern {
    /// Short label, e.g. "Orient → Scope → Read"
    pub(crate) shape: &'static str,
    /// When to apply this pattern
    pub(crate) when: &'static str,
    /// The abstract steps and the concrete tools that implement each
    pub(crate) steps: Vec<MetapatternStep>,
    /// Concrete named workflows that are instances of this shape
    pub(crate) instances: Vec<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct MetapatternStep {
    pub(crate) phase: &'static str,
    pub(crate) tools: Vec<&'static str>,
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
    /// Number of files skipped because mtime+size matched the cached index.
    /// Omitted when zero (first bake or full rebuild).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) files_skipped: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct SymbolPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    pub(crate) matches: Vec<SymbolMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_hint: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct SymbolMatch {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
    /// True on the single most-likely definition when the name is ambiguous
    /// (multiple files define it). Ranked by incoming call count, then complexity.
    pub(crate) primary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) visibility: Option<crate::lang::Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) module_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) qualified_name: Option<String>,
    /// Calls to other project-defined functions only (stdlib/built-ins excluded).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) calls: Vec<crate::lang::CallSite>,
    /// For methods: the struct/enum/trait this is defined on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) params: Vec<crate::lang::SignatureParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) return_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) receiver: Option<String>,
    /// For structs/enums: traits this type implements.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) implements: Vec<String>,
    /// For traits: concrete types that implement this trait.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) implementors: Vec<String>,
    /// For structs: parsed field names and types.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) fields: Vec<crate::lang::FieldInfo>,
    /// True when this match came from an installed toolchain stdlib, not user code.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) is_stdlib: bool,
    /// Structural signature fingerprint — hash of (param_types, return_type). Name-agnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sig_hash: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_hint: Option<&'static str>,
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
    pub(crate) intent: Option<String>,
    pub(crate) total_dirs: usize,
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
pub(crate) struct FindDocsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) doc_type: String,
    pub(crate) truncated: bool,
    pub(crate) matches: Vec<DocMatch>,
}

#[derive(Serialize)]
pub(crate) struct DocMatch {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snippet: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct SyntaxError {
    pub(crate) line: u32,
    pub(crate) kind: String, // "error" | "missing"
    pub(crate) text: String, // up to 80 chars of the offending node
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) patched_source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) syntax_errors: Vec<SyntaxError>,
}

#[derive(Serialize)]
#[allow(dead_code)]
pub(crate) struct PatchBytesPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) file: String,
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
    pub(crate) new_bytes: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) syntax_errors: Vec<SyntaxError>,
}

#[derive(Serialize)]
pub(crate) struct MultiPatchPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) files_written: usize,
    pub(crate) edits_applied: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) syntax_errors: Vec<SyntaxError>,
}

#[derive(Serialize)]
pub(crate) struct GraphRenamePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) old_name: String,
    pub(crate) new_name: String,
    pub(crate) scope: String,
    pub(crate) files_changed: usize,
    pub(crate) occurrences_renamed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct GraphAddPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) entity_type: String,
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) inserted_at_byte: usize,
}

#[derive(Serialize)]
pub(crate) struct GraphCreatePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) file: String,
    pub(crate) function_name: String,
    pub(crate) language: String,
}

#[derive(Serialize)]
pub(crate) struct GraphMovePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    pub(crate) from_file: String,
    pub(crate) to_file: String,
    pub(crate) imports_added: Vec<String>,
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
pub(crate) struct FileFunctionsPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) file: String,
    pub(crate) include_summaries: bool,
    pub(crate) depth: String,
    pub(crate) functions: Vec<FileFunctionSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) types: Vec<TypeInspectMatch>,
}

#[derive(Serialize)]
pub(crate) struct FileFunctionSummary {
    pub(crate) name: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_type: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct TypeMethodSummary {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) complexity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) visibility: Option<crate::lang::Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) implemented_trait: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sig_hash: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct TypeInspectMatch {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) primary: bool,
    pub(crate) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) declaration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) visibility: Option<crate::lang::Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) module_path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) fields: Vec<crate::lang::FieldInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) methods: Vec<TypeMethodSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) implements: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) implementors: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) is_stdlib: bool,
}

#[derive(Serialize)]
pub(crate) struct TraceNode {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) qualifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) boundary: Option<String>,
    pub(crate) resolved: bool,
}

#[derive(Serialize)]
pub(crate) struct TraceDownPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) symbol: String,
    pub(crate) chain: Vec<TraceNode>,
    pub(crate) unresolved: Vec<String>,
}

// ── flow ──────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct FlowHandlerInfo {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FlowPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) endpoint: EndpointSummary,
    pub(crate) handler: FlowHandlerInfo,
    pub(crate) call_chain: Vec<TraceNode>,
    pub(crate) boundaries: Vec<String>,
    pub(crate) unresolved: Vec<String>,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) chain_warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_hint: Option<&'static str>,
}

// ── health ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct HealthPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    /// Fowler: Dead Code — functions never called; safe to delete.
    pub(crate) dead_code: Vec<DeadFunction>,
    /// Fowler: Large Function — high complexity × high fan-out composite.
    pub(crate) large_functions: Vec<LargeFunction>,
    /// Fowler: Long Method — functions too long to read on one screen.
    pub(crate) long_methods: Vec<LongMethod>,
    /// Fowler: Feature Envy — functions that reach into other modules more than their own.
    pub(crate) feature_envy: Vec<FeatureEnvy>,
    /// Fowler: Shotgun Surgery — functions called from many different files; one change, many edit sites.
    pub(crate) shotgun_surgery: Vec<ShotgunSurgery>,
    /// Fowler: Insider Trading — file pairs with bidirectional coupling.
    pub(crate) insider_trading: Vec<InsiderTrading>,
    /// Fowler: Duplicate Code — same-stem functions spread across multiple files.
    pub(crate) duplicate_code: Vec<DuplicateGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_hint: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct HealthCompactPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) view: &'static str,
    pub(crate) summary: String,
    pub(crate) sections: Vec<CompactSection>,
    pub(crate) detail_hints: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_hint: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct DeadFunction {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) lines: u32,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
}

/// Formerly GodFunction. Renamed to match Fowler's vocabulary.
#[derive(Serialize)]
pub(crate) struct LargeFunction {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) complexity: u32,
    pub(crate) fan_out: usize,
    pub(crate) score: u32,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
    pub(crate) why: String,
}

#[derive(Serialize)]
pub(crate) struct LongMethod {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) lines: u32,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
    pub(crate) why: String,
}

#[derive(Serialize)]
pub(crate) struct FeatureEnvy {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    /// The file this function calls most outside its own module.
    pub(crate) envies: String,
    pub(crate) cross_file_calls: usize,
    pub(crate) same_file_calls: usize,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
    pub(crate) why: String,
}

#[derive(Serialize)]
pub(crate) struct ShotgunSurgery {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    /// Number of distinct files that call this function.
    pub(crate) caller_files: usize,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
    pub(crate) why: String,
}

#[derive(Serialize)]
pub(crate) struct InsiderTrading {
    pub(crate) file_a: String,
    pub(crate) file_b: String,
    /// Calls from file_a into functions defined in file_b.
    pub(crate) a_calls_b: usize,
    /// Calls from file_b into functions defined in file_a.
    pub(crate) b_calls_a: usize,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
    pub(crate) why: String,
}

#[derive(Serialize)]
pub(crate) struct DuplicateGroup {
    pub(crate) stem: String,
    pub(crate) functions: Vec<DuplicateEntry>,
    pub(crate) smell: &'static str,
    pub(crate) refactoring: &'static str,
}

#[derive(Serialize)]
pub(crate) struct DuplicateEntry {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
}

// ── semantic_search ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct SemanticSearchPayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) query: String,
    pub(crate) results: Vec<SemanticMatch>,
    /// Present when the embeddings index is not yet ready. Results are TF-IDF
    /// until the background build (started by bake) finishes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct SemanticMatch {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_type: Option<String>,
    pub(crate) kind: &'static str,
}

// ── judge_change ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct JudgeChangePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) symbol_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file_hint: Option<String>,
    pub(crate) ownership_layer: JudgeOwnershipLayer,
    pub(crate) candidate_symbols: Vec<JudgeCandidateSymbol>,
    pub(crate) candidate_files: Vec<JudgeCandidateFile>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) rejected_alternatives: Vec<JudgeRejectedAlternative>,
    pub(crate) invariants: Vec<JudgeFinding>,
    pub(crate) regression_risks: Vec<JudgeFinding>,
    pub(crate) verification_commands: Vec<JudgeCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_hint: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct JudgeOwnershipLayer {
    pub(crate) name: String,
    pub(crate) why: String,
    pub(crate) evidence_files: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct JudgeCandidateSymbol {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) kind: &'static str,
    pub(crate) score: f32,
    pub(crate) why: String,
    pub(crate) incoming_callers: usize,
    pub(crate) caller_files: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) visibility: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct JudgeCandidateFile {
    pub(crate) file: String,
    pub(crate) role: String,
    pub(crate) why: String,
}

#[derive(Serialize)]
pub(crate) struct JudgeRejectedAlternative {
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) kind: &'static str,
    pub(crate) reason: String,
}

#[derive(Serialize)]
pub(crate) struct JudgeFinding {
    pub(crate) text: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct JudgeCommand {
    pub(crate) command: String,
    pub(crate) why: String,
}

// ── graph_delete ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct GraphDeletePayload {
    pub(crate) tool: &'static str,
    pub(crate) version: &'static str,
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) bytes_removed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}
