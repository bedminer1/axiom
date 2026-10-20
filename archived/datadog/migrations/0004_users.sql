CREATE TABLE users (
 id INTEGER PRIMARY KEY,
 telegram_user_id INTEGER UNIQUE,
 username TEXT,
 display_name TEXT NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('guest','user','superuser')),
 status TEXT NOT NULL CHECK(status IN ('pending','approved','rejected')),
 private_chat_id INTEGER,
 requested_at INTEGER,
 approved_at INTEGER,
 approved_by INTEGER REFERENCES users(id),
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX users_username ON users(lower(username)) WHERE username IS NOT NULL;
CREATE INDEX users_access ON users(status, role);

INSERT INTO users(
 telegram_user_id, username, display_name, role, status, private_chat_id,
 requested_at, approved_at, created_at, updated_at
) VALUES(
 1416878311, 'dkffjlrtm', 'alex', 'superuser', 'approved', 1416878311,
 unixepoch(), unixepoch(), unixepoch(), unixepoch()
);

CREATE TABLE user_access_audit (
 id INTEGER PRIMARY KEY,
 target_user INTEGER NOT NULL REFERENCES users(id),
 actor_user INTEGER REFERENCES users(id),
 action TEXT NOT NULL,
 created_at INTEGER NOT NULL
);
