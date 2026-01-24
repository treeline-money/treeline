-- Initial logging schema
CREATE TABLE sys_logs (
    id UBIGINT PRIMARY KEY,
    timestamp BIGINT NOT NULL,

    entry_point VARCHAR NOT NULL,
    app_version VARCHAR NOT NULL,
    platform VARCHAR NOT NULL,

    event VARCHAR NOT NULL,

    integration VARCHAR,
    page VARCHAR,
    command VARCHAR,

    error_message VARCHAR,
    error_details VARCHAR
);

CREATE INDEX idx_logs_timestamp ON sys_logs(timestamp);
CREATE INDEX idx_logs_event ON sys_logs(event);
