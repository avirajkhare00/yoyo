use std::path::Path;

use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::{params, Connection};


const BATCH_SIZE: usize = 64;
const MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2;

/// Build or refresh the embeddings database at `db_path`.
/// Called at the end of `bake()`. Idempotent — uses INSERT OR REPLACE.
pub fn build_embeddings(bake_dir: &Path) -> Result<()> {
    let bake_path = bake_dir.join("bake.json");
    let db_path = bake_dir.join("embeddings.db");

    let bake_str = std::fs::read_to_string(&bake_path)?;
    let bake: super::types::BakeIndex = serde_json::from_str(&bake_str)?;

    if bake.functions.is_empty() {
        return Ok(());
    }

    eprintln!("[yoyo] Building embeddings for {} functions…", bake.functions.len());

    let mut model = TextEmbedding::try_new(
        InitOptions::new(MODEL).with_show_download_progress(true),
    )?;

    let conn = open_db(&db_path)?;

    // Build input strings and metadata in parallel
    let inputs: Vec<String> = bake
        .functions
        .iter()
        .map(|f| {
            let callees = f
                .calls
                .iter()
                .map(|c| c.callee.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let file_stem = Path::new(&f.file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            format!("{} {} {} {}", f.name, f.qualified_name, file_stem, callees)
        })
        .collect();

    // Embed in batches
    for (chunk_idx, chunk) in inputs.chunks(BATCH_SIZE).enumerate() {
        let embeddings = model.embed(chunk.to_vec(), None)?;
        let base = chunk_idx * BATCH_SIZE;

        for (i, embedding) in embeddings.iter().enumerate() {
            let func = &bake.functions[base + i];
            let id = format!("{}::{}", func.qualified_name, func.file);
            let bytes = f32_slice_to_bytes(embedding);
            conn.execute(
                "INSERT OR REPLACE INTO embeddings (id, name, file, start_line, parent_type, embedding) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    func.name,
                    func.file,
                    func.start_line,
                    func.parent_type,
                    bytes,
                ],
            )?;
        }
    }

    eprintln!("[yoyo] Embeddings stored → {}", db_path.display());
    Ok(())
}

/// Search the embeddings DB using cosine similarity.
/// Falls back gracefully if DB does not exist.
pub fn vector_search(
    bake_dir: &Path,
    query: &str,
    limit: usize,
    file_filter: Option<&str>,
) -> Result<Option<Vec<VectorMatch>>> {
    let db_path = bake_dir.join("embeddings.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let mut model = TextEmbedding::try_new(InitOptions::new(MODEL))?;
    let query_emb = model.embed(vec![query.to_string()], None)?;
    let query_vec = &query_emb[0];

    let conn = Connection::open(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT name, file, start_line, parent_type, embedding FROM embeddings",
    )?;

    let mut scored: Vec<(f32, VectorMatch)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .filter(|(_, file, _, _, _)| {
            file_filter.map_or(true, |ff| file.to_lowercase().contains(ff))
        })
        .map(|(name, file, start_line, parent_type, bytes)| {
            let emb = bytes_to_f32_vec(&bytes);
            let score = cosine_sim(query_vec, &emb);
            (score, VectorMatch { name, file, start_line, parent_type, score })
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    Ok(Some(scored.into_iter().map(|(_, m)| m).collect()))
}

pub struct VectorMatch {
    pub name: String,
    pub file: String,
    pub start_line: u32,
    pub parent_type: Option<String>,
    pub score: f32,
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn open_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS embeddings (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            file        TEXT NOT NULL,
            start_line  INTEGER NOT NULL,
            parent_type TEXT,
            embedding   BLOB NOT NULL
        );",
    )?;
    Ok(conn)
}

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_f32_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}
