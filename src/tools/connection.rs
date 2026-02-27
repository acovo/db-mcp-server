//! Dynamic connection management tools.
//!
//! This module provides MCP tools for adding, deleting, and updating
//! database connections at runtime without server restart.

use crate::db::{ConnectionManager, TransactionRegistry};
use crate::error::{DbError, DbResult};
use crate::models::{ConnectionConfig, DatabaseType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

/// Input for the add_connection tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = schemars::transform::RestrictFormats::default())]
pub struct AddConnectionInput {
    /// Database connection URL (e.g., 'sqlite:data.db', 'postgres://user:pass@host/db')
    pub url: String,
    /// Custom connection ID. Auto-generated if not provided.
    #[serde(default)]
    pub connection_id: Option<String>,
    /// Allow write operations (INSERT, UPDATE, DELETE, DDL). Default: false
    #[serde(default)]
    pub writable: bool,
}

/// Output for the add_connection tool.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(transform = schemars::transform::RestrictFormats::default())]
pub struct AddConnectionOutput {
    /// ID of the created connection
    pub connection_id: String,
    /// Type of database: "mysql", "postgres", or "sqlite"
    pub database_type: DatabaseType,
    /// Whether write operations are allowed
    pub writable: bool,
}

/// Input for the delete_connection tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = schemars::transform::RestrictFormats::default())]
pub struct DeleteConnectionInput {
    /// ID of the connection to delete
    pub connection_id: String,
}

/// Output for the delete_connection tool.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(transform = schemars::transform::RestrictFormats::default())]
pub struct DeleteConnectionOutput {
    /// Always true on success
    pub deleted: bool,
    /// ID of the deleted connection
    pub connection_id: String,
}

/// Input for the update_connection tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = schemars::transform::RestrictFormats::default())]
pub struct UpdateConnectionInput {
    /// ID of the connection to update
    pub connection_id: String,
    /// New connection URL (recreates pool if changed)
    #[serde(default)]
    pub url: Option<String>,
    /// New writable setting
    #[serde(default)]
    pub writable: Option<bool>,
}

/// Output for the update_connection tool.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(transform = schemars::transform::RestrictFormats::default())]
pub struct UpdateConnectionOutput {
    /// ID of the updated connection
    pub connection_id: String,
    /// Type of database
    pub database_type: DatabaseType,
    /// Whether write operations are allowed
    pub writable: bool,
    /// Whether the URL was changed (pool recreated)
    pub url_changed: bool,
}

/// Handler for connection management tools.
pub struct ConnectionToolHandler {
    connection_manager: Arc<ConnectionManager>,
    transaction_registry: Arc<TransactionRegistry>,
}

impl ConnectionToolHandler {
    /// Create a new connection tool handler.
    pub fn new(
        connection_manager: Arc<ConnectionManager>,
        transaction_registry: Arc<TransactionRegistry>,
    ) -> Self {
        Self {
            connection_manager,
            transaction_registry,
        }
    }

    /// Add a new database connection.
    pub async fn add_connection(&self, input: AddConnectionInput) -> DbResult<AddConnectionOutput> {
        let config = ConnectionConfig::from_url_with_options(
            &input.url,
            input.connection_id.as_deref(),
            input.writable,
        )?;

        let connection_id = config.id.clone();
        let db_type = config.db_type;

        let info = self.connection_manager.connect(config).await?;

        info!(
            connection_id = %connection_id,
            database_type = %db_type,
            writable = input.writable,
            "Connection added dynamically"
        );

        Ok(AddConnectionOutput {
            connection_id: info.connection_id,
            database_type: info.database_type,
            writable: info.writable,
        })
    }

    /// Delete an existing database connection.
    pub async fn delete_connection(
        &self,
        input: DeleteConnectionInput,
    ) -> DbResult<DeleteConnectionOutput> {
        let connection_id = input.connection_id.trim();
        if connection_id.is_empty() {
            return Err(DbError::invalid_input("connection_id is required"));
        }

        // Check for active transactions
        if self
            .transaction_registry
            .has_active_transactions(connection_id)
            .await
        {
            return Err(DbError::active_transactions(
                connection_id,
                "Complete transactions using commit or rollback first",
            ));
        }

        self.connection_manager
            .delete_connection(connection_id)
            .await?;

        info!(connection_id = %connection_id, "Connection deleted dynamically");

        Ok(DeleteConnectionOutput {
            deleted: true,
            connection_id: connection_id.to_string(),
        })
    }

    /// Update an existing database connection.
    pub async fn update_connection(
        &self,
        input: UpdateConnectionInput,
    ) -> DbResult<UpdateConnectionOutput> {
        let connection_id = input.connection_id.trim();
        if connection_id.is_empty() {
            return Err(DbError::invalid_input("connection_id is required"));
        }

        if input.url.is_none() && input.writable.is_none() {
            return Err(DbError::invalid_input(
                "No update parameters provided. Provide at least one of: url, writable",
            ));
        }

        // Check for active transactions
        if self
            .transaction_registry
            .has_active_transactions(connection_id)
            .await
        {
            return Err(DbError::active_transactions(
                connection_id,
                "Complete transactions using commit or rollback first",
            ));
        }

        let result = self
            .connection_manager
            .update_connection(connection_id, input.url.as_deref(), input.writable)
            .await?;

        info!(
            connection_id = %connection_id,
            url_changed = result.url_changed,
            writable = result.writable,
            "Connection updated dynamically"
        );

        Ok(UpdateConnectionOutput {
            connection_id: connection_id.to_string(),
            database_type: result.database_type,
            writable: result.writable,
            url_changed: result.url_changed,
        })
    }
}
