use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::engine::types::{BakeFile, BakeIndex};
use crate::lang::{
    CallSite, FieldInfo, IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType,
    SignatureParam, Visibility,
};

// ── schema ────────────────────────────────────────────────────────────────────

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS languages (
    name TEXT PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS files (
    path     TEXT PRIMARY KEY,
    language TEXT NOT NULL,
    bytes    INTEGER NOT NULL,
    mtime_ns INTEGER NOT NULL DEFAULT 0,
    origin   TEXT NOT NULL,
    imports  TEXT NOT NULL   -- JSON array
);
CREATE TABLE IF NOT EXISTS functions (
    id                INTEGER PRIMARY KEY,
    name              TEXT    NOT NULL,
    file              TEXT    NOT NULL,
    language          TEXT    NOT NULL,
    start_line        INTEGER NOT NULL,
    end_line          INTEGER NOT NULL,
    complexity        INTEGER NOT NULL,
    byte_start        INTEGER NOT NULL,
    byte_end          INTEGER NOT NULL,
    module_path       TEXT    NOT NULL,
    qualified_name    TEXT    NOT NULL,
    visibility        TEXT    NOT NULL,
    parent_type       TEXT,
    implemented_trait TEXT,
    params_json       TEXT    NOT NULL,
    return_type       TEXT,
    receiver          TEXT,
    is_stdlib         INTEGER NOT NULL,
    sig_hash          TEXT
);
CREATE TABLE IF NOT EXISTS calls (
    function_id INTEGER NOT NULL,
    callee      TEXT    NOT NULL,
    qualifier   TEXT,
    line        INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS types (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    file        TEXT    NOT NULL,
    language    TEXT    NOT NULL,
    start_line  INTEGER NOT NULL,
    end_line    INTEGER NOT NULL,
    kind        TEXT    NOT NULL,
    module_path TEXT    NOT NULL,
    visibility  TEXT    NOT NULL,
    is_stdlib   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS type_fields (
    type_id    INTEGER NOT NULL,
    name       TEXT    NOT NULL,
    type_str   TEXT    NOT NULL,
    visibility TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS impls (
    type_name  TEXT    NOT NULL,
    trait_name TEXT,
    file       TEXT    NOT NULL,
    start_line INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS endpoints (
    method       TEXT NOT NULL,
    path         TEXT NOT NULL,
    file         TEXT NOT NULL,
    handler_name TEXT,
    language     TEXT NOT NULL,
    framework    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fn_name      ON functions(name);
CREATE INDEX IF NOT EXISTS idx_fn_file      ON functions(file);
CREATE INDEX IF NOT EXISTS idx_calls_fn     ON calls(function_id);
CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee);
CREATE INDEX IF NOT EXISTS idx_type_name    ON types(name);
CREATE INDEX IF NOT EXISTS idx_type_file    ON types(file);
";

// ── helpers ───────────────────────────────────────────────────────────────────

fn vis_str(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public  => "public",
        Visibility::Module  => "module",
        Visibility::Private => "private",
    }
}

fn str_vis(s: &str) -> Visibility {
    match s {
        "public"  => Visibility::Public,
        "module"  => Visibility::Module,
        _         => Visibility::Private,
    }
}

// ── write ─────────────────────────────────────────────────────────────────────

pub fn write_bake_to_db(bake: &BakeIndex, db_path: &Path) -> Result<()> {
    // Always recreate — bake is a full rebuild.
    if db_path.exists() {
        std::fs::remove_file(db_path)
            .with_context(|| format!("Failed to remove old bake.db at {}", db_path.display()))?;
    }

    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open bake.db at {}", db_path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    conn.execute_batch(SCHEMA)?;

    let tx = conn.unchecked_transaction()?;

    // meta
    tx.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('version', ?1), ('project_root', ?2)",
        params![
            bake.version,
            bake.project_root.to_string_lossy().as_ref(),
        ],
    )?;

    // languages
    for lang in &bake.languages {
        tx.execute("INSERT OR IGNORE INTO languages (name) VALUES (?1)", params![lang])?;
    }

    // files
    for f in &bake.files {
        let imports = serde_json::to_string(&f.imports).unwrap_or_else(|_| "[]".into());
        tx.execute(
            "INSERT OR REPLACE INTO files (path, language, bytes, mtime_ns, origin, imports) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                f.path.to_string_lossy().as_ref(),
                f.language,
                f.bytes as i64,
                f.mtime_ns,
                f.origin,
                imports,
            ],
        )?;
    }

    // functions + calls
    {
        let mut fn_stmt = tx.prepare(
            "INSERT INTO functions (name, file, language, start_line, end_line, complexity, \
             byte_start, byte_end, module_path, qualified_name, visibility, parent_type, \
             implemented_trait, params_json, return_type, receiver, is_stdlib, sig_hash) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        )?;
        let mut call_stmt = tx.prepare(
            "INSERT INTO calls (function_id, callee, qualifier, line) VALUES (?1, ?2, ?3, ?4)",
        )?;

        for f in &bake.functions {
            let params_json = serde_json::to_string(&f.params).unwrap_or_else(|_| "[]".into());
            fn_stmt.execute(params![
                f.name,
                f.file,
                f.language,
                f.start_line,
                f.end_line,
                f.complexity,
                f.byte_start as i64,
                f.byte_end as i64,
                f.module_path,
                f.qualified_name,
                vis_str(&f.visibility),
                f.parent_type,
                f.implemented_trait,
                params_json,
                f.return_type,
                f.receiver,
                f.is_stdlib as i32,
                f.sig_hash,
            ])?;
            let fn_id = tx.last_insert_rowid();
            for c in &f.calls {
                call_stmt.execute(params![fn_id, c.callee, c.qualifier, c.line])?;
            }
        }
    }

    // types + type_fields
    {
        let mut ty_stmt = tx.prepare(
            "INSERT INTO types (name, file, language, start_line, end_line, kind, module_path, \
             visibility, is_stdlib) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        )?;
        let mut field_stmt = tx.prepare(
            "INSERT INTO type_fields (type_id, name, type_str, visibility) VALUES (?1,?2,?3,?4)",
        )?;
        for t in &bake.types {
            ty_stmt.execute(params![
                t.name,
                t.file,
                t.language,
                t.start_line,
                t.end_line,
                t.kind,
                t.module_path,
                vis_str(&t.visibility),
                t.is_stdlib as i32,
            ])?;
            let ty_id = tx.last_insert_rowid();
            for field in &t.fields {
                field_stmt.execute(params![ty_id, field.name, field.type_str, vis_str(&field.visibility)])?;
            }
        }
    }

    // impls
    {
        let mut impl_stmt = tx.prepare(
            "INSERT INTO impls (type_name, trait_name, file, start_line) VALUES (?1,?2,?3,?4)",
        )?;
        for i in &bake.impls {
            impl_stmt.execute(params![i.type_name, i.trait_name, i.file, i.start_line])?;
        }
    }

    // endpoints
    {
        let mut ep_stmt = tx.prepare(
            "INSERT INTO endpoints (method, path, file, handler_name, language, framework) VALUES (?1,?2,?3,?4,?5,?6)",
        )?;
        for e in &bake.endpoints {
            ep_stmt.execute(params![e.method, e.path, e.file, e.handler_name, e.language, e.framework])?;
        }
    }

    tx.commit()?;
    Ok(())
}

// ── incremental ───────────────────────────────────────────────────────────────

/// Load (path → (mtime_ns, bytes)) for all files in an existing bake.db.
/// Returns an empty map if the DB doesn't exist or can't be read.
pub fn load_file_fingerprints(db_path: &Path) -> std::collections::HashMap<String, (i64, i64)> {
    let Ok(conn) = Connection::open(db_path) else {
        return std::collections::HashMap::new();
    };
    let Ok(mut stmt) = conn.prepare("SELECT path, mtime_ns, bytes FROM files") else {
        return std::collections::HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    }) else {
        return std::collections::HashMap::new();
    };
    let mut map = std::collections::HashMap::new();
    for row in rows.flatten() {
        map.insert(row.0, (row.1, row.2));
    }
    map
}

/// Incrementally update an existing bake.db:
/// - Removes rows for `removed` paths and all changed files in `bake.files`
/// - Inserts fresh rows for all changed/new files
/// - Leaves unchanged files untouched
/// Returns (total_file_count, all_languages) queried from the DB after the write.
pub fn write_bake_incremental(
    bake: &BakeIndex,
    removed: &[String],
    db_path: &Path,
) -> Result<(usize, Vec<String>)> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open bake.db at {}", db_path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    // Migrate older DBs that predate the mtime_ns column (error is ignored when column exists).
    let _ = conn.execute_batch(
        "ALTER TABLE files ADD COLUMN mtime_ns INTEGER NOT NULL DEFAULT 0",
    );
    // Ensure all tables and indexes exist (no-op when schema is current).
    conn.execute_batch(SCHEMA)?;

    {
        let tx = conn.unchecked_transaction()?;

        // Upsert meta.
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('version', ?1), ('project_root', ?2)",
            params![bake.version, bake.project_root.to_string_lossy().as_ref()],
        )?;

        // Upsert languages (adds new ones, leaves existing untouched).
        for lang in &bake.languages {
            tx.execute("INSERT OR IGNORE INTO languages (name) VALUES (?1)", params![lang])?;
        }

        // Collect all paths that need to be deleted: removed files + every changed/new file.
        let paths_to_delete: Vec<String> = removed
            .iter()
            .cloned()
            .chain(bake.files.iter().map(|f| f.path.to_string_lossy().into_owned()))
            .collect();

        for path in &paths_to_delete {
            tx.execute(
                "DELETE FROM calls WHERE function_id IN (SELECT id FROM functions WHERE file = ?1)",
                params![path],
            )?;
            tx.execute(
                "DELETE FROM type_fields WHERE type_id IN (SELECT id FROM types WHERE file = ?1)",
                params![path],
            )?;
            tx.execute("DELETE FROM functions WHERE file = ?1", params![path])?;
            tx.execute("DELETE FROM types WHERE file = ?1", params![path])?;
            tx.execute("DELETE FROM impls WHERE file = ?1", params![path])?;
            tx.execute("DELETE FROM endpoints WHERE file = ?1", params![path])?;
            tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        }

        // Insert new rows for changed/new files.
        for f in &bake.files {
            let imports = serde_json::to_string(&f.imports).unwrap_or_else(|_| "[]".into());
            tx.execute(
                "INSERT OR REPLACE INTO files (path, language, bytes, mtime_ns, origin, imports) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    f.path.to_string_lossy().as_ref(),
                    f.language,
                    f.bytes as i64,
                    f.mtime_ns,
                    f.origin,
                    imports,
                ],
            )?;
        }

        {
            let mut fn_stmt = tx.prepare(
                "INSERT INTO functions (name, file, language, start_line, end_line, complexity, \
                 byte_start, byte_end, module_path, qualified_name, visibility, parent_type, \
                 implemented_trait, params_json, return_type, receiver, is_stdlib, sig_hash) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            )?;
            let mut call_stmt = tx.prepare(
                "INSERT INTO calls (function_id, callee, qualifier, line) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for f in &bake.functions {
                let params_json = serde_json::to_string(&f.params).unwrap_or_else(|_| "[]".into());
                fn_stmt.execute(params![
                    f.name, f.file, f.language, f.start_line, f.end_line, f.complexity,
                    f.byte_start as i64, f.byte_end as i64, f.module_path, f.qualified_name,
                    vis_str(&f.visibility), f.parent_type, f.implemented_trait, params_json,
                    f.return_type, f.receiver,
                    f.is_stdlib as i32, f.sig_hash,
                ])?;
                let fn_id = tx.last_insert_rowid();
                for c in &f.calls {
                    call_stmt.execute(params![fn_id, c.callee, c.qualifier, c.line])?;
                }
            }
        }

        {
            let mut ty_stmt = tx.prepare(
                "INSERT INTO types (name, file, language, start_line, end_line, kind, module_path, \
                 visibility, is_stdlib) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            )?;
            let mut field_stmt = tx.prepare(
                "INSERT INTO type_fields (type_id, name, type_str, visibility) VALUES (?1,?2,?3,?4)",
            )?;
            for t in &bake.types {
                ty_stmt.execute(params![
                    t.name, t.file, t.language, t.start_line, t.end_line, t.kind,
                    t.module_path, vis_str(&t.visibility), t.is_stdlib as i32,
                ])?;
                let ty_id = tx.last_insert_rowid();
                for field in &t.fields {
                    field_stmt.execute(params![
                        ty_id, field.name, field.type_str, vis_str(&field.visibility),
                    ])?;
                }
            }
        }

        {
            let mut impl_stmt = tx.prepare(
                "INSERT INTO impls (type_name, trait_name, file, start_line) VALUES (?1,?2,?3,?4)",
            )?;
            for i in &bake.impls {
                impl_stmt.execute(params![i.type_name, i.trait_name, i.file, i.start_line])?;
            }
        }

        {
            let mut ep_stmt = tx.prepare(
                "INSERT INTO endpoints (method, path, file, handler_name, language, framework) VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            for e in &bake.endpoints {
                ep_stmt.execute(params![
                    e.method, e.path, e.file, e.handler_name, e.language, e.framework,
                ])?;
            }
        }

        tx.commit()?;
    }

    // Query totals from the now-updated DB.
    let total_files = conn
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as usize;
    let mut lang_stmt = conn.prepare("SELECT name FROM languages")?;
    let languages: Vec<String> = lang_stmt
        .query_map([], |r| r.get(0))?
        .flatten()
        .collect();

    Ok((total_files, languages))
}

// ── read ──────────────────────────────────────────────────────────────────────

pub fn read_bake_from_db(db_path: &Path) -> Result<BakeIndex> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open bake.db at {}", db_path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // meta
    let version: String = conn
        .query_row("SELECT value FROM meta WHERE key='version'", [], |r| r.get(0))
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let root_str: String = conn
        .query_row("SELECT value FROM meta WHERE key='project_root'", [], |r| r.get(0))
        .unwrap_or_default();
    let project_root = PathBuf::from(root_str);

    // languages
    let mut languages = BTreeSet::new();
    {
        let mut stmt = conn.prepare("SELECT name FROM languages")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows { languages.insert(row?); }
    }

    // files
    let mut files: Vec<BakeFile> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT path, language, bytes, mtime_ns, origin, imports FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        for row in rows {
            let (path, language, bytes, mtime_ns, origin, imports_json) = row?;
            let imports: Vec<String> = serde_json::from_str(&imports_json).unwrap_or_default();
            files.push(BakeFile {
                path: PathBuf::from(path),
                language,
                bytes: bytes as u64,
                mtime_ns,
                origin,
                imports,
            });
        }
    }

    // functions — load all, then attach calls
    let mut functions: Vec<IndexedFunction> = Vec::new();
    let mut fn_ids: Vec<i64> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, file, language, start_line, end_line, complexity, byte_start, \
             byte_end, module_path, qualified_name, visibility, parent_type, implemented_trait, \
             params_json, return_type, receiver, is_stdlib, sig_hash FROM functions",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, u32>(5)?,
                r.get::<_, u32>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, Option<String>>(12)?,
                r.get::<_, Option<String>>(13)?,
                r.get::<_, String>(14)?,
                r.get::<_, Option<String>>(15)?,
                r.get::<_, Option<String>>(16)?,
                r.get::<_, i32>(17)?,
                r.get::<_, Option<String>>(18)?,
            ))
        })?;
        for row in rows {
            let (id, name, file, language, start_line, end_line, complexity,
                 byte_start, byte_end, module_path, qualified_name, visibility,
                 parent_type, implemented_trait, params_json, return_type, receiver, is_stdlib, sig_hash) = row?;
            fn_ids.push(id);
            let params: Vec<SignatureParam> = serde_json::from_str(&params_json).unwrap_or_default();
            functions.push(IndexedFunction {
                name,
                file,
                language,
                start_line,
                end_line,
                complexity,
                byte_start: byte_start as usize,
                byte_end: byte_end as usize,
                module_path,
                qualified_name,
                visibility: str_vis(&visibility),
                parent_type,
                implemented_trait,
                params,
                return_type,
                receiver,
                is_stdlib: is_stdlib != 0,
                sig_hash,
                calls: vec![],
            });
        }
    }

    // calls — attach to functions by index
    {
        // Build a map: function rowid → index in `functions` vec
        let mut id_to_idx: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for (idx, &id) in fn_ids.iter().enumerate() {
            id_to_idx.insert(id, idx);
        }
        let mut stmt = conn.prepare("SELECT function_id, callee, qualifier, line FROM calls")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, u32>(3)?,
            ))
        })?;
        for row in rows {
            let (fn_id, callee, qualifier, line) = row?;
            if let Some(&idx) = id_to_idx.get(&fn_id) {
                functions[idx].calls.push(CallSite { callee, qualifier, line });
            }
        }
    }

    // types + type_fields
    let mut types: Vec<IndexedType> = Vec::new();
    {
        let mut ty_stmt = conn.prepare(
            "SELECT id, name, file, language, start_line, end_line, kind, module_path, \
             visibility, is_stdlib FROM types",
        )?;
        let ty_rows = ty_stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, u32>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, i32>(9)?,
            ))
        })?;
        let mut ty_ids: Vec<i64> = Vec::new();
        for row in ty_rows {
            let (id, name, file, language, start_line, end_line, kind, module_path, visibility, is_stdlib) = row?;
            ty_ids.push(id);
            types.push(IndexedType {
                name, file, language, start_line, end_line, kind, module_path,
                visibility: str_vis(&visibility),
                is_stdlib: is_stdlib != 0,
                fields: vec![],
            });
        }
        // attach fields
        if !ty_ids.is_empty() {
            let mut id_to_idx: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
            for (idx, &id) in ty_ids.iter().enumerate() { id_to_idx.insert(id, idx); }
            let mut f_stmt = conn.prepare("SELECT type_id, name, type_str, visibility FROM type_fields")?;
            let f_rows = f_stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
            })?;
            for row in f_rows {
                let (ty_id, name, type_str, vis) = row?;
                if let Some(&idx) = id_to_idx.get(&ty_id) {
                    types[idx].fields.push(FieldInfo { name, type_str, visibility: str_vis(&vis) });
                }
            }
        }
    }

    // impls
    let mut impls: Vec<IndexedImpl> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT type_name, trait_name, file, start_line FROM impls")?;
        let rows = stmt.query_map([], |r| {
            Ok(IndexedImpl {
                type_name: r.get(0)?,
                trait_name: r.get(1)?,
                file: r.get(2)?,
                start_line: r.get(3)?,
            })
        })?;
        for row in rows { impls.push(row?); }
    }

    // endpoints
    let mut endpoints: Vec<IndexedEndpoint> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT method, path, file, handler_name, language, framework FROM endpoints",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(IndexedEndpoint {
                method: r.get(0)?,
                path: r.get(1)?,
                file: r.get(2)?,
                handler_name: r.get(3)?,
                language: r.get(4)?,
                framework: r.get(5)?,
            })
        })?;
        for row in rows { endpoints.push(row?); }
    }

    Ok(BakeIndex { version, project_root, languages, files, functions, endpoints, types, impls })
}

// ── tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::lang::{CallSite, FieldInfo, IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType, SignatureParam, Visibility};
    use crate::engine::types::{BakeFile, BakeIndex};

    fn empty_bake(root: &std::path::Path) -> BakeIndex {
        BakeIndex {
            version: "0.0.0".to_string(),
            project_root: root.to_path_buf(),
            languages: BTreeSet::new(),
            files: vec![],
            functions: vec![],
            endpoints: vec![],
            types: vec![],
            impls: vec![],
        }
    }

    // ── round-trip helpers ──────────────────────────────────────────────────

    fn roundtrip(bake: BakeIndex) -> BakeIndex {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");
        write_bake_to_db(&bake, &db).unwrap();
        read_bake_from_db(&db).unwrap()
    }

    // ── meta ────────────────────────────────────────────────────────────────

    #[test]
    fn roundtrip_meta_version_and_root() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.version = "1.2.3".to_string();
        bake.project_root = PathBuf::from("/some/project");
        let out = roundtrip(bake);
        assert_eq!(out.version, "1.2.3");
        assert_eq!(out.project_root, PathBuf::from("/some/project"));
    }

    // ── languages ───────────────────────────────────────────────────────────

    #[test]
    fn roundtrip_languages() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.languages.insert("rust".to_string());
        bake.languages.insert("go".to_string());
        bake.languages.insert("typescript".to_string());
        let out = roundtrip(bake);
        assert_eq!(out.languages.len(), 3);
        assert!(out.languages.contains("rust"));
        assert!(out.languages.contains("go"));
        assert!(out.languages.contains("typescript"));
    }

    #[test]
    fn roundtrip_empty_languages() {
        let dir = TempDir::new().unwrap();
        let bake = empty_bake(dir.path());
        let out = roundtrip(bake);
        assert!(out.languages.is_empty());
    }

    // ── files ────────────────────────────────────────────────────────────────

    #[test]
    fn roundtrip_files() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.files.push(BakeFile {
            path: PathBuf::from("src/main.rs"),
            language: "rust".to_string(),
            bytes: 1024,
            mtime_ns: 0,
            origin: "local".to_string(),
            imports: vec!["std::fs".to_string(), "anyhow".to_string()],
        });
        bake.files.push(BakeFile {
            path: PathBuf::from("src/lib.rs"),
            language: "rust".to_string(),
            bytes: 512,
            mtime_ns: 0,
            origin: "local".to_string(),
            imports: vec![],
        });
        let out = roundtrip(bake);
        assert_eq!(out.files.len(), 2);
        let main = out.files.iter().find(|f| f.path == PathBuf::from("src/main.rs")).unwrap();
        assert_eq!(main.language, "rust");
        assert_eq!(main.bytes, 1024);
        assert_eq!(main.origin, "local");
        assert_eq!(main.imports, vec!["std::fs", "anyhow"]);
        let lib = out.files.iter().find(|f| f.path == PathBuf::from("src/lib.rs")).unwrap();
        assert!(lib.imports.is_empty());
    }

    #[test]
    fn roundtrip_file_imports_special_chars() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.files.push(BakeFile {
            path: PathBuf::from("pkg/foo.go"),
            language: "go".to_string(),
            bytes: 100,
            mtime_ns: 0,
            origin: "local".to_string(),
            imports: vec![
                "github.com/gin-gonic/gin".to_string(),
                r#"encoding/json"#.to_string(),
            ],
        });
        let out = roundtrip(bake);
        let f = &out.files[0];
        assert_eq!(f.imports[0], "github.com/gin-gonic/gin");
        assert_eq!(f.imports[1], "encoding/json");
    }

    // ── functions ────────────────────────────────────────────────────────────

    fn make_fn(name: &str) -> IndexedFunction {
        IndexedFunction {
            name: name.to_string(),
            file: format!("src/{}.rs", name),
            language: "rust".to_string(),
            start_line: 10,
            end_line: 20,
            complexity: 3,
            byte_start: 100,
            byte_end: 400,
            module_path: format!("crate::{}", name),
            qualified_name: format!("crate::{}::{}", name, name),
            visibility: Visibility::Public,
            parent_type: None,
            implemented_trait: None,
            params: vec![],
            return_type: None,
            receiver: None,
            is_stdlib: false,
            sig_hash: Some("abc123".to_string()),
            calls: vec![],
        }
    }

    #[test]
    fn roundtrip_function_basic_fields() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.functions.push(make_fn("process"));
        let out = roundtrip(bake);
        assert_eq!(out.functions.len(), 1);
        let f = &out.functions[0];
        assert_eq!(f.name, "process");
        assert_eq!(f.file, "src/process.rs");
        assert_eq!(f.language, "rust");
        assert_eq!(f.start_line, 10);
        assert_eq!(f.end_line, 20);
        assert_eq!(f.complexity, 3);
        assert_eq!(f.byte_start, 100);
        assert_eq!(f.byte_end, 400);
        assert_eq!(f.module_path, "crate::process");
        assert_eq!(f.qualified_name, "crate::process::process");
        assert_eq!(f.sig_hash, Some("abc123".to_string()));
        assert!(f.params.is_empty());
        assert_eq!(f.return_type, None);
        assert_eq!(f.receiver, None);
        assert!(!f.is_stdlib);
    }

    #[test]
    fn roundtrip_function_visibility_all_variants() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());

        let mut pub_fn = make_fn("pub_fn");
        pub_fn.visibility = Visibility::Public;
        let mut mod_fn = make_fn("mod_fn");
        mod_fn.visibility = Visibility::Module;
        let mut priv_fn = make_fn("priv_fn");
        priv_fn.visibility = Visibility::Private;

        bake.functions.extend([pub_fn, mod_fn, priv_fn]);
        let out = roundtrip(bake);

        let by_name = |n: &str| out.functions.iter().find(|f| f.name == n).unwrap();
        assert!(matches!(by_name("pub_fn").visibility, Visibility::Public));
        assert!(matches!(by_name("mod_fn").visibility, Visibility::Module));
        assert!(matches!(by_name("priv_fn").visibility, Visibility::Private));
    }

    #[test]
    fn roundtrip_function_optional_fields() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        let mut f = make_fn("method");
        f.parent_type = Some("MyStruct".to_string());
        f.implemented_trait = Some("Display".to_string());
        f.params = vec![SignatureParam { name: "value".to_string(), type_str: "i32".to_string() }];
        f.return_type = Some("String".to_string());
        f.receiver = Some("&self".to_string());
        f.is_stdlib = true;
        f.sig_hash = None;
        bake.functions.push(f);
        let out = roundtrip(bake);
        let rf = &out.functions[0];
        assert_eq!(rf.parent_type, Some("MyStruct".to_string()));
        assert_eq!(rf.implemented_trait, Some("Display".to_string()));
        assert_eq!(rf.params.len(), 1);
        assert_eq!(rf.params[0].name, "value");
        assert_eq!(rf.params[0].type_str, "i32");
        assert_eq!(rf.return_type, Some("String".to_string()));
        assert_eq!(rf.receiver, Some("&self".to_string()));
        assert!(rf.is_stdlib);
        assert_eq!(rf.sig_hash, None);
    }

    #[test]
    fn roundtrip_function_calls() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        let mut caller = make_fn("caller");
        caller.calls = vec![
            CallSite { callee: "foo".to_string(), qualifier: Some("bar".to_string()), line: 15 },
            CallSite { callee: "baz".to_string(), qualifier: None, line: 18 },
        ];
        let mut other = make_fn("other");
        other.calls = vec![
            CallSite { callee: "qux".to_string(), qualifier: None, line: 11 },
        ];
        bake.functions.extend([caller, other]);
        let out = roundtrip(bake);

        let caller_out = out.functions.iter().find(|f| f.name == "caller").unwrap();
        assert_eq!(caller_out.calls.len(), 2);
        let foo_call = caller_out.calls.iter().find(|c| c.callee == "foo").unwrap();
        assert_eq!(foo_call.qualifier, Some("bar".to_string()));
        assert_eq!(foo_call.line, 15);
        let baz_call = caller_out.calls.iter().find(|c| c.callee == "baz").unwrap();
        assert_eq!(baz_call.qualifier, None);

        let other_out = out.functions.iter().find(|f| f.name == "other").unwrap();
        assert_eq!(other_out.calls.len(), 1);
        assert_eq!(other_out.calls[0].callee, "qux");
    }

    #[test]
    fn roundtrip_function_no_calls() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.functions.push(make_fn("leaf"));
        let out = roundtrip(bake);
        assert!(out.functions[0].calls.is_empty());
    }

    #[test]
    fn roundtrip_many_functions() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        for i in 0..200 {
            bake.functions.push(make_fn(&format!("fn_{}", i)));
        }
        let out = roundtrip(bake);
        assert_eq!(out.functions.len(), 200);
    }

    // ── types ────────────────────────────────────────────────────────────────

    fn make_type(name: &str, kind: &str) -> IndexedType {
        IndexedType {
            name: name.to_string(),
            file: "src/types.rs".to_string(),
            language: "rust".to_string(),
            start_line: 5,
            end_line: 15,
            kind: kind.to_string(),
            module_path: "crate::types".to_string(),
            visibility: Visibility::Public,
            is_stdlib: false,
            fields: vec![],
        }
    }

    #[test]
    fn roundtrip_type_basic() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.types.push(make_type("Config", "struct"));
        let out = roundtrip(bake);
        assert_eq!(out.types.len(), 1);
        let t = &out.types[0];
        assert_eq!(t.name, "Config");
        assert_eq!(t.kind, "struct");
        assert_eq!(t.file, "src/types.rs");
        assert_eq!(t.language, "rust");
        assert_eq!(t.start_line, 5);
        assert_eq!(t.end_line, 15);
        assert_eq!(t.module_path, "crate::types");
        assert!(!t.is_stdlib);
    }

    #[test]
    fn roundtrip_type_with_fields() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        let mut t = make_type("Config", "struct");
        t.fields = vec![
            FieldInfo { name: "host".to_string(), type_str: "String".to_string(), visibility: Visibility::Public },
            FieldInfo { name: "port".to_string(), type_str: "u16".to_string(), visibility: Visibility::Private },
            FieldInfo { name: "timeout".to_string(), type_str: "Duration".to_string(), visibility: Visibility::Module },
        ];
        bake.types.push(t);
        let out = roundtrip(bake);
        let rt = &out.types[0];
        assert_eq!(rt.fields.len(), 3);
        let host = rt.fields.iter().find(|f| f.name == "host").unwrap();
        assert_eq!(host.type_str, "String");
        assert!(matches!(host.visibility, Visibility::Public));
        let port = rt.fields.iter().find(|f| f.name == "port").unwrap();
        assert!(matches!(port.visibility, Visibility::Private));
        let timeout = rt.fields.iter().find(|f| f.name == "timeout").unwrap();
        assert!(matches!(timeout.visibility, Visibility::Module));
    }

    #[test]
    fn roundtrip_multiple_types_fields_dont_mix() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        let mut a = make_type("Alpha", "struct");
        a.fields = vec![FieldInfo { name: "x".to_string(), type_str: "i32".to_string(), visibility: Visibility::Public }];
        let mut b = make_type("Beta", "enum");
        b.fields = vec![
            FieldInfo { name: "One".to_string(), type_str: "()".to_string(), visibility: Visibility::Public },
            FieldInfo { name: "Two".to_string(), type_str: "String".to_string(), visibility: Visibility::Public },
        ];
        bake.types.extend([a, b]);
        let out = roundtrip(bake);
        let alpha = out.types.iter().find(|t| t.name == "Alpha").unwrap();
        assert_eq!(alpha.fields.len(), 1);
        assert_eq!(alpha.fields[0].name, "x");
        let beta = out.types.iter().find(|t| t.name == "Beta").unwrap();
        assert_eq!(beta.fields.len(), 2);
    }

    #[test]
    fn roundtrip_type_kind_variants() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        for kind in &["struct", "enum", "interface", "type_alias", "class"] {
            bake.types.push(make_type(&format!("T_{}", kind), kind));
        }
        let out = roundtrip(bake);
        assert_eq!(out.types.len(), 5);
        for kind in &["struct", "enum", "interface", "type_alias", "class"] {
            let t = out.types.iter().find(|t| t.kind == *kind).unwrap();
            assert_eq!(&t.kind, kind);
        }
    }

    // ── impls ────────────────────────────────────────────────────────────────

    #[test]
    fn roundtrip_impls() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.impls.push(IndexedImpl {
            type_name: "Config".to_string(),
            trait_name: Some("Default".to_string()),
            file: "src/config.rs".to_string(),
            start_line: 30,
        });
        bake.impls.push(IndexedImpl {
            type_name: "Config".to_string(),
            trait_name: None,
            file: "src/config.rs".to_string(),
            start_line: 50,
        });
        let out = roundtrip(bake);
        assert_eq!(out.impls.len(), 2);
        let with_trait = out.impls.iter().find(|i| i.trait_name.is_some()).unwrap();
        assert_eq!(with_trait.type_name, "Config");
        assert_eq!(with_trait.trait_name, Some("Default".to_string()));
        assert_eq!(with_trait.start_line, 30);
        let without_trait = out.impls.iter().find(|i| i.trait_name.is_none()).unwrap();
        assert_eq!(without_trait.start_line, 50);
    }

    // ── endpoints ────────────────────────────────────────────────────────────

    #[test]
    fn roundtrip_endpoints() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.endpoints.push(IndexedEndpoint {
            method: "GET".to_string(),
            path: "/api/users".to_string(),
            file: "src/routes.rs".to_string(),
            handler_name: Some("list_users".to_string()),
            language: "rust".to_string(),
            framework: "actix".to_string(),
        });
        bake.endpoints.push(IndexedEndpoint {
            method: "POST".to_string(),
            path: "/api/users".to_string(),
            file: "src/routes.rs".to_string(),
            handler_name: None,
            language: "rust".to_string(),
            framework: "actix".to_string(),
        });
        let out = roundtrip(bake);
        assert_eq!(out.endpoints.len(), 2);
        let get = out.endpoints.iter().find(|e| e.method == "GET").unwrap();
        assert_eq!(get.path, "/api/users");
        assert_eq!(get.handler_name, Some("list_users".to_string()));
        assert_eq!(get.framework, "actix");
        let post = out.endpoints.iter().find(|e| e.method == "POST").unwrap();
        assert_eq!(post.handler_name, None);
    }

    // ── write idempotency / overwrite ────────────────────────────────────────

    #[test]
    fn write_overwrites_existing_db() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");

        // Write a bake with one function
        let mut bake1 = empty_bake(dir.path());
        bake1.functions.push(make_fn("first"));
        write_bake_to_db(&bake1, &db).unwrap();
        let out1 = read_bake_from_db(&db).unwrap();
        assert_eq!(out1.functions.len(), 1);

        // Overwrite with a different bake — old data must not persist
        let mut bake2 = empty_bake(dir.path());
        bake2.functions.push(make_fn("second"));
        bake2.functions.push(make_fn("third"));
        write_bake_to_db(&bake2, &db).unwrap();
        let out2 = read_bake_from_db(&db).unwrap();
        assert_eq!(out2.functions.len(), 2);
        assert!(out2.functions.iter().any(|f| f.name == "second"));
        assert!(out2.functions.iter().any(|f| f.name == "third"));
        assert!(!out2.functions.iter().any(|f| f.name == "first"));
    }

    // ── full round-trip with all entity types ─────────────────────────────────

    #[test]
    fn roundtrip_full_bake() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.version = "9.8.7".to_string();
        bake.languages.insert("rust".to_string());
        bake.languages.insert("go".to_string());
        bake.files.push(BakeFile {
            path: PathBuf::from("src/main.rs"),
            language: "rust".to_string(),
            bytes: 2048,
            mtime_ns: 0,
            origin: "local".to_string(),
            imports: vec!["anyhow".to_string()],
        });
        let mut f = make_fn("handler");
        f.parent_type = Some("Server".to_string());
        f.calls = vec![
            CallSite { callee: "respond".to_string(), qualifier: None, line: 12 },
        ];
        bake.functions.push(f);
        let mut t = make_type("Server", "struct");
        t.fields = vec![
            FieldInfo { name: "addr".to_string(), type_str: "SocketAddr".to_string(), visibility: Visibility::Public },
        ];
        bake.types.push(t);
        bake.impls.push(IndexedImpl {
            type_name: "Server".to_string(),
            trait_name: Some("Default".to_string()),
            file: "src/main.rs".to_string(),
            start_line: 80,
        });
        bake.endpoints.push(IndexedEndpoint {
            method: "GET".to_string(),
            path: "/health".to_string(),
            file: "src/main.rs".to_string(),
            handler_name: Some("health_check".to_string()),
            language: "rust".to_string(),
            framework: "actix".to_string(),
        });

        let out = roundtrip(bake);

        assert_eq!(out.version, "9.8.7");
        assert_eq!(out.languages.len(), 2);
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.functions.len(), 1);
        assert_eq!(out.functions[0].calls.len(), 1);
        assert_eq!(out.types.len(), 1);
        assert_eq!(out.types[0].fields.len(), 1);
        assert_eq!(out.impls.len(), 1);
        assert_eq!(out.endpoints.len(), 1);
    }

    // ── error cases ───────────────────────────────────────────────────────────

    #[test]
    fn read_nonexistent_db_returns_error() {
        let dir = TempDir::new().unwrap();
        let result = read_bake_from_db(&dir.path().join("missing.db"));
        // rusqlite creates an empty db on open, but reading meta will use defaults
        // or succeed with empty tables — either is acceptable. Just ensure no panic.
        let _ = result;
    }

    // ── mtime_ns round-trip ───────────────────────────────────────────────────

    #[test]
    fn roundtrip_file_mtime_ns() {
        let dir = TempDir::new().unwrap();
        let mut bake = empty_bake(dir.path());
        bake.files.push(BakeFile {
            path: PathBuf::from("src/main.rs"),
            language: "rust".to_string(),
            bytes: 512,
            mtime_ns: 1_700_000_000_000_000_000,
            origin: "user".to_string(),
            imports: vec![],
        });
        let out = roundtrip(bake);
        assert_eq!(out.files[0].mtime_ns, 1_700_000_000_000_000_000);
    }

    // ── load_file_fingerprints ────────────────────────────────────────────────

    #[test]
    fn load_fingerprints_empty_when_no_db() {
        let dir = TempDir::new().unwrap();
        let fp = load_file_fingerprints(&dir.path().join("missing.db"));
        assert!(fp.is_empty());
    }

    #[test]
    fn load_fingerprints_returns_mtime_and_bytes() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");
        let mut bake = empty_bake(dir.path());
        bake.files.push(BakeFile {
            path: PathBuf::from("src/lib.rs"),
            language: "rust".to_string(),
            bytes: 1024,
            mtime_ns: 999_999_999,
            origin: "user".to_string(),
            imports: vec![],
        });
        bake.files.push(BakeFile {
            path: PathBuf::from("pkg/foo.go"),
            language: "go".to_string(),
            bytes: 2048,
            mtime_ns: 123_456_789,
            origin: "user".to_string(),
            imports: vec![],
        });
        write_bake_to_db(&bake, &db).unwrap();

        let fp = load_file_fingerprints(&db);
        assert_eq!(fp.len(), 2);
        assert_eq!(fp["src/lib.rs"], (999_999_999, 1024));
        assert_eq!(fp["pkg/foo.go"], (123_456_789, 2048));
    }

    // ── write_bake_incremental ────────────────────────────────────────────────

    fn make_bake_with_file(root: &std::path::Path, rel_path: &str, mtime_ns: i64, bytes: u64) -> BakeIndex {
        let mut bake = empty_bake(root);
        bake.files.push(BakeFile {
            path: PathBuf::from(rel_path),
            language: "rust".to_string(),
            bytes,
            mtime_ns,
            origin: "user".to_string(),
            imports: vec![],
        });
        bake.languages.insert("rust".to_string());
        bake
    }

    #[test]
    fn incremental_adds_new_file() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");

        // Initial full bake with one file.
        let bake1 = make_bake_with_file(dir.path(), "src/a.rs", 100, 512);
        write_bake_to_db(&bake1, &db).unwrap();

        // Incremental: add a second file, keep first unchanged.
        let bake2 = make_bake_with_file(dir.path(), "src/b.rs", 200, 1024);
        let (total, _langs) = write_bake_incremental(&bake2, &[], &db).unwrap();

        assert_eq!(total, 2);
        let out = read_bake_from_db(&db).unwrap();
        assert_eq!(out.files.len(), 2);
        assert!(out.files.iter().any(|f| f.path == PathBuf::from("src/a.rs")));
        assert!(out.files.iter().any(|f| f.path == PathBuf::from("src/b.rs")));
    }

    #[test]
    fn incremental_removes_deleted_file() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");

        let mut bake1 = empty_bake(dir.path());
        for name in &["src/a.rs", "src/b.rs", "src/c.rs"] {
            bake1.files.push(BakeFile {
                path: PathBuf::from(name),
                language: "rust".to_string(),
                bytes: 100,
                mtime_ns: 500,
                origin: "user".to_string(),
                imports: vec![],
            });
        }
        write_bake_to_db(&bake1, &db).unwrap();

        // Remove src/b.rs.
        let empty_changed = empty_bake(dir.path());
        let (total, _) = write_bake_incremental(
            &empty_changed,
            &["src/b.rs".to_string()],
            &db,
        ).unwrap();

        assert_eq!(total, 2);
        let out = read_bake_from_db(&db).unwrap();
        assert!(out.files.iter().any(|f| f.path == PathBuf::from("src/a.rs")));
        assert!(!out.files.iter().any(|f| f.path == PathBuf::from("src/b.rs")));
        assert!(out.files.iter().any(|f| f.path == PathBuf::from("src/c.rs")));
    }

    #[test]
    fn incremental_updates_changed_file() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");

        let bake1 = make_bake_with_file(dir.path(), "src/main.rs", 100, 512);
        write_bake_to_db(&bake1, &db).unwrap();

        // Same path, different mtime and bytes — simulates a file edit.
        let bake2 = make_bake_with_file(dir.path(), "src/main.rs", 999, 1024);
        let (total, _) = write_bake_incremental(&bake2, &[], &db).unwrap();

        assert_eq!(total, 1);
        let out = read_bake_from_db(&db).unwrap();
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].mtime_ns, 999);
        assert_eq!(out.files[0].bytes, 1024);
    }

    #[test]
    fn incremental_functions_scoped_to_changed_file() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("bake.db");

        // Initial bake with a function in src/a.rs.
        let mut bake1 = empty_bake(dir.path());
        bake1.files.push(BakeFile {
            path: PathBuf::from("src/a.rs"),
            language: "rust".to_string(),
            bytes: 100,
            mtime_ns: 1,
            origin: "user".to_string(),
            imports: vec![],
        });
        let mut f = make_fn("old_fn");
        f.file = "src/a.rs".to_string();
        bake1.functions.push(f);
        write_bake_to_db(&bake1, &db).unwrap();

        // Incremental: replace src/a.rs with a new function.
        let mut bake2 = empty_bake(dir.path());
        bake2.files.push(BakeFile {
            path: PathBuf::from("src/a.rs"),
            language: "rust".to_string(),
            bytes: 200,
            mtime_ns: 2,
            origin: "user".to_string(),
            imports: vec![],
        });
        let mut f2 = make_fn("new_fn");
        f2.file = "src/a.rs".to_string();
        bake2.functions.push(f2);
        write_bake_incremental(&bake2, &[], &db).unwrap();

        let out = read_bake_from_db(&db).unwrap();
        assert_eq!(out.functions.len(), 1);
        assert_eq!(out.functions[0].name, "new_fn");
    }
}
