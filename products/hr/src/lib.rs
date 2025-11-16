//! HR module placeholder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Employee {
    pub id: String,
    pub full_name: String,
}

#[derive(Default, Debug)]
pub struct HrModule;

impl HrModule {
    pub fn employees(&self) -> Vec<Employee> {
        vec![Employee {
            id: "hr-demo".into(),
            full_name: "Demo Employee".into(),
        }]
    }
}
