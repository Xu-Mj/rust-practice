CREATE TABLE messages
(
    send_id      VARCHAR NOT NULL,
    receiver_id  VARCHAR NOT NULL,
    local_id     VARCHAR NOT NULL,
    server_id    VARCHAR NOT NULL,
    send_time    BIGINT  NOT NULL,
    msg_type     INT,
    content_type INT,
    content      TEXT,
    PRIMARY KEY (send_id, server_id, send_time)
);
