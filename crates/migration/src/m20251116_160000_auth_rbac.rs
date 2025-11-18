use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
#[sea_orm(iden = "app_user")]
enum AppUser {
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
enum UserIdentity {
    Table,
    Id,
    UserId,
    Provider,
    Subject,
    CreatedAt,
}

#[derive(DeriveIden)]
enum UserRole {
    Table,
    UserId,
    Role,
}

#[derive(DeriveIden)]
enum UserSecret {
    Table,
    UserId,
    PasswordHash,
    UpdatedAt,
}

#[derive(DeriveIden, Copy, Clone)]
enum Company {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden, Copy, Clone)]
enum Contact {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden, Copy, Clone)]
enum Deal {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[derive(DeriveIden, Copy, Clone)]
enum Task {
    Table,
    AssignedUserId,
    CreatedBy,
    UpdatedBy,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AppUser::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AppUser::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(
                        ColumnDef::new(AppUser::Email)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(AppUser::DisplayName).string().not_null())
                    .col(ColumnDef::new(AppUser::AvatarUrl).string())
                    .col(
                        ColumnDef::new(AppUser::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(AppUser::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .col(
                        ColumnDef::new(AppUser::UpdatedAt)
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
                    .col(ColumnDef::new(UserIdentity::UserId).uuid().not_null())
                    .col(ColumnDef::new(UserIdentity::Provider).string().not_null())
                    .col(ColumnDef::new(UserIdentity::Subject).string().not_null())
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
                    .name("fk_user_identity_app_user")
                    .from(UserIdentity::Table, UserIdentity::UserId)
                    .to(AppUser::Table, AppUser::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserRole::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(UserRole::UserId).uuid().not_null())
                    .col(ColumnDef::new(UserRole::Role).string().not_null())
                    .primary_key(
                        Index::create()
                            .name("pk_user_role")
                            .col(UserRole::UserId)
                            .col(UserRole::Role),
                    )
                    .check(Expr::cust("(role IN ('OWNER','ADMIN','SALES','VIEWER'))"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_user_role_app_user")
                    .from(UserRole::Table, UserRole::UserId)
                    .to(AppUser::Table, AppUser::Id)
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
                    .col(ColumnDef::new(UserSecret::PasswordHash).string().not_null())
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
                    .name("fk_user_secret_app_user")
                    .from(UserSecret::Table, UserSecret::UserId)
                    .to(AppUser::Table, AppUser::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        add_assignment_and_audit(
            manager,
            Company::Table,
            Company::AssignedUserId,
            Company::CreatedBy,
            Company::UpdatedBy,
            "fk_company_assigned_user",
            "fk_company_created_by",
            "fk_company_updated_by",
        )
        .await?;
        add_assignment_and_audit(
            manager,
            Contact::Table,
            Contact::AssignedUserId,
            Contact::CreatedBy,
            Contact::UpdatedBy,
            "fk_contact_assigned_user",
            "fk_contact_created_by",
            "fk_contact_updated_by",
        )
        .await?;
        add_assignment_and_audit(
            manager,
            Deal::Table,
            Deal::AssignedUserId,
            Deal::CreatedBy,
            Deal::UpdatedBy,
            "fk_deal_assigned_user",
            "fk_deal_created_by",
            "fk_deal_updated_by",
        )
        .await?;
        add_assignment_and_audit(
            manager,
            Task::Table,
            Task::AssignedUserId,
            Task::CreatedBy,
            Task::UpdatedBy,
            "fk_task_assigned_user",
            "fk_task_created_by",
            "fk_task_updated_by",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_assignment_and_audit(
            manager,
            Task::Table,
            Task::AssignedUserId,
            Task::CreatedBy,
            Task::UpdatedBy,
            "fk_task_assigned_user",
            "fk_task_created_by",
            "fk_task_updated_by",
        )
        .await?;
        drop_assignment_and_audit(
            manager,
            Deal::Table,
            Deal::AssignedUserId,
            Deal::CreatedBy,
            Deal::UpdatedBy,
            "fk_deal_assigned_user",
            "fk_deal_created_by",
            "fk_deal_updated_by",
        )
        .await?;
        drop_assignment_and_audit(
            manager,
            Contact::Table,
            Contact::AssignedUserId,
            Contact::CreatedBy,
            Contact::UpdatedBy,
            "fk_contact_assigned_user",
            "fk_contact_created_by",
            "fk_contact_updated_by",
        )
        .await?;
        drop_assignment_and_audit(
            manager,
            Company::Table,
            Company::AssignedUserId,
            Company::CreatedBy,
            Company::UpdatedBy,
            "fk_company_assigned_user",
            "fk_company_created_by",
            "fk_company_updated_by",
        )
        .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_user_secret_app_user")
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
                    .name("fk_user_role_app_user")
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
                    .name("fk_user_identity_app_user")
                    .table(UserIdentity::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(UserIdentity::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AppUser::Table).to_owned())
            .await?;
        Ok(())
    }
}

async fn add_assignment_and_audit<T, A, C, U>(
    manager: &SchemaManager<'_>,
    table: T,
    assignee: A,
    created_by: C,
    updated_by: U,
    fk_assignee: &str,
    fk_created: &str,
    fk_updated: &str,
) -> Result<(), DbErr>
where
    T: Iden + Copy + 'static,
    A: Iden + Clone + 'static,
    C: Iden + Clone + 'static,
    U: Iden + Clone + 'static,
{
    let assignee_col = assignee.clone();
    let created_col = created_by.clone();
    let updated_col = updated_by.clone();
    manager
        .alter_table(
            Table::alter()
                .table(table)
                .add_column_if_not_exists(ColumnDef::new(assignee_col).uuid())
                .add_column_if_not_exists(ColumnDef::new(created_col).uuid())
                .add_column_if_not_exists(ColumnDef::new(updated_col).uuid())
                .to_owned(),
        )
        .await?;
    let assignee_fk = assignee.clone();
    manager
        .create_foreign_key(
            ForeignKey::create()
                .name(fk_assignee)
                .from(table, assignee_fk)
                .to(AppUser::Table, AppUser::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;
    let created_fk = created_by.clone();
    manager
        .create_foreign_key(
            ForeignKey::create()
                .name(fk_created)
                .from(table, created_fk)
                .to(AppUser::Table, AppUser::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;
    let updated_fk = updated_by.clone();
    manager
        .create_foreign_key(
            ForeignKey::create()
                .name(fk_updated)
                .from(table, updated_fk)
                .to(AppUser::Table, AppUser::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await
}

async fn drop_assignment_and_audit<T, A, C, U>(
    manager: &SchemaManager<'_>,
    table: T,
    assignee: A,
    created_by: C,
    updated_by: U,
    fk_assignee: &str,
    fk_created: &str,
    fk_updated: &str,
) -> Result<(), DbErr>
where
    T: Iden + Copy + 'static,
    A: Iden + Clone + 'static,
    C: Iden + Clone + 'static,
    U: Iden + Clone + 'static,
{
    let assignee_col = assignee.clone();
    let created_col = created_by.clone();
    let updated_col = updated_by.clone();
    manager
        .drop_foreign_key(ForeignKey::drop().name(fk_assignee).table(table).to_owned())
        .await?;
    manager
        .drop_foreign_key(ForeignKey::drop().name(fk_created).table(table).to_owned())
        .await?;
    manager
        .drop_foreign_key(ForeignKey::drop().name(fk_updated).table(table).to_owned())
        .await?;
    manager
        .alter_table(
            Table::alter()
                .table(table)
                .drop_column(assignee_col)
                .drop_column(created_col)
                .drop_column(updated_col)
                .to_owned(),
        )
        .await
}
