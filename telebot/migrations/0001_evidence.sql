CREATE TABLE projects (
 id TEXT PRIMARY KEY, location TEXT NOT NULL, location_key TEXT NOT NULL,
 owner_chat INTEGER NOT NULL, archived INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL, UNIQUE(owner_chat,location_key)
);
CREATE TABLE aliases (
 chat INTEGER NOT NULL, location_key TEXT NOT NULL, project TEXT NOT NULL REFERENCES projects(id),
 PRIMARY KEY(chat,location_key)
);
CREATE TABLE chats (
 chat INTEGER NOT NULL, topic INTEGER NOT NULL DEFAULT 0,
 project TEXT REFERENCES projects(id), checklist_message INTEGER,
 PRIMARY KEY(chat,topic)
);
CREATE TABLE elevations (
 id INTEGER PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id),
 name TEXT NOT NULL, name_key TEXT NOT NULL, UNIQUE(project,name_key)
);
CREATE TABLE findings (
 id INTEGER PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), number INTEGER NOT NULL,
 elevation INTEGER NOT NULL REFERENCES elevations(id), level TEXT,
 created_at INTEGER NOT NULL, UNIQUE(project,number)
);
CREATE TABLE messages (
 id INTEGER PRIMARY KEY, chat INTEGER NOT NULL, topic INTEGER NOT NULL,
 telegram_id INTEGER NOT NULL, sender TEXT NOT NULL, sent_at INTEGER NOT NULL,
 received_at INTEGER NOT NULL, raw TEXT NOT NULL, text TEXT NOT NULL,
 project TEXT REFERENCES projects(id), status TEXT NOT NULL DEFAULT 'pending',
 attempts INTEGER NOT NULL DEFAULT 0, next_attempt INTEGER NOT NULL DEFAULT 0,
 analysis TEXT, model TEXT, usage TEXT, error TEXT,
 UNIQUE(chat,telegram_id)
);
CREATE TABLE evidence (
 id INTEGER PRIMARY KEY, message INTEGER NOT NULL UNIQUE REFERENCES messages(id),
 project TEXT REFERENCES projects(id), elevation INTEGER REFERENCES elevations(id),
 finding INTEGER REFERENCES findings(id), label TEXT,
 description TEXT, assignment_source TEXT, status TEXT NOT NULL DEFAULT 'unassigned',
 path TEXT, preview_path TEXT, annotated_path TEXT, sha256 TEXT,
 file_id TEXT, file_unique_id TEXT, filename TEXT, mime TEXT,
 bytes INTEGER, width INTEGER, height INTEGER, exif TEXT,
 yolo_status TEXT NOT NULL DEFAULT 'none', yolo TEXT, yolo_attempts INTEGER NOT NULL DEFAULT 0,
 yolo_next_attempt INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL
);
CREATE INDEX evidence_project ON evidence(project, label, finding);
CREATE TABLE contexts (
 chat INTEGER NOT NULL, topic INTEGER NOT NULL, sender TEXT NOT NULL,
 project TEXT REFERENCES projects(id), finding INTEGER REFERENCES findings(id),
 evidence INTEGER REFERENCES evidence(id), prompt TEXT, prompted_finding INTEGER,
 PRIMARY KEY(chat,topic,sender)
);
CREATE TABLE audit (
 id INTEGER PRIMARY KEY, message INTEGER NOT NULL REFERENCES messages(id),
 evidence INTEGER REFERENCES evidence(id), action TEXT NOT NULL,
 before_json TEXT, after_json TEXT NOT NULL, created_at INTEGER NOT NULL
);
CREATE TABLE outbox (
 id INTEGER PRIMARY KEY, chat INTEGER NOT NULL, topic INTEGER NOT NULL,
 reply_to INTEGER, kind TEXT NOT NULL, text TEXT NOT NULL, path TEXT,
 evidence INTEGER REFERENCES evidence(id), attempts INTEGER NOT NULL DEFAULT 0,
 next_attempt INTEGER NOT NULL DEFAULT 0, delivered_at INTEGER, error TEXT
);
CREATE INDEX messages_pending ON messages(status,next_attempt,id);
CREATE INDEX outbox_pending ON outbox(delivered_at,next_attempt,id);
ALTER TABLE outbox ADD COLUMN telegram_message INTEGER;
CREATE TABLE descriptions (
 id INTEGER PRIMARY KEY, message INTEGER NOT NULL REFERENCES messages(id),
 finding INTEGER NOT NULL REFERENCES findings(id), evidence INTEGER REFERENCES evidence(id),
 text TEXT NOT NULL, created_at INTEGER NOT NULL, UNIQUE(message,finding)
);
