//! Authorization primitives for CRM + HR modules.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthzError {
    #[error("action {action} denied for resource {resource}")]
    Denied { action: String, resource: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PolicyContext {
    pub subject: String,
    pub action: String,
    pub resource: String,
}

#[derive(Default, Debug)]
pub struct PolicyEngine;

impl PolicyEngine {
    pub fn check(&self, ctx: &PolicyContext) -> Result<(), AuthzError> {
        if ctx.action.starts_with("read:") {
            Ok(())
        } else {
            Err(AuthzError::Denied {
                action: ctx.action.clone(),
                resource: ctx.resource.clone(),
            })
        }
    }
}
