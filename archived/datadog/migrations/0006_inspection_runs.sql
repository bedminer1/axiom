-- A project is a persistent building/location. Each visit/report is a separate
-- inspection run so previous reports can be referenced without mixing evidence.
ALTER TABLE projects ADD COLUMN address TEXT;
ALTER TABLE projects ADD COLUMN building_type TEXT;
ALTER TABLE projects ADD COLUMN owner_name TEXT;
ALTER TABLE projects ADD COLUMN top_csc_date TEXT;
ALTER TABLE projects ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';

CREATE TABLE inspection_runs (
 id TEXT PRIMARY KEY,
 project TEXT NOT NULL REFERENCES projects(id),
 run_number INTEGER NOT NULL,
 title TEXT NOT NULL,
 report_reference TEXT,
 status TEXT NOT NULL CHECK(status IN ('setup','collecting','reviewing','drafting','cp_review','complete','archived')),
 current_step TEXT NOT NULL,
 inspection_type TEXT NOT NULL DEFAULT 'periodic_facade',
 notice_reference TEXT,
 notice_date INTEGER,
 inspection_started_at INTEGER,
 inspection_ended_at INTEGER,
 report_due_at INTEGER,
 created_by INTEGER REFERENCES users(id),
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL,
 completed_at INTEGER,
 metadata TEXT NOT NULL DEFAULT '{}',
 UNIQUE(project,run_number),
 UNIQUE(project,report_reference)
);
CREATE INDEX inspection_runs_project ON inspection_runs(project,status,run_number DESC);

CREATE TABLE inspection_steps (
 inspection_run TEXT NOT NULL REFERENCES inspection_runs(id),
 step_key TEXT NOT NULL,
 ordinal INTEGER NOT NULL,
 title TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('pending','in_progress','complete','blocked','skipped')),
 required INTEGER NOT NULL DEFAULT 1,
 started_at INTEGER,
 completed_at INTEGER,
 metadata TEXT NOT NULL DEFAULT '{}',
 PRIMARY KEY(inspection_run,step_key)
);
CREATE INDEX inspection_steps_progress ON inspection_steps(inspection_run,ordinal,status);

CREATE TABLE report_drafts (
 id TEXT PRIMARY KEY,
 inspection_run TEXT NOT NULL REFERENCES inspection_runs(id),
 version INTEGER NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('draft','reviewed','superseded','final')),
 template_version TEXT NOT NULL,
 snapshot_json TEXT NOT NULL,
 path TEXT,
 sha256 TEXT,
 generated_by INTEGER REFERENCES users(id),
 generated_at INTEGER NOT NULL,
 reviewed_at INTEGER,
 finalized_at INTEGER,
 UNIQUE(inspection_run,version)
);
CREATE INDEX report_drafts_run ON report_drafts(inspection_run,version DESC);

ALTER TABLE chats ADD COLUMN inspection_run TEXT REFERENCES inspection_runs(id);
ALTER TABLE messages ADD COLUMN inspection_run TEXT REFERENCES inspection_runs(id);
ALTER TABLE evidence ADD COLUMN inspection_run TEXT REFERENCES inspection_runs(id);
ALTER TABLE findings ADD COLUMN inspection_run TEXT REFERENCES inspection_runs(id);
ALTER TABLE evidence ADD COLUMN captured_at INTEGER;
ALTER TABLE evidence ADD COLUMN capture_metadata TEXT NOT NULL DEFAULT '{}';

-- Preserve existing data as inspection run 1 for each project.
INSERT INTO inspection_runs(
 id,project,run_number,title,status,current_step,created_by,created_at,updated_at
)
SELECT
 id || ':inspection:1', id, 1, 'Inspection 1',
 CASE WHEN archived=1 THEN 'archived' ELSE 'collecting' END,
 'evidence_collection', lead_user, created_at, unixepoch()
FROM projects;

INSERT INTO inspection_steps(inspection_run,step_key,ordinal,title,status,started_at)
SELECT id,'project_setup',1,'Project setup','complete',created_at FROM inspection_runs
UNION ALL
SELECT id,'document_review',2,'Document review','pending',NULL FROM inspection_runs
UNION ALL
SELECT id,'evidence_collection',3,'Visual evidence collection','in_progress',created_at FROM inspection_runs
UNION ALL
SELECT id,'close_range_inspection',4,'Close-range inspection','pending',NULL FROM inspection_runs
UNION ALL
SELECT id,'findings_review',5,'Findings review','pending',NULL FROM inspection_runs
UNION ALL
SELECT id,'report_drafting',6,'Report drafting','pending',NULL FROM inspection_runs
UNION ALL
SELECT id,'cp_review',7,'CP review and certification','pending',NULL FROM inspection_runs;

UPDATE chats
SET inspection_run=(SELECT id FROM inspection_runs r WHERE r.project=chats.project AND r.run_number=1)
WHERE project IS NOT NULL;
UPDATE messages
SET inspection_run=(SELECT id FROM inspection_runs r WHERE r.project=messages.project AND r.run_number=1)
WHERE project IS NOT NULL;
UPDATE evidence
SET inspection_run=(SELECT id FROM inspection_runs r WHERE r.project=evidence.project AND r.run_number=1),
    captured_at=created_at
WHERE project IS NOT NULL;
UPDATE findings
SET inspection_run=(SELECT id FROM inspection_runs r WHERE r.project=findings.project AND r.run_number=1)
WHERE project IS NOT NULL;

CREATE INDEX evidence_run ON evidence(inspection_run,label,finding);
CREATE INDEX findings_run ON findings(inspection_run,number);
CREATE INDEX messages_run ON messages(inspection_run,status,id);
