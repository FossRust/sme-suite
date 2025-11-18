use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
#[sea_orm(iden = "user")]
enum User {
    Table,
    Id,
    Email,
    DisplayName,
    AvatarUrl,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
#[sea_orm(iden = "user_identity")]
enum UserIdentity {
    Table,
    Id,
    UserId,
    Provider,
    Subject,
    CreatedAt,
}

#[derive(DeriveIden)]
#[sea_orm(iden = "user_role")]
enum UserRole {
    Table,
    UserId,
    Role,
}

#[derive(DeriveIden)]
#[sea_orm(iden = "user_secret")]
enum UserSecret {
    Table,
    UserId,
    PasswordHash,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Company {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden)]
enum Contact {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden)]
enum Deal {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden)]
enum Task {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden)]
enum Activity {
    Table,
    CreatedBy,
    UpdatedBy,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(User::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(User::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(
                        ColumnDef::new(User::Email)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(User::DisplayName).string().not_null())
                    .col(ColumnDef::new(User::AvatarUrl).string())
                    .col(
                        ColumnDef::new(User::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(User::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .col(
                        ColumnDef::new(User::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserIdentity::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserIdentity::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(
                        ColumnDef::new(UserIdentity::UserId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserIdentity::Provider)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserIdentity::Subject)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserIdentity::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .index(
                        Index::create()
                            .name("idx_user_identity_provider_subject")
                            .col(UserIdentity::Provider)
                            .col(UserIdentity::Subject)
                            .unique(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_user_identity_user")
                    .from(UserIdentity::Table, UserIdentity::UserId)
                    .to(User::Table, User::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserRole::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserRole::UserId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserRole::Role)
                            .string()
                            .not_null(),
                    )
                    .index(
                        Index::create()
                            .name("pk_user_role")
                            .col(UserRole::UserId)
                            .col(UserRole::Role)
                            .unique(),
                    )
                    .check(Expr::cust(
                        "(role IN ('OWNER','ADMIN','SALES','VIEWER'))",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_user_role_user")
                    .from(UserRole::Table, UserRole::UserId)
                    .to(User::Table, User::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserSecret::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserSecret::UserId)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(UserSecret::PasswordHash)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserSecret::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_user_secret_user")
                    .from(UserSecret::Table, UserSecret::UserId)
                    .to(User::Table, User::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        add_assignment_columns(
            manager,
            Company::Table,
            Company::AssignedUserId,
            "fk_company_assigned_user",
        )
        .await?;
        add_assignment_columns(
            manager,
            Contact::Table,
            Contact::AssignedUserId,
            "fk_contact_assigned_user",
        )
        .await?;
        add_assignment_columns(
            manager,
            Deal::Table,
            Deal::AssignedUserId,
            "fk_deal_assigned_user",
        )
        .await?;
        add_assignment_columns(
            manager,
            Task::Table,
            Task::AssignedUserId,
            "fk_task_assigned_user",
        )
        .await?;

        add_audit_columns(
            manager,
            Company::Table,
            Company::CreatedBy,
            Company::UpdatedBy,
            "fk_company_created_by",
            "fk_company_updated_by",
        )
        .await?;
        add_audit_columns(
            manager,
            Contact::Table,
            Contact::CreatedBy,
            Contact::UpdatedBy,
            "fk_contact_created_by",
            "fk_contact_updated_by",
        )
        .await?;
        add_audit_columns(
            manager,
            Deal::Table,
            Deal::CreatedBy,
            Deal::UpdatedBy,
            "fk_deal_created_by",
            "fk_deal_updated_by",
        )
        .await?;
        add_audit_columns(
            manager,
            Task::Table,
            Task::CreatedBy,
            Task::UpdatedBy,
            "fk_task_created_by",
            "fk_task_updated_by",
        )
        .await?;
        manager
            .exec_stmt(Statement::from_string(
                manager.get_database_backend(),
                "ALTER TABLE activity DROP COLUMN IF EXISTS created_by",
            ))
            .await?;
        add_audit_columns(
            manager,
            Activity::Table,
            Activity::CreatedBy,
            Activity::UpdatedBy,
            "fk_activity_created_by",
            "fk_activity_updated_by",
        )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_audit_columns(
            manager,
            Activity::Table,
            Activity::CreatedBy,
            Activity::UpdatedBy,
            "fk_activity_created_by",
            "fk_activity_updated_by",
        )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Activity::Table)
                    .add_column(
                        ColumnDef::new(Activity::CreatedBy)
                            .string_len(128),
                    )
                    .to_owned(),
            )
            .await?;
        drop_audit_columns(
            manager,
            Task::Table,
            Task::CreatedBy,
            Task::UpdatedBy,
            "fk_task_created_by",
            "fk_task_updated_by",
        )
        .await?;
        drop_audit_columns(
            manager,
            Deal::Table,
            Deal::CreatedBy,
            Deal::UpdatedBy,
            "fk_deal_created_by",
            "fk_deal_updated_by",
        )
        .await?;
        drop_audit_columns(
            manager,
            Contact::Table,
            Contact::CreatedBy,
            Contact::UpdatedBy,
            "fk_contact_created_by",
            "fk_contact_updated_by",
        )
        .await?;
        drop_audit_columns(
            manager,
            Company::Table,
            Company::CreatedBy,
            Company::UpdatedBy,
            "fk_company_created_by",
            "fk_company_updated_by",
        )
        .await?;

        drop_assignment_columns(
            manager,
            Task::Table,
            Task::AssignedUserId,
            "fk_task_assigned_user",
        )
        .await?;
        drop_assignment_columns(
            manager,
            Deal::Table,
            Deal::AssignedUserId,
            "fk_deal_assigned_user",
        )
        .await?;
        drop_assignment_columns(
            manager,
            Contact::Table,
            Contact::AssignedUserId,
            "fk_contact_assigned_user",
        )
        .await?;
        drop_assignment_columns(
            manager,
            Company::Table,
            Company::AssignedUserId,
            "fk_company_assigned_user",
        )
        .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_user_secret_user")
                    .table(UserSecret::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(UserSecret::Table).to_owned())
            .await?;
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_user_role_user")
                    .table(UserRole::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(UserRole::Table).to_owned())
            .await?;
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_user_identity_user")
                    .table(UserIdentity::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(UserIdentity::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(User::Table).to_owned())
            .await?;
        Ok(())
    }
}

async fn add_assignment_columns(
    manager: &SchemaManager,
    table: impl Iden + Copy,
    column: impl Iden,
    fk_name: &str,
) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(table)
                .add_column_if_not_exists(ColumnDef::new(column).uuid())
                .to_owned(),
        )
        .await?;
    manager
        .create_foreign_key(
            ForeignKey::create()
                .name(fk_name)
                .from(table, column)
                .to(User::Table, User::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await
}

async fn drop_assignment_columns(
    manager: &SchemaManager,
    table: impl Iden + Copy,
    column: impl Iden,
    fk_name: &str,
) -> Result<(), DbErr> {
    manager
        .drop_foreign_key(
            ForeignKey::drop()
                .name(fk_name)
                .table(table)
                .to_owned(),
        )
        .await?;
    manager
        .alter_table(
            Table::alter()
                .table(table)
                .drop_column(column)
                .to_owned(),
        )
        .await
}

async fn add_audit_columns(
    manager: &SchemaManager,
    table: impl Iden + Copy,
    created: impl Iden,
    updated: impl Iden,
    created_fk: &str,
    updated_fk: &str,
) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(table)
                .add_column_if_not_exists(ColumnDef::new(created).uuid())
                .add_column_if_not_exists(ColumnDef::new(updated).uuid())
                .to_owned(),
        )
        .await?;
    manager
        .create_foreign_key(
            ForeignKey::create()
                .name(created_fk)
                .from(table, created)
                .to(User::Table, User::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;
    manager
        .create_foreign_key(
            ForeignKey::create()
                .name(updated_fk)
                .from(table, updated)
                .to(User::Table, User::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await
}

async fn drop_audit_columns(
    manager: &SchemaManager,
    table: impl Iden + Copy,
    created: impl Iden,
    updated: impl Iden,
    created_fk: &str,
    updated_fk: &str,
) -> Result<(), DbErr> {
    manager
        .drop_foreign_key(
            ForeignKey::drop()
                .name(created_fk)
                .table(table)
                .to_owned(),
        )
        .await?;
    manager
        .drop_foreign_key(
            ForeignKey::drop()
                .name(updated_fk)
                .table(table)
                .to_owned(),
        )
        .await?;
    manager
        .alter_table(
            Table::alter()
                .table(table)
                .drop_column(created)
                .drop_column(updated)
                .to_owned(),
        )
        .await
}
