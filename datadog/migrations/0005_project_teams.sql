ALTER TABLE projects ADD COLUMN lead_user INTEGER REFERENCES users(id);

UPDATE projects
SET lead_user = (
  SELECT id FROM users WHERE telegram_user_id = projects.owner_chat
)
WHERE lead_user IS NULL;

CREATE TABLE project_members (
 project TEXT NOT NULL REFERENCES projects(id),
 user INTEGER NOT NULL REFERENCES users(id),
 added_by INTEGER NOT NULL REFERENCES users(id),
 created_at INTEGER NOT NULL,
 PRIMARY KEY(project,user)
);
CREATE INDEX project_members_user ON project_members(user,project);

INSERT OR IGNORE INTO project_members(project,user,added_by,created_at)
SELECT id,lead_user,lead_user,created_at
FROM projects
WHERE lead_user IS NOT NULL;
