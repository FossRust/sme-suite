use std::sync::Arc;

use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, Json, Object, Schema, SimpleObject,
    ID,
};
use chrono::{DateTime, NaiveDate, Utc};
use entity::{activity, deal, deal_stage_history};
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use serde_json::json;
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
