use crate::auth::{issue_token, AuthConfig, CurrentUser, UserRole, SESSION_COOKIE};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, InputObject, Json, Object, Schema,
    SimpleObject, ID,
};
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use entity::{
    activity, company, contact, deal, deal_stage_history, stage_meta, task, user, user_identity,
    user_role, user_secret,
};
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::{Expr, Func, OnConflict, SimpleExpr};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, DbErr, EntityTrait, FromQueryResult, Order, QueryFilter, QueryOrder,
    QuerySelect, Select, Statement, TransactionTrait, Value,
};
use serde_json::json;
use tracing::info_span;
use uuid::Uuid;

pub struct AppSchema(pub Schema<QueryRoot, MutationRoot, EmptySubscription>);

pub fn build_schema(
    db: Arc<DatabaseConnection>,
    auth: Arc<AuthConfig>,
) -> AppSchema {
    let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(db)
        .data(auth)
        .finish();
    AppSchema(schema)
}

pub struct QueryRoot;
pub struct MutationRoot;

const MAX_TASKS_PAGE: i32 = 100;
const MAX_SEARCH_PAGE: i32 = 50;

#[derive(Enum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum CrmSearchKind {
    #[graphql(name = "COMPANY")]
    Company,
    #[graphql(name = "CONTACT")]
    Contact,
    #[graphql(name = "DEAL")]
    Deal,
}

impl CrmSearchKind {}

#[derive(Clone, Debug, SimpleObject)]
pub struct SearchHit {
    pub kind: CrmSearchKind,
    pub id: ID,
    pub title: String,
    pub subtitle: Option<String>,
    pub score: f64,
    pub href: Option<String>,
}

#[Object]
impl QueryRoot {
    async fn crm(&self) -> CrmQuery {
        CrmQuery
    }
}

#[Object]
impl MutationRoot {
    async fn crm(&self) -> CrmMutation {
        CrmMutation
    }
}

#[derive(Default)]
pub struct CrmQuery;

#[derive(Default)]
pub struct CrmMutation;

#[Object]
impl CrmQuery {
    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<MePayload> {
        let viewer = require_viewer(ctx)?;
        let db = database(ctx)?;
        let (model, roles) = load_user_with_roles(db.as_ref(), viewer.user_id).await?;
        let node = UserNode::from_model(model, roles.clone());
        Ok(MePayload {
            user: node,
            roles: roles.iter().map(|r| r.as_str().to_string()).collect(),
        })
    }

    async fn users(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        offset: Option<i32>,
        q: Option<String>,
    ) -> async_graphql::Result<Vec<UserNode>> {
        require_role(ctx, UserRole::Admin)?;
        let db = database(ctx)?;
        let limit = first.unwrap_or(50).clamp(1, 200) as u64;
        let skip = offset.unwrap_or(0).max(0) as u64;
        let mut query = user::Entity::find();
        if let Some(filter) = sanitize_optional_filter(q) {
            let pattern = format!("%{}%", filter);
            query = query.filter(
                Condition::any()
                    .add(user::Column::Email.ilike(pattern.clone()))
                    .add(user::Column::DisplayName.ilike(pattern)),
            );
        }
        let records = query
            .order_by_asc(user::Column::Email)
            .limit(limit)
            .offset(skip)
            .all(db.as_ref())
            .await
            .map_err(db_error)?;
        let role_map = load_roles_for_users(db.as_ref(), &records).await?;
        Ok(records
            .into_iter()
            .map(|model| {
                let roles = role_map.get(&model.id).cloned().unwrap_or_default();
                UserNode::from_model(model, roles)
            })
            .collect())
    }

    async fn search(
        &self,
        ctx: &Context<'_>,
        q: String,
        kinds: Option<Vec<CrmSearchKind>>,
        first: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<Vec<SearchHit>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let trimmed = validate_search_query(&q)?;
        let requested = first.unwrap_or(20);
        let limit = enforce_search_limit(requested, MAX_SEARCH_PAGE)?;
        let skip = offset.unwrap_or(0).max(0) as u64;
        let selected_kinds = kinds.unwrap_or_else(default_search_kinds);
        search_hits(db.as_ref(), trimmed, &selected_kinds, limit, skip).await
    }

    async fn suggest_companies(
        &self,
        ctx: &Context<'_>,
        q: String,
        first: Option<i32>,
    ) -> async_graphql::Result<Vec<CompanyNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let trimmed = validate_search_query(&q)?;
        let requested = first.unwrap_or(10);
        let limit = enforce_search_limit(requested, MAX_SEARCH_PAGE)?;
        let hits = search_hits(db.as_ref(), trimmed, &[CrmSearchKind::Company], limit, 0).await?;
        let ids: Vec<Uuid> = hits
            .iter()
            .filter_map(|hit| Uuid::parse_str(hit.id.as_str()).ok())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let records = company::Entity::find()
            .filter(company::Column::Id.is_in(ids.clone()))
            .all(db.as_ref())
            .await
            .map_err(db_error)?;
        Ok(order_by_ids(ids, records, |model| model.id)
            .into_iter()
            .map(CompanyNode::from)
            .collect())
    }

    async fn suggest_contacts(
        &self,
        ctx: &Context<'_>,
        q: String,
        first: Option<i32>,
    ) -> async_graphql::Result<Vec<ContactNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let trimmed = validate_search_query(&q)?;
        let requested = first.unwrap_or(10);
        let limit = enforce_search_limit(requested, MAX_SEARCH_PAGE)?;
        let hits = search_hits(db.as_ref(), trimmed, &[CrmSearchKind::Contact], limit, 0).await?;
        let ids: Vec<Uuid> = hits
            .iter()
            .filter_map(|hit| Uuid::parse_str(hit.id.as_str()).ok())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let records = contact::Entity::find()
            .filter(contact::Column::Id.is_in(ids.clone()))
            .all(db.as_ref())
            .await
            .map_err(db_error)?;
        Ok(order_by_ids(ids, records, |model| model.id)
            .into_iter()
            .map(ContactNode::from)
            .collect())
    }

    async fn suggest_deals(
        &self,
        ctx: &Context<'_>,
        q: String,
        first: Option<i32>,
    ) -> async_graphql::Result<Vec<DealNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let trimmed = validate_search_query(&q)?;
        let requested = first.unwrap_or(10);
        let limit = enforce_search_limit(requested, MAX_SEARCH_PAGE)?;
        let hits = search_hits(db.as_ref(), trimmed, &[CrmSearchKind::Deal], limit, 0).await?;
        let ids: Vec<Uuid> = hits
            .iter()
            .filter_map(|hit| Uuid::parse_str(hit.id.as_str()).ok())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let records = deal::Entity::find()
            .filter(deal::Column::Id.is_in(ids.clone()))
            .all(db.as_ref())
            .await
            .map_err(db_error)?;
        Ok(order_by_ids(ids, records, |model| model.id)
            .into_iter()
            .map(DealNode::from)
            .collect())
    }

    #[graphql(name = "dealStageHistory")]
    async fn deal_stage_history(
        &self,
        ctx: &Context<'_>,
        deal_id: ID,
        first: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<Vec<DealStageHistoryNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let deal_uuid = parse_uuid(&deal_id)?;
        let limit = first.unwrap_or(50).clamp(1, 200) as u64;
        let skip = offset.unwrap_or(0).max(0) as u64;

        let rows = deal_stage_history::Entity::find()
            .filter(deal_stage_history::Column::DealId.eq(deal_uuid))
            .order_by_desc(deal_stage_history::Column::ChangedAt)
            .limit(limit)
            .offset(skip)
            .all(db.as_ref())
            .await
            .map_err(db_error)?;

        Ok(rows.into_iter().map(DealStageHistoryNode::from).collect())
    }

    #[graphql(name = "dealActivities")]
    async fn deal_activities(
        &self,
        ctx: &Context<'_>,
        deal_id: ID,
        first: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<Vec<ActivityNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let deal_uuid = parse_uuid(&deal_id)?;
        let limit = first.unwrap_or(50).clamp(1, 200) as u64;
        let skip = offset.unwrap_or(0).max(0) as u64;

        let rows = activity::Entity::find()
            .filter(activity::Column::EntityType.eq("deal"))
            .filter(activity::Column::EntityId.eq(deal_uuid))
            .order_by_desc(activity::Column::CreatedAt)
            .limit(limit)
            .offset(skip)
            .all(db.as_ref())
            .await
            .map_err(db_error)?;

        Ok(rows.into_iter().map(ActivityNode::from).collect())
    }

    #[graphql(name = "tasks")]
    async fn tasks(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        offset: Option<i32>,
        filter: Option<TaskFilter>,
        #[graphql(name = "orderBy", default)] order_by: TaskOrder,
    ) -> async_graphql::Result<Vec<TaskNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let requested = first.unwrap_or(25);
        let limit = enforce_task_limit(requested)?;
        let skip = offset.unwrap_or(0).max(0) as u64;
        let filter_snapshot = filter.clone();
        let status_tag = filter_snapshot
            .as_ref()
            .and_then(|f| f.status)
            .map(|s| s.as_str())
            .unwrap_or("");
        let priority_tag = filter_snapshot
            .as_ref()
            .and_then(|f| f.priority)
            .map(|p| p.as_str())
            .unwrap_or("");
        let has_q = filter_snapshot
            .as_ref()
            .and_then(|f| f.q.as_ref())
            .map(|q| !q.trim().is_empty())
            .unwrap_or(false);
        let span = info_span!(
            "crm.tasks.list",
            status = status_tag,
            priority = priority_tag,
            has_q = has_q,
            order = order_by.as_str(),
            first = requested
        );
        let _guard = span.enter();

        let mut query = task::Entity::find();
        if let Some(filter) = filter {
            if let Some(company_id) = parse_optional_id("companyId", &filter.company_id)? {
                query = query.filter(task::Column::CompanyId.eq(company_id));
            }
            if let Some(contact_id) = parse_optional_id("contactId", &filter.contact_id)? {
                query = query.filter(task::Column::ContactId.eq(contact_id));
            }
            if let Some(deal_id) = parse_optional_id("dealId", &filter.deal_id)? {
                query = query.filter(task::Column::DealId.eq(deal_id));
            }
            if let Some(status) = filter.status {
                query = query.filter(task::Column::Status.eq(task::Status::from(status)));
            }
            if let Some(priority) = filter.priority {
                query = query.filter(task::Column::Priority.eq(task::Priority::from(priority)));
            }
            if let Some(before) = filter.due_before {
                let ts: DateTimeWithTimeZone = before.into();
                query = query.filter(task::Column::DueAt.lt(ts));
            }
            if let Some(after) = filter.due_after {
                let ts: DateTimeWithTimeZone = after.into();
                query = query.filter(task::Column::DueAt.gt(ts));
            }
            if let Some(q) = filter.q {
                let trimmed = q.trim();
                if !trimmed.is_empty() {
                    let lowered = trimmed.to_lowercase();
                    let pattern = format!("%{}%", lowered);
                    let title_expr = Expr::expr(Func::lower(Expr::col(task::Column::Title)));
                    let notes_expr = Expr::expr(Func::lower(Expr::col(task::Column::NotesMd)));
                    let condition = Condition::any()
                        .add(title_expr.like(pattern.clone()))
                        .add(notes_expr.like(pattern));
                    query = query.filter(condition);
                }
            }
        }
        query = apply_task_ordering(query, order_by);
        let rows = query
            .limit(limit)
            .offset(skip)
            .all(db.as_ref())
            .await
            .map_err(db_error)?;

        Ok(rows.into_iter().map(TaskNode::from).collect())
    }

    #[graphql(name = "task")]
    async fn task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<TaskNode>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let record = task::Entity::find_by_id(task_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        Ok(record.map(TaskNode::from))
    }

    async fn pipeline_stages(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<PipelineStage>> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let stages = load_stage_meta(db.as_ref()).await?;
        Ok(stages.iter().map(PipelineStage::from).collect())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pipeline_board(
        &self,
        ctx: &Context<'_>,
        #[graphql(name = "firstPerStage")] first_per_stage: Option<i32>,
        #[graphql(name = "stageKeys")] stage_keys: Option<Vec<String>>,
        #[graphql(name = "companyId")] company_id: Option<ID>,
        q: Option<String>,
        #[graphql(name = "orderByUpdated")] order_by_updated: Option<bool>,
    ) -> async_graphql::Result<PipelineBoard> {
        require_viewer(ctx)?;
        let db = database(ctx)?;
        let requested = first_per_stage.unwrap_or(25);
        if requested < 0 {
            return Err(validation_error("firstPerStage must be non-negative"));
        }
        if requested > 100 {
            return Err(error_with_code(
                "LIMIT_EXCEEDED",
                "firstPerStage cannot exceed 100",
            ));
        }
        let company_filter = match company_id {
            Some(id) => Some(parse_uuid(&id)?),
            None => None,
        };
        let query_filter = sanitize_optional_filter(q);
        let order_by_updated = order_by_updated.unwrap_or(true);
        let has_stage_filter = stage_keys.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
        let span = info_span!(
            "crm.pipelineBoard",
            first = requested,
            has_stage_filter,
            has_company = company_filter.is_some(),
            has_q = query_filter.is_some(),
            order_by_updated
        );
        let _guard = span.enter();
        let stages = load_stage_meta(db.as_ref()).await?;
        if stages.is_empty() {
            return Ok(PipelineBoard {
                columns: vec![],
                total_count: 0,
                total_amount_cents: Some(0),
                total_expected_cents: Some(0),
            });
        }
        let stage_sequence = select_stage_sequence(&stages, stage_keys.as_ref())?;
        if stage_sequence.is_empty() {
            return Ok(PipelineBoard {
                columns: vec![],
                total_count: 0,
                total_amount_cents: Some(0),
                total_expected_cents: Some(0),
            });
        }
        let totals =
            query_pipeline_stage_totals(db.as_ref(), company_filter, query_filter.as_deref())
                .await?;
        let totals_map: HashMap<String, StageAggregateRow> = totals
            .into_iter()
            .map(|row| (row.stage_key.clone(), row))
            .collect();
        let mut columns: Vec<PipelineColumn> = Vec::new();
        for stage in stage_sequence {
            let totals_row = totals_map.get(&stage.key);
            let deals = if requested == 0 {
                vec![]
            } else {
                query_stage_deals(
                    db.as_ref(),
                    &stage.key,
                    company_filter,
                    query_filter.as_deref(),
                    order_by_updated,
                    requested as u64,
                )
                .await?
            };
            let column = PipelineColumn {
                stage: PipelineStage::from(&stage),
                total_count: totals_row.map(|row| row.total_count as i32).unwrap_or(0),
                total_amount_cents: Some(totals_row.map(|row| row.total_amount_cents).unwrap_or(0)),
                expected_value_cents: Some(
                    totals_row.map(|row| row.total_expected_cents).unwrap_or(0),
                ),
                deals,
            };
            columns.push(column);
        }
        let total_count: i32 = columns.iter().map(|col| col.total_count).sum();
        let total_amount_cents: i64 = columns
            .iter()
            .map(|col| col.total_amount_cents.unwrap_or(0))
            .sum();
        let total_expected_cents: i64 = columns
            .iter()
            .map(|col| col.expected_value_cents.unwrap_or(0))
            .sum();
        Ok(PipelineBoard {
            columns,
            total_count,
            total_amount_cents: Some(total_amount_cents),
            total_expected_cents: Some(total_expected_cents),
        })
    }

    async fn pipeline_report(
        &self,
        ctx: &Context<'_>,
        range: DateRange,
        group: Option<TimeGroup>,
        #[graphql(name = "includeLost")] include_lost: Option<bool>,
    ) -> async_graphql::Result<PipelineReport> {
        require_viewer(ctx)?;
        if range.from > range.to {
            return Err(validation_error("range.from must be on or before range.to"));
        }
        let grouping = group.unwrap_or(TimeGroup::Month);
        if grouping != TimeGroup::Month {
            return Err(validation_error("Only MONTH grouping is supported"));
        }
        let include_lost = include_lost.unwrap_or(false);
        let db = database(ctx)?;
        let span = info_span!(
            "crm.pipelineReport",
            from = range.from.to_string(),
            to = range.to.to_string(),
            include_lost
        );
        let _guard = span.enter();
        let stages = load_stage_meta(db.as_ref()).await?;
        let stage_rows = query_report_stage_totals(db.as_ref(), &range, include_lost).await?;
        let stage_row_map: HashMap<String, StageReportRow> = stage_rows
            .into_iter()
            .map(|row| (row.stage_key.clone(), row))
            .collect();
        let mut stage_totals = Vec::new();
        for stage in stages.iter() {
            if let Some(row) = stage_row_map.get(&stage.key) {
                stage_totals.push(StageTotals {
                    stage: PipelineStage::from(stage),
                    count: row.total_count as i32,
                    amount_cents: Some(row.amount_cents),
                    expected_cents: Some(row.expected_cents),
                });
            }
        }
        let forecast_rows = query_forecast_points(db.as_ref(), &range, include_lost).await?;
        let forecast = build_forecast_points(&range, forecast_rows);
        let velocity_rows = query_velocity_rows(db.as_ref(), &range).await?;
        let velocity = compute_velocity_stats(velocity_rows);

        Ok(PipelineReport {
            stage_totals,
            forecast,
            velocity,
        })
    }
}

