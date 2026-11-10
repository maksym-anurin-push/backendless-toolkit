use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::client::ConsoleClient;
use crate::error::{Error, Result};

/// Column metadata from Console `GET /{appId}/console/data/tables`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    #[serde(default)]
    pub data_type: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub unique: Option<bool>,
    #[serde(default)]
    pub indexed: Option<bool>,
    #[serde(default)]
    pub default_value: Option<Value>,
    #[serde(default)]
    pub is_primary_key: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Table metadata including schema (columns / relations).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub name: String,
    #[serde(default)]
    pub table_id: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_util::null_as_default")]
    pub columns: Vec<ColumnInfo>,
    #[serde(default, deserialize_with = "crate::serde_util::null_as_default")]
    pub relations: Vec<Value>,
    #[serde(default, deserialize_with = "crate::serde_util::null_as_default")]
    pub parent_relations: Vec<Value>,
    #[serde(default, deserialize_with = "crate::serde_util::null_as_default")]
    pub geo_relations: Vec<Value>,
    #[serde(default)]
    pub view: Option<bool>,
    #[serde(default)]
    pub data_connector: Option<Value>,
    #[serde(default)]
    pub system: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct TablesResponse {
    #[serde(default, deserialize_with = "crate::serde_util::null_as_default")]
    tables: Vec<TableInfo>,
}

/// Query parameters for Console find (`POST .../data/{table}/find`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindQuery {
    pub page_size: u32,
    pub offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<String>,
}

impl FindQuery {
    pub fn page(page_size: u32, offset: u32) -> Self {
        Self {
            page_size,
            offset,
            where_clause: None,
        }
    }
}

/// Body shape expected by Console find endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FindBody {
    page_size: u32,
    offset: u32,
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    where_clause: Option<String>,
}

impl From<&FindQuery> for FindBody {
    fn from(q: &FindQuery) -> Self {
        Self {
            page_size: q.page_size,
            offset: q.offset,
            where_clause: q.where_clause.clone(),
        }
    }
}

pub const DEFAULT_PAGE_SIZE: u32 = 100;

/// Mirrors `DataTypes` from backendless-console-sdk.
pub mod data_types {
    pub const BOOLEAN: &str = "BOOLEAN";
    pub const DATA_REF: &str = "DATA_REF";
    pub const GEO_REF: &str = "GEO_REF";
}

/// Apply Console SDK `normalizeTable` semantics (and boolean default coercion).
pub fn normalize_table(mut table: TableInfo) -> TableInfo {
    table.relations = table
        .relations
        .into_iter()
        .map(normalize_data_relation)
        .collect();
    table.geo_relations = table
        .geo_relations
        .into_iter()
        .map(normalize_geo_relation)
        .collect();
    for column in &mut table.columns {
        normalize_column_default_value(column);
    }
    table
}

fn normalize_data_relation(mut relation: Value) -> Value {
    if let Some(obj) = relation.as_object_mut() {
        obj.insert(
            "dataType".to_string(),
            Value::String(data_types::DATA_REF.to_string()),
        );
    }
    relation
}

fn normalize_geo_relation(mut relation: Value) -> Value {
    if let Some(obj) = relation.as_object_mut() {
        obj.insert(
            "dataType".to_string(),
            Value::String(data_types::GEO_REF.to_string()),
        );
    }
    relation
}

/// Console often stores BOOLEAN defaults as strings `"true"` / `"false"`.
fn normalize_column_default_value(column: &mut ColumnInfo) {
    let is_boolean = column
        .data_type
        .as_deref()
        .is_some_and(|t| t == data_types::BOOLEAN);
    if !is_boolean {
        return;
    }

    if let Some(Value::String(s)) = column.default_value.as_ref() {
        match s.as_str() {
            "true" => column.default_value = Some(Value::Bool(true)),
            "false" => column.default_value = Some(Value::Bool(false)),
            _ => {}
        }
    }
}

impl ConsoleClient {
    fn tables_path(app_id: &str) -> String {
        format!("{app_id}/console/data/tables")
    }

    fn find_path(app_id: &str, table_name: &str) -> String {
        // Console SDK: `/{appId}/console/data/{tableName}/find`
        format!(
            "{app_id}/console/data/{}/find",
            urlencoding_path_segment(table_name)
        )
    }

    /// List all data tables (with column schema) for an application.
    pub async fn list_tables(&self, app_id: &str) -> Result<Vec<TableInfo>> {
        let path = Self::tables_path(app_id);
        // Some Console deployments return `{ tables: [...] }`, others a bare array.
        let value: Value = self
            .request_json(Method::GET, &path, None::<&()>, true)
            .await?;

        let tables: Vec<TableInfo> = if let Some(arr) = value.as_array() {
            serde_json::from_value(Value::Array(arr.clone()))?
        } else {
            let wrapped: TablesResponse = serde_json::from_value(value)?;
            wrapped.tables
        };

        Ok(tables.into_iter().map(normalize_table).collect())
    }

    /// Find a single table by name (case-sensitive).
    pub async fn get_table(&self, app_id: &str, table_name: &str) -> Result<TableInfo> {
        let tables = self.list_tables(app_id).await?;
        tables
            .into_iter()
            .find(|t| t.name == table_name)
            .ok_or_else(|| Error::TableNotFound(table_name.to_string()))
    }

