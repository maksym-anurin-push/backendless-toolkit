//! CLI for Backendless Console data inspection / schema restore.
//!
//! ```text
//! backendless-toolkit-db show_tables [table...]
//! backendless-toolkit-db dump_table_structure <table>
//! backendless-toolkit-db dump_table_data <table>
//! cat table.yaml | backendless-toolkit-db restore_table_structure <table> [--dry-run] ...
//! cat table.csv | backendless-toolkit-db restore_table_data <table> [--bulk]
//! ```

use backendless_console_sdk::{ConsoleClient, DEFAULT_PAGE_SIZE, Error as SdkError, TableInfo, data_types};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::io::{self, Read, Write};
use std::process::ExitCode;

const UPSERT_BATCH_SIZE: usize = 50;

const ENV_URL: &str = "BACKENDLESS_TOOLKIT__CONSOLE__URL";
const ENV_APP_ID: &str = "BACKENDLESS_TOOLKIT__CONSOLE__APP_ID";
const ENV_LOGIN: &str = "BACKENDLESS_TOOLKIT__CONSOLE__LOGIN";
const ENV_PASSWORD: &str = "BACKENDLESS_TOOLKIT__CONSOLE__PASSWORD";

const SYSTEM_COLUMNS: &[&str] = &["objectId", "created", "updated", "ownerId"];

const COMPARE_KEYS: &[&str] = &[
    "dataType",
    "unique",
    "indexed",
    "defaultValue",
    "dataSize",
    // dataSizeAuto omitted: deprecated (API 1316); never restore/compare it
    "customRegex",
    "expression",
    "toTableName",
    "relationshipType",
    "relatedColumnName",
    "autoLoad",
];

/// Default STRING length when dump has null size (auto-size era dumps).
const DEFAULT_STRING_DATA_SIZE: u64 = 250;