#[Object]
impl CrmMutation {
    async fn login(
        &self,
        ctx: &Context<'_>,
        email: String,
        password: String,
    ) -> async_graphql::Result<AuthPayload> {
        let auth = auth_config(ctx)?;
        if !auth.local_auth_enabled {
            return Err(error_with_code("FORBIDDEN", "Local authentication is disabled"));
        }
        let db = database(ctx)?;
        let normalized = normalize_email(&email)?;
        let identity = user_identity::Entity::find()
            .filter(user_identity::Column::Provider.eq("local"))
            .filter(user_identity::Column::Subject.eq(normalized.clone()))
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        let Some(identity) = identity else {
            return Ok(AuthPayload {
                ok: false,
                user: None,
                error: Some("Invalid credentials".into()),
            });
        };
        let user = user::Entity::find_by_id(identity.user_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        let Some(user) = user else {
            return Ok(AuthPayload {
                ok: false,
                user: None,
                error: Some("Invalid credentials".into()),
            });
        };
        if !user.is_active {
            return Ok(AuthPayload {
                ok: false,
                user: None,
                error: Some("Account disabled".into()),
            });
        }
        let secret = user_secret::Entity::find_by_id(user.id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        let Some(secret) = secret else {
            return Ok(AuthPayload {
                ok: false,
                user: None,
                error: Some("Invalid credentials".into()),
            });
        };
        let parsed_hash = PasswordHash::new(&secret.password_hash)
            .map_err(|_| error_with_code("INTERNAL", "Invalid password hash"))?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_err()
        {
            return Ok(AuthPayload {
                ok: false,
                user: None,
                error: Some("Invalid credentials".into()),
            });
        }
        let roles = load_roles(db.as_ref(), user.id).await?;
        let token = issue_token(user.id, &roles, &auth)
            .map_err(|_| error_with_code("INTERNAL", "Failed to issue session token"))?;
        append_session_cookie(ctx, &token, auth.session_ttl_minutes);
        Ok(AuthPayload {
            ok: true,
            user: Some(UserNode::from_model(user, roles)),
            error: None,
        })
    }

    async fn logout(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        append_session_cookie(ctx, "", -1);
        Ok(true)
    }

    async fn get_auth_url(
        &self,
        ctx: &Context<'_>,
        provider: String,
    ) -> async_graphql::Result<String> {
        let auth = auth_config(ctx)?;
        if !auth.oidc_enabled {
            return Err(error_with_code(
                "VALIDATION",
                format!("OIDC provider {} is not configured", provider),
            ));
        }
        Err(error_with_code(
            "NOT_IMPLEMENTED",
            "OIDC integrations are not yet configured",
        ))
    }

    async fn handle_oidc_callback(
        &self,
        _ctx: &Context<'_>,
        _provider: String,
        _code: String,
        _state: String,
    ) -> async_graphql::Result<AuthPayload> {
        Err(error_with_code(
            "NOT_IMPLEMENTED",
            "OIDC integrations are not yet configured",
        ))
    }

    async fn create_user(
        &self,
        ctx: &Context<'_>,
        input: NewUserInput,
    ) -> async_graphql::Result<UserNode> {
        require_role(ctx, UserRole::Admin)?;
        let db = database(ctx)?;
        let email = normalize_email(&input.email)?;
        let display_name = validate_display_name(&input.display_name)?;
        let roles = parse_roles(&input.roles)?;
        if roles.is_empty() {
            return Err(validation_error("roles must include at least one entry"));
        }
        let txn = db.begin().await.map_err(db_error)?;
        let now: DateTimeWithTimeZone = Utc::now().into();
        let user_id = Uuid::new_v4();
        user::ActiveModel {
            id: Set(user_id),
            email: Set(email.clone()),
            display_name: Set(display_name),
            avatar_url: Set(None),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await
        .map_err(db_error)?;
        user_identity::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            provider: Set("local".into()),
            subject: Set(email),
            created_at: Set(now),
        }
        .insert(&txn)
        .await
        .map_err(db_error)?;
        insert_roles(&txn, user_id, &roles).await?;
        txn.commit().await.map_err(db_error)?;
        let record = user::Entity::find_by_id(user_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?
            .ok_or_else(|| error_with_code("INTERNAL", "Failed to load new user"))?;
        Ok(UserNode::from_model(record, roles))
    }

    async fn update_user(
        &self,
        ctx: &Context<'_>,
        input: UpdateUserInput,
    ) -> async_graphql::Result<UserNode> {
        require_role(ctx, UserRole::Admin)?;
        let db = database(ctx)?;
        let user_id = parse_uuid(&input.id)?;
        let mut model = user::Entity::find_by_id(user_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?
            .ok_or_else(|| error_with_code("NOT_FOUND", "User not found"))?;
        if let Some(display_name) = &input.display_name {
            model.display_name = validate_display_name(display_name)?;
        }
        if let Some(is_active) = input.is_active {
            model.is_active = is_active;
        }
        model.updated_at = Utc::now().into();
        let mut active: user::ActiveModel = model.clone().into();
        if let Some(display_name) = &input.display_name {
            active.display_name = Set(display_name.trim().to_string());
        }
        if let Some(is_active) = input.is_active {
            active.is_active = Set(is_active);
        }
        active.updated_at = Set(Utc::now().into());
        let updated = active.update(db.as_ref()).await.map_err(db_error)?;
        let mut roles = load_roles(db.as_ref(), user_id).await?;
        if let Some(role_values) = input.roles {
            let parsed = parse_roles(&role_values)?;
            let txn = db.begin().await.map_err(db_error)?;
            user_role::Entity::delete_many()
                .filter(user_role::Column::UserId.eq(user_id))
                .exec(&txn)
                .await
                .map_err(db_error)?;
            insert_roles(&txn, user_id, &parsed).await?;
            txn.commit().await.map_err(db_error)?;
            roles = parsed;
        }
        Ok(UserNode::from_model(updated, roles))
    }

    #[graphql(name = "assignCompany")]
    async fn assign_company(
        &self,
        ctx: &Context<'_>,
        id: ID,
        #[graphql(name = "userId")] user_id: Option<ID>,
    ) -> async_graphql::Result<CompanyNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let company_id = parse_uuid(&id)?;
        let target_user = match user_id {
            Some(uid) => Some(ensure_active_user(db.as_ref(), parse_uuid(&uid)?).await?),
            None => None,
        };
        let now: DateTimeWithTimeZone = Utc::now().into();
        let company = company::Entity::find_by_id(company_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?
            .ok_or_else(|| error_with_code("NOT_FOUND", "Company not found"))?;
        let mut active: company::ActiveModel = company.into();
        active.assigned_user_id = Set(target_user);
        active.updated_by = Set(Some(current.user_id));
        active.updated_at = Set(now);
        let updated = active.update(db.as_ref()).await.map_err(db_error)?;
        Ok(updated.into())
    }

    #[graphql(name = "assignContact")]
    async fn assign_contact(
        &self,
        ctx: &Context<'_>,
        id: ID,
        #[graphql(name = "userId")] user_id: Option<ID>,
    ) -> async_graphql::Result<ContactNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let contact_id = parse_uuid(&id)?;
        let target_user = match user_id {
            Some(uid) => Some(ensure_active_user(db.as_ref(), parse_uuid(&uid)?).await?),
            None => None,
        };
        let now: DateTimeWithTimeZone = Utc::now().into();
        let contact = contact::Entity::find_by_id(contact_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?
            .ok_or_else(|| error_with_code("NOT_FOUND", "Contact not found"))?;
        let mut active: contact::ActiveModel = contact.into();
        active.assigned_user_id = Set(target_user);
        active.updated_by = Set(Some(current.user_id));
        active.updated_at = Set(now);
        let updated = active.update(db.as_ref()).await.map_err(db_error)?;
        Ok(updated.into())
    }

    #[graphql(name = "assignDeal")]
    async fn assign_deal(
        &self,
        ctx: &Context<'_>,
        id: ID,
        #[graphql(name = "userId")] user_id: Option<ID>,
    ) -> async_graphql::Result<DealNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let deal_id = parse_uuid(&id)?;
        let target_user = match user_id {
            Some(uid) => Some(ensure_active_user(db.as_ref(), parse_uuid(&uid)?).await?),
            None => None,
        };
        let now: DateTimeWithTimeZone = Utc::now().into();
        let deal = deal::Entity::find_by_id(deal_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?
            .ok_or_else(|| error_with_code("NOT_FOUND", "Deal not found"))?;
        let mut active: deal::ActiveModel = deal.into();
        active.assigned_user_id = Set(target_user);
        active.updated_by = Set(Some(current.user_id));
        active.updated_at = Set(now);
        let updated = active.update(db.as_ref()).await.map_err(db_error)?;
        Ok(updated.into())
    }

    #[graphql(name = "assignTask")]
    async fn assign_task(
        &self,
        ctx: &Context<'_>,
        id: ID,
        #[graphql(name = "userId")] user_id: Option<ID>,
    ) -> async_graphql::Result<TaskNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let target_user = match user_id {
            Some(uid) => Some(ensure_active_user(db.as_ref(), parse_uuid(&uid)?).await?),
            None => None,
        };
        let now: DateTimeWithTimeZone = Utc::now().into();
        let task = task::Entity::find_by_id(task_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?
            .ok_or_else(|| error_with_code("NOT_FOUND", "Task not found"))?;
        let mut active: task::ActiveModel = task.into();
        active.assigned_user_id = Set(target_user);
        active.updated_by = Set(Some(current.user_id));
        active.updated_at = Set(now);
        let updated = active.update(db.as_ref()).await.map_err(db_error)?;
        Ok(TaskNode::from(updated))
    }
    #[graphql(name = "moveDealStage")]
    async fn move_deal_stage(
        &self,
        ctx: &Context<'_>,
        id: ID,
        stage: DealStage,
        note: Option<String>,
    ) -> async_graphql::Result<DealNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let deal_id = parse_uuid(&id)?;
        let target_stage: deal::Stage = stage.into();

        let model =
            move_deal_stage_internal(db.as_ref(), deal_id, target_stage, note, Some(current.user_id))
            .await
            .map_err(stage_move_error)?;

        Ok(model.into())
    }

    #[graphql(name = "createTask")]
    async fn create_task(
        &self,
        ctx: &Context<'_>,
        input: NewTaskInput,
    ) -> async_graphql::Result<TaskNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let span = info_span!(
            "crm.tasks.create",
            status = TaskStatus::Open.as_str(),
            priority = input.priority.as_str(),
            has_q = false,
            order = "",
            first = 0
        );
        let _guard = span.enter();
        let task = create_task_internal(db.as_ref(), input, &current).await?;
        Ok(task.into())
    }

    #[graphql(name = "updateTask")]
    async fn update_task(
        &self,
        ctx: &Context<'_>,
        input: UpdateTaskInput,
    ) -> async_graphql::Result<TaskNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let task = update_task_internal(db.as_ref(), input, &current).await?;
        Ok(task.into())
    }

    #[graphql(name = "completeTask")]
    async fn complete_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<TaskNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let existing = task::Entity::find_by_id(task_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        let Some(existing) = existing else {
            return Err(error_with_code("NOT_FOUND", "Task not found"));
        };
        let span = info_span!(
            "crm.tasks.complete",
            status = TaskStatus::Done.as_str(),
            priority = TaskPriority::from(existing.priority).as_str(),
            has_q = false,
            order = "",
            first = 0
        );
        let _guard = span.enter();
        let task = transition_task_status(
            db.as_ref(),
            existing,
            task::Status::Done,
            Some(Utc::now().into()),
            &current,
        )
        .await?;
        Ok(task.into())
    }

    #[graphql(name = "cancelTask")]
    async fn cancel_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<TaskNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let existing = task::Entity::find_by_id(task_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        let Some(existing) = existing else {
            return Err(error_with_code("NOT_FOUND", "Task not found"));
        };
        let span = info_span!(
            "crm.tasks.cancel",
            status = TaskStatus::Cancelled.as_str(),
            priority = TaskPriority::from(existing.priority).as_str(),
            has_q = false,
            order = "",
            first = 0
        );
        let _guard = span.enter();
        let task = transition_task_status(
            db.as_ref(),
            existing,
            task::Status::Cancelled,
            None,
            &current,
        )
        .await?;
        Ok(task.into())
    }

    #[graphql(name = "reopenTask")]
    async fn reopen_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<TaskNode> {
        let current = require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let existing = task::Entity::find_by_id(task_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        let Some(existing) = existing else {
            return Err(error_with_code("NOT_FOUND", "Task not found"));
        };
        let span = info_span!(
            "crm.tasks.reopen",
            status = TaskStatus::Open.as_str(),
            priority = TaskPriority::from(existing.priority).as_str(),
            has_q = false,
            order = "",
            first = 0
        );
        let _guard = span.enter();
        let task = transition_task_status(
            db.as_ref(),
            existing,
            task::Status::Open,
            None,
            &current,
        )
        .await?;
        Ok(task.into())
    }

    #[graphql(name = "deleteTask")]
    async fn delete_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<bool> {
        require_role(ctx, UserRole::Sales)?;
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let res = task::Entity::delete_by_id(task_id)
            .exec(db.as_ref())
            .await
            .map_err(db_error)?;
        Ok(res.rows_affected > 0)
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum DealStage {
    New,
    Qualify,
    Proposal,
    Negotiate,
    Won,
    Lost,
}

impl From<deal::Stage> for DealStage {
    fn from(value: deal::Stage) -> Self {
        match value {
            deal::Stage::New => DealStage::New,
            deal::Stage::Qualify => DealStage::Qualify,
            deal::Stage::Proposal => DealStage::Proposal,
            deal::Stage::Negotiate => DealStage::Negotiate,
            deal::Stage::Won => DealStage::Won,
            deal::Stage::Lost => DealStage::Lost,
        }
    }
}

impl From<DealStage> for deal::Stage {
    fn from(value: DealStage) -> Self {
        match value {
            DealStage::New => deal::Stage::New,
            DealStage::Qualify => deal::Stage::Qualify,
            DealStage::Proposal => deal::Stage::Proposal,
            DealStage::Negotiate => deal::Stage::Negotiate,
            DealStage::Won => deal::Stage::Won,
            DealStage::Lost => deal::Stage::Lost,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum TaskStatus {
    Open,
    Done,
    Cancelled,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Open
    }
}

impl TaskStatus {
    fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Open => "OPEN",
            TaskStatus::Done => "DONE",
            TaskStatus::Cancelled => "CANCELLED",
        }
    }
}

impl From<task::Status> for TaskStatus {
    fn from(value: task::Status) -> Self {
        match value {
            task::Status::Open => TaskStatus::Open,
            task::Status::Done => TaskStatus::Done,
            task::Status::Cancelled => TaskStatus::Cancelled,
        }
    }
}

impl From<TaskStatus> for task::Status {
    fn from(value: TaskStatus) -> Self {
        match value {
            TaskStatus::Open => task::Status::Open,
            TaskStatus::Done => task::Status::Done,
            TaskStatus::Cancelled => task::Status::Cancelled,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::Medium
    }
}

impl TaskPriority {
    fn as_str(self) -> &'static str {
        match self {
            TaskPriority::Low => "LOW",
            TaskPriority::Medium => "MEDIUM",
            TaskPriority::High => "HIGH",
        }
    }
}

impl From<task::Priority> for TaskPriority {
    fn from(value: task::Priority) -> Self {
        match value {
            task::Priority::Low => TaskPriority::Low,
            task::Priority::Medium => TaskPriority::Medium,
            task::Priority::High => TaskPriority::High,
        }
    }
}

impl From<TaskPriority> for task::Priority {
    fn from(value: TaskPriority) -> Self {
        match value {
            TaskPriority::Low => task::Priority::Low,
            TaskPriority::Medium => task::Priority::Medium,
            TaskPriority::High => task::Priority::High,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum TaskOrder {
    DueAsc,
    DueDesc,
    PriorityDesc,
    UpdatedDesc,
}

impl Default for TaskOrder {
    fn default() -> Self {
        TaskOrder::DueAsc
    }
}

impl TaskOrder {
    fn as_str(self) -> &'static str {
        match self {
            TaskOrder::DueAsc => "DUE_ASC",
            TaskOrder::DueDesc => "DUE_DESC",
            TaskOrder::PriorityDesc => "PRIORITY_DESC",
            TaskOrder::UpdatedDesc => "UPDATED_DESC",
        }
    }
}

#[derive(InputObject, Default, Clone)]
pub struct TaskFilter {
    #[graphql(name = "companyId")]
    pub company_id: Option<ID>,
    #[graphql(name = "contactId")]
    pub contact_id: Option<ID>,
    #[graphql(name = "dealId")]
    pub deal_id: Option<ID>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    #[graphql(name = "dueBefore")]
    pub due_before: Option<DateTime<Utc>>,
    #[graphql(name = "dueAfter")]
    pub due_after: Option<DateTime<Utc>>,
    pub q: Option<String>,
}

#[derive(InputObject, Clone)]
pub struct NewTaskInput {
    pub title: String,
    #[graphql(name = "notesMd")]
    pub notes_md: Option<String>,
    #[graphql(default)]
    pub priority: TaskPriority,
    pub assignee: Option<String>,
    #[graphql(name = "dueAt")]
    pub due_at: Option<DateTime<Utc>>,
    #[graphql(name = "companyId")]
    pub company_id: Option<ID>,
    #[graphql(name = "contactId")]
    pub contact_id: Option<ID>,
    #[graphql(name = "dealId")]
    pub deal_id: Option<ID>,
}

#[derive(InputObject, Clone)]
pub struct UpdateTaskInput {
    pub id: ID,
    pub title: Option<String>,
    #[graphql(name = "notesMd")]
    pub notes_md: Option<String>,
    pub priority: Option<TaskPriority>,
    pub assignee: Option<String>,
    #[graphql(name = "dueAt")]
    pub due_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Company")]
pub struct CompanyNode {
    pub id: ID,
    pub name: String,
    pub website: Option<String>,
    pub phone: Option<String>,
    #[graphql(name = "assignedUserId")]
    pub assigned_user_id: Option<ID>,
    #[graphql(name = "createdBy")]
    pub created_by: Option<ID>,
    #[graphql(name = "updatedBy")]
    pub updated_by: Option<ID>,
    #[graphql(name = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[graphql(name = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

impl From<company::Model> for CompanyNode {
    fn from(model: company::Model) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            name: model.name,
            website: model.website,
            phone: model.phone,
            assigned_user_id: model.assigned_user_id.map(|id| ID::from(id.to_string())),
            created_by: model.created_by.map(|id| ID::from(id.to_string())),
            updated_by: model.updated_by.map(|id| ID::from(id.to_string())),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Contact")]
pub struct ContactNode {
    pub id: ID,
    pub email: String,
    #[graphql(name = "firstName")]
    pub first_name: Option<String>,
    #[graphql(name = "lastName")]
    pub last_name: Option<String>,
    pub phone: Option<String>,
    #[graphql(name = "companyId")]
    pub company_id: Option<ID>,
    #[graphql(name = "assignedUserId")]
    pub assigned_user_id: Option<ID>,
    #[graphql(name = "createdBy")]
    pub created_by: Option<ID>,
    #[graphql(name = "updatedBy")]
    pub updated_by: Option<ID>,
    #[graphql(name = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[graphql(name = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

impl From<contact::Model> for ContactNode {
    fn from(model: contact::Model) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            email: model.email,
            first_name: model.first_name,
            last_name: model.last_name,
            phone: model.phone,
            company_id: model.company_id.map(|id| ID::from(id.to_string())),
            assigned_user_id: model.assigned_user_id.map(|id| ID::from(id.to_string())),
            created_by: model.created_by.map(|id| ID::from(id.to_string())),
            updated_by: model.updated_by.map(|id| ID::from(id.to_string())),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Task")]
pub struct TaskNode {
    pub id: ID,
    pub title: String,
    #[graphql(name = "notesMd")]
    pub notes_md: Option<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub assignee: Option<String>,
    #[graphql(name = "assignedUserId")]
    pub assigned_user_id: Option<ID>,
    #[graphql(name = "dueAt")]
    pub due_at: Option<DateTime<Utc>>,
    #[graphql(name = "completedAt")]
    pub completed_at: Option<DateTime<Utc>>,
    #[graphql(name = "companyId")]
    pub company_id: Option<ID>,
    #[graphql(name = "contactId")]
    pub contact_id: Option<ID>,
    #[graphql(name = "dealId")]
    pub deal_id: Option<ID>,
    #[graphql(name = "createdBy")]
    pub created_by: Option<ID>,
    #[graphql(name = "updatedBy")]
    pub updated_by: Option<ID>,
    #[graphql(name = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[graphql(name = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

impl From<task::Model> for TaskNode {
    fn from(model: task::Model) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            title: model.title,
            notes_md: model.notes_md,
            status: model.status.into(),
            priority: model.priority.into(),
            assignee: model.assignee,
            assigned_user_id: model.assigned_user_id.map(|id| ID::from(id.to_string())),
            due_at: model.due_at.map(|d| d.into()),
            completed_at: model.completed_at.map(|d| d.into()),
            company_id: model.company_id.map(|id| ID::from(id.to_string())),
            contact_id: model.contact_id.map(|id| ID::from(id.to_string())),
            deal_id: model.deal_id.map(|id| ID::from(id.to_string())),
            created_by: model.created_by.map(|id| ID::from(id.to_string())),
            updated_by: model.updated_by.map(|id| ID::from(id.to_string())),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Deal")]
pub struct DealNode {
    pub id: ID,
    pub title: String,
    #[graphql(name = "amountCents")]
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    pub stage: DealStage,
    #[graphql(name = "closeDate")]
    pub close_date: Option<NaiveDate>,
    #[graphql(name = "companyId")]
    pub company_id: ID,
    #[graphql(name = "assignedUserId")]
    pub assigned_user_id: Option<ID>,
    #[graphql(name = "createdBy")]
    pub created_by: Option<ID>,
    #[graphql(name = "updatedBy")]
    pub updated_by: Option<ID>,
    #[graphql(name = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[graphql(name = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

impl From<deal::Model> for DealNode {
    fn from(model: deal::Model) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            title: model.title,
            amount_cents: model.amount_cents,
            currency: model.currency,
            stage: model.stage.into(),
            close_date: model.close_date,
            company_id: ID::from(model.company_id.to_string()),
            assigned_user_id: model.assigned_user_id.map(|id| ID::from(id.to_string())),
            created_by: model.created_by.map(|id| ID::from(id.to_string())),
            updated_by: model.updated_by.map(|id| ID::from(id.to_string())),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "DealStageHistory")]
pub struct DealStageHistoryNode {
    pub id: ID,
    #[graphql(name = "dealId")]
    pub deal_id: ID,
    #[graphql(name = "fromStage")]
    pub from_stage: DealStage,
    #[graphql(name = "toStage")]
    pub to_stage: DealStage,
    #[graphql(name = "note")]
    pub note: Option<String>,
    #[graphql(name = "changedAt")]
    pub changed_at: DateTime<Utc>,
    #[graphql(name = "changedBy")]
    pub changed_by: Option<String>,
}

impl From<deal_stage_history::Model> for DealStageHistoryNode {
    fn from(model: deal_stage_history::Model) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            deal_id: ID::from(model.deal_id.to_string()),
            from_stage: model.from_stage.into(),
            to_stage: model.to_stage.into(),
            note: model.note,
            changed_at: model.changed_at.into(),
            changed_by: model.changed_by,
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Activity")]
pub struct ActivityNode {
    pub id: ID,
    #[graphql(name = "entityType")]
    pub entity_type: String,
    #[graphql(name = "entityId")]
    pub entity_id: ID,
    pub kind: String,
    pub subject: Option<String>,
    #[graphql(name = "bodyMd")]
    pub body_md: Option<String>,
    #[graphql(name = "metaJson")]
    pub meta_json: Json<serde_json::Value>,
    #[graphql(name = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[graphql(name = "createdBy")]
    pub created_by: Option<ID>,
}

impl From<activity::Model> for ActivityNode {
    fn from(model: activity::Model) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            entity_type: model.entity_type,
            entity_id: ID::from(model.entity_id.to_string()),
            kind: match model.kind {
                activity::Kind::StageChange => "stage_change".to_string(),
            },
            subject: model.subject,
            body_md: model.body_md,
            meta_json: Json(model.meta_json),
            created_at: model.created_at.into(),
            created_by: model.created_by.map(|id| ID::from(id.to_string())),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "User")]
pub struct UserNode {
    pub id: ID,
    pub email: String,
    #[graphql(name = "displayName")]
    pub display_name: String,
    #[graphql(name = "avatarUrl")]
    pub avatar_url: Option<String>,
    #[graphql(name = "isActive")]
    pub is_active: bool,
    pub roles: Vec<String>,
    #[graphql(name = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[graphql(name = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

impl UserNode {
    fn from_model(model: user::Model, roles: Vec<UserRole>) -> Self {
        Self {
            id: ID::from(model.id.to_string()),
            email: model.email,
            display_name: model.display_name,
            avatar_url: model.avatar_url,
            is_active: model.is_active,
            roles: roles.into_iter().map(|r| r.as_str().to_string()).collect(),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct MePayload {
    pub user: UserNode,
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, SimpleObject, Default)]
pub struct AuthPayload {
    pub ok: bool,
    pub user: Option<UserNode>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, InputObject)]
pub struct NewUserInput {
    pub email: String,
    #[graphql(name = "displayName")]
    pub display_name: String,
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, InputObject)]
pub struct UpdateUserInput {
    pub id: ID,
    #[graphql(name = "displayName")]
    pub display_name: Option<String>,
    pub roles: Option<Vec<String>>,
    #[graphql(name = "isActive")]
    pub is_active: Option<bool>,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct PipelineStage {
    pub key: String,
    #[graphql(name = "displayName")]
    pub display_name: String,
    #[graphql(name = "sortOrder")]
    pub sort_order: i32,
    pub probability: i32,
    #[graphql(name = "isWon")]
    pub is_won: bool,
    #[graphql(name = "isLost")]
    pub is_lost: bool,
}

impl From<&stage_meta::Model> for PipelineStage {
    fn from(model: &stage_meta::Model) -> Self {
        Self {
            key: model.key.clone(),
            display_name: model.display_name.clone(),
            sort_order: model.sort_order as i32,
            probability: model.probability as i32,
            is_won: model.is_won,
            is_lost: model.is_lost,
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct PipelineDeal {
    pub id: ID,
    pub title: String,
    #[graphql(name = "amountCents")]
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    #[graphql(name = "stageKey")]
    pub stage_key: String,
    #[graphql(name = "companyId")]
    pub company_id: ID,
    #[graphql(name = "companyName")]
    pub company_name: Option<String>,
    #[graphql(name = "expectedClose")]
    pub expected_close: Option<NaiveDate>,
    #[graphql(name = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct PipelineColumn {
    pub stage: PipelineStage,
    #[graphql(name = "totalCount")]
    pub total_count: i32,
    #[graphql(name = "totalAmountCents")]
    pub total_amount_cents: Option<i64>,
    #[graphql(name = "expectedValueCents")]
    pub expected_value_cents: Option<i64>,
    pub deals: Vec<PipelineDeal>,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct PipelineBoard {
    pub columns: Vec<PipelineColumn>,
    #[graphql(name = "totalCount")]
    pub total_count: i32,
    #[graphql(name = "totalAmountCents")]
    pub total_amount_cents: Option<i64>,
    #[graphql(name = "totalExpectedCents")]
    pub total_expected_cents: Option<i64>,
}

#[derive(Clone, Debug, InputObject)]
pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum TimeGroup {
    #[graphql(name = "MONTH")]
    Month,
    #[graphql(name = "WEEK")]
    Week,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct StageTotals {
    pub stage: PipelineStage,
    pub count: i32,
    #[graphql(name = "amountCents")]
    pub amount_cents: Option<i64>,
    #[graphql(name = "expectedCents")]
    pub expected_cents: Option<i64>,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct ForecastPoint {
    pub period: String,
    #[graphql(name = "amountCents")]
    pub amount_cents: Option<i64>,
    #[graphql(name = "expectedCents")]
    pub expected_cents: Option<i64>,
    pub deals: i32,
}

#[derive(Clone, Debug, SimpleObject)]
pub struct VelocityStats {
    #[graphql(name = "dealsWon")]
    pub deals_won: i32,
    #[graphql(name = "avgDaysToWin")]
    pub avg_days_to_win: f64,
    #[graphql(name = "p50DaysToWin")]
    pub p50_days_to_win: f64,
    #[graphql(name = "p90DaysToWin")]
    pub p90_days_to_win: f64,
}

impl Default for VelocityStats {
    fn default() -> Self {
        Self {
            deals_won: 0,
            avg_days_to_win: 0.0,
            p50_days_to_win: 0.0,
            p90_days_to_win: 0.0,
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct PipelineReport {
    #[graphql(name = "stageTotals")]
    pub stage_totals: Vec<StageTotals>,
    pub forecast: Vec<ForecastPoint>,
    pub velocity: VelocityStats,
}

#[derive(Debug)]
pub enum StageMoveError {
    NotFound,
    Db(DbErr),
}

impl From<DbErr> for StageMoveError {
    fn from(value: DbErr) -> Self {
        StageMoveError::Db(value)
    }
}

fn stage_move_error(err: StageMoveError) -> Error {
    match err {
        StageMoveError::NotFound => error_with_code("NOT_FOUND", "Deal not found"),
        StageMoveError::Db(e) => db_error(e),
    }
}

async fn move_deal_stage_internal(
    db: &DatabaseConnection,
    deal_id: Uuid,
    stage: deal::Stage,
    note: Option<String>,
    changed_by: Option<Uuid>,
) -> Result<deal::Model, StageMoveError> {
    let txn = db.begin().await?;
    let existing = deal::Entity::find_by_id(deal_id)
        .one(&txn)
        .await?
        .ok_or(StageMoveError::NotFound)?;

    let now: DateTimeWithTimeZone = Utc::now().into();
    if existing.stage == stage {
        let mut active: deal::ActiveModel = existing.into();
        active.updated_at = Set(now.clone());
        let updated = active.update(&txn).await?;
        txn.commit().await?;
        return Ok(updated);
    }

    let from_stage = existing.stage;
    let mut active: deal::ActiveModel = existing.into();
    let actor = changed_by;
    active.stage = Set(stage);
    active.updated_at = Set(now.clone());
    active.updated_by = Set(actor);
    let updated = active.update(&txn).await?;

    let history = deal_stage_history::ActiveModel {
        id: Set(Uuid::new_v4()),
        deal_id: Set(deal_id),
        from_stage: Set(from_stage),
        to_stage: Set(stage),
        changed_at: Set(now.clone()),
        note: Set(note.clone()),
        changed_by: Set(actor.map(|id| id.to_string())),
    };
    deal_stage_history::Entity::insert(history)
        .exec_without_returning(&txn)
        .await?;

    let activity = activity_stage_change(deal_id, from_stage, stage, note, actor, now.clone());
    activity::Entity::insert(activity)
        .exec_without_returning(&txn)
        .await?;

    txn.commit().await?;
    Ok(updated)
}

fn activity_stage_change(
    deal_id: Uuid,
    from: deal::Stage,
    to: deal::Stage,
    note: Option<String>,
    changed_by: Option<Uuid>,
    timestamp: DateTimeWithTimeZone,
) -> activity::ActiveModel {
    let subject = format!("Stage: {} -> {}", stage_str(from), stage_str(to));
    activity::ActiveModel {
        id: Set(Uuid::new_v4()),
        entity_type: Set("deal".to_string()),
        entity_id: Set(deal_id),
        kind: Set(activity::Kind::StageChange),
        subject: Set(Some(subject)),
        body_md: Set(note.clone()),
        meta_json: Set(json!({ "from": stage_str(from), "to": stage_str(to) })),
        created_at: Set(timestamp),
        created_by: Set(changed_by),
        updated_by: Set(None),
    }
}

fn stage_str(stage: deal::Stage) -> &'static str {
    match stage {
        deal::Stage::New => "NEW",
        deal::Stage::Qualify => "QUALIFY",
        deal::Stage::Proposal => "PROPOSAL",
        deal::Stage::Negotiate => "NEGOTIATE",
        deal::Stage::Won => "WON",
        deal::Stage::Lost => "LOST",
    }
}

fn database(ctx: &Context<'_>) -> async_graphql::Result<Arc<DatabaseConnection>> {
    ctx.data::<Arc<DatabaseConnection>>()
        .cloned()
        .map_err(|_| error_with_code("INTERNAL", "Missing database connection"))
}

fn auth_config(ctx: &Context<'_>) -> async_graphql::Result<Arc<AuthConfig>> {
    ctx.data::<Arc<AuthConfig>>()
        .cloned()
        .map_err(|_| error_with_code("INTERNAL", "Missing auth configuration"))
}

fn current_user(ctx: &Context<'_>) -> async_graphql::Result<CurrentUser> {
    ctx.data::<CurrentUser>()
        .cloned()
        .map_err(|_| error_with_code("UNAUTHENTICATED", "Login required"))
}

fn require_role(ctx: &Context<'_>, role: UserRole) -> async_graphql::Result<CurrentUser> {
    let user = current_user(ctx)?;
    if user.has_role(role) {
        Ok(user)
    } else {
        Err(error_with_code("FORBIDDEN", "Insufficient permissions"))
    }
}

fn require_viewer(ctx: &Context<'_>) -> async_graphql::Result<CurrentUser> {
    require_role(ctx, UserRole::Viewer)
}

fn parse_uuid(id: &ID) -> async_graphql::Result<Uuid> {
    Uuid::parse_str(id.as_str()).map_err(|_| error_with_code("BAD_REQUEST", "Invalid ID"))
}

fn db_error(err: DbErr) -> Error {
    error_with_code("INTERNAL", format!("Database error: {}", err))
}

fn error_with_code(code: &'static str, message: impl Into<String>) -> Error {
    Error::new(message).extend_with(|_, e| e.set("code", code))
}

#[derive(Debug, Clone)]
pub struct SeededCrmRecords {
    pub users: Vec<user::Model>,
    pub companies: Vec<company::Model>,
   pub contacts: Vec<contact::Model>,
   pub deals: Vec<deal::Model>,
}

impl SeededCrmRecords {
    pub fn user_email(&self, email: &str) -> Option<&user::Model> {
        self.users.iter().find(|u| u.email == email)
    }

    pub fn company_named(&self, name: &str) -> Option<&company::Model> {
        self.companies.iter().find(|c| c.name == name)
    }

    pub fn contact_email(&self, email: &str) -> Option<&contact::Model> {
        self.contacts.iter().find(|c| c.email == email)
    }

    pub fn deal_titled(&self, title: &str) -> Option<&deal::Model> {
        self.deals.iter().find(|d| d.title == title)
    }
}

pub async fn seed_crm_demo(db: &DatabaseConnection) -> Result<SeededCrmRecords, DbErr> {
    ensure_stage_meta_defaults(db).await?;
    let seeded_at: DateTimeWithTimeZone = Utc::now().into();
    let owner = insert_seed_user(
        db,
        "owner@sme.test",
        "Owner One",
        &[user_role::Role::Owner, user_role::Role::Admin],
        "ownerpass",
    )
    .await?;
    let admin = insert_seed_user(
        db,
        "admin@sme.test",
        "Admin Ada",
        &[user_role::Role::Admin],
        "adminpass",
    )
    .await?;
    let sales = insert_seed_user(
        db,
        "sales@sme.test",
        "Sales Sam",
        &[user_role::Role::Sales],
        "salespass",
    )
    .await?;
    let acme = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("ACME, Inc.".into()),
        website: Set(Some("https://acme.test".into())),
        phone: Set(Some("+1-555-0100".into())),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let fossrust = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("FossRust Labs".into()),
        website: Set(Some("https://fossrust.test".into())),
        phone: Set(Some("+1-555-0300".into())),
        assigned_user_id: Set(Some(admin.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let nuflights = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("NuFlights LLC".into()),
        website: Set(Some("https://nuflights.test".into())),
        phone: Set(Some("+1-555-0200".into())),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let ada = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("ada@acme.test".into()),
        first_name: Set(Some("Ada".into())),
        last_name: Set(Some("Lovelace".into())),
        phone: Set(Some("+1-555-0110".into())),
        company_id: Set(Some(acme.id)),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let charles = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("charles@acme.test".into()),
        first_name: Set(Some("Charles".into())),
        last_name: Set(Some("Babbage".into())),
        phone: Set(Some("+1-555-0111".into())),
        company_id: Set(Some(acme.id)),
        assigned_user_id: Set(Some(admin.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let linus = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("linus@fossrust.test".into()),
        first_name: Set(Some("Linus".into())),
        last_name: Set(Some("Torvalds".into())),
        phone: Set(Some("+1-555-0310".into())),
        company_id: Set(Some(fossrust.id)),
        assigned_user_id: Set(Some(admin.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let grace = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("grace@nuflights.test".into()),
        first_name: Set(Some("Grace".into())),
        last_name: Set(Some("Hopper".into())),
        phone: Set(Some("+1-555-0210".into())),
        company_id: Set(Some(nuflights.id)),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(seeded_at),
        updated_at: Set(seeded_at),
    }
    .insert(db)
    .await?;

    let acme_pilot = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("ACME Pilot".into()),
        amount_cents: Set(Some(120_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Qualify),
        close_date: Set(Some(naive_date(2025, 1, 10))),
        company_id: Set(acme.id),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2024, 12, 1)),
        updated_at: Set(timestamp(2024, 12, 15)),
    }
    .insert(db)
    .await?;

    let tooling = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Rust Tooling Upgrade".into()),
        amount_cents: Set(Some(75_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Proposal),
        close_date: Set(Some(naive_date(2025, 2, 15))),
        company_id: Set(fossrust.id),
        assigned_user_id: Set(Some(admin.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2024, 12, 5)),
        updated_at: Set(timestamp(2024, 12, 20)),
    }
    .insert(db)
    .await?;

    let renewal = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("NuFlights Annual".into()),
        amount_cents: Set(Some(210_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Qualify),
        close_date: Set(Some(naive_date(2025, 3, 5))),
        company_id: Set(nuflights.id),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2024, 12, 10)),
        updated_at: Set(timestamp(2025, 1, 3)),
    }
    .insert(db)
    .await?;

    let retainer = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("ACME Retainer".into()),
        amount_cents: Set(Some(60_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Negotiate),
        close_date: Set(Some(naive_date(2025, 2, 28))),
        company_id: Set(acme.id),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2024, 12, 12)),
        updated_at: Set(timestamp(2025, 1, 12)),
    }
    .insert(db)
    .await?;

    let expansion = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("FossRust Expansion".into()),
        amount_cents: Set(Some(95_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Won),
        close_date: Set(Some(naive_date(2025, 1, 20))),
        company_id: Set(fossrust.id),
        assigned_user_id: Set(Some(admin.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2024, 12, 15)),
        updated_at: Set(timestamp(2025, 1, 22)),
    }
    .insert(db)
    .await?;

    let quick_win = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Quick Win".into()),
        amount_cents: Set(Some(40_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Won),
        close_date: Set(Some(naive_date(2025, 2, 10))),
        company_id: Set(acme.id),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2025, 1, 5)),
        updated_at: Set(timestamp(2025, 2, 2)),
    }
    .insert(db)
    .await?;

    let lost_trial = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Stalled Trial".into()),
        amount_cents: Set(Some(25_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Lost),
        close_date: Set(Some(naive_date(2025, 1, 25))),
        company_id: Set(nuflights.id),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2024, 12, 18)),
        updated_at: Set(timestamp(2025, 1, 25)),
    }
    .insert(db)
    .await?;

    let fresh_prospect = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Fresh Prospect".into()),
        amount_cents: Set(Some(55_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::New),
        close_date: Set(Some(naive_date(2025, 3, 15))),
        company_id: Set(acme.id),
        assigned_user_id: Set(Some(sales.id)),
        created_by: Set(Some(owner.id)),
        updated_by: Set(Some(owner.id)),
        created_at: Set(timestamp(2025, 1, 20)),
        updated_at: Set(timestamp(2025, 1, 20)),
    }
    .insert(db)
    .await?;

    let won_histories = vec![
        deal_stage_history::ActiveModel {
            id: Set(Uuid::new_v4()),
            deal_id: Set(expansion.id),
            from_stage: Set(deal::Stage::Negotiate),
            to_stage: Set(deal::Stage::Won),
            changed_at: Set(timestamp(2025, 1, 22)),
            note: Set(Some("Signed master services.".into())),
            changed_by: Set(Some(owner.id.to_string())),
        },
        deal_stage_history::ActiveModel {
            id: Set(Uuid::new_v4()),
            deal_id: Set(quick_win.id),
            from_stage: Set(deal::Stage::Proposal),
            to_stage: Set(deal::Stage::Won),
            changed_at: Set(timestamp(2025, 2, 2)),
            note: Set(Some("Fast track approval.".into())),
            changed_by: Set(Some(owner.id.to_string())),
        },
    ];
    for history in won_histories {
        deal_stage_history::Entity::insert(history)
            .exec_without_returning(db)
            .await?;
    }

    Ok(SeededCrmRecords {
        users: vec![owner.clone(), admin.clone(), sales.clone()],
        companies: vec![acme, fossrust, nuflights],
        contacts: vec![ada, charles, linus, grace],
        deals: vec![
            acme_pilot.clone(),
            tooling.clone(),
            renewal.clone(),
            retainer.clone(),
            expansion.clone(),
            quick_win.clone(),
            lost_trial.clone(),
            fresh_prospect.clone(),
        ],
    })
}

async fn insert_seed_user(
    db: &DatabaseConnection,
    email: &str,
    display_name: &str,
    roles: &[user_role::Role],
    password: &str,
) -> Result<user::Model, DbErr> {
    let now: DateTimeWithTimeZone = Utc::now().into();
    let model = user::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(email.to_string()),
        display_name: Set(display_name.to_string()),
        avatar_url: Set(None),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    user_identity::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(model.id),
        provider: Set("local".into()),
        subject: Set(email.to_string()),
        created_at: Set(now),
    }
    .insert(db)
    .await?;
    user_secret::ActiveModel {
        user_id: Set(model.id),
        password_hash: Set(hash_password(password)?),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    for role in roles {
        user_role::ActiveModel {
            user_id: Set(model.id),
            role: Set(*role),
        }
        .insert(db)
        .await?;
    }
    Ok(model)
}

fn hash_password(password: &str) -> Result<String, DbErr> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| DbErr::Custom(format!("hash error: {}", err)))
}

const STAGE_META_DEFAULTS: [(&str, &str, i16, i16, bool, bool); 6] = [
    ("NEW", "New", 10, 10, false, false),
    ("QUALIFY", "Qualify", 20, 25, false, false),
    ("PROPOSAL", "Proposal", 30, 50, false, false),
    ("NEGOTIATE", "Negotiate", 40, 70, false, false),
    ("WON", "Won", 90, 100, true, false),
    ("LOST", "Lost", 95, 0, false, true),
];

async fn ensure_stage_meta_defaults(db: &DatabaseConnection) -> Result<(), DbErr> {
    let rows: Vec<stage_meta::ActiveModel> = STAGE_META_DEFAULTS
        .iter()
        .map(
            |(key, display, order, prob, is_won, is_lost)| stage_meta::ActiveModel {
                key: Set((*key).to_string()),
                display_name: Set((*display).to_string()),
                sort_order: Set(*order),
                probability: Set(*prob),
                is_won: Set(*is_won),
                is_lost: Set(*is_lost),
            },
        )
        .collect();
    stage_meta::Entity::insert_many(rows)
        .on_conflict(
            OnConflict::column(stage_meta::Column::Key)
                .do_nothing()
                .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}

fn naive_date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid seed date")
}

fn timestamp(year: i32, month: u32, day: u32) -> DateTimeWithTimeZone {
    Utc.with_ymd_and_hms(year, month, day, 12, 0, 0)
        .single()
        .expect("valid seed timestamp")
        .into()
}

/// Exposed for seeders/tests to drive the same transactional logic.
pub async fn move_deal_stage_service(
    db: &DatabaseConnection,
    deal_id: Uuid,
    stage: deal::Stage,
    note: Option<String>,
    changed_by: Option<Uuid>,
) -> Result<deal::Model, StageMoveError> {
    move_deal_stage_internal(db, deal_id, stage, note, changed_by).await
}

async fn create_task_internal(
    db: &DatabaseConnection,
    input: NewTaskInput,
    current: &CurrentUser,
) -> async_graphql::Result<task::Model> {
    let title = validate_task_title(&input.title)?;
    let notes_md = validate_notes_md(input.notes_md.clone())?;
    let assignee = validate_assignee(input.assignee.clone())?;
    let due_at = input.due_at.map(|d| d.into());
    let target = select_task_target(&input.company_id, &input.contact_id, &input.deal_id)?;
    ensure_task_target_exists(db, &target).await?;

    let task_id = Uuid::new_v4();
    let now: DateTimeWithTimeZone = Utc::now().into();
    let mut active = task::ActiveModel {
        id: Set(task_id),
        title: Set(title),
        notes_md: Set(notes_md),
        status: Set(task::Status::Open),
        priority: Set(task::Priority::from(input.priority)),
        assignee: Set(assignee),
        assigned_user_id: Set(None),
        due_at: Set(due_at),
        completed_at: Set(None),
        company_id: Set(None),
        contact_id: Set(None),
        deal_id: Set(None),
        created_by: Set(Some(current.user_id)),
        updated_by: Set(Some(current.user_id)),
        created_at: Set(now),
        updated_at: Set(now),
    };
    apply_target_to_model(&mut active, &target);
    task::Entity::insert(active)
        .exec_without_returning(db)
        .await
        .map_err(db_error)?;
    let record = task::Entity::find_by_id(task_id)
        .one(db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error_with_code("INTERNAL", "Failed to load inserted task"))?;
    Ok(record)
}

async fn update_task_internal(
    db: &DatabaseConnection,
    input: UpdateTaskInput,
    current: &CurrentUser,
) -> async_graphql::Result<task::Model> {
    let task_id = parse_uuid(&input.id)?;
    let existing = task::Entity::find_by_id(task_id)
        .one(db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error_with_code("NOT_FOUND", "Task not found"))?;
    let mut active: task::ActiveModel = existing.into();
    if let Some(title) = &input.title {
        active.title = Set(validate_task_title(title)?);
    }
    if input.notes_md.is_some() {
        active.notes_md = Set(validate_notes_md(input.notes_md.clone())?);
    }
    if let Some(priority) = input.priority {
        active.priority = Set(task::Priority::from(priority));
    }
    if input.assignee.is_some() {
        active.assignee = Set(validate_assignee(input.assignee.clone())?);
    }
    if let Some(due_at) = input.due_at {
        active.due_at = Set(Some(due_at.into()));
    }
    let now: DateTimeWithTimeZone = Utc::now().into();
    active.updated_at = Set(now);
    active.updated_by = Set(Some(current.user_id));
    let updated = active.update(db).await.map_err(db_error)?;
    Ok(updated)
}

async fn transition_task_status(
    db: &DatabaseConnection,
    existing: task::Model,
    next_status: task::Status,
    completed_at: Option<DateTimeWithTimeZone>,
    current: &CurrentUser,
) -> async_graphql::Result<task::Model> {
    if existing.status == next_status && existing.completed_at == completed_at {
        return Ok(existing);
    }
    let mut active: task::ActiveModel = existing.into();
    active.status = Set(next_status);
    active.completed_at = Set(completed_at);
    let now: DateTimeWithTimeZone = Utc::now().into();
    active.updated_at = Set(now);
    active.updated_by = Set(Some(current.user_id));
    let updated = active.update(db).await.map_err(db_error)?;
    Ok(updated)
}

fn default_search_kinds() -> Vec<CrmSearchKind> {
    vec![
        CrmSearchKind::Company,
        CrmSearchKind::Contact,
        CrmSearchKind::Deal,
    ]
}

fn validate_search_query(q: &str) -> async_graphql::Result<&str> {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return Err(error_with_code(
            "VALIDATION",
            "Search query cannot be empty",
        ));
    }
    Ok(trimmed)
}

fn enforce_search_limit(requested: i32, max: i32) -> async_graphql::Result<u64> {
    if requested > max {
        return Err(error_with_code(
            "LIMIT_EXCEEDED",
            format!("first cannot exceed {}", max),
        ));
    }
    Ok(requested.max(0) as u64)
}

fn order_by_ids<T, K, F>(ids: Vec<K>, records: Vec<T>, key: F) -> Vec<T>
where
    K: Eq + std::hash::Hash + Copy,
    T: Clone,
    F: Fn(&T) -> K,
{
    let mut map = HashMap::new();
    for record in records {
        map.insert(key(&record), record);
    }
    ids.into_iter()
        .filter_map(|id| map.get(&id).cloned())
        .collect()
}

#[derive(Debug, FromQueryResult)]
struct SearchHitRow {
    kind: String,
    id: Uuid,
    title: String,
    subtitle: Option<String>,
    score: f64,
    href: Option<String>,
}

impl TryFrom<SearchHitRow> for SearchHit {
    type Error = Error;

    fn try_from(row: SearchHitRow) -> Result<Self, Self::Error> {
        let kind = match row.kind.as_str() {
            "COMPANY" => CrmSearchKind::Company,
            "CONTACT" => CrmSearchKind::Contact,
            "DEAL" => CrmSearchKind::Deal,
            _ => return Err(error_with_code("INTERNAL", "Unknown search kind")),
        };
        Ok(SearchHit {
            kind,
            id: ID::from(row.id.to_string()),
            title: row.title,
            subtitle: row.subtitle,
            score: row.score,
            href: row.href,
        })
    }
}

async fn search_hits(
    db: &DatabaseConnection,
    q: &str,
    kinds: &[CrmSearchKind],
    limit: u64,
    offset: u64,
) -> async_graphql::Result<Vec<SearchHit>> {
    let allow_company = kinds.iter().any(|k| *k == CrmSearchKind::Company);
    let allow_contact = kinds.iter().any(|k| *k == CrmSearchKind::Contact);
    let allow_deal = kinds.iter().any(|k| *k == CrmSearchKind::Deal);
    if !allow_company && !allow_contact && !allow_deal {
        return Ok(vec![]);
    }
    let use_fts = q.len() >= 2 && has_tsquery_terms(db, q).await?;
    if use_fts {
        run_fts_search(
            db,
            q,
            allow_company,
            allow_contact,
            allow_deal,
            limit,
            offset,
        )
        .await
    } else {
        run_trgm_search(
            db,
            q,
            allow_company,
            allow_contact,
            allow_deal,
            limit,
            offset,
        )
        .await
    }
}

async fn has_tsquery_terms(db: &DatabaseConnection, q: &str) -> async_graphql::Result<bool> {
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT length(websearch_to_tsquery('simple', $1)::text) > 0 AS has_terms",
        vec![q.to_owned().into()],
    );
    let row = db.query_one(stmt).await.map_err(db_error)?;
    Ok(row
        .and_then(|r| r.try_get("", "has_terms").ok())
        .unwrap_or(false))
}

async fn run_fts_search(
    db: &DatabaseConnection,
    q: &str,
    allow_company: bool,
    allow_contact: bool,
    allow_deal: bool,
    limit: u64,
    offset: u64,
) -> async_graphql::Result<Vec<SearchHit>> {
    let mut selects: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    if allow_company {
        selects.push(
            "SELECT 'COMPANY' AS kind, id, name AS title, website AS subtitle,\
             LEAST(1.0, ts_rank_cd(tsv, websearch_to_tsquery('simple', ?), ARRAY[1.0,0.4,0.2,0.1])) AS score,\
             '/crm/company/' || id::text AS href\
             FROM company\
             WHERE tsv @@ websearch_to_tsquery('simple', ?)"
                .to_string(),
        );
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
    }
    if allow_contact {
        selects.push(
            "SELECT 'CONTACT' AS kind, contact.id,\
             COALESCE(NULLIF(trim(coalesce(contact.first_name, '') || ' ' || coalesce(contact.last_name, '')), ''), contact.email) AS title,\
             companies.name AS subtitle,\
             LEAST(1.0, ts_rank_cd(contact.tsv, websearch_to_tsquery('simple', ?), ARRAY[1.0,0.4,0.2,0.1])) AS score,\
             '/crm/contact/' || contact.id::text AS href\
             FROM contact\
             LEFT JOIN company AS companies ON companies.id = contact.company_id\
             WHERE contact.tsv @@ websearch_to_tsquery('simple', ?)"
                .to_string(),
        );
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
    }
    if allow_deal {
        selects.push(
            "SELECT 'DEAL' AS kind, deal.id, deal.title AS title, companies.name AS subtitle,\
             LEAST(1.0, ts_rank_cd(deal.tsv, websearch_to_tsquery('simple', ?), ARRAY[1.0,0.4,0.2,0.1])) AS score,\
             '/crm/deal/' || deal.id::text AS href\
             FROM deal\
             LEFT JOIN company AS companies ON companies.id = deal.company_id\
             WHERE deal.tsv @@ websearch_to_tsquery('simple', ?)"
                .to_string(),
        );
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
    }
    if selects.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!(
        "{} ORDER BY score DESC, title ASC LIMIT {} OFFSET {}",
        selects.join(" UNION ALL "),
        limit,
        offset
    );
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    let rows = SearchHitRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| SearchHit::try_from(row).ok())
        .collect())
}

async fn run_trgm_search(
    db: &DatabaseConnection,
    q: &str,
    allow_company: bool,
    allow_contact: bool,
    allow_deal: bool,
    limit: u64,
    offset: u64,
) -> async_graphql::Result<Vec<SearchHit>> {
    let mut selects: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    let pattern = format!("%{}%", q);
    if allow_company {
        selects.push(
            "SELECT 'COMPANY' AS kind, id, name AS title, website AS subtitle,\
             LEAST(1.0, GREATEST(similarity(name, ?), similarity(coalesce(website, ''), ?))) AS score,\
             '/crm/company/' || id::text AS href\
             FROM company\
             WHERE name % ? OR name ILIKE ? OR website ILIKE ?"
                .to_string(),
        );
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(pattern.clone().into());
        values.push(pattern.clone().into());
    }
    if allow_contact {
        selects.push(
            "SELECT 'CONTACT' AS kind, contact.id,\
             COALESCE(NULLIF(trim(coalesce(contact.first_name, '') || ' ' || coalesce(contact.last_name, '')), ''), contact.email) AS title,\
             companies.name AS subtitle,\
             LEAST(1.0, GREATEST(similarity(contact.email, ?), similarity(coalesce(contact.first_name, ''), ?), similarity(coalesce(contact.last_name, ''), ?))) AS score,\
             '/crm/contact/' || contact.id::text AS href\
             FROM contact\
             LEFT JOIN company AS companies ON companies.id = contact.company_id\
             WHERE contact.email % ? OR contact.first_name % ? OR contact.last_name % ?\
             OR contact.email ILIKE ? OR contact.first_name ILIKE ? OR contact.last_name ILIKE ?"
                .to_string(),
        );
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(pattern.clone().into());
        values.push(pattern.clone().into());
        values.push(pattern.clone().into());
    }
    if allow_deal {
        selects.push(
            "SELECT 'DEAL' AS kind, deal.id, deal.title AS title, companies.name AS subtitle,\
             LEAST(1.0, similarity(deal.title, ?)) AS score,\
             '/crm/deal/' || deal.id::text AS href\
             FROM deal\
             LEFT JOIN company AS companies ON companies.id = deal.company_id\
             WHERE deal.title % ? OR deal.title ILIKE ?"
                .to_string(),
        );
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
        values.push(pattern.clone().into());
    }
    if selects.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!(
        "{} ORDER BY score DESC, title ASC LIMIT {} OFFSET {}",
        selects.join(" UNION ALL "),
        limit,
        offset
    );
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    let rows = SearchHitRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| SearchHit::try_from(row).ok())
        .collect())
}

async fn load_stage_meta(db: &DatabaseConnection) -> async_graphql::Result<Vec<stage_meta::Model>> {
    stage_meta::Entity::find()
        .order_by_asc(stage_meta::Column::SortOrder)
        .all(db)
        .await
        .map_err(db_error)
}

fn sanitize_optional_filter(value: Option<String>) -> Option<String> {
    value.and_then(|input| {
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn select_stage_sequence(
    stages: &[stage_meta::Model],
    requested: Option<&Vec<String>>,
) -> async_graphql::Result<Vec<stage_meta::Model>> {
    let Some(keys) = requested else {
        return Ok(stages.to_vec());
    };
    if keys.is_empty() {
        return Err(validation_error(
            "stageKeys must contain at least one value",
        ));
    }
    let mut requested_set = HashSet::new();
    for key in keys.iter() {
        let normalized = normalize_stage_key(key)
            .ok_or_else(|| validation_error("stageKeys cannot contain blank values"))?;
        requested_set.insert(normalized);
    }
    if requested_set.is_empty() {
        return Err(validation_error("stageKeys cannot contain blank values"));
    }
    let available: HashSet<String> = stages
        .iter()
        .map(|stage| stage.key.to_uppercase())
        .collect();
    for key in requested_set.iter() {
        if !available.contains(key) {
            return Err(validation_error(format!("Unknown stage key {}", key)));
        }
    }
    let filtered = stages
        .iter()
        .filter(|stage| requested_set.contains(&stage.key.to_uppercase()))
        .cloned()
        .collect();
    Ok(filtered)
}

fn normalize_stage_key(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_uppercase())
    }
}

fn deal_filter_clauses(company_id: Option<Uuid>, q: Option<&str>) -> (Vec<String>, Vec<Value>) {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    if let Some(uuid) = company_id {
        clauses.push("d.company_id = ?".to_string());
        values.push(uuid.into());
    }
    if let Some(term) = q {
        clauses.push("d.title ILIKE ?".to_string());
        values.push(format!("%{}%", term).into());
    }
    (clauses, values)
}

fn where_clause(clauses: &[String]) -> String {
    if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    }
}

#[derive(Debug, FromQueryResult)]
struct StageAggregateRow {
    stage_key: String,
    total_count: i64,
    total_amount_cents: i64,
    total_expected_cents: i64,
}

async fn query_pipeline_stage_totals(
    db: &DatabaseConnection,
    company_id: Option<Uuid>,
    q: Option<&str>,
) -> async_graphql::Result<Vec<StageAggregateRow>> {
    let (clauses, values) = deal_filter_clauses(company_id, q);
    let where_sql = where_clause(&clauses);
    let sql = format!(
        "SELECT d.stage::text AS stage_key, COUNT(*) AS total_count,\
         COALESCE(SUM(COALESCE(d.amount_cents, 0)), 0) AS total_amount_cents,\
         COALESCE(SUM(((COALESCE(d.amount_cents, 0)::bigint) * sm.probability::bigint) / 100), 0) AS total_expected_cents\
         FROM deal d\
         JOIN stage_meta sm ON sm.key = d.stage::text\
         {where_sql}\
         GROUP BY d.stage"
    );
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    StageAggregateRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)
}

#[derive(Debug, FromQueryResult)]
struct PipelineDealRow {
    id: Uuid,
    title: String,
    amount_cents: Option<i64>,
    currency: Option<String>,
    stage_key: String,
    company_id: Uuid,
    company_name: Option<String>,
    expected_close: Option<NaiveDate>,
    updated_at: DateTimeWithTimeZone,
}

async fn query_stage_deals(
    db: &DatabaseConnection,
    stage_key: &str,
    company_id: Option<Uuid>,
    q: Option<&str>,
    order_by_updated: bool,
    limit: u64,
) -> async_graphql::Result<Vec<PipelineDeal>> {
    let (clauses, mut values) = deal_filter_clauses(company_id, q);
    let mut sql = String::from(
        "SELECT d.id, d.title, d.amount_cents, d.currency,\
         d.stage::text AS stage_key, d.company_id, c.name AS company_name,\
         d.close_date AS expected_close, d.updated_at\
         FROM deal d\
         JOIN company c ON c.id = d.company_id\
         WHERE d.stage = ?::deal_stage",
    );
    values.insert(0, stage_key.to_string().into());
    if !clauses.is_empty() {
        sql.push_str(" AND ");
        sql.push_str(&clauses.join(" AND "));
    }
    let order_col = if order_by_updated {
        "d.updated_at"
    } else {
        "d.created_at"
    };
    sql.push_str(&format!(" ORDER BY {order_col} DESC LIMIT {}", limit));
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    let rows = PipelineDealRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)?;
    Ok(rows.into_iter().map(map_pipeline_deal).collect())
}

fn map_pipeline_deal(row: PipelineDealRow) -> PipelineDeal {
    PipelineDeal {
        id: ID::from(row.id.to_string()),
        title: row.title,
        amount_cents: row.amount_cents,
        currency: row.currency,
        stage_key: row.stage_key,
        company_id: ID::from(row.company_id.to_string()),
        company_name: row.company_name,
        expected_close: row.expected_close,
        updated_at: row.updated_at.into(),
    }
}

#[derive(Debug, FromQueryResult)]
struct StageReportRow {
    stage_key: String,
    total_count: i64,
    amount_cents: i64,
    expected_cents: i64,
}

async fn query_report_stage_totals(
    db: &DatabaseConnection,
    range: &DateRange,
    include_lost: bool,
) -> async_graphql::Result<Vec<StageReportRow>> {
    let mut clauses = vec!["d.close_date BETWEEN ?::date AND ?::date".to_string()];
    if !include_lost {
        clauses.push("sm.is_lost = false".to_string());
    }
    let where_sql = where_clause(&clauses);
    let sql = format!(
        "SELECT d.stage::text AS stage_key, COUNT(*) AS total_count,\
         COALESCE(SUM(COALESCE(d.amount_cents, 0)), 0) AS amount_cents,\
         COALESCE(SUM(((COALESCE(d.amount_cents, 0)::bigint) * sm.probability::bigint) / 100), 0) AS expected_cents\
         FROM deal d\
         JOIN stage_meta sm ON sm.key = d.stage::text\
         {where_sql}\
         GROUP BY d.stage",
    );
    let values = vec![range.from.to_string().into(), range.to.to_string().into()];
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    StageReportRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)
}

#[derive(Debug, FromQueryResult)]
struct ForecastAggregateRow {
    period: String,
    amount_cents: i64,
    expected_cents: i64,
    deals: i64,
}

async fn query_forecast_points(
    db: &DatabaseConnection,
    range: &DateRange,
    include_lost: bool,
) -> async_graphql::Result<Vec<ForecastAggregateRow>> {
    let mut clauses = vec!["d.close_date BETWEEN ?::date AND ?::date".to_string()];
    if !include_lost {
        clauses.push("sm.is_lost = false".to_string());
    }
    let where_sql = where_clause(&clauses);
    let sql = format!(
        "SELECT to_char(date_trunc('month', d.close_date::timestamp), 'YYYY-MM') AS period,\
         COALESCE(SUM(COALESCE(d.amount_cents, 0)), 0) AS amount_cents,\
         COALESCE(SUM(((COALESCE(d.amount_cents, 0)::bigint) * sm.probability::bigint) / 100), 0) AS expected_cents,\
         COUNT(*) AS deals\
         FROM deal d\
         JOIN stage_meta sm ON sm.key = d.stage::text\
         {where_sql}\
         GROUP BY period\
         ORDER BY period",
    );
    let values = vec![range.from.to_string().into(), range.to.to_string().into()];
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    ForecastAggregateRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)
}

fn build_forecast_points(range: &DateRange, rows: Vec<ForecastAggregateRow>) -> Vec<ForecastPoint> {
    let mut map: HashMap<String, ForecastAggregateRow> = HashMap::new();
    for row in rows {
        map.insert(row.period.clone(), row);
    }
    enumerate_months(range)
        .into_iter()
        .map(|month| {
            let key = format!("{:04}-{:02}", month.year(), month.month());
            if let Some(row) = map.get(&key) {
                ForecastPoint {
                    period: key,
                    amount_cents: Some(row.amount_cents),
                    expected_cents: Some(row.expected_cents),
                    deals: row.deals as i32,
                }
            } else {
                ForecastPoint {
                    period: key,
                    amount_cents: Some(0),
                    expected_cents: Some(0),
                    deals: 0,
                }
            }
        })
        .collect()
}

fn enumerate_months(range: &DateRange) -> Vec<NaiveDate> {
    let mut cursor = NaiveDate::from_ymd_opt(range.from.year(), range.from.month(), 1)
        .expect("valid start month");
    let end =
        NaiveDate::from_ymd_opt(range.to.year(), range.to.month(), 1).expect("valid end month");
    let mut months = Vec::new();
    while cursor <= end {
        months.push(cursor);
        cursor = next_month(cursor);
    }
    months
}

fn next_month(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1).expect("valid next month")
}

#[derive(Debug, FromQueryResult)]
struct VelocityRow {
    created_at: DateTimeWithTimeZone,
    won_at: DateTimeWithTimeZone,
}

async fn query_velocity_rows(
    db: &DatabaseConnection,
    range: &DateRange,
) -> async_graphql::Result<Vec<VelocityRow>> {
    let sql = "WITH won AS (
            SELECT deal_id, MIN(changed_at) AS won_at
            FROM deal_stage_history
            WHERE to_stage = 'WON'
            GROUP BY deal_id
        )
        SELECT d.created_at, won.won_at
        FROM won
        JOIN deal d ON d.id = won.deal_id
        WHERE won.won_at::date BETWEEN ?::date AND ?::date";
    let values = vec![range.from.to_string().into(), range.to.to_string().into()];
    let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values);
    VelocityRow::find_by_statement(stmt)
        .all(db)
        .await
        .map_err(db_error)
}

fn compute_velocity_stats(rows: Vec<VelocityRow>) -> VelocityStats {
    if rows.is_empty() {
        return VelocityStats::default();
    }
    let mut durations: Vec<f64> = rows
        .into_iter()
        .map(|row| {
            let delta = row.won_at - row.created_at;
            delta.num_seconds() as f64 / 86_400.0
        })
        .collect();
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let avg = durations.iter().sum::<f64>() / durations.len() as f64;
    VelocityStats {
        deals_won: durations.len() as i32,
        avg_days_to_win: avg,
        p50_days_to_win: percentile(&durations, 0.5),
        p90_days_to_win: percentile(&durations, 0.9),
    }
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let clamped = percentile.clamp(0.0, 1.0);
    let rank = (clamped * values.len() as f64).ceil().max(1.0) as usize - 1;
    let idx = rank.min(values.len() - 1);
    values[idx]
}

fn append_session_cookie(ctx: &Context<'_>, token: &str, ttl_minutes: i64) {
    let max_age = (ttl_minutes.max(0) * 60).to_string();
    let cookie = if ttl_minutes < 0 {
        format!(
            "{}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax",
            SESSION_COOKIE
        )
    } else {
        format!(
            "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Lax",
            SESSION_COOKIE, token, max_age
        )
    };
    ctx.append_http_header("Set-Cookie", cookie);
}

fn normalize_email(value: &str) -> async_graphql::Result<String> {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() || !trimmed.contains('@') {
        return Err(validation_error("Invalid email address"));
    }
    Ok(trimmed)
}

fn validate_display_name(value: &str) -> async_graphql::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(validation_error("displayName is required"));
    }
    if trimmed.chars().count() > 100 {
        return Err(validation_error("displayName must be <= 100 characters"));
    }
    Ok(trimmed.to_string())
}

fn parse_roles(values: &[String]) -> async_graphql::Result<Vec<UserRole>> {
    let mut roles = Vec::new();
    for value in values {
        let upper = value.trim().to_uppercase();
        let role = UserRole::from_str(&upper)
            .ok_or_else(|| validation_error(format!("Unknown role {}", value)))?;
        if !roles.iter().any(|r| r.as_str() == role.as_str()) {
            roles.push(role);
        }
    }
    Ok(roles)
}

async fn load_roles(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> async_graphql::Result<Vec<UserRole>> {
    let rows = user_role::Entity::find()
        .filter(user_role::Column::UserId.eq(user_id))
        .all(db)
        .await
        .map_err(db_error)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| UserRole::from_str(row.role.to_string().as_str()))
        .collect())
}

