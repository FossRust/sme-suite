use crate::graphql::{GraphqlData, RequestUser};
use async_graphql::SimpleObject;

#[derive(Clone, Debug, SimpleObject)]
pub struct MePayload {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub org: OrgPayload,
    pub roles: Vec<String>,
    pub entitlements: Vec<String>,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct OrgPayload {
    pub id: String,
    pub slug: String,
    pub name: String,
}

impl MePayload {
    pub fn from_requester(data: &GraphqlData, user: RequestUser) -> Self {
        Self {
            id: user.id.to_string(),
            email: user.email,
            name: user.name,
            org: OrgPayload {
                id: data.default_org_id.to_string(),
                slug: data.default_org_slug.clone(),
                name: data.default_org_name.clone(),
            },
            roles: user.roles,
            entitlements: Vec::new(),
        }
    }
}