struct Config {
    url: String,
    app_id: String,
    login: String,
    password: String,
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let require = |key: &str| {
            env::var(key).map_err(|_| format!("missing required environment variable: {key}"))
        };
        Ok(Self {
            url: require(ENV_URL)?,
            app_id: require(ENV_APP_ID)?,
            login: require(ENV_LOGIN)?,
            password: require(ENV_PASSWORD)?,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct RestoreFlags {
    apply_required_flag: bool,
    delete_obsolete_columns: bool,
    dry_run: bool,
}

impl RestoreFlags {
    fn parse(args: &[String]) -> Result<(String, Self), String> {
        let mut table = None;
        let mut flags = Self::default();
        for arg in args {
            match arg.as_str() {
                "--apply-required-flag" => flags.apply_required_flag = true,
                "--delete-obsolete-columns" => flags.delete_obsolete_columns = true,
                "--dry-run" => flags.dry_run = true,
                other if other.starts_with('-') => {
                    return Err(format!("unknown flag: {other}"));
                }
                other => {
                    if table.is_some() {
                        return Err(format!("unexpected argument: {other}"));
                    }
                    table = Some(other.to_string());
                }
            }
        }
        let table =
            table.ok_or_else(|| "restore_table_structure requires <table>".to_string())?;
        Ok((table, flags))
    }
}

fn parse_restore_table_data_args(args: &[String]) -> Result<(String, bool), String> {
    let mut table = None;
    let mut bulk = false;
    for arg in args {
        match arg.as_str() {
            "--bulk" => bulk = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown flag: {other}"));
            }
            other => {
                if table.is_some() {
                    return Err(format!("unexpected argument: {other}"));
                }
                table = Some(other.to_string());
            }
        }
    }
    let table = table.ok_or_else(|| "restore_table_data requires <table>".to_string())?;
    Ok((table, bulk))
}

#[derive(Debug, Clone)]
enum DiffOp {
    CreateTable,
    Add { name: String, column: Value },
    Update {
        name: String,
        current: Value,
        desired: Value,
    },
    Delete { name: String, column: Value },
}

impl DiffOp {
    fn describe(&self) -> String {
        match self {
            DiffOp::CreateTable => "CREATE TABLE".to_string(),
            DiffOp::Add { name, column } => {
                format!(
                    "ADD COLUMN {name} ({})",
                    column_data_type(column).unwrap_or("?")
                )
            }
            DiffOp::Update { name, .. } => format!("UPDATE COLUMN {name}"),
            DiffOp::Delete { name, column } => {
                format!(
                    "DELETE COLUMN {name} ({})",
                    column_data_type(column).unwrap_or("?")
                )
            }
        }
    }
}

#[derive(Debug, Default)]
struct RestorePlan {
    ops: Vec<DiffOp>,
}

fn usage() -> String {
    "Usage:\n  \
     backendless-toolkit-db show_tables [table...]\n  \
     backendless-toolkit-db dump_table_structure <table>\n  \
     backendless-toolkit-db dump_table_data <table>\n  \
     cat <table>.yaml | backendless-toolkit-db restore_table_structure <table> \\\n    \
     [--apply-required-flag] [--delete-obsolete-columns] [--dry-run]\n  \
     cat <table>.csv | backendless-toolkit-db restore_table_data <table> [--bulk]\n"
        .to_string()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if let Err(err) = dotenvy::dotenv() {
        eprintln!("failed to load .env: {err}");
        return ExitCode::FAILURE;
    }

    let mut args = env::args().skip(1);
    let command = match args.next() {
        Some(c) => c,
        None => {
            eprint!("{}", usage());
            return ExitCode::FAILURE;
        }
    };

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    let result = run(&command, args.collect(), &config).await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: &str, args: Vec<String>, config: &Config) -> Result<(), String> {
    let mut client =
        ConsoleClient::new(&config.url).map_err(|e| format!("invalid console URL: {e}"))?;

    client
        .login(&config.login, &config.password)
        .await
        .map_err(format_sdk_error)?;

    match command {
        "show_tables" => show_tables(&client, &config.app_id, &args).await,
        "dump_table_structure" => {
            let table = args
                .first()
                .ok_or_else(|| "dump_table_structure requires <table>".to_string())?;
            if args.len() != 1 {
                return Err(usage());
            }
            dump_table_structure(&client, &config.app_id, table).await
        }
        "dump_table_data" => {
            let table = args
                .first()
                .ok_or_else(|| "dump_table_data requires <table>".to_string())?;
            if args.len() != 1 {
                return Err(usage());
            }
            dump_table_data(&client, &config.app_id, table).await
        }
        "restore_table_structure" => {
            let (table, flags) = RestoreFlags::parse(&args)?;
            restore_table_structure(&client, &config.app_id, &table, &flags).await
        }
        "restore_table_data" => {
            let (table, bulk) = parse_restore_table_data_args(&args)?;
            restore_table_data(&client, &config.app_id, &table, bulk).await
        }
        other => Err(format!("unknown command: {other}\n{}", usage())),
    }
}

async fn show_tables(
    client: &ConsoleClient,
    app_id: &str,
    seeds: &[String],
) -> Result<(), String> {
    let tables = client
        .list_tables(app_id)
        .await
        .map_err(format_sdk_error)?;
    let subset = if seeds.is_empty() {
        tables
    } else {
        tables_dependency_closure(&tables, seeds)?
    };
    let (ordered, cycles) = tables_dependency_order(&subset);
    if !cycles.is_empty() {
        eprintln!("{}", format_dependency_cycles(&cycles));
    }
    let mut stdout = io::stdout().lock();
    for name in ordered {
        writeln!(stdout, "{name}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Transitive closure of dependency targets for `seeds` via outgoing `relations`.
/// Edge `A → B` means A depends on B (`A.relations[].toTableName == B`).
/// Unknown seed names fail. Unknown `toTableName` targets are skipped.
fn tables_dependency_closure(
    tables: &[TableInfo],
    seeds: &[String],
) -> Result<Vec<TableInfo>, String> {
    let by_name: HashMap<&str, &TableInfo> =
        tables.iter().map(|t| (t.name.as_str(), t)).collect();

    let mut selected: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = Vec::new();

    for seed in seeds {
        if !by_name.contains_key(seed.as_str()) {
            return Err(format!("unknown table: {seed}"));
        }
        if selected.insert(seed.clone()) {
            stack.push(seed.clone());
        }
    }

    while let Some(name) = stack.pop() {
        let Some(table) = by_name.get(name.as_str()) else {
            continue;
        };
        for rel in &table.relations {
            let Some(to) = rel.get("toTableName").and_then(|v| v.as_str()) else {
                continue;
            };
            if !by_name.contains_key(to) {
                continue;
            }
            if selected.insert(to.to_string()) {
                stack.push(to.to_string());
            }
        }
    }

    Ok(tables
        .iter()
        .filter(|t| selected.contains(&t.name))
        .cloned()
        .collect())
}

/// Topological order: independent tables (no outgoing `relations`) first, then dependents.
/// Edge `A → B` means A depends on B (`A.relations[].toTableName == B`), so B is emitted before A.
/// Cycles are reported separately; cyclic leftovers are appended in input order (no extra sort).
fn tables_dependency_order(tables: &[TableInfo]) -> (Vec<String>, Vec<Vec<String>>) {
    let names: BTreeSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();

    // dependee → set of tables that depend on it
    let mut reverse: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    // depender → set of tables it depends on
    let mut forward: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut indegree: BTreeMap<&str, usize> = names.iter().map(|n| (*n, 0usize)).collect();

    for table in tables {
        let from = table.name.as_str();
        let mut seen_deps = BTreeSet::new();
        for rel in &table.relations {
            let Some(to) = rel.get("toTableName").and_then(|v| v.as_str()) else {
                continue;
            };
            if !names.contains(to) {
                continue;
            }
            if !seen_deps.insert(to) {
                continue;
            }
            *indegree.get_mut(from).expect("node present") += 1;
            reverse.entry(to).or_default().insert(from);
            forward.entry(from).or_default().insert(to);
        }
    }

    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(n, _)| *n)
        .collect();

    let mut ordered = Vec::with_capacity(tables.len());
    while let Some(next) = ready.pop_first() {
        ordered.push(next.to_string());
        if let Some(dependers) = reverse.get(next) {
            for &depender in dependers {
                let deg = indegree.get_mut(depender).expect("node present");
                *deg -= 1;
                if *deg == 0 {
                    ready.insert(depender);
                }
            }
        }
    }

    if ordered.len() == names.len() {
        return (ordered, Vec::new());
    }

    let residual: BTreeSet<&str> = indegree
        .into_iter()
        .filter(|(_, deg)| *deg > 0)
        .map(|(n, _)| n)
        .collect();
    let cycles = find_dependency_cycles(&forward, &residual);
    for table in tables {
        if residual.contains(table.name.as_str()) {
            ordered.push(table.name.clone());
        }
    }
    (ordered, cycles)
}

/// Find elementary directed cycles among `residual` nodes using DFS + canonicalization.
fn find_dependency_cycles(
    forward: &BTreeMap<&str, BTreeSet<&str>>,
    residual: &BTreeSet<&str>,
) -> Vec<Vec<String>> {
    let mut found: BTreeSet<Vec<String>> = BTreeSet::new();

    for &start in residual {
        let mut path: Vec<&str> = Vec::new();
        let mut in_path: BTreeSet<&str> = BTreeSet::new();
        dfs_dependency_cycles(start, forward, residual, &mut path, &mut in_path, &mut found);
    }

    found.into_iter().collect()
}

fn dfs_dependency_cycles<'a>(
    node: &'a str,
    forward: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    residual: &BTreeSet<&'a str>,
    path: &mut Vec<&'a str>,
    in_path: &mut BTreeSet<&'a str>,
    found: &mut BTreeSet<Vec<String>>,
) {
    path.push(node);
    in_path.insert(node);

    if let Some(neighbors) = forward.get(node) {
        for &next in neighbors {
            if !residual.contains(next) {
                continue;
            }
            if in_path.contains(next) {
                let idx = path.iter().position(|&n| n == next).expect("in path");
                found.insert(canonicalize_cycle(&path[idx..]));
            } else {
                dfs_dependency_cycles(next, forward, residual, path, in_path, found);
            }
        }
    }

    path.pop();
    in_path.remove(node);
}

/// Rotate cycle so the lexicographically smallest name is first.
fn canonicalize_cycle(nodes: &[&str]) -> Vec<String> {
    let n = nodes.len();
    debug_assert!(n > 0);
    let min_idx = nodes
        .iter()
        .enumerate()
        .min_by_key(|(_, name)| *name)
        .map(|(i, _)| i)
        .unwrap_or(0);
    (0..n)
        .map(|i| nodes[(min_idx + i) % n].to_string())
        .collect()
}

fn format_dependency_cycles(cycles: &[Vec<String>]) -> String {
    let mut out = String::from("dependency cycle:");
    for cycle in cycles {
        let mut path = cycle.clone();
        if let Some(first) = cycle.first() {
            path.push(first.clone());
        }
        out.push('\n');
        out.push_str(&path.join(" -> "));
    }
    out
}

async fn dump_table_structure(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
) -> Result<(), String> {
    let table = client
        .get_table(app_id, table_name)
        .await
        .map_err(format_sdk_error)?;
    let yaml = table_structure_yaml(&table)?;
    print!("{yaml}");
    if !yaml.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn table_structure_yaml(table: &TableInfo) -> Result<String, String> {
    let value = serde_json::to_value(table).map_err(|e| format!("JSON serialize error: {e}"))?;
    let sorted = sort_value_keys(value);
    serde_yaml::to_string(&sorted).map_err(|e| format!("YAML serialize error: {e}"))
}

fn sort_value_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, sort_value_keys(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_value_keys).collect()),
        other => other,
    }
}

async fn dump_table_data(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
) -> Result<(), String> {
    let rows = client
        .find_all_records(app_id, table_name, DEFAULT_PAGE_SIZE)
        .await
        .map_err(format_sdk_error)?;

    // Empty table → empty file (0 bytes); restore treats that as a no-op skip.
    if rows.is_empty() {
        return Ok(());
    }

    let headers = collect_csv_headers(&rows);
    let mut writer = csv::Writer::from_writer(io::stdout());

    writer
        .write_record(&headers)
        .map_err(|e| format!("CSV write error: {e}"))?;

    for row in &rows {
        let record = match row.as_object() {
            Some(map) => map,
            None => {
                return Err(format!(
                    "expected object row, got: {}",
                    serde_json::to_string(row).unwrap_or_default()
                ));
            }
        };
        let mut values = Vec::with_capacity(headers.len());
        for key in &headers {
            values.push(json_cell(record.get(key)));
        }
        writer
            .write_record(&values)
            .map_err(|e| format!("CSV write error: {e}"))?;
    }

    writer.flush().map_err(|e| format!("CSV flush error: {e}"))?;
    Ok(())
}

fn collect_csv_headers(rows: &[Value]) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for row in rows {
        if let Some(map) = row.as_object() {
            for key in map.keys() {
                keys.insert(key.clone());
            }
        }
    }