async fn load_roles_for_users(
    db: &DatabaseConnection,
    users: &[user::Model],
) -> async_graphql::Result<HashMap<Uuid, Vec<UserRole>>> {
    let ids: Vec<Uuid> = users.iter().map(|u| u.id).collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = user_role::Entity::find()
        .filter(user_role::Column::UserId.is_in(ids.clone()))
        .all(db)
        .await
        .map_err(db_error)?;
    let mut map: HashMap<Uuid, Vec<UserRole>> = HashMap::new();
    for row in rows {
        if let Some(role) = UserRole::from_str(row.role.to_string().as_str()) {
            map.entry(row.user_id).or_default().push(role);
        }
    }
    Ok(map)
}

async fn insert_roles<C>(
    conn: &C,
    user_id: Uuid,
    roles: &[UserRole],
) -> async_graphql::Result<()>
where
    C: ConnectionTrait,
{
    for role in roles {
        user_role::ActiveModel {
            user_id: Set(user_id),
            role: Set(match role {
                UserRole::Owner => user_role::Role::Owner,
                UserRole::Admin => user_role::Role::Admin,
                UserRole::Sales => user_role::Role::Sales,
                UserRole::Viewer => user_role::Role::Viewer,
            }),
        }
        .insert(conn)
        .await
        .map_err(db_error)?;
    }
    Ok(())
}

