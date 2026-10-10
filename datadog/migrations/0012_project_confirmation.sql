ALTER TABLE chats ADD COLUMN pending_project_name TEXT;
ALTER TABLE chats ADD COLUMN pending_project_user INTEGER REFERENCES users(id);
ALTER TABLE chats ADD COLUMN pending_project_message INTEGER REFERENCES messages(id);
