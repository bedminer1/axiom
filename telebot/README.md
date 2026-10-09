# DataDog evidence collector

A standalone Rust/teloxide Telegram bot with conversational project management,
a local SQLite evidence register, a six-type checklist, DeepSeek text/vision
classification and the existing crack-trained YOLO image replies.

## Run

From the repository root:

```sh
uv run --python 3.12 telebot/setup_model.py
# For a first installation only; preserve an existing configured .env.
cp telebot/.env.example telebot/.env
chmod 600 telebot/.env
# Set TELOXIDE_TOKEN and DEEPSEEK_API_KEY in that file.
cargo run --release --manifest-path telebot/Cargo.toml
```

Python is only used for the one-time ONNX export/runtime download. The running
bot, database access, Telegram handling and inference pipeline are Rust.
DeepSeek defaults to `https://api.deepseek.com`, model `deepseek-flash` (V4.1
Flash), enabled thinking and `reasoning_effort: high`. Each relevant text message
or photo can incur an API request. Classification receives a 1600-pixel preview,
its caption and focused project/sender/reply context. API usage and the returned
model identifier are retained; reasoning traces are not retained.

Only one polling instance may use this Telegram token. Stop with Ctrl-C. Remove
an existing webhook before using polling. The bot runs while its local process
and computer are running. It has no slash commands and does not import old chat
history.

For groups, disable privacy through BotFather and re-add the bot, or give it
admin access so Telegram delivers ordinary messages and photos. Channel posts
are supported when the bot has posting permission. Replies stay in the original
forum topic. Bot-generated messages are excluded from collection.

## Conversation

```text
new project guys, Star Vista
track East and West elevations
[photo] building overview
[image of drawing] plan drawing
[photo] East elevation overview
[photo] new finding, East elevation, level 3. Crack beside window.
[reply with another photo] close-up for F001
[reply to a saved photo] that is the context photo for F001
show the checklist
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
Projects are isolated by chat; sharing a site between different chats is not
implemented. Each chat/topic has an active project, and each sender has separate
finding and clarification context. Changing projects clears the sender contexts.
Archive is reversible and retains evidence; permanent deletion is not exposed.

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

A tick indicates saved and assigned evidence. This is an evidence collection
MVP; it does not calculate inspection coverage, physical crack measurements or
produce an engineering assessment/report. Plan input currently accepts JPEG,
PNG or WebP images; PDF/CAD files are not imported.

## Storage and recovery

Paths are relative to `telebot/`, regardless of the launch directory:

```text
data/telebot.sqlite                 SQLite register (WAL)
data/evidence/E0001/source.jpg      Downloaded source (PNG/WebP also supported)
data/evidence/E0001/preview.jpg     Oriented image for DeepSeek
data/evidence/E0001/annotated.jpg   YOLO result
```

Set `BOT_DATA_DIR` to change this directory. `.env`, `data/`, `models/` and `run/`
are ignored by Git. Restrict access to the local evidence directory and back up
it together with SQLite. For a consistent copy, stop the bot first (or use
SQLite's backup API); a live database may also have WAL/SHM files.

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
multiple messages. Summaries are not automatically pinned.

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

## YOLO

Crack-trained YOLOv8s uses the pinned
[Utbird/crack_detection checkpoint](https://huggingface.co/Utbird/crack_detection).
A whole-image 640-pixel pass plus overlapping native-resolution crops finds
crack candidates. Large sources are resized to 3072 pixels maximum; phone
orientation is applied. `YOLO_CONFIDENCE=0.35` and `YOLO_IOU=0.45` are defaults.
A missing YOLO installation does not prevent evidence collection. Detections
are model candidates; distant cracks absent from the source detail can be missed.

## Implementation references

- [teloxide shared state example](https://github.com/teloxide/teloxide/blob/master/crates/teloxide/examples/shared_state.rs)
- [teloxide dispatcher example](https://github.com/teloxide/teloxide/blob/master/crates/teloxide/examples/dispatching_features.rs)
- [DeepSeek Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion/)
- [DeepSeek model metadata](https://api-docs.deepseek.com/api/list-models/)