async fn load_user_with_roles(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> async_graphql::Result<(user::Model, Vec<UserRole>)> {
    let model = user::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error_with_code("UNAUTHENTICATED", "User not found"))?;
    if !model.is_active {
        return Err(error_with_code("FORBIDDEN", "Account disabled"));
    }
    let roles = load_roles(db, user_id).await?;
    Ok((model, roles))
}

async fn ensure_active_user(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> async_graphql::Result<Uuid> {
    let user = user::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| error_with_code("NOT_FOUND", "User not found"))?;
    if !user.is_active {
        return Err(error_with_code("VALIDATION", "User is inactive"));
    }
    Ok(user_id)
}

fn validate_task_title(value: &str) -> async_graphql::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(validation_error("Title is required"));
    }
    validate_length("title", trimmed, 256)?;
    Ok(trimmed.to_string())
}

fn validate_notes_md(value: Option<String>) -> async_graphql::Result<Option<String>> {
    if let Some(ref notes) = value {
        validate_length("notesMd", notes, 65_535)?;
    }
    Ok(value)
}

fn validate_assignee(value: Option<String>) -> async_graphql::Result<Option<String>> {
    if let Some(ref assignee) = value {
        validate_length("assignee", assignee, 256)?;
    }
    Ok(value)
}

fn validate_length(field: &str, value: &str, max: usize) -> async_graphql::Result<()> {
    if value.chars().count() > max {
        return Err(validation_error(format!(
            "{} must be at most {} characters",
            field, max
        )));
    }
    Ok(())
}

