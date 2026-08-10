# Tenant migration runbook

This directory contains the T1 database-only migration for the tenant contract frozen
in `docs/adr/tenant-model.md` and `docs/adr/tenant-isolation.md`.

## Files and order

1. `V5__expand_tenant_model.sql` creates `tenant`, `tenant_member`,
   `tenant_invitation`, and `tenant_audit_log`. It adds nullable `tenant_id` columns
   and tenant-leading indexes to every resource table present in V1:
   `project`, `bid_document`, `audit_task`, `audit_issue`, `audit_report`,
   `audit_task_event`, `knowledge_file`, `knowledge_chunk`, `chat_message`,
   `document_parse_job`, and `rag_trigger_outbox`.
   When the optional trace schema exists, the same migration also adds nullable
   `tenant_id` and tenant-leading indexes to `trace_sessions`, `trace_events`, and
   `trace_event_blocks`.
2. `V6__backfill_tenant_data.sql` creates one stable personal tenant per `sys_user`,
   adds the OWNER membership, then backfills resources in parent-first order.
3. `tenant_migration_validation.sql` is a manual, read-only validation script. It is
   intentionally not named as a Flyway migration.
4. `tenant_migration_rollback.sql` is a manual rollback script for a disposable,
   pre-backfill environment only. It is intentionally not named as a Flyway migration.

The application configuration has Flyway disabled and `spring.sql.init.mode` explicitly
set to `never` in both the base and production profiles. `schema-locations` is not
configured. The legacy `audit_task_event.sql` and `trace_schema.sql` files are therefore
not startup inputs; deployment must apply V1 through V6 using the team's normal Flyway
runner or an explicit migration command. Do not enable Flyway in application code as
part of T1.

## Execution safety

V5 and V6 must be run in order by exactly one migration runner. Do not run them from
application startup, from concurrent runners, or while another schema/data migration is
active. Before starting the migration, take and verify a restorable database backup,
pause application writes and background workers/consumers, and keep the write pause in
place through validation.

Before V6, run the two read-only conflict-detection queries below. Each query must
return zero rows, and its immediately following `SHOW WARNINGS` must return no warning.
Any result or warning is a stop condition and must be resolved before V6 is started. Do
not treat an empty application log as a substitute for reviewing the database results.

At minimum, the preflight must return no rows for these checks:

```sql
SELECT t.id, t.tenant_code, t.owner_user_id, u.id AS user_id
FROM sys_user AS u
JOIN tenant AS t
  ON t.tenant_code = CONCAT('user-', u.id)
WHERE t.owner_user_id <> u.id;

SHOW WARNINGS;

SELECT t.id, t.tenant_code, tm.user_id, tm.role, tm.status
FROM sys_user AS u
JOIN tenant AS t
  ON t.tenant_code = CONCAT('user-', u.id)
JOIN tenant_member AS tm
  ON tm.tenant_id = t.id
 AND tm.user_id = u.id
WHERE tm.role <> 'OWNER'
   OR tm.status <> 'ACTIVE';

SHOW WARNINGS;
```

V5 changes the schema with DDL, so its index/column changes remain non-transactional.
Schedule V5 in a maintenance window and validate it in pre-production against the exact
MySQL version and representative data volume, including the expected index algorithm and
lock behavior, before production execution. Keep the backup and migration evidence with
the change record.

Published migration files are immutable once any environment may have executed them.
Do not rewrite V5 or V6 in place; a future repair or correction must be delivered as a
new, reviewed migration version under normal change control.

The `trace_schema.sql` tables are optional because they are not part of the V1 baseline.
V5 checks table existence before expanding them; V1's 11 required resource tables remain
unguarded and fail fast if missing. V6 conditionally backfills Trace rows through
`trace_sessions.task_id -> audit_task.task_id`, `trace_events.session_id ->
trace_sessions.id`, and `trace_event_blocks.event_id -> trace_events.event_id`.
Missing parents or unresolved parent tenants remain NULL. The validation script reports
Trace table presence, columns, indexes, NULL rows, and parent mismatches without failing
when the optional schema is absent. Provision `trace_schema.sql` before V5/V6 when it is
enabled; if it is introduced later, rerun the idempotent V5/V6 SQL under change control
before Contract.

