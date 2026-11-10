//! CLI for Backendless Console data inspection / schema restore.
//!
//! ```text
//! backendless-toolkit-db show_tables
//! backendless-toolkit-db dump_table_structure <table>
//! backendless-toolkit-db dump_table_data <table>
//! cat table.yaml | backendless-toolkit-db restore_table_structure <table> [--dry-run] ...
//! ```

use backendless_console_sdk::{ConsoleClient, DEFAULT_PAGE_SIZE, Error as SdkError, TableInfo, data_types};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::io::{self, Read, Write};
use std::process::ExitCode;

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
    "dataSizeAuto",
    "customRegex",
    "expression",
    "toTableName",
    "relationshipType",
    "relatedColumnName",
    "autoLoad",
];

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
     backendless-toolkit-db show_tables\n  \
     backendless-toolkit-db dump_table_structure <table>\n  \
     backendless-toolkit-db dump_table_data <table>\n  \
     cat <table>.yaml | backendless-toolkit-db restore_table_structure <table> \\\n    \
     [--apply-required-flag] [--delete-obsolete-columns] [--dry-run]\n"
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
        "show_tables" => {
            if !args.is_empty() {
                return Err(usage());
            }
            show_tables(&client, &config.app_id).await
        }
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
        other => Err(format!("unknown command: {other}\n{}", usage())),
    }
}

async fn show_tables(client: &ConsoleClient, app_id: &str) -> Result<(), String> {
    let tables = client
        .list_tables(app_id)
        .await
        .map_err(format_sdk_error)?;
    let mut stdout = io::stdout().lock();
    for table in tables {
        writeln!(stdout, "{}", table.name).map_err(|e| e.to_string())?;
    }
    Ok(())
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
        ] {
            obj.remove(key);
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
}
