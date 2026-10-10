# DataDog architecture

## Purpose

DataDog helps inspection teams:

1. organize project photos and inspector descriptions;
2. identify possible visible façade defects for human review; and
3. draft a report from verified project records.

It is an evidence assistant, not an autonomous inspector. AI observations are unverified candidates. The appointed Competent Person (CP) owns the assessment, recommendations, certification, and submission.

## Design rules

- Keep Telegram, future web apps, and future WhatsApp integrations as replaceable interfaces.
- Put business decisions in deterministic domain and application code. Keep network, database, filesystem, clock, and model calls behind explicit ports.
- A model may propose a typed action or observation; it never receives database, shell, filesystem, deployment, or messaging authority.
- Validate authorization, state transitions, identifiers, and output limits after model inference and before every write.
- Preserve original evidence, provenance, human corrections, and an append-only audit trail.
- Generate reports from a versioned, immutable project snapshot. Never generate them directly from chat history.
- Prefer small, honest functions: pure functions return values; functions with side effects say so in their name and signature.

## System shape

```text
Telegram adapter      Web adapter       WhatsApp adapter
       |                    |                   |
       +---------- normalized commands/events -+
                            |
                    Application services
       access | projects | evidence | analysis | reports
                            |
                  Domain policy and types
                            |
       +--------------------+--------------------+
       |                    |                    |
 Repository ports     Model/skill ports    Storage/report ports
       |                    |                    |
 Turso HTTP/libSQL    DeepSeek adapter      Object/filesystem storage
 SQLx SQLite          Local model adapter   DOCX report renderer
 Other database       Other API adapter     Message outbox
```

Dependencies point inward. Interface and infrastructure adapters may depend on application/domain code; domain code must not import Telegram, SQLx, DeepSeek, DOCX, or container concerns.

## Layers

### 1. Interface adapters

Each interface translates provider-specific input into shared application requests and renders shared results into provider-specific messages.

Examples:

- `ApplyForAccess`
- `CreateProject`
- `AddProjectMember`
- `AddElevation`
- `SubmitEvidence`
- `CorrectEvidenceAssignment`
- `ShowChecklist`
- `RequestAnalysis`
- `DraftReport`

Adapters authenticate the provider identity, enforce transport size/rate limits, download uploads to a quarantine area, and attach correlation IDs. They do not decide project access, classify evidence, or construct reports.

### 2. Application services

Application services coordinate one use case through ports. A service loads state, invokes pure domain rules, persists the result in a transaction, and emits durable events or outbox messages.

```rust
pub async fn submit_evidence(
    command: SubmitEvidence,
    repos: &impl Repositories,
    blobs: &impl BlobStore,
    clock: &impl Clock,
) -> Result<SubmitEvidenceResult>;
```

The pure part is separate and directly testable:

```rust
pub fn decide_evidence_assignment(
    project: &ProjectState,
    actor: &Actor,
    proposal: &AssignmentProposal,
) -> Result<Vec<DomainEvent>>;
```

Retries must be idempotent. Writes that produce user-visible messages use the transactional outbox pattern so state and delivery intent commit together.

### 3. Domain

The domain owns entities and policies: users, access requests, projects, memberships, elevations, findings, evidence, descriptions, checklist status, analysis reviews, report snapshots, and audit events.

It enforces:

- approved-user and project-membership access;
- lead/superuser privileges for team and project administration;
- evidence ownership and project boundaries;
- explicit evidence types and valid state transitions;
- separation of submitted facts, AI proposals, and human-verified findings;
- report readiness and required human review.

Checklist ticks mean that required evidence is present and assigned. They do not represent statutory coverage or engineering acceptance.

### 4. Infrastructure adapters

Infrastructure implements narrow ports:

- `UserRepository`, `ProjectRepository`, `EvidenceRepository`, `AuditRepository`;
- `UnitOfWork` for atomic transactions;
- `BlobStore` for source images, previews, annotations, and reports;
- `VisionModel` and `LanguageModel`;
- `ReportRenderer`;
- `MessagePublisher` and durable `Outbox`;
- `Clock`, `IdGenerator`, and `HashingService`.

Adapters return domain types rather than SQL rows or provider JSON.

## Complete user flow

### Access and team setup

1. A person opens an interface and receives the privacy notice and terms.
2. They actively consent and apply using their provider identity.
3. A superuser approves or declines the request; the decision is audited.
4. An approved user creates a project and becomes its lead.
5. The lead adds approved users to the project team. Membership is checked on every project operation.

### Organize evidence

1. A member selects a project and defines its elevations.
2. The system displays the versioned checklist: building overview, plan, each elevation overview, and each finding's context photo, close-up, and submitted description.
3. A member uploads a photo or description. The interface quarantines and validates it before permanent storage.
4. Rules or a model propose its project, elevation, finding, and evidence type.
5. Domain policy validates the proposal. Ambiguous input stays unassigned and produces a focused clarification request.
6. The source, hash, sender, timestamps, assignment source, corrections, and audit events are retained. Human corrections supersede model proposals without erasing history.

### Analyze photos

1. A worker reads a durable analysis job and builds the minimum necessary, authorized context.
2. Image preprocessing removes unnecessary metadata from the model copy, bounds dimensions, and rejects malformed files. The original remains unchanged.
3. The selected skill invokes a configured vision provider and requests schema-constrained output.
4. The output is parsed into typed observations, size-limited, sanitized, and checked against allowed defect vocabulary and confidence bounds.
5. Results are stored as `proposed`, linked to model/skill versions and evidence IDs.
6. An inspector or CP accepts, edits, or rejects each proposal. Only reviewed observations may be presented as confirmed project findings.

