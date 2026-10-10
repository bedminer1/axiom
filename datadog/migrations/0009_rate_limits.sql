CREATE TABLE message_rate_limits (
 chat INTEGER NOT NULL,
 user INTEGER NOT NULL REFERENCES users(id),
 window_started_at INTEGER NOT NULL,
 accepted_count INTEGER NOT NULL,
 PRIMARY KEY(chat,user)
);
