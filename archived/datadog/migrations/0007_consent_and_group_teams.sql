ALTER TABLE users ADD COLUMN terms_version TEXT;
ALTER TABLE users ADD COLUMN privacy_version TEXT;
ALTER TABLE users ADD COLUMN legal_accepted_at INTEGER;

ALTER TABLE chats ADD COLUMN owner_telegram_id INTEGER;
ALTER TABLE chats ADD COLUMN is_private INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chats ADD COLUMN awaiting_project_name INTEGER NOT NULL DEFAULT 0;

CREATE INDEX chats_owner ON chats(owner_telegram_id);
