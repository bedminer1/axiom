ALTER TABLE messages ADD COLUMN processing_message INTEGER;
ALTER TABLE messages ADD COLUMN processing_frame INTEGER NOT NULL DEFAULT 0;
ALTER TABLE outbox ADD COLUMN source_message INTEGER REFERENCES messages(id);
CREATE INDEX outbox_source_message ON outbox(source_message,delivered_at);
