CREATE TABLE chat_participants (
 chat INTEGER NOT NULL,
 topic INTEGER NOT NULL DEFAULT 0,
 user INTEGER NOT NULL REFERENCES users(id),
 first_seen_at INTEGER NOT NULL,
 last_seen_at INTEGER NOT NULL,
 PRIMARY KEY(chat,topic,user)
);
CREATE INDEX chat_participants_user ON chat_participants(user,chat);

ALTER TABLE chats ADD COLUMN checklist_pin_notice INTEGER NOT NULL DEFAULT 0;

UPDATE users SET role='guest' WHERE status='pending' AND role='user';
