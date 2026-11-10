//! CLI for Backendless Console data inspection.
//!
//! ```text
//! backendless-toolkit-db show_tables
//! backendless-toolkit-db dump_table_structure <table>
//! backendless-toolkit-db dump_table_data <table>
//! ```

use backendless_console_sdk::{ConsoleClient, DEFAULT_PAGE_SIZE, Error as SdkError, TableInfo};
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

const ENV_URL: &str = "BACKENDLESS_TOOLKIT__CONSOLE__URL";
const ENV_APP_ID: &str = "BACKENDLESS_TOOLKIT__CONSOLE__APP_ID";
const ENV_LOGIN: &str = "BACKENDLESS_TOOLKIT__CONSOLE__LOGIN";
const ENV_PASSWORD: &str = "BACKENDLESS_TOOLKIT__CONSOLE__PASSWORD";

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

fn usage() -> String {
    "Usage:\n  \
     backendless-toolkit-db show_tables\n  \
     backendless-toolkit-db dump_table_structure <table>\n  \
     backendless-toolkit-db dump_table_data <table>\n"
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
    // Full Console table metadata with recursively alphabetized object keys
    // so successive dumps produce stable diffs.
    let value = serde_json::to_value(table).map_err(|e| format!("JSON serialize error: {e}"))?;
    let sorted = sort_value_keys(value);
    serde_yaml::to_string(&sorted).map_err(|e| format!("YAML serialize error: {e}"))
}

/// Recursively rewrite every JSON object with alphabetically ordered keys.
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

/// Stable header order: objectId, created, updated, then remaining keys alphabetical.
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

fn format_sdk_error(err: SdkError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