    let mut headers = Vec::new();
    for preferred in ["objectId", "created", "updated"] {
        if keys.remove(preferred) {
            headers.push(preferred.to_string());
        }
    }
    headers.extend(keys);
    headers
}

fn json_cell(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

async fn restore_table_structure(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
    flags: &RestoreFlags,
) -> Result<(), String> {
    let desired = read_desired_table_from_stdin()?;
    if desired.name != table_name {
        return Err(format!(
            "YAML table name {:?} does not match argv table {:?}",
            desired.name, table_name
        ));
    }

    let all_tables = client
        .list_tables(app_id)
        .await
        .map_err(format_sdk_error)?;
    preflight_relations(&desired, &all_tables)?;

    let current = all_tables.iter().find(|t| t.name == table_name).cloned();
    let plan = build_plan(&desired, current.as_ref(), flags);

    print_plan(&plan, flags.dry_run)?;

    if flags.dry_run {
        eprintln!("dry-run: no changes applied");
        return Ok(());
    }

    apply_plan(client, app_id, table_name, &plan).await
}

fn read_desired_table_from_stdin() -> Result<TableInfo, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("failed to read stdin: {e}"))?;
    if buf.trim().is_empty() {
        return Err("stdin is empty; pipe a dump_table_structure YAML document".into());
    }
    let value: Value =
        serde_yaml::from_str(&buf).map_err(|e| format!("invalid YAML on stdin: {e}"))?;
    serde_json::from_value(value).map_err(|e| format!("YAML does not match table schema: {e}"))
}

