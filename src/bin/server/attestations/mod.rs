use crate::state::AppState;
use axum::{extract::State, Json};
use bls::{AggregateSignature, Hash256, PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use sqlx::SqlitePool;
use ssz_derive::{Decode, Encode};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug)]
pub struct PriceValueEntry {
    pub validator_public_key: String,
    pub value: i64,
    pub slot_number: i64,
    pub signature: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PriceIntervalEntry {
    pub validator_public_key: String,
    pub value: i64,
    pub slot_number: i64,
    pub signature: String,
    pub interval_size: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AggregatePriceIntervalEntry {
    pub value: i64,
    pub slot_number: i64,
    pub aggregate_signature: String,
    pub interval_size: i64,
    pub num_validators: i64,
}

#[derive(Clone, Debug, Encode, Decode, Serialize, Deserialize)]
pub struct Price {
    pub value: u64, // TODO: Check if we need to add further info here such as timestamp
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleMessage {
    pub value_message: SignedPriceValueMessage,
    pub interval_inclusion_messages: Vec<SignedIntervalInclusionMessage>,
    pub validator_public_key: PublicKey,
}

#[derive(Clone, Debug, Decode, Encode, Serialize, Deserialize)]
pub struct PriceValueMessage {
    pub price: Price,
    pub slot_number: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedPriceValueMessage {
    pub message: PriceValueMessage,
    pub signature: Signature,
}

#[derive(Clone, Debug, Decode, Encode, Serialize, Deserialize)]
pub struct IntervalInclusionMessage {
    pub value: u64,
    pub interval_size: u64,
    pub slot_number: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedIntervalInclusionMessage {
    pub message: IntervalInclusionMessage,
    pub signature: Signature,
}

pub async fn get_price_value_attestations(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<PriceValueEntry>> {
    let db_pool = &state.db_pool;
    let entries: Vec<PriceValueEntry> = sqlx::query!(
        "
        SELECT
            validator_public_key,
            value,
            slot_number,
            signature
        FROM
            price_value_attestations;
        "
    )
    .fetch_all(db_pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| PriceValueEntry {
        validator_public_key: row.validator_public_key,
        value: row.value,
        slot_number: row.slot_number,
        signature: row.signature,
    })
    .collect();
    Json(entries)
}

pub async fn get_aggregate_price_interval_attestations(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<AggregatePriceIntervalEntry>> {
    let db_pool = &state.db_pool;
    let entries: Vec<AggregatePriceIntervalEntry> = sqlx::query!(
        "
        SELECT
            value,
            slot_number,
            aggregate_signature,
            interval_size,
            num_validators
        FROM
            aggregate_interval_attestations 
        "
    )
    .fetch_all(db_pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| AggregatePriceIntervalEntry {
        value: row.value,
        slot_number: row.slot_number,
        aggregate_signature: row.aggregate_signature,
        interval_size: row.interval_size,
        num_validators: row.num_validators,
    })
    .collect();
    Json(entries)
}

pub async fn get_price_interval_attestations(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<PriceIntervalEntry>> {
    let db_pool = &state.db_pool;
    let entries: Vec<PriceIntervalEntry> = sqlx::query!(
        "
        SELECT
            validator_public_key,
            value,
            slot_number,
            signature,
            interval_size
        FROM
            price_interval_attestations;
        "
    )
    .fetch_all(db_pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| PriceIntervalEntry {
        validator_public_key: row.validator_public_key,
        value: row.value,
        slot_number: row.slot_number,
        signature: row.signature,
        interval_size: row.interval_size,
    })
    .collect();
    Json(entries)
}

pub async fn post_oracle_message(
    State(state): State<Arc<AppState>>,
    Json(message): Json<OracleMessage>,
) -> Result<(), axum::http::StatusCode> {
    tracing::info!("Received oracle message: {:?}", message);
    let db_pool = &state.db_pool;
    let validator_public_key = message.validator_public_key;
    // TODO: Improve error handling instead of returning "BAD REQUEST" for any kind of error
    save_price_value_attestation(db_pool, &message.value_message, &validator_public_key)
        .await
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    save_price_interval_attestations(
        db_pool,
        &message.interval_inclusion_messages,
        &validator_public_key,
    )
    .await
    .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    Ok(())
}

async fn save_price_value_attestation(
    db_pool: &SqlitePool,
    message: &SignedPriceValueMessage,
    validator_public_key: &PublicKey,
) -> eyre::Result<()> {
    if !validate_message(validator_public_key, &message.message, &message.signature) {
        return Err(eyre::eyre!("Invalid signature"));
    }
    let value = message.message.price.value.to_string();
    let slot_number = message.message.slot_number.to_string();
    let signature = message.signature.to_string();
    let pk_string = validator_public_key.to_string();

    // Save price_value_message in DB
    sqlx::query!(
        "
        INSERT INTO price_value_attestations(
            validator_public_key,
            value,
            slot_number,
            signature
        )
        VALUES (
            ?1,
            ?2,
            ?3,
            ?4
        );
        ",
        pk_string,
        value,
        slot_number,
        signature,
    )
    .execute(db_pool)
    .await?;
    Ok(())
}

async fn save_price_interval_attestations(
    db_pool: &SqlitePool,
    messages: &Vec<SignedIntervalInclusionMessage>,
    validator_public_key: &PublicKey,
) -> eyre::Result<()> {
    for message in messages {
        save_price_interval_attestation(db_pool, message, validator_public_key).await?;
    }
    Ok(())
}

async fn save_price_interval_attestation(
    db_pool: &SqlitePool,
    message: &SignedIntervalInclusionMessage,
    validator_public_key: &PublicKey,
) -> eyre::Result<()> {
    if !validate_message(validator_public_key, &message.message, &message.signature) {
        return Err(eyre::eyre!("Invalid signature"));
    }
    let value = message.message.value.to_string();
    let interval_size = message.message.interval_size.to_string();
    let slot_number = message.message.slot_number.to_string();
    let signature = message.signature.to_string();
    let pk_string = validator_public_key.to_string();

    // Save price_value_message in DB
    sqlx::query!(
        "
        INSERT INTO price_interval_attestations(
            validator_public_key,
            value,
            interval_size,
            slot_number,
            signature
        )
        VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5
        );
        ",
        pk_string,
        value,
        interval_size,
        slot_number,
        signature,
    )
    .execute(db_pool)
    .await?;

    // TODO: Review if we really want to aggregate every time we receive a new message
    extend_or_create_aggregate_interval_attestation(db_pool, message).await?;
    Ok(())
}

async fn extend_or_create_aggregate_interval_attestation(
    db_pool: &SqlitePool,
    message: &SignedIntervalInclusionMessage,
) -> eyre::Result<()> {
    let interval_size = message.message.interval_size.to_string();
    let slot_number = message.message.slot_number.to_string();
    let value = message.message.value.to_string();
    let (new_num_validators, mut aggregate_signature) = if let Some(entry) = sqlx::query!(
        "
        SELECT
            num_validators,
            aggregate_signature
        FROM
            aggregate_interval_attestations
        WHERE
            interval_size = ?1
        AND
            slot_number = ?2
        AND
            value = ?3;
        ",
        interval_size,
        slot_number,
        value,
    )
    .fetch_optional(db_pool)
    .await?
    {
        (
            entry.num_validators + 1,
            AggregateSignature::deserialize(&hex::decode(entry.aggregate_signature)?)
                .map_err(|_| eyre::eyre!("Invalid aggregate signature in DB"))?,
        )
    } else {
        (1, AggregateSignature::infinity())
    };

    aggregate_signature.add_assign(&message.signature);
    let new_aggregate_signature = hex::encode(aggregate_signature.serialize());

    if new_num_validators == 1 {
        // Create new db entry
        sqlx::query!(
            "
            INSERT INTO aggregate_interval_attestations(
                value,
                interval_size,
                slot_number,
                num_validators,
                aggregate_signature
            )
            VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                ?5
            );
            ",
            value,
            interval_size,
            slot_number,
            new_num_validators,
            new_aggregate_signature,
        )
        .execute(db_pool)
        .await?;
    } else {
        // Update existing db entry
        sqlx::query!(
            "
            UPDATE aggregate_interval_attestations
            SET
                num_validators = ?1,
                aggregate_signature = ?2
            WHERE
                interval_size = ?3
            AND
                slot_number = ?4
            AND
                value = ?5;
            ",
            new_num_validators,
            new_aggregate_signature,
            interval_size,
            slot_number,
            value,
        )
        .execute(db_pool)
        .await?;
    }

    Ok(())
}

fn validate_message<T: ssz::Encode>(
    public_key: &PublicKey,
    message: &T,
    signature: &Signature,
) -> bool {
    let message_digest = get_message_digest(&message);
    signature.verify(public_key, message_digest)
}

pub fn get_message_digest<T: ssz::Encode>(message: &T) -> Hash256 {
    let message_ssz = message.as_ssz_bytes();
    Hash256::from_slice(&Sha3_256::digest(message_ssz))
}
