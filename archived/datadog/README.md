# DataDog inspection assistant

DataDog organizes Telegram inspection photos, records preliminary visible-condition observations, and compiles project records into a fillable PFI report draft. The appointed Competent Person (CP) must verify observations, complete missing technical and inspection fields, make condition assessments, and sign the statutory report.

## Run

From the repository root:

```sh
uv run --python 3.12 datadog/setup_model.py
# For a first installation only; preserve an existing configured .env.
cp datadog/.env.example datadog/.env
chmod 600 datadog/.env
# Set TELOXIDE_TOKEN and DEEPSEEK_API_KEY in that file.
cargo run --release --manifest-path datadog/Cargo.toml
```

For hosted storage, set `TURSO_DATABASE_URL` and `TURSO_AUTH_TOKEN` in
`datadog/.env`. Startup applies numbered migrations to Turso automatically.
Remove both variables to use `BOT_DATA_DIR/telebot.sqlite` locally. Never commit
the database token. Apply migrations without starting Telegram using:

```sh
cargo run --release --manifest-path datadog/Cargo.toml --bin migrate
```

Set `DATADOG_PRIVACY_URL` and `DATADOG_TERMS_URL` to the public website pages
shown during consent. New users must accept the current policy versions before
the bot processes project data.

Python is only used for the one-time ONNX export/runtime download. The running
bot, database access, Telegram handling and inference pipeline are Rust. Persistent metadata uses a backend-neutral database interface with Turso for hosted deployments and local SQLite as the development fallback.
DeepSeek defaults to `https://api.deepseek.com`, model `deepseek-flash` (V4.1
Flash), enabled thinking and `reasoning_effort: high`. Each relevant text message
or photo can incur an API request. Photo review receives a 1600-pixel preview,
its caption and focused project/sender/reply context. API usage and the returned
model identifier are retained; reasoning traces are not retained. The first report
request installs the small `python-docx` report dependency with `uv`.

Only one polling instance may use this Telegram token. DataDog holds an
exclusive lock in its data directory so a second local instance exits instead
of processing and delivering the same queue rows. Stop with Ctrl-C. Remove an
existing webhook before using polling. The bot runs while its local process and
computer are running and does not import old chat history.

For groups, disable privacy through BotFather and re-add the bot, or give it
admin access so Telegram delivers ordinary messages and photos. Channel posts
are supported when the bot has posting permission. Replies stay in the original
forum topic. Bot-generated messages are excluded from collection.

Say `stop listening` or `/pause` to make DataDog silently ignore new group
messages and photos. Say `start listening` or `/resume` to continue. Pausing
keeps the active project, evidence and checklist intact.

## Conversation

New people receive three short privacy and usage points. Tapping **OK, continue**
records consent and activates a normal user account immediately. No approval is
required. Superusers receive an informational registration notice. `@dkffjlrtm`
is the initial superuser. Superusers can use
`/me` to inspect their own role, plus `/users`, `/promote @username`, and
`/demote @username` to manage privileged accounts.

Every project has one lead and an explicit team. The Telegram group owner leads
group projects; the creator leads a private project. Normal users can only list, select, inspect or report
on projects where they are the lead or a team member; superusers can access all
projects. A project lead or superuser can manage the team by username.

Starting or selecting a project posts a fresh checklist and explains what to
send next. `show checklist` also sends a new checklist message, instead of
silently editing an older post.

```text
new project guys, Star Vista
add @inspector_username to the team
show project team
remove @inspector_username from the team
track East and West elevations
[photo] building overview
[image of drawing] plan drawing
[photo] East elevation overview
[photo] new finding, East elevation, level 3. Crack beside window.
[reply with another photo] close-up for F001
[reply to a saved photo] that is the context photo for F001
show the checklist
generate report
list projects
select Star Vista
rename Star Vista to The Star Vista
archive The Star Vista
restore The Star Vista
```

Location matching normalizes case, punctuation and whitespace. Project IDs are
UUIDs; normalized location keys and aliases prevent duplicate sites **within a
Telegram chat**. Similar names prompt clarification. To intentionally create a
separate similarly named site, use `new distinct location: FULL LOCATION`.
Project teams control access to a shared building record. Each chat/topic has an
active project and inspection, and each sender has separate finding and
clarification context. Changing projects clears the sender contexts.
Archive is reversible and retains evidence; permanent deletion is not exposed.

In a group, the Telegram group owner becomes project lead. Users who accept the
policies are added to that group’s active projects when they interact. Guest
accounts can contribute to assigned projects but cannot create private projects.

A project represents one building/location. Each visit or report cycle is an
inspection with its own stable number, optional report reference, evidence,
findings, workflow steps, and versioned report drafts. Use `list inspections`,
`start a new inspection`, or `select inspection R002`. Prior inspection metadata
and user-authored findings are supplied to analysis as explicitly historical
context; they never fill the current checklist.

If a photo cannot be assigned confidently, it stays unassigned and the bot asks
for its purpose, elevation or finding. Directions and levels are supplied by the
inspector rather than inferred from the photo. Reply to the original image or its
bot receipt/annotation to correct its assignment. Evidence IDs look like E0001;
finding IDs like F001 are scoped to their project.

## Exact checklist

