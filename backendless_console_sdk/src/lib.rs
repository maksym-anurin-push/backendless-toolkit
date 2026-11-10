//! Rust client for the Backendless **Console** (admin) HTTP API.
//!
//! Endpoints are taken from the official JS
//! [`backendless-console-sdk`](https://github.com/Backendless/Console-SDK).
//! The Console API is internal and may change without notice.

mod auth;
mod client;
mod error;
mod serde_util;
mod tables;

pub use auth::LoginResult;
pub use client::ConsoleClient;
pub use error::{Error, Result};
pub use tables::{
    ColumnInfo, DEFAULT_PAGE_SIZE, FindQuery, TableInfo, data_types, normalize_table,
};
