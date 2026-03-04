use clap::{Args, Subcommand};

/// High-level yoyo commands exposed to humans.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Prime directive and usage instructions for yoyo.
    LlmInstructions(LlmInstructionsArgs),
    /// Repository overview similar to Shake.readme.
    Shake(ShakeArgs),
    /// Build and persist a bake index under the project root.
    Bake(BakeArgs),
    /// Fuzzy search over functions and files from the bake index.
    Search(SearchArgs),
    /// Detailed lookup of a function symbol from the bake index.
    Symbol(SymbolArgs),
    /// List all detected API endpoints from the bake index.
    AllEndpoints(AllEndpointsArgs),
    /// Read a specific line range of a file.
    Slice(SliceArgs),
    /// Exported API summary grouped by module (TypeScript-only for now).
    ApiSurface(ApiSurfaceArgs),
    /// Per-file function overview from the bake index.
    FileFunctions(FileFunctionsArgs),
    /// Text-based search over TS/JS source files.
    Supersearch(SupersearchArgs),
    /// Deep-dive summary of a package/module directory.
    PackageSummary(PackageSummaryArgs),
    /// Project structure map and placement hints.
    ArchitectureMap(ArchitectureMapArgs),
    /// Suggest where to place a new function.
    SuggestPlacement(SuggestPlacementArgs),
    /// Entity-level CRUD matrix inferred from endpoints.
    CrudOperations(CrudOperationsArgs),
    /// Trace an API endpoint through backend handlers.
    ApiTrace(ApiTraceArgs),
    /// Find documentation/config files.
    FindDocs(FindDocsArgs),
    /// Apply a line-range patch to a file.
    Patch(PatchArgs),
}

#[derive(Args, Debug)]
pub struct LlmInstructionsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShakeArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct BakeArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Search query text.
    #[arg(long)]
    pub q: String,

    /// Maximum number of results for functions and files.
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct SymbolArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Symbol (function) name to look up.
    #[arg(long)]
    pub name: String,

    /// Include function body (source) inline in each match.
    #[arg(long, default_value_t = false)]
    pub include_source: bool,
}

#[derive(Args, Debug)]
pub struct AllEndpointsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct SliceArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to the project root.
    #[arg(long)]
    pub file: String,

    /// 1-based start line (inclusive).
    #[arg(long)]
    pub start: u32,

    /// 1-based end line (inclusive).
    #[arg(long)]
    pub end: u32,
}

#[derive(Args, Debug)]
pub struct ApiSurfaceArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Optional package/module filter (substring match on module or file paths).
    #[arg(long)]
    pub package: Option<String>,

    /// Maximum number of functions per module (default 20).
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct FileFunctionsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to the project root.
    #[arg(long)]
    pub file: String,

    /// Whether to include summaries (currently a no-op placeholder).
    #[arg(long, default_value_t = true)]
    pub include_summaries: bool,
}

#[derive(Args, Debug)]
pub struct SupersearchArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Search query text.
    #[arg(long)]
    pub query: String,

    /// Search context: all | strings | comments | identifiers.
    #[arg(long, default_value = "all")]
    pub context: String,

    /// Pattern: all | call | assign | return.
    #[arg(long, default_value = "all")]
    pub pattern: String,

    /// Whether to exclude likely test files.
    #[arg(long, default_value_t = true)]
    pub exclude_tests: bool,
}

#[derive(Args, Debug)]
pub struct PackageSummaryArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Package/module name or directory substring.
    #[arg(long)]
    pub package: String,
}

#[derive(Args, Debug)]
pub struct ArchitectureMapArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Intent description, e.g. "user handler" or "auth service".
    #[arg(long)]
    pub intent: String,
}

#[derive(Args, Debug)]
pub struct SuggestPlacementArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Name of the function to add.
    #[arg(long)]
    pub function_name: String,

    /// Function type: handler | service | repository | model | util | test.
    #[arg(long)]
    pub function_type: String,

    /// Existing related symbol or substring (optional).
    #[arg(long)]
    pub related_to: Option<String>,
}

#[derive(Args, Debug)]
pub struct CrudOperationsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Optional entity filter (e.g. "user").
    #[arg(long)]
    pub entity: Option<String>,
}

#[derive(Args, Debug)]
pub struct ApiTraceArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Endpoint path (or substring), e.g. "/users".
    #[arg(long)]
    pub endpoint: String,

    /// Optional HTTP method (GET, POST, etc.).
    #[arg(long)]
    pub method: Option<String>,
}

#[derive(Args, Debug)]
pub struct FindDocsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Documentation type: readme | env | config | docker | all.
    #[arg(long)]
    pub doc_type: String,
}

#[derive(Args, Debug)]
pub struct PatchArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to the project root.
    #[arg(long)]
    pub file: String,

    /// 1-based start line (inclusive).
    #[arg(long)]
    pub start: u32,

    /// 1-based end line (inclusive).
    #[arg(long)]
    pub end: u32,

    /// Replacement content for the specified line range.
    #[arg(long)]
    pub new_content: String,
}

