CREATE TABLE IF NOT EXISTS userdata (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    userdata         TEXT NOT NULL,
    friend_request_disabled  INT NOT NULL
);
CREATE TABLE IF NOT EXISTS userhome (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    userhome         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS missions (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    missions         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS loginbonus (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    loginbonus       TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sifcards (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    sifcards         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS friends (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    friends          TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chats (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    chats            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS event (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    event            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS eventloginbonus (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    eventloginbonus  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS server_data (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    server_data      TEXT NOT NULL
);

INSERT INTO userdata (user_id, userdata, friend_request_disabled) SELECT user_id, userdata, friend_request_disabled FROM users;

INSERT INTO userhome (user_id, userhome) SELECT user_id, userhome FROM users;
INSERT INTO missions (user_id, missions) SELECT user_id, missions FROM users;
INSERT INTO loginbonus (user_id, loginbonus) SELECT user_id, loginbonus FROM users;
INSERT INTO sifcards (user_id, sifcards) SELECT user_id, sifcards FROM users;
INSERT INTO friends (user_id, friends) SELECT user_id, friends FROM users;
INSERT INTO chats (user_id, chats) SELECT user_id, chats FROM users;
INSERT INTO event (user_id, event) SELECT user_id, event FROM users;
INSERT INTO eventloginbonus (user_id, eventloginbonus) SELECT user_id, eventloginbonus FROM users;
INSERT INTO server_data (user_id, server_data) SELECT user_id, server_data FROM users;

DROP TABLE users;
VACUUM;