fn enforce_task_limit(limit: i32) -> async_graphql::Result<u64> {
    if limit <= 0 {
        return Err(validation_error("first must be positive"));
    }
    if limit > MAX_TASKS_PAGE {
        return Err(error_with_code(
            "LIMIT_EXCEEDED",
            format!("Cannot request more than {} tasks at once", MAX_TASKS_PAGE),
        ));
    }
    Ok(limit as u64)
}

fn parse_optional_id(field: &str, value: &Option<ID>) -> async_graphql::Result<Option<Uuid>> {
    match value {
        Some(id) => Uuid::parse_str(id.as_str())
            .map(Some)
            .map_err(|_| validation_error(format!("Invalid {}", field))),
        None => Ok(None),
    }
}

#[derive(Clone, Copy, Debug)]
enum TaskTarget {
    Company(Uuid),
    Contact(Uuid),
    Deal(Uuid),
}

fn select_task_target(
    company_id: &Option<ID>,
    contact_id: &Option<ID>,
    deal_id: &Option<ID>,
) -> async_graphql::Result<TaskTarget> {
    let mut targets = Vec::new();
    if let Some(id) = company_id {
        let uuid =
            Uuid::parse_str(id.as_str()).map_err(|_| validation_error("Invalid companyId"))?;
        targets.push(TaskTarget::Company(uuid));
    }
    if let Some(id) = contact_id {
        let uuid =
            Uuid::parse_str(id.as_str()).map_err(|_| validation_error("Invalid contactId"))?;
        targets.push(TaskTarget::Contact(uuid));
    }
    if let Some(id) = deal_id {
        let uuid = Uuid::parse_str(id.as_str()).map_err(|_| validation_error("Invalid dealId"))?;
        targets.push(TaskTarget::Deal(uuid));
    }
    if targets.len() != 1 {
        return Err(validation_error(
            "Exactly one of companyId, contactId, or dealId must be provided",
        ));
    }
    Ok(targets[0])
}

