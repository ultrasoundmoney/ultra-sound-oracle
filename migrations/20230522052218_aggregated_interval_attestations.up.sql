-- Add up migration script here
CREATE TABLE aggregate_interval_attestations (
    aggregate_signature text NOT NULL PRIMARY KEY,
    slot_number integer NOT NULL,
    value integer NOT NULL,
    interval_size integer NOT NULL,
    num_validators integer NOT NULL
);