fn preflight_relations(desired: &TableInfo, all_tables: &[TableInfo]) -> Result<(), String> {
    let by_name: BTreeMap<&str, &TableInfo> =
        all_tables.iter().map(|t| (t.name.as_str(), t)).collect();

    let mut errors = Vec::new();
    for rel in desired.relations.iter().chain(desired.geo_relations.iter()) {
        let Some(obj) = rel.as_object() else {
            errors.push("relation entry is not an object".to_string());
            continue;
        };
        let rel_name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>");
        let Some(to_table) = obj.get("toTableName").and_then(|v| v.as_str()) else {
            errors.push(format!("relation {rel_name}: missing toTableName"));
            continue;
        };
        let Some(target) = by_name.get(to_table) else {
            errors.push(format!(
                "relation {rel_name}: target table {to_table:?} does not exist"
            ));
            continue;
        };

        if let Some(related_col) = obj.get("relatedColumnName").and_then(|v| v.as_str()) {
            let target_cols = collect_column_map(target);
            if !target_cols.contains_key(related_col) {
                errors.push(format!(
                    "relation {rel_name}: target table {to_table:?} has no column {related_col:?}"
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "relation preflight failed (no changes applied):\n  - {}",
            errors.join("\n  - ")
        ))
    }
}

fn build_plan(
    desired: &TableInfo,
    current: Option<&TableInfo>,
    flags: &RestoreFlags,
) -> RestorePlan {
    let mut plan = RestorePlan::default();
    let desired_cols = collect_column_map(desired);

    let current_cols = match current {
        None => {
            plan.ops.push(DiffOp::CreateTable);
            BTreeMap::new()
        }
        Some(table) => collect_column_map(table),
    };

    for (name, desired_col) in &desired_cols {
        if is_protected(desired_col) {
            continue;
        }
        match current_cols.get(name) {
            None => {
                let mut column = desired_col.clone();
                force_required_false(&mut column);
                plan.ops.push(DiffOp::Add {
                    name: name.clone(),
                    column,
                });
            }
            Some(current_col) => {
                if columns_need_update(current_col, desired_col, flags.apply_required_flag) {
                    let mut column = desired_col.clone();
                    if !flags.apply_required_flag {
                        if let Some(req) = current_col.get("required").cloned() {
                            if let Some(obj) = column.as_object_mut() {
                                obj.insert("required".to_string(), req);
                            }
                        }
                    }
                    plan.ops.push(DiffOp::Update {
                        name: name.clone(),
                        current: current_col.clone(),
                        desired: column,
                    });
                }
            }
        }
    }

    if flags.delete_obsolete_columns {
        for (name, current_col) in &current_cols {
            if is_protected(current_col) {
                continue;
            }
            if !desired_cols.contains_key(name) {
                plan.ops.push(DiffOp::Delete {
                    name: name.clone(),
                    column: current_col.clone(),
                });
            }
        }
    }

    plan
}

fn print_plan(plan: &RestorePlan, dry_run: bool) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    if plan.ops.is_empty() {
        writeln!(stderr, "plan: no changes").map_err(|e| e.to_string())?;
        return Ok(());
    }
    let prefix = if dry_run { "dry-run plan" } else { "plan" };
    writeln!(stderr, "{prefix}:").map_err(|e| e.to_string())?;
    for (i, op) in plan.ops.iter().enumerate() {
        writeln!(stderr, "  {}. {}", i + 1, op.describe()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn apply_plan(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
    plan: &RestorePlan,
) -> Result<(), String> {
    let mut stderr = io::stderr();
    for op in &plan.ops {
        writeln!(stderr, "applying: {}", op.describe()).map_err(|e| e.to_string())?;
        match op {
            DiffOp::CreateTable => {
                client
                    .create_table(app_id, table_name)
                    .await
                    .map_err(format_sdk_error)?;
            }
            DiffOp::Add { column, .. } => {
                let body = mutation_body(column);
                client
                    .create_column(app_id, table_name, &body)
                    .await
                    .map_err(format_sdk_error)?;
            }
            DiffOp::Update {
                name,
                current,
                desired,
            } => {
                let body = mutation_body(desired);
                client
                    .update_column(
                        app_id,
                        table_name,
                        column_data_type(current),
                        name,
                        &body,
                    )
                    .await
                    .map_err(format_sdk_error)?;
            }
            DiffOp::Delete { name, column } => {
                client
                    .delete_column(app_id, table_name, name, column_data_type(column))
                    .await
                    .map_err(format_sdk_error)?;
            }
        }
    }
    Ok(())
}

fn collect_column_map(table: &TableInfo) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();

    for col in &table.columns {
        if let Ok(value) = serde_json::to_value(col) {
            map.insert(col.name.clone(), value);
        }
    }
    for rel in table.relations.iter().chain(table.geo_relations.iter()) {
        if let Some(name) = rel.get("name").and_then(|v| v.as_str()) {
            map.insert(name.to_string(), rel.clone());
        }
    }
    map
}

fn is_protected(column: &Value) -> bool {
    let name = column.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if SYSTEM_COLUMNS.contains(&name) {
        return true;
    }
    column
        .get("readOnly")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn column_data_type(column: &Value) -> Option<&str> {
    column.get("dataType").and_then(|v| v.as_str())
}

fn force_required_false(column: &mut Value) {
    if let Some(obj) = column.as_object_mut() {
        obj.insert("required".to_string(), Value::Bool(false));
    }
}

fn columns_need_update(current: &Value, desired: &Value, apply_required: bool) -> bool {
    let mut keys: BTreeSet<&str> = COMPARE_KEYS.iter().copied().collect();
    if apply_required {
        keys.insert("required");
    }
    for key in keys {
        if normalize_cmp(current.get(key)) != normalize_cmp(desired.get(key)) {
            return true;
        }
    }
    false
}

fn normalize_cmp(value: Option<&Value>) -> Option<Value> {
    match value {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.clone()),
    }
}

fn needs_explicit_data_size(data_type: Option<&str>) -> bool {
    matches!(
        data_type,
        Some(data_types::STRING)
            | Some(data_types::STRING_ID)
            | Some(data_types::EXTENDED_STRING)
            | Some(data_types::TEXT)
            | Some(data_types::LARGE_TEXT)
    )
}

fn mutation_body(column: &Value) -> Value {
    let mut body = column.clone();
    if let Some(obj) = body.as_object_mut() {
        for key in [
            "columnId",
            "fromTableId",
            "fromTableName",
            "toTableId",
            "isPrimaryKey",
            "readOnly",
            "identity",
            // Deprecated: Console returns API 1316 "Auto size is deprecated" if present.
            "dataSizeAuto",
        ] {
            obj.remove(key);
        }
        // Always send an explicit size for string-like columns (auto-size dumps).
        if needs_explicit_data_size(column_data_type(column)) {
            let missing_or_null = match obj.get("dataSize") {
                None | Some(Value::Null) => true,
                _ => false,
            };
            if missing_or_null {
                obj.insert(
                    "dataSize".to_string(),
                    Value::Number(DEFAULT_STRING_DATA_SIZE.into()),
                );
            }
        }
        if column_data_type(column) == Some(data_types::DATA_REF)
            || column_data_type(column) == Some(data_types::GEO_REF)
        {
            if !obj.contains_key("dataType") {
                obj.insert(
                    "dataType".to_string(),
                    Value::String(data_types::DATA_REF.to_string()),
                );
            }
        }
    }
    body
}

fn format_sdk_error(err: SdkError) -> String {
    err.to_string()
}

async fn restore_table_data(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
    bulk: bool,
) -> Result<(), String> {
    if bulk && table_name == "Users" {
        return Err(
            "restore_table_data --bulk is not supported for Users (API 3057); omit --bulk"
                .into(),
        );
    }

    let mut csv_buf = String::new();
    io::stdin()
        .read_to_string(&mut csv_buf)
        .map_err(|e| format!("failed to read stdin: {e}"))?;
    if csv_buf.trim().is_empty() {
        eprintln!("skipped: empty CSV");
        return Ok(());
    }

    let schema = client
        .get_table(app_id, table_name)
        .await
        .map_err(format_sdk_error)?;
    let type_map = schema_type_map(&schema);

    let mut reader = csv::Reader::from_reader(csv_buf.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| format!("failed to read CSV header: {e}"))?
        .iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if headers.is_empty() || headers.iter().all(|h| h.trim().is_empty()) {
        eprintln!("skipped: empty CSV");
        return Ok(());
    }

    let mut records = Vec::new();
    let mut warned: BTreeSet<String> = BTreeSet::new();
    for (row_idx, result) in reader.records().enumerate() {
        let row = result.map_err(|e| format!("CSV row {}: {e}", row_idx + 2))?;
        let record = csv_row_to_record(&headers, &row, &type_map, &mut warned)?;
        records.push(record);
    }
    if records.is_empty() {
        eprintln!("skipped: no data rows");
        return Ok(());
    }

    for name in &warned {
        eprintln!("warning: skipping unknown CSV column {name:?}");
    }

    let total = records.len();
    if bulk {
        return restore_table_data_bulk(client, app_id, table_name, &records, total).await;
    }
    restore_table_data_per_row(client, app_id, table_name, &records, total).await
}

async fn restore_table_data_bulk(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
    records: &[Value],
    total: usize,
) -> Result<(), String> {
    let mut prepared = Vec::with_capacity(records.len());
    for record in records {
        let mut body = record.clone();
        if let Some(obj) = body.as_object_mut() {
            obj.remove("updated");
        }
        prepared.push(body);
    }

    let mut done = 0usize;
    for chunk in chunk_records(&prepared, UPSERT_BATCH_SIZE) {
        client
            .bulk_upsert_records(app_id, table_name, chunk)
            .await
            .map_err(format_sdk_error)?;
        done += chunk.len();
        eprintln!("upserted {done}/{total}");
    }
    Ok(())
}

async fn restore_table_data_per_row(
    client: &ConsoleClient,
    app_id: &str,
    table_name: &str,
    records: &[Value],
    total: usize,
) -> Result<(), String> {
    for (idx, record) in records.iter().enumerate() {
        let object_id = record
            .get("objectId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("CSV row {}: missing objectId", idx + 2))?;
        let exists = client
            .record_exists(app_id, table_name, object_id)
            .await
            .map_err(format_sdk_error)?;
        let mut body = record.clone();
        // Reserved system props — strip for both create and update (discover one-by-one).
        if let Some(obj) = body.as_object_mut() {
            obj.remove("updated");
        }
        if exists {
            client
                .put_record(app_id, table_name, object_id, &body)
                .await
                .map_err(format_sdk_error)?;
        } else {
            if table_name == "Users" {
                if let Some(obj) = body.as_object_mut() {
                    let needs_password = match obj.get("password") {
                        None => true,
                        Some(Value::Null) => true,
                        Some(Value::String(s)) => s.is_empty(),
                        Some(_) => false,
                    };
                    if needs_password {
                        obj.insert(
                            "password".into(),
                            Value::String("RestorePass123!".into()),
                        );
                    }
                }
            }
            client
                .create_record(app_id, table_name, &body)
                .await
                .map_err(format_sdk_error)?;
        }
        let done = idx + 1;
        let op = if exists { "updated" } else { "created" };
        eprintln!("{op} {done}/{total}");
    }
    Ok(())
}

fn schema_type_map(table: &TableInfo) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for col in &table.columns {
        if let Some(dt) = col.data_type.as_ref() {
            map.insert(col.name.clone(), dt.clone());
        }
    }
    for rel in table.relations.iter().chain(table.geo_relations.iter()) {
        if let Some(name) = rel.get("name").and_then(|v| v.as_str()) {
            let dt = rel
                .get("dataType")
                .and_then(|v| v.as_str())
                .unwrap_or(data_types::DATA_REF);
            map.insert(name.to_string(), dt.to_string());
        }
    }
    // Always allow objectId / created / updated / ownerId as passthrough.
    map.entry("objectId".into())
        .or_insert_with(|| data_types::STRING_ID.to_string());
    map.entry("created".into())
        .or_insert_with(|| data_types::DATETIME.to_string());
    map.entry("updated".into())
        .or_insert_with(|| data_types::DATETIME.to_string());
    map.entry("ownerId".into())
        .or_insert_with(|| data_types::STRING.to_string());
    map
}

fn should_skip_csv_column(name: &str) -> bool {
    name == "___class" || name.starts_with("__related__")
}

fn csv_row_to_record(
    headers: &[String],
    row: &csv::StringRecord,
    type_map: &BTreeMap<String, String>,
    warned: &mut BTreeSet<String>,
) -> Result<Value, String> {
    let mut obj = serde_json::Map::new();
    for (i, header) in headers.iter().enumerate() {
        if should_skip_csv_column(header) {
            continue;
        }
        let raw = row.get(i).unwrap_or("").trim();
        if raw.is_empty() {
            continue;
        }
        let Some(data_type) = type_map.get(header) else {
            warned.insert(header.clone());
            continue;
        };
        let value = coerce_csv_value(raw, data_type)
            .map_err(|e| format!("column {header:?}: {e}"))?;
        obj.insert(header.clone(), value);
    }
    Ok(Value::Object(obj))
}

fn coerce_csv_value(raw: &str, data_type: &str) -> Result<Value, String> {
    match data_type {
        data_types::BOOLEAN => match raw.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            other => Err(format!("invalid boolean {other:?}")),
        },
        data_types::INT => raw
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .map_err(|_| format!("invalid int {raw:?}")),
        data_types::DOUBLE => {
            let n: f64 = raw
                .parse()
                .map_err(|_| format!("invalid double {raw:?}"))?;
            serde_json::Number::from_f64(n)
                .map(Value::Number)
                .ok_or_else(|| format!("invalid double {raw:?}"))
        }
        data_types::DATETIME => {
            if raw.chars().all(|c| c.is_ascii_digit()) {
                let n: i64 = raw
                    .parse()
                    .map_err(|_| format!("invalid datetime epoch {raw:?}"))?;
                Ok(Value::Number(n.into()))
            } else {
                Ok(Value::String(raw.to_string()))
            }
        }
        data_types::DATA_REF | data_types::GEO_REF => {
            let trimmed = raw.trim();
            if trimmed.starts_with('[') {
                let parsed: Value = serde_json::from_str(trimmed)
                    .map_err(|e| format!("invalid relation JSON array: {e}"))?;
                if !parsed.is_array() {
                    return Err("relation value must be a JSON array of ids".into());
                }
                Ok(parsed)
            } else {
                Ok(Value::String(trimmed.to_string()))
            }
        }
        _ => Ok(Value::String(raw.to_string())),
    }
}

fn chunk_records(records: &[Value], batch_size: usize) -> Vec<&[Value]> {
    let batch_size = batch_size.max(1);
    records.chunks(batch_size).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table(name: &str, columns: Vec<Value>, relations: Vec<Value>) -> TableInfo {
        TableInfo {
            name: name.into(),
            table_id: None,
            columns: columns
                .into_iter()
                .map(|c| serde_json::from_value(c).unwrap())
                .collect(),
            relations,
            parent_relations: vec![],
            geo_relations: vec![],
            view: Some(false),
            data_connector: Some(Value::Bool(false)),
            system: Some(false),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn sort_value_keys_orders_nested_objects() {
        let input = json!({
            "z": 1,
            "a": { "c": 2, "b": 3 },
            "m": [{ "y": 1, "x": 2 }]
        });
        let sorted = sort_value_keys(input);
        let keys: Vec<&str> = sorted
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["a", "m", "z"]);
        let nested: Vec<&str> = sorted["a"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(nested, vec!["b", "c"]);
        let item: Vec<&str> = sorted["m"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(item, vec!["x", "y"]);
    }

    #[test]
    fn flags_parse() {
        let args = vec![
            "MyTable".into(),
            "--dry-run".into(),
            "--apply-required-flag".into(),
            "--delete-obsolete-columns".into(),
        ];
        let (name, flags) = RestoreFlags::parse(&args).unwrap();
        assert_eq!(name, "MyTable");
        assert!(flags.dry_run);
        assert!(flags.apply_required_flag);
        assert!(flags.delete_obsolete_columns);
    }

    #[test]
    fn restore_table_data_args_default_is_not_bulk() {
        let (table, bulk) = parse_restore_table_data_args(&["MyTable".into()]).unwrap();
        assert_eq!(table, "MyTable");
        assert!(!bulk);
    }

    #[test]
    fn restore_table_data_args_bulk_before_table() {
        let (table, bulk) =
            parse_restore_table_data_args(&["--bulk".into(), "MyTable".into()]).unwrap();
        assert_eq!(table, "MyTable");
        assert!(bulk);
    }

    #[test]
    fn restore_table_data_args_bulk_after_table() {
        let (table, bulk) =
            parse_restore_table_data_args(&["MyTable".into(), "--bulk".into()]).unwrap();
        assert_eq!(table, "MyTable");
        assert!(bulk);
    }

    #[test]
    fn restore_table_data_args_rejects_extra() {
        let err =
            parse_restore_table_data_args(&["A".into(), "B".into()]).unwrap_err();
        assert!(err.contains("unexpected argument"));
    }

    #[test]
    fn build_plan_adds_with_required_false() {
        let desired = table(
            "T",
            vec![json!({"name":"email","dataType":"STRING","required":true})],
            vec![],
        );
        let current = table("T", vec![], vec![]);
        let plan = build_plan(&desired, Some(&current), &RestoreFlags::default());
        assert_eq!(plan.ops.len(), 1);
        match &plan.ops[0] {
            DiffOp::Add { name, column } => {
                assert_eq!(name, "email");
                assert_eq!(column["required"], false);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn build_plan_skips_system_and_readonly() {
        let desired = table(
            "Users",
            vec![
                json!({"name":"objectId","dataType":"STRING_ID"}),
                json!({"name":"password","dataType":"STRING","readOnly":true}),
                json!({"name":"nick","dataType":"STRING","required":false}),
            ],
            vec![],
        );
        let current = table(
            "Users",
            vec![
                json!({"name":"objectId","dataType":"STRING_ID"}),
                json!({"name":"password","dataType":"STRING","readOnly":true}),
                json!({"name":"obsolete","dataType":"STRING","readOnly":false}),
            ],
            vec![],
        );
        let mut flags = RestoreFlags::default();
        flags.delete_obsolete_columns = true;
        let plan = build_plan(&desired, Some(&current), &flags);
        let describes: Vec<_> = plan.ops.iter().map(DiffOp::describe).collect();
        assert!(describes.iter().any(|d| d.contains("ADD COLUMN nick")));
        assert!(describes
            .iter()
            .any(|d| d.contains("DELETE COLUMN obsolete")));
        assert!(!describes.iter().any(|d| d.contains("objectId")));
        assert!(!describes.iter().any(|d| d.contains("password")));
    }

    #[test]
    fn build_plan_create_table_when_missing() {
        let desired = table(
            "T",
            vec![json!({"name":"a","dataType":"INT","required":false})],
            vec![],
        );
        let plan = build_plan(&desired, None, &RestoreFlags::default());
        assert!(matches!(plan.ops[0], DiffOp::CreateTable));
        assert!(matches!(plan.ops[1], DiffOp::Add { .. }));
    }

    #[test]
    fn preflight_fails_on_missing_target() {
        let desired = table(
            "T",
            vec![],
            vec![json!({
                "name": "sessions",
                "dataType": "DATA_REF",
                "toTableName": "Sessions"
            })],
        );
        let err = preflight_relations(&desired, &[]).unwrap_err();
        assert!(err.contains("Sessions"));
    }

    fn rel(to_table: &str) -> Value {
        json!({
            "name": "r",
            "dataType": "DATA_REF",
            "toTableName": to_table
        })
    }

    #[test]
    fn dependency_order_chain() {
        let tables = vec![
            table("Card", vec![], vec![rel("Users")]),
            table("Users", vec![], vec![rel("Sessions")]),
            table("Sessions", vec![], vec![]),
        ];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert!(cycles.is_empty());
        assert_eq!(ordered, vec!["Sessions", "Users", "Card"]);
    }

    #[test]
    fn dependency_order_no_relations_is_alphabetical() {
        let tables = vec![
            table("Zebra", vec![], vec![]),
            table("Alpha", vec![], vec![]),
            table("Middle", vec![], vec![]),
        ];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert!(cycles.is_empty());
        assert_eq!(ordered, vec!["Alpha", "Middle", "Zebra"]);
    }

    #[test]
    fn dependency_order_reports_cycle_without_failing() {
        let tables = vec![
            table("A", vec![], vec![rel("B")]),
            table("B", vec![], vec![rel("A")]),
            table("C", vec![], vec![]),
        ];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert_eq!(format_dependency_cycles(&cycles), "dependency cycle:\nA -> B -> A");
        assert_eq!(ordered, vec!["C", "A", "B"]);
    }

    #[test]
    fn dependency_order_reports_self_relation_without_failing() {
        let tables = vec![table("Tree", vec![], vec![rel("Tree")])];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert_eq!(
            format_dependency_cycles(&cycles),
            "dependency cycle:\nTree -> Tree"
        );
        assert_eq!(ordered, vec!["Tree"]);
    }

    #[test]
    fn dependency_order_reports_two_disjoint_cycles() {
        let tables = vec![
            table("A", vec![], vec![rel("B")]),
            table("B", vec![], vec![rel("A")]),
            table("X", vec![], vec![rel("Y")]),
            table("Y", vec![], vec![rel("X")]),
        ];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert_eq!(
            format_dependency_cycles(&cycles),
            "dependency cycle:\nA -> B -> A\nX -> Y -> X"
        );
        assert_eq!(ordered, vec!["A", "B", "X", "Y"]);
    }

    #[test]
    fn dependency_order_cycle_omits_tail_from_cycle_paths() {
        let tables = vec![
            table("A", vec![], vec![rel("B")]),
            table("B", vec![], vec![rel("A")]),
            table("D", vec![], vec![rel("A")]),
        ];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert_eq!(format_dependency_cycles(&cycles), "dependency cycle:\nA -> B -> A");
        assert_eq!(ordered, vec!["A", "B", "D"]);
    }

    #[test]
    fn dependency_order_skips_unknown_target() {
        let tables = vec![
            table("Users", vec![], vec![rel("MissingExternal")]),
            table("Alpha", vec![], vec![]),
        ];
        let (ordered, cycles) = tables_dependency_order(&tables);
        assert!(cycles.is_empty());
        assert_eq!(ordered, vec!["Alpha", "Users"]);
    }

    #[test]
    fn dependency_closure_follows_chain() {
        let tables = vec![
            table("Card", vec![], vec![rel("Users")]),
            table("Users", vec![], vec![rel("Sessions")]),
            table("Sessions", vec![], vec![]),
            table("Unrelated", vec![], vec![]),
        ];
        let closure = tables_dependency_closure(&tables, &["Card".into()]).unwrap();
        let mut names: Vec<_> = closure.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Card", "Sessions", "Users"]);
    }

    #[test]
    fn dependency_closure_multi_seed() {
        let tables = vec![
            table("Card", vec![], vec![rel("Users")]),
            table("Users", vec![], vec![]),
            table("Invoice", vec![], vec![rel("Merchant")]),
            table("Merchant", vec![], vec![]),
            table("Unrelated", vec![], vec![]),
        ];
        let closure =
            tables_dependency_closure(&tables, &["Card".into(), "Invoice".into()]).unwrap();
        let mut names: Vec<_> = closure.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Card", "Invoice", "Merchant", "Users"]);
    }

    #[test]
    fn dependency_closure_unknown_seed_fails() {
        let tables = vec![table("Users", vec![], vec![])];
        let err = tables_dependency_closure(&tables, &["Missing".into()]).unwrap_err();
        assert!(err.contains("unknown table: Missing"));
    }

    #[test]
    fn dependency_closure_then_order() {
        let tables = vec![
            table("Card", vec![], vec![rel("Users")]),
            table("Users", vec![], vec![rel("Sessions")]),
            table("Sessions", vec![], vec![]),
            table("Unrelated", vec![], vec![]),
        ];
        let closure = tables_dependency_closure(&tables, &["Card".into()]).unwrap();
        let (ordered, cycles) = tables_dependency_order(&closure);
        assert!(cycles.is_empty());
        assert_eq!(ordered, vec!["Sessions", "Users", "Card"]);
    }

    #[test]
    fn update_ignores_required_without_flag() {
        let desired = table(
            "T",
            vec![json!({"name":"a","dataType":"STRING","required":true,"unique":false})],
            vec![],
        );
        let current = table(
            "T",
            vec![json!({"name":"a","dataType":"STRING","required":false,"unique":false})],
            vec![],
        );
        let plan = build_plan(&desired, Some(&current), &RestoreFlags::default());
        assert!(plan.ops.is_empty());

        let mut flags = RestoreFlags::default();
        flags.apply_required_flag = true;
        let plan = build_plan(&desired, Some(&current), &flags);
        assert_eq!(plan.ops.len(), 1);
        assert!(matches!(plan.ops[0], DiffOp::Update { .. }));
    }

    #[test]
    fn update_ignores_deprecated_data_size_auto() {
        let desired = table(
            "T",
            vec![json!({
                "name":"arabicName",
                "dataType":"STRING",
                "dataSize":168,
                "dataSizeAuto":true,
                "unique":false,
                "indexed":false
            })],
            vec![],
        );
        let current = table(
            "T",
            vec![json!({
                "name":"arabicName",
                "dataType":"STRING",
                "dataSize":168,
                "dataSizeAuto":false,
                "unique":false,
                "indexed":false
            })],
            vec![],
        );
        let plan = build_plan(&desired, Some(&current), &RestoreFlags::default());
        assert!(plan.ops.is_empty());
    }

    #[test]
    fn mutation_body_strips_data_size_auto_keeps_explicit_size() {
        let body = mutation_body(&json!({
            "name":"arabicName",
            "dataType":"STRING",
            "dataSize":168,
            "dataSizeAuto":true,
            "columnId":"deadbeef"
        }));
        assert!(body.get("dataSizeAuto").is_none());
        assert!(body.get("columnId").is_none());
        assert_eq!(body["dataSize"], 168);
    }

    #[test]
    fn mutation_body_defaults_string_size_when_missing() {
        let body = mutation_body(&json!({
            "name":"title",
            "dataType":"STRING",
            "dataSize":null,
            "dataSizeAuto":true
        }));
        assert!(body.get("dataSizeAuto").is_none());
        assert_eq!(body["dataSize"], 250);
    }

    #[test]
    fn csv_row_coerces_and_skips_related() {
        let type_map = BTreeMap::from([
            ("objectId".into(), data_types::STRING_ID.to_string()),
            ("enabled".into(), data_types::BOOLEAN.to_string()),
            ("age".into(), data_types::INT.to_string()),
            ("created".into(), data_types::DATETIME.to_string()),
            ("sessions".into(), data_types::DATA_REF.to_string()),
            ("name".into(), data_types::STRING.to_string()),
        ]);
        let headers = vec![
            "objectId".into(),
            "___class".into(),
            "__related__Bill.user".into(),
            "enabled".into(),
            "age".into(),
            "created".into(),
            "sessions".into(),
            "name".into(),
            "unknownCol".into(),
            "empty".into(),
        ];
        let row = csv::StringRecord::from(vec![
            "ID-1",
            "Users",
            r#"{"objects":0,"table":"Bill"}"#,
            "true",
            "42",
            "1661886455692",
            "REL-1",
            "Alice",
            "x",
            "",
        ]);
        let mut warned = BTreeSet::new();
        let value = csv_row_to_record(&headers, &row, &type_map, &mut warned).unwrap();
        assert_eq!(value["objectId"], "ID-1");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["age"], 42);
        assert_eq!(value["created"], 1661886455692_i64);
        assert_eq!(value["sessions"], "REL-1");
        assert_eq!(value["name"], "Alice");
        assert!(value.get("___class").is_none());
        assert!(value.get("__related__Bill.user").is_none());
        assert!(value.get("empty").is_none());
        assert!(warned.contains("unknownCol"));
    }

    #[test]
    fn chunk_records_batches_by_fifty() {
        let records: Vec<Value> = (0..120).map(|i| json!({ "n": i })).collect();
        let chunks = chunk_records(&records, UPSERT_BATCH_SIZE);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 50);
        assert_eq!(chunks[1].len(), 50);
        assert_eq!(chunks[2].len(), 20);
    }
}