| Scope | Required evidence |
| --- | --- |
| Project | Building overview photo |
| Project | Plan drawing image |
| Each named elevation | Elevation overview photo |
| Each finding | Context photo |
| Each finding | Close-up photo |
| Each finding | Text description |

The bot creates additional elevation/finding slots as they are identified. One
photo has one primary assignment, and extra photos can supplement that slot. A
caption can also provide separate description evidence. Descriptions quote the
submitted text, rather than generating engineering observations from images.

A tick indicates saved and assigned evidence. This evidence checklist does not
calculate PFI inspection coverage or physical crack measurements. Plan input
currently accepts JPEG, PNG or WebP images; PDF/CAD files are not imported.

## Storage and recovery

Paths are relative to `datadog/`, regardless of the launch directory:

```text
data/telebot.sqlite                 Local SQLite fallback (WAL)
data/evidence/E0001/source.jpg      Downloaded source (PNG/WebP also supported)
data/evidence/E0001/preview.jpg     Oriented image for DeepSeek
data/evidence/E0001/annotated.jpg   YOLO result
```

Set `BOT_DATA_DIR` to change this directory. `.env`, `data/`, `models/` and `run/`
are ignored by Git. Restrict access to the local evidence directory and back up
it together with a Turso backup/export. Local SQLite development copies should
be stopped before copying (or use SQLite's backup API); a live database may
also have WAL/SHM files.

The register retains Telegram source JSON with sender details, source/receipt
timestamps, chat/topic/message/album IDs, captions, reply relationships and file
identifiers. Image records include filenames, actual MIME types, dimensions,
byte counts, SHA-256 hashes, available EXIF fields and storage paths. It also
retains assignments, descriptions, model/usage metadata, YOLO boxes and change
history. YOLO coordinates refer to the annotated image's recorded dimensions.
An undecodable downloaded source is retained as `source.bin`.

Telegram photos can be compressed; sending an image **as a file** preserves the
supplied file. Download limit: 20 MB. Album photos each retain their own evidence
record and annotation. Checklist updates are debounced/coalesced into one
editable summary message; receipts remain per photo. Large checklists spill into
multiple messages. In groups, DataDog keeps one checklist message updated and
tries to pin it. If it lacks permission, it asks the group once to grant the
**Pin Messages** admin permission or pin the checklist manually.

Durable workers separately download images, classify/interpret messages, run
YOLO, and send queued replies. Restarting resumes pending work. Photo storage and
YOLO continue if DeepSeek is slow or unavailable. Classification retries up to
five times with backoff; source download and YOLO each retry up to three times.
Explicit manual labels remain available during API outages by replying with
`building overview`, `plan`, `East elevation overview`, `F001 close-up` or
`F001 context photo`. Project creation and free-form descriptions need DeepSeek.

Telegram updates are deduplicated by chat/message ID. Repeated uploads retain
separate provenance and hashes; the checklist uses presence, not photo counts.
Database changes, audit records and response queue entries commit together.
Delivery uses an outbox with retries; a crash after Telegram accepts a message
but before SQLite records delivery can cause a duplicate reply. Message edits
are not automatically reprocessed: send a correction as a new reply.

## Photo review and report drafts

DeepSeek vision records preliminary photo-visible candidates such as cracks,
bulging, spalling, exposed rebar, visible corrosion, misalignment, missing or
displaced parts, sealant damage, gasket deformation, water staining and possible
surface delamination. Each observation is stored against its evidence ID and
included in the generated report as an AI candidate for human review. The
crack-trained YOLO detector continues to annotate crack candidates independently.

Photographs do not establish hollowness, looseness, concealed corrosion or
fixings, carbonation, adhesion failure or hidden debonding. DataDog flags the
physical verification needed where it can be identified. It does not assign the
BCA “Safe”, “Require Repair” or “Unsafe” classification, certify a PFI, measure
defect extent or recommend engineering repairs.

Send `generate report` in the project chat to create and receive a DOCX draft.
It includes project particulars, evidence checklist status, elevations, photo
register, recorded AI observations and field notes, plus blank fields for
inspection methods, 10% close-range calculations, CP assessment, recommendations
and certification. Missing particulars are marked for completion. The draft must
be checked against the current BCA submission template before use.

## YOLO

The crack-only YOLOv8s detector uses the pinned
[Utbird/crack_detection checkpoint](https://huggingface.co/Utbird/crack_detection).
A whole-image 640-pixel pass plus overlapping native-resolution crops finds
crack candidates. Large sources are resized to 3072 pixels maximum; phone
orientation is applied. `YOLO_CONFIDENCE=0.35` and `YOLO_IOU=0.45` are defaults.
A missing YOLO installation does not prevent evidence collection. Detections
are model candidates; distant cracks absent from the source detail can be missed.
YOLO image overlays are experimental and disabled by default because detector
boxes can be misleading in varied field conditions. Set `YOLO_ANNOTATIONS=true`
only for controlled evaluation; VLM observations and original evidence remain
available when overlays are disabled.

## Implementation references

- [teloxide shared state example](https://github.com/teloxide/teloxide/blob/master/crates/teloxide/examples/shared_state.rs)
- [teloxide dispatcher example](https://github.com/teloxide/teloxide/blob/master/crates/teloxide/examples/dispatching_features.rs)
- [DeepSeek Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion/)
- [DeepSeek model metadata](https://api-docs.deepseek.com/api/list-models/)
