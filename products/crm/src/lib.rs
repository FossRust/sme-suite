//! CRM module placeholder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Account {
    pub id: String,
    pub name: String,
}

#[derive(Default, Debug)]
pub struct CrmModule;

impl CrmModule {
    pub fn accounts(&self) -> Vec<Account> {
        vec![Account {
            id: "demo".into(),
            name: "Demo Account".into(),
        }]
    }
}