pub async fn run(command: Option<Command>) -> anyhow::Result<()> {
    match command {
        Some(Command::LlmInstructions(args)) => run_llm_instructions(args).await?,
        Some(Command::Shake(args)) => run_shake(args).await?,
        Some(Command::Bake(args)) => run_bake(args).await?,
        Some(Command::Search(args)) => run_search(args).await?,
        Some(Command::Symbol(args)) => run_symbol(args).await?,
        Some(Command::AllEndpoints(args)) => run_all_endpoints(args).await?,
        Some(Command::Slice(args)) => run_slice(args).await?,
        Some(Command::ApiSurface(args)) => run_api_surface(args).await?,
        Some(Command::FileFunctions(args)) => run_file_functions(args).await?,
        Some(Command::Supersearch(args)) => run_supersearch(args).await?,
        Some(Command::PackageSummary(args)) => run_package_summary(args).await?,
        Some(Command::ArchitectureMap(args)) => run_architecture_map(args).await?,
        Some(Command::SuggestPlacement(args)) => run_suggest_placement(args).await?,
        Some(Command::CrudOperations(args)) => run_crud_operations(args).await?,
        Some(Command::ApiTrace(args)) => run_api_trace(args).await?,
        Some(Command::FindDocs(args)) => run_find_docs(args).await?,
        Some(Command::Patch(args)) => run_patch(args).await?,
        None => {
            // For now, print a minimal hint. More commands will be added later.
            eprintln!(
                "No command provided. Try `yoyo llm-instructions --help`, `yoyo shake --help`, `yoyo bake --help`, `yoyo search --help`, `yoyo symbol --help`, `yoyo all-endpoints --help`, `yoyo slice --help`, `yoyo api-surface --help`, `yoyo file-functions --help`, `yoyo supersearch --help`, `yoyo package-summary --help`, `yoyo architecture-map --help`, `yoyo suggest-placement --help`, `yoyo crud-operations --help`, `yoyo api-trace --help`, `yoyo find-docs --help`, or `yoyo patch --help`."
            );
        }
    }
    Ok(())
}

async fn run_llm_instructions(args: LlmInstructionsArgs) -> anyhow::Result<()> {
    let json = crate::engine::llm_instructions(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_shake(args: ShakeArgs) -> anyhow::Result<()> {
    let json = crate::engine::shake(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_bake(args: BakeArgs) -> anyhow::Result<()> {
    let json = crate::engine::bake(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_search(args: SearchArgs) -> anyhow::Result<()> {
    let json = crate::engine::search(args.path, args.q, Some(args.limit))?;
    println!("{json}");
    Ok(())
}

async fn run_symbol(args: SymbolArgs) -> anyhow::Result<()> {
    let json = crate::engine::symbol(args.path, args.name, args.include_source)?;
    println!("{json}");
    Ok(())
}

async fn run_all_endpoints(args: AllEndpointsArgs) -> anyhow::Result<()> {
    let json = crate::engine::all_endpoints(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_slice(args: SliceArgs) -> anyhow::Result<()> {
    let json = crate::engine::slice(args.path, args.file, args.start, args.end)?;
    println!("{json}");
    Ok(())
}

async fn run_api_surface(args: ApiSurfaceArgs) -> anyhow::Result<()> {
    let json = crate::engine::api_surface(args.path, args.package, Some(args.limit))?;
    println!("{json}");
    Ok(())
}

async fn run_file_functions(args: FileFunctionsArgs) -> anyhow::Result<()> {
    let json =
        crate::engine::file_functions(args.path, args.file, Some(args.include_summaries))?;
    println!("{json}");
    Ok(())
}

async fn run_supersearch(args: SupersearchArgs) -> anyhow::Result<()> {
    let json = crate::engine::supersearch(
        args.path,
        args.query,
        args.context,
        args.pattern,
        Some(args.exclude_tests),
    )?;
    println!("{json}");
    Ok(())
}

async fn run_package_summary(args: PackageSummaryArgs) -> anyhow::Result<()> {
    let json = crate::engine::package_summary(args.path, args.package)?;
    println!("{json}");
    Ok(())
}

async fn run_architecture_map(args: ArchitectureMapArgs) -> anyhow::Result<()> {
    let json = crate::engine::architecture_map(args.path, args.intent)?;
    println!("{json}");
    Ok(())
}

async fn run_suggest_placement(args: SuggestPlacementArgs) -> anyhow::Result<()> {
    let json = crate::engine::suggest_placement(
        args.path,
        args.function_name,
        args.function_type,
        args.related_to,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_crud_operations(args: CrudOperationsArgs) -> anyhow::Result<()> {
    let json = crate::engine::crud_operations(args.path, args.entity)?;
    println!("{json}");
    Ok(())
}

async fn run_api_trace(args: ApiTraceArgs) -> anyhow::Result<()> {
    let json = crate::engine::api_trace(args.path, args.endpoint, args.method)?;
    println!("{json}");
    Ok(())
}

async fn run_find_docs(args: FindDocsArgs) -> anyhow::Result<()> {
    let json = crate::engine::find_docs(args.path, args.doc_type)?;
    println!("{json}");
    Ok(())
}

async fn run_patch(args: PatchArgs) -> anyhow::Result<()> {
    let json = crate::engine::patch(args.path, args.file, args.start, args.end, args.new_content)?;
    println!("{json}");
    Ok(())
}

