use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use postgres::{Client, NoTls};
use transaction_import::{ImportOutcome, import_csv};

#[test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
fn imports_exact_values_deduplicates_files_and_rolls_back_failures() {
    let mut client = Client::connect(&env::var("TEST_DATABASE_URL").unwrap(), NoTls).unwrap();
    let marker = format!(
        "integration-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let sample = include_str!("../examples/transactions.csv").replace("Example Store", &marker);
    let (header, row) = sample.split_once('\n').unwrap();
    // Identical rows can represent separate purchases. Preserve both.
    let data = format!("{header}\n{row}{row}");
    assert_eq!(
        import_csv(&mut client, "sample.csv", data.as_bytes()).unwrap(),
        ImportOutcome::Imported(2)
    );
    assert_eq!(
        import_csv(&mut client, "renamed.csv", data.as_bytes()).unwrap(),
        ImportOutcome::AlreadyImported
    );
    let rows = client
        .query(
            "SELECT date::text, amount::text, excluded, account_mask, parent_category,
         tags, note, recurring, transaction_type, category, status, account
         FROM transaction_import.transactions WHERE name = $1 ORDER BY row_number",
            &[&marker],
        )
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<_, String>(0), "2026-01-15");
    assert_eq!(rows[0].get::<_, String>(1), "12.34");
    assert!(rows[0].get::<_, bool>(2));
    assert_eq!(rows[0].get::<_, String>(3), "0007");
    for column in 4..=7 {
        assert_eq!(rows[0].get::<_, Option<String>>(column), None);
    }
    assert_eq!(rows[0].get::<_, String>(8), "regular");
    assert_eq!(rows[0].get::<_, String>(9), "Returns");
    assert_eq!(rows[0].get::<_, String>(10), "pending");
    assert_eq!(rows[0].get::<_, String>(11), "Example Card");

    let count = |client: &mut Client| -> (i64, i64) {
        let row = client
            .query_one(
                "SELECT (SELECT count(*) FROM transaction_import.imports),
                    (SELECT count(*) FROM transaction_import.transactions)",
                &[],
            )
            .unwrap();
        (row.get(0), row.get(1))
    };
    let before = count(&mut client);
    let malformed = format!("{data}invalid,row\n");
    assert!(import_csv(&mut client, "invalid.csv", malformed.as_bytes()).is_err());
    assert_eq!(count(&mut client), before);

    // NUL is valid UTF-8/CSV but forbidden by PostgreSQL TEXT. The first insert
    // succeeds before the second fails, proving the entire import rolls back.
    let db_failure = format!("{header}\n{row}{}", row.replace(&marker, "bad\0name"));
    assert!(import_csv(&mut client, "db-failure.csv", db_failure.as_bytes()).is_err());
    assert_eq!(count(&mut client), before);

    let precise = sample
        .replace("12.34", "-0.1234567890123456789012345678")
        .replace("0007", "0019");
    assert_eq!(
        import_csv(&mut client, "precise.csv", precise.as_bytes()).unwrap(),
        ImportOutcome::Imported(1)
    );
    let row = client
        .query_one(
            "SELECT amount::text, account_mask FROM transaction_import.transactions
         WHERE name = $1 AND amount < 0",
            &[&marker],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "-0.1234567890123456789012345678");
    assert_eq!(row.get::<_, String>(1), "0019");
}
