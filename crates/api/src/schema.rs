use std::{collections::HashMap, sync::Arc};

use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, InputObject, Json, Object, Schema,
    SimpleObject, ID,
};
use chrono::{DateTime, NaiveDate, Utc};
use entity::{activity, company, contact, deal, deal_stage_history, task};
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::{Expr, Func, SimpleExpr};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, DbErr, EntityTrait, FromQueryResult, Order, QueryFilter, QueryOrder,
    QuerySelect, Select, Statement, TransactionTrait, Value,
};
use serde_json::json;
use tracing::info_span;
use uuid::Uuid;

pub struct AppSchema(pub Schema<QueryRoot, MutationRoot, EmptySubscription>);

pub fn build_schema(db: Arc<DatabaseConnection>) -> AppSchema {
    let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(db)
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
    async fn search(
        &self,
        ctx: &Context<'_>,
        q: String,
        kinds: Option<Vec<CrmSearchKind>>,
        first: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<Vec<SearchHit>> {
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
        let db = database(ctx)?;
        let task_id = parse_uuid(&id)?;
        let record = task::Entity::find_by_id(task_id)
            .one(db.as_ref())
            .await
            .map_err(db_error)?;
        Ok(record.map(TaskNode::from))
    }
}

#[Object]
impl CrmMutation {
    #[graphql(name = "moveDealStage")]
    async fn move_deal_stage(
        &self,
        ctx: &Context<'_>,
        id: ID,
        stage: DealStage,
        note: Option<String>,
    ) -> async_graphql::Result<DealNode> {
        let db = database(ctx)?;
        let deal_id = parse_uuid(&id)?;
        let target_stage: deal::Stage = stage.into();

        let model = move_deal_stage_internal(db.as_ref(), deal_id, target_stage, note, None)
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
        let task = create_task_internal(db.as_ref(), input).await?;
        Ok(task.into())
    }

    #[graphql(name = "updateTask")]
    async fn update_task(
        &self,
        ctx: &Context<'_>,
        input: UpdateTaskInput,
    ) -> async_graphql::Result<TaskNode> {
        let db = database(ctx)?;
        let task = update_task_internal(db.as_ref(), input).await?;
        Ok(task.into())
    }

    #[graphql(name = "completeTask")]
    async fn complete_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<TaskNode> {
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
        )
        .await?;
        Ok(task.into())
    }

    #[graphql(name = "cancelTask")]
    async fn cancel_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<TaskNode> {
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
        let task =
            transition_task_status(db.as_ref(), existing, task::Status::Cancelled, None).await?;
        Ok(task.into())
    }

    #[graphql(name = "reopenTask")]
    async fn reopen_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<TaskNode> {
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
        let task = transition_task_status(db.as_ref(), existing, task::Status::Open, None).await?;
        Ok(task.into())
    }

    #[graphql(name = "deleteTask")]
    async fn delete_task(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<bool> {
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
            due_at: model.due_at.map(|d| d.into()),
            completed_at: model.completed_at.map(|d| d.into()),
            company_id: model.company_id.map(|id| ID::from(id.to_string())),
            contact_id: model.contact_id.map(|id| ID::from(id.to_string())),
            deal_id: model.deal_id.map(|id| ID::from(id.to_string())),
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
    pub created_by: Option<String>,
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
            created_by: model.created_by,
        }
    }
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
    changed_by: Option<String>,
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
    active.stage = Set(stage);
    active.updated_at = Set(now.clone());
    let updated = active.update(&txn).await?;

    let history = deal_stage_history::ActiveModel {
        id: Set(Uuid::new_v4()),
        deal_id: Set(deal_id),
        from_stage: Set(from_stage),
        to_stage: Set(stage),
        changed_at: Set(now.clone()),
        note: Set(note.clone()),
        changed_by: Set(changed_by.clone()),
    };
    deal_stage_history::Entity::insert(history)
        .exec_without_returning(&txn)
        .await?;

    let activity = activity_stage_change(
        deal_id,
        from_stage,
        stage,
        note,
        changed_by.clone(),
        now.clone(),
    );
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
    changed_by: Option<String>,
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
        created_by: Set(changed_by.clone()),
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
    pub companies: Vec<company::Model>,
    pub contacts: Vec<contact::Model>,
    pub deals: Vec<deal::Model>,
}

impl SeededCrmRecords {
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
    let now: DateTimeWithTimeZone = Utc::now().into();
    let acme = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("ACME, Inc.".into()),
        website: Set(Some("https://acme.test".into())),
        phone: Set(Some("+1-555-0100".into())),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    let fossrust = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("FossRust Labs".into()),
        website: Set(Some("https://fossrust.test".into())),
        phone: Set(Some("+1-555-0300".into())),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    let nuflights = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("NuFlights LLC".into()),
        website: Set(Some("https://nuflights.test".into())),
        phone: Set(Some("+1-555-0200".into())),
        created_at: Set(now),
        updated_at: Set(now),
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
        created_at: Set(now),
        updated_at: Set(now),
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
        created_at: Set(now),
        updated_at: Set(now),
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
        created_at: Set(now),
        updated_at: Set(now),
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
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    let acme_pilot = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("ACME Pilot".into()),
        amount_cents: Set(Some(120_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::New),
        close_date: Set(None),
        company_id: Set(acme.id),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    let tooling = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Rust Tooling Upgrade".into()),
        amount_cents: Set(Some(75_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Proposal),
        close_date: Set(None),
        company_id: Set(fossrust.id),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    let renewal = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("NuFlights Annual".into()),
        amount_cents: Set(Some(210_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::Qualify),
        close_date: Set(None),
        company_id: Set(nuflights.id),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    if let Err(err) = move_deal_stage_service(
        db,
        acme_pilot.id,
        deal::Stage::Qualify,
        Some("Qualified via discovery".into()),
        Some("seed".into()),
    )
    .await
    {
        return Err(DbErr::Custom(format!(
            "seed stage change failed: {:?}",
            err
        )));
    }

    Ok(SeededCrmRecords {
        companies: vec![acme, fossrust, nuflights],
        contacts: vec![ada, charles, linus, grace],
        deals: vec![acme_pilot, tooling, renewal],
    })
}

/// Exposed for seeders/tests to drive the same transactional logic.
pub async fn move_deal_stage_service(
    db: &DatabaseConnection,
    deal_id: Uuid,
    stage: deal::Stage,
    note: Option<String>,
    changed_by: Option<String>,
) -> Result<deal::Model, StageMoveError> {
    move_deal_stage_internal(db, deal_id, stage, note, changed_by).await
}

async fn create_task_internal(
    db: &DatabaseConnection,
    input: NewTaskInput,
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
        due_at: Set(due_at),
        completed_at: Set(None),
        company_id: Set(None),
        contact_id: Set(None),
        deal_id: Set(None),
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
    active.updated_at = Set(Utc::now().into());
    let updated = active.update(db).await.map_err(db_error)?;
    Ok(updated)
}

async fn transition_task_status(
    db: &DatabaseConnection,
    existing: task::Model,
    next_status: task::Status,
    completed_at: Option<DateTimeWithTimeZone>,
) -> async_graphql::Result<task::Model> {
    if existing.status == next_status && existing.completed_at == completed_at {
        return Ok(existing);
    }
    let mut active: task::ActiveModel = existing.into();
    active.status = Set(next_status);
    active.completed_at = Set(completed_at);
    active.updated_at = Set(Utc::now().into());
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