async fn ensure_task_target_exists(
    db: &DatabaseConnection,
    target: &TaskTarget,
) -> async_graphql::Result<()> {
    let exists = match target {
        TaskTarget::Company(id) => company::Entity::find_by_id(*id)
            .one(db)
            .await
            .map_err(db_error)?
            .is_some(),
        TaskTarget::Contact(id) => contact::Entity::find_by_id(*id)
            .one(db)
            .await
            .map_err(db_error)?
            .is_some(),
        TaskTarget::Deal(id) => deal::Entity::find_by_id(*id)
            .one(db)
            .await
            .map_err(db_error)?
            .is_some(),
    };
    if !exists {
        return Err(validation_error("Target record not found"));
    }
    Ok(())
}

fn apply_target_to_model(model: &mut task::ActiveModel, target: &TaskTarget) {
    match target {
        TaskTarget::Company(id) => model.company_id = Set(Some(*id)),
        TaskTarget::Contact(id) => model.contact_id = Set(Some(*id)),
        TaskTarget::Deal(id) => model.deal_id = Set(Some(*id)),
    }
}

fn apply_task_ordering(mut query: Select<task::Entity>, order: TaskOrder) -> Select<task::Entity> {
    match order {
        TaskOrder::DueAsc => {
            query = query
                .order_by(due_nulls_expr(), Order::Asc)
                .order_by(task::Column::DueAt, Order::Asc)
                .order_by(task::Column::Id, Order::Asc);
        }
        TaskOrder::DueDesc => {
            query = query
                .order_by(due_nulls_expr(), Order::Asc)
                .order_by(task::Column::DueAt, Order::Desc)
                .order_by(task::Column::Id, Order::Asc);
        }
        TaskOrder::PriorityDesc => {
            query = query
                .order_by(priority_order_expr(), Order::Desc)
                .order_by(task::Column::Id, Order::Asc);
        }
        TaskOrder::UpdatedDesc => {
            query = query
                .order_by(task::Column::UpdatedAt, Order::Desc)
                .order_by(task::Column::Id, Order::Asc);
        }
    }
    query
}

fn priority_order_expr() -> SimpleExpr {
    Expr::cust(
        "CASE WHEN task.priority = 'HIGH' THEN 2 WHEN task.priority = 'MEDIUM' THEN 1 ELSE 0 END",
    )
}

fn due_nulls_expr() -> SimpleExpr {
    Expr::cust("CASE WHEN task.due_at IS NULL THEN 1 ELSE 0 END")
}

fn validation_error(message: impl Into<String>) -> Error {
    error_with_code("VALIDATION", message)
}
