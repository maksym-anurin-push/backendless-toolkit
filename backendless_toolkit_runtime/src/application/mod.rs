pub mod ports;

pub use self::ports::*;

use std::sync::Arc;

/// Set of UseCases for RPC protocol handlers and composition root.
#[derive(Clone)]
pub struct UseCasesBundle {
    pub app_info: Arc<crate::domain::AppInfo>,
}