    /// Fetch one page of records from a table.
    pub async fn find_records(
        &self,
        app_id: &str,
        table_name: &str,
        query: &FindQuery,
    ) -> Result<Vec<Value>> {
        let path = Self::find_path(app_id, table_name);
        let body = FindBody::from(query);
        let value: Value = self
            .request_json(Method::POST, &path, Some(&body), true)
            .await?;

        match value {
            Value::Array(rows) => Ok(rows),
            Value::Object(map) => {
                // Some responses wrap rows as `{ data: [...] }`.
                if let Some(Value::Array(rows)) = map.get("data") {
                    Ok(rows.clone())
                } else {
                    Ok(vec![Value::Object(map)])
                }
            }
            other => Ok(vec![other]),
        }
    }

    /// Fetch all records by paging until a short/empty page.
    pub async fn find_all_records(
        &self,
        app_id: &str,
        table_name: &str,
        page_size: u32,
    ) -> Result<Vec<Value>> {
        let page_size = page_size.max(1);
        let mut offset: u32 = 0;
        let mut all = Vec::new();

        loop {
            let page = self
                .find_records(app_id, table_name, &FindQuery::page(page_size, offset))
                .await?;
            let count = page.len() as u32;
            all.extend(page);
            if count < page_size {
                break;
            }
            offset = offset.saturating_add(count);
        }

        Ok(all)
    }
}

/// Encode a single path segment (table name) without turning `/` into path separators.
fn urlencoding_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(nibble(b >> 4));
                out.push(nibble(b & 0xf));
            }
        }
    }
    out
}

fn nibble(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'A' + (n - 10) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_body_uses_where_key() {
        let q = FindQuery {
            page_size: 50,
            offset: 10,
            where_clause: Some("age > 18".into()),
        };
        let body = FindBody::from(&q);
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["pageSize"], 50);
        assert_eq!(json["offset"], 10);
        assert_eq!(json["where"], "age > 18");
    }

    #[test]
    fn tables_path_format() {
        assert_eq!(
            ConsoleClient::tables_path("APP-ID"),
            "APP-ID/console/data/tables"
        );
    }

    #[test]
    fn find_path_encodes_table_name() {
        assert_eq!(
            ConsoleClient::find_path("APP", "My Table"),
            "APP/console/data/My%20Table/find"
        );
    }

    #[test]
    fn table_info_roundtrip_minimal() {
        let raw = r#"{
            "name": "Users",
            "tableId": "abc",
            "columns": [{"name": "objectId", "dataType": "STRING_ID"}],
            "system": true
        }"#;
        let table: TableInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(table.name, "Users");
        assert_eq!(table.table_id.as_deref(), Some("abc"));
        assert_eq!(table.columns.len(), 1);
        assert_eq!(table.columns[0].name, "objectId");
    }

    #[test]
    fn table_info_keeps_relations_and_flags() {
        let raw = r#"{
            "name": "Users",
            "tableId": "abc",
            "view": false,
            "dataConnector": false,
            "columns": [],
            "relations": [{"name": "roles"}],
            "parentRelations": [{"name": "owner"}],
            "geoRelations": [],
            "system": true
        }"#;
        let table: TableInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(table.view, Some(false));
        assert_eq!(table.data_connector, Some(Value::Bool(false)));
        assert_eq!(table.relations.len(), 1);
        assert_eq!(table.parent_relations.len(), 1);
        assert!(table.geo_relations.is_empty());

        let out = serde_json::to_value(&table).unwrap();
        assert_eq!(out["view"], false);
        assert_eq!(out["dataConnector"], false);
        assert!(out["relations"].as_array().unwrap().len() == 1);
        assert!(out["parentRelations"].as_array().unwrap().len() == 1);
        assert!(out["geoRelations"].as_array().unwrap().is_empty());
    }

    #[test]
    fn table_info_accepts_null_relation_arrays() {
        let raw = r#"{
            "name": "Users",
            "columns": null,
            "relations": null,
            "parentRelations": null,
            "geoRelations": null
        }"#;
        let table: TableInfo = serde_json::from_str(raw).unwrap();
        assert!(table.columns.is_empty());
        assert!(table.relations.is_empty());
        assert!(table.parent_relations.is_empty());
        assert!(table.geo_relations.is_empty());
    }

    #[test]
    fn normalize_table_adds_relation_data_types_and_bool_defaults() {
        let raw = r#"{
            "name": "Users",
            "columns": [
                {"name": "enabled", "dataType": "BOOLEAN", "defaultValue": "true"},
                {"name": "label", "dataType": "STRING", "defaultValue": "true"}
            ],
            "relations": [{"name": "sessions"}],
            "geoRelations": [{"name": "location"}]
        }"#;
        let table = normalize_table(serde_json::from_str(raw).unwrap());
        assert_eq!(table.relations[0]["dataType"], "DATA_REF");
        assert_eq!(table.geo_relations[0]["dataType"], "GEO_REF");
        assert_eq!(table.columns[0].default_value, Some(Value::Bool(true)));
        assert_eq!(
            table.columns[1].default_value,
            Some(Value::String("true".into()))
        );
    }
}
