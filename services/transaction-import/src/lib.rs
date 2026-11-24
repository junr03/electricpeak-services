use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use postgres::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de};
use sha2::{Digest, Sha256};

pub const HEADERS: [&str; 13] = [
    "date",
    "name",
    "amount",
    "status",
    "category",
    "parent category",
    "excluded",
    "tags",
    "type",
    "account",
    "account mask",
    "note",
    "recurring",
];

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub date: NaiveDate,
    pub name: String,
    #[serde(deserialize_with = "decimal")]
    pub amount: Decimal,
    pub status: String,
    pub category: Option<String>,
    #[serde(rename = "parent category")]
    pub parent_category: Option<String>,
    pub excluded: bool,
    pub tags: Option<String>,
    #[serde(rename = "type")]
    pub transaction_type: String,
    pub account: String,
    #[serde(rename = "account mask")]
    pub account_mask: Option<String>,
    pub note: Option<String>,
    pub recurring: Option<String>,
}

fn decimal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    let value = String::deserialize(deserializer)?;
    Decimal::from_str_exact(&value).map_err(|_| {
        de::Error::custom("amount must be an exact decimal with at most 28 fractional digits")
    })
}

pub fn parse_csv(data: &[u8]) -> Result<Vec<Transaction>> {
    let mut reader = csv::ReaderBuilder::new().from_reader(data);
    let mut headers: Vec<_> = reader
        .headers()
        .context("Cannot read CSV header")?
        .iter()
        .collect();
    let mut expected = HEADERS.to_vec();
    headers.sort_unstable();
    expected.sort_unstable();
    if headers != expected {
        bail!(
            "CSV must contain exactly these headers (in any order): {}",
            HEADERS.join(", ")
        );
    }

    reader
        .deserialize()
        .enumerate()
        .map(|(index, record)| {
            record.with_context(|| format!("Invalid CSV data record {}", index + 1))
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    Imported(usize),
    AlreadyImported,
}

/// Validate before opening a DB transaction, then commit the entire file at once.
/// Only identical files are deduplicated: this format has no stable transaction ID.
pub fn import_csv(client: &mut Client, source_file: &str, data: &[u8]) -> Result<ImportOutcome> {
    let records = parse_csv(data)?;
    let hash = Sha256::digest(data).to_vec();
    let mut tx = client.transaction().context("Cannot begin import")?;
    // Serialize schema initialization and imports, including concurrent first runs.
    tx.query_one("SELECT pg_advisory_xact_lock(76523190428901)", &[])?;
    tx.batch_execute(include_str!("../migrations/001_initial.sql"))
        .context("Cannot initialize transaction_import schema")?;
    let inserted = tx.execute(
        "INSERT INTO transaction_import.imports (sha256, source_file, row_count)
         VALUES ($1, $2, $3) ON CONFLICT (sha256) DO NOTHING",
        &[&hash, &source_file, &(records.len() as i64)],
    )?;
    if inserted == 0 {
        tx.commit()?;
        return Ok(ImportOutcome::AlreadyImported);
    }

    let statement = tx.prepare(
        "INSERT INTO transaction_import.transactions
         (import_sha256, row_number, date, name, amount, status, category,
          parent_category, excluded, tags, transaction_type, account, account_mask, note, recurring)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )?;
    for (index, record) in records.iter().enumerate() {
        tx.execute(
            &statement,
            &[
                &hash,
                &((index + 1) as i64),
                &record.date,
                &record.name,
                &record.amount,
                &record.status,
                &record.category,
                &record.parent_category,
                &record.excluded,
                &record.tags,
                &record.transaction_type,
                &record.account,
                &record.account_mask,
                &record.note,
                &record.recurring,
            ],
        )
        .with_context(|| format!("Cannot insert CSV data record {}", index + 1))?;
    }
    tx.commit().context("Cannot commit import")?;
    Ok(ImportOutcome::Imported(records.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../examples/transactions.csv");

    #[test]
    fn reads_export_without_losing_precision_or_optional_values() {
        let records = parse_csv(SAMPLE.as_bytes()).unwrap();
        let row = &records[0];
        assert_eq!(row.date, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());
        assert_eq!(row.amount.to_string(), "12.34");
        assert_eq!(row.category.as_deref(), Some("Returns"));
        assert!(row.excluded);
        assert_eq!(row.parent_category, None);
        assert_eq!(row.tags, None);
        assert_eq!(row.note, None);
        assert_eq!(row.recurring, None);
        assert_eq!(row.account_mask.as_deref(), Some("0007"));
    }

    #[test]
    fn preserves_negative_amounts_leading_zeroes_and_quoted_text() {
        let data = SAMPLE
            .replace("12.34", "-0.1234567890123456789012345678")
            .replace("0007", "0019")
            .replace("Example Store", "Café, \"\"books\"\"\nshop");
        let row = parse_csv(data.as_bytes()).unwrap().remove(0);
        assert_eq!(row.amount.to_string(), "-0.1234567890123456789012345678");
        assert_eq!(row.account_mask.as_deref(), Some("0019"));
        assert_eq!(row.name, "Café, \"books\"\nshop");
    }

    #[test]
    fn rejects_invalid_values_and_inexact_amounts() {
        for data in [
            SAMPLE.replace("2026-01-15", "2026-02-30"),
            SAMPLE.replace("12.34", "NaN"),
            SAMPLE.replace("12.34", "0.12345678901234567890123456789"),
            SAMPLE.replace("true", "maybe"),
            SAMPLE.replace("12.34", ""),
            format!("{SAMPLE}too,few,columns\n"),
        ] {
            assert!(parse_csv(data.as_bytes()).is_err());
        }
    }

    #[test]
    fn requires_the_complete_header_even_for_empty_exports() {
        for data in ["", "date,name\n", "date,date\n"] {
            assert!(parse_csv(data.as_bytes()).is_err());
        }
        assert!(
            parse_csv(format!("{}\n", HEADERS.join(",")).as_bytes())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn matches_columns_by_header_in_any_order() {
        let mut reader = csv::Reader::from_reader(SAMPLE.as_bytes());
        let mut writer = csv::Writer::from_writer(Vec::new());
        writer
            .write_record(reader.headers().unwrap().iter().rev())
            .unwrap();
        for record in reader.records() {
            writer.write_record(record.unwrap().iter().rev()).unwrap();
        }
        let rows = parse_csv(&writer.into_inner().unwrap()).unwrap();
        assert_eq!(rows[0].name, "Example Store");
    }
}