### Draft the report

1. An authorized member requests a report draft.
2. The application creates an immutable snapshot containing project particulars, checklist status, evidence references, descriptions, and reviewed/unreviewed observations with clear labels.
3. A versioned report skill validates required sections and renders DOCX/PDF through `ReportRenderer`.
4. The CP completes missing fields, confirms inspection coverage, makes the assessment and recommendations, and signs outside the AI workflow.
5. Every generated draft records its template, skill, source snapshot, and generation time.

## Model providers and skills

Use provider-neutral requests and typed responses:

```rust
#[async_trait]
pub trait VisionModel {
    async fn infer(&self, request: VisionRequest) -> Result<ModelResponse>;
}
```

The first adapter uses DeepSeek. Future adapters may use a local model or another API without changing application services. Provider configuration controls endpoint, model, timeout, retry policy, and data-handling mode; provider credentials never enter prompts or logs.

A **skill** is a versioned, testable package that defines:

- its purpose and permitted input fields;
- prompt/instructions and JSON schema;
- allowed tools (normally none);
- input/output limits and validation rules;
- examples and regression fixtures;
- quality checks, version, and changelog.

Initial skills:

- `evidence-classifier`: proposes an evidence assignment;
- `visible-defect-reviewer`: proposes photo-visible defects and required physical verification;
- `pfi-report-drafter`: validates a report snapshot and maps it to the current template.

Skills cannot bypass authorization or write state. Application code validates every result. Changing a provider must pass the same schema, safety, and regression suite before rollout.

## Persistence and portability

Use the `PersistentDatabase` port for all durable queries and transactions. The hosted adapter speaks Turso's HTTPS pipeline protocol; the local adapter uses SQLx SQLite. Keep backend types out of application and domain code.

- Store schema changes as ordered migrations.
- Use portable SQL where practical and document deliberate database-specific features.
- Keep transaction boundaries in `UnitOfWork`, not in domain functions.
- Use application-generated UUIDs and UTC timestamps.
- Store large binaries in `BlobStore`; store hashes, ownership, metadata, and object keys in the database.
- Add contract tests that run every repository implementation against the same behavior.
- Export projects as a documented, provider-neutral archive so database migration is not the only recovery path.

SQLite/libSQL consistency and transaction semantics must be verified before enabling remote replicas. Do not assume behavior matches PostgreSQL; a future PostgreSQL adapter should implement the same repository contracts and contract tests.

## Security boundaries

Treat all text, filenames, captions, documents, images, EXIF, model responses, and quoted prior messages as hostile.

- Normalize and length-limit text; escape it for Telegram, HTML, DOCX, logs, and filenames at the output boundary.
- Detect file types from content, allowlist formats, cap decoded dimensions/bytes, generate server-side names, and prevent path traversal and decompression bombs.
- Use parameterized SQL only. Re-authorize every resource by actor and project; never trust IDs supplied by a model.
- Do not place secrets, unrelated projects, full chat history, or unnecessary personal data in model context.
- Apply per-user and per-project rate, upload, analysis, and report quotas. Bound retries and queue depth.
- Separate source evidence, derived assets, and temporary quarantine storage. Encrypt transport and backups; restrict storage credentials by service and path.
- Log security and audit events without message bodies, photos, tokens, or provider secrets.
- Require explicit confirmation for high-impact operations such as access approval, role changes, archive/delete, exports, and final report release.

Run the service as an unprivileged user in a rootless or user-namespaced container with a read-only root filesystem, dropped Linux capabilities, `no-new-privileges`, seccomp/AppArmor, CPU/memory/PID limits, and a writable mount only for required temporary or local data. Never mount the Docker socket, host root, SSH keys, or deployment credentials. Bake dependencies into a pinned image, scan and sign it, and keep the host firewall closed to inbound application traffic when using Telegram polling.

## Deployment units

- **Interface service:** receives Telegram updates today; other adapters may be separate processes later.
- **Worker service:** downloads and validates evidence, runs skills, and renders reports from durable jobs.
- **Database:** Turso/libSQL initially, accessed only through repository adapters.
- **Blob storage:** durable evidence and report objects with separate backup and retention policies.

Use separate least-privilege credentials for services, health checks that do not expose data, structured metrics, alerting for failed queues and unusual usage, and tested database-plus-blob restore procedures. Deployment must support disabling model analysis while evidence collection continues.

## Suggested code boundaries

```text
src/
  domain/          # pure entities, policies, state transitions
  application/     # use cases and port traits
  skills/          # versioned schemas, validators, quality fixtures
  adapters/
    telegram/      # Telegram parsing and rendering
    database/      # PersistentDatabase port and Turso/SQLx SQLite adapters
    models/        # DeepSeek and local/provider adapters
    storage/       # blob/filesystem adapters
    reports/       # DOCX/PDF rendering
  workers/         # durable job consumers
  config/          # validated startup configuration
```

This is the target boundary for refactoring the current bot. Migration should be incremental: introduce domain types and ports around existing behavior, move Telegram rendering and SQLx queries into adapters, then move model prompts into versioned skills.