## Idempotency and ownership rules

- V5 uses `CREATE TABLE IF NOT EXISTS`. Its temporary procedures check
  `information_schema` before adding each column or index, then remove themselves.
- V6 uses the unique `tenant_code` and `(tenant_id, user_id)` keys with `INSERT IGNORE`.
  Resource updates target the source row's primary key and require `tenant_id IS NULL`.
  Re-running V6 therefore does not duplicate tenants, members, or assignments and does
  not overwrite an existing assignment.
- Optional Trace backfill in V6 uses top-level conditional `PREPARE`/`EXECUTE` statements,
  keeps the parent-first chain and `tenant_id IS NULL` guards, and is safe to rerun without
  routine DDL. MySQL DDL is not transactional, so an interrupted V5 run or rollback can
  leave a partially expanded schema; use backup evidence and change control.
- `project.user_id` is the strongest V1 owner signal.
- `bid_document.upload_user_id` wins when it agrees with the resolved project owner;
  a missing uploader may inherit a resolved project owner. A conflict is left NULL.
- `audit_task.audit_user_id` wins when it agrees with the resolved bid/project owner;
  otherwise the resolved parent is used only when the explicit audit user is absent.
  Conflicts and unresolved parents remain NULL.
- `knowledge_file.upload_user_id` and `chat_message.user_id` map to the user's
  personal tenant. `audit_issue`, `audit_report`, `audit_task_event`,
  `knowledge_chunk`, `document_parse_job`, and `rag_trigger_outbox` inherit only from
  resolved parents. No random or guessed tenant is assigned.
- Every backfilled personal tenant and OWNER membership is ACTIVE so the tenant owner
  invariant remains true. `sys_user.status` is still an independent authentication
  gate; an inactive legacy user cannot establish an ordinary tenant context merely
  because this membership row is ACTIVE.
- `tenant_invitation` and `tenant_audit_log` are created empty. T1 does not fabricate
  invitation or audit history.

The logical idempotency key for every resource is `(source_table, source_id)`, where
`source_id` is the V1 primary key (or `task_id` for `audit_task_event` parent lookup).
The validation script is the migration isolation queue for rows that cannot be assigned
with confidence; those rows must be resolved or explicitly accepted before Contract.

## Validation

Run the read-only script after V6 against the same database, for example:

```text
mysql --database=smart_tender_system < tenant_migration_validation.sql
```

Review all result sets. In particular, before Enforce/Contract:

- all required columns and tenant-leading indexes exist;
- active users have an ACTIVE membership and an OWNER invariant is satisfied;
- null `tenant_id` rows are either zero or explicitly listed as unresolved;
- parent/child tenant mismatch results are empty;
- tenant-visible counts match the saved pre-migration evidence.

The validation script is read-only: it uses no routine DDL, creates no routines or other
database objects, and uses only top-level information_schema conditions plus prepared
SELECT statements. When Trace tables or their `tenant_id` columns are absent, the optional
result sets identify the condition as `skipped/absent` instead of referencing a missing
table.

## Rollback and application flags

T1 does not implement application dual-write, tenant filtering, `NOT NULL`, Contract, or
feature-flag wiring. The application owner must keep `tenant.enforce` and any tenant
gray rollout disabled until V6 and validation pass. For an incident after backfill,
disable those flags, pause async/Rust paths that could cross tenant boundaries, and keep
the Expand schema and data in place while reads use the verified compatibility path.

The SQL rollback file defaults to a deliberate failure. In the same client session, set
`@tenant_expand_rollback_confirmed = 'YES'` only after a backup and proof that no tenant
rows or non-NULL resource assignments exist; then source the file. It can drop the new
columns, indexes, and empty tenant tables only in that pre-backfill state. It must never
be used after dual-write, Enforce, or Contract has written tenant data. Contract rollback
requires a database restore drill and an approved schema change; it is not a direct
inverse of V5/V6.
