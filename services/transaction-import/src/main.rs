use std::{
    env, fs,
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use postgres::{Config, NoTls};
use transaction_import::{ImportOutcome, import_csv};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Transaction CSV to import, or - to read standard input
    file: PathBuf,
    /// PostgreSQL URL or connection string; otherwise uses PGHOST/PGPORT/PGUSER/PGDATABASE
    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    database_url: Option<String>,
    /// Read the database password from this file instead of PGPASSWORD
    #[arg(long, env = "PGPASSWORD_FILE")]
    password_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let data = if args.file.as_os_str() == "-" {
        let mut data = Vec::new();
        io::stdin()
            .read_to_end(&mut data)
            .context("Cannot read CSV from standard input")?;
        data
    } else {
        fs::read(&args.file).context("Cannot read CSV file")?
    };
    let mut config = if let Some(url) = args.database_url {
        url.parse::<Config>()
            .map_err(|_| anyhow::anyhow!("Invalid database connection string"))?
    } else {
        let mut config = Config::new();
        config.host(&env::var("PGHOST").unwrap_or_else(|_| "localhost".into()));
        config.port(
            env::var("PGPORT")
                .unwrap_or_else(|_| "5432".into())
                .parse()
                .context("Invalid PGPORT")?,
        );
        config.user(&env::var("PGUSER").unwrap_or_else(|_| "transactions".into()));
        config.dbname(&env::var("PGDATABASE").unwrap_or_else(|_| "transactions".into()));
        if let Ok(password) = env::var("PGPASSWORD") {
            config.password(password);
        }
        config
    };
    if let Some(path) = args.password_file {
        let password = fs::read_to_string(path).context("Cannot read database password file")?;
        let password = password.trim_end_matches(['\r', '\n']);
        if password.is_empty() {
            bail!("Database password file is empty");
        }
        config.password(password);
    }
    config.connect_timeout(Duration::from_secs(10));
    let mut client = config
        .connect(NoTls)
        .context("Cannot connect to PostgreSQL")?;
    let source = args
        .file
        .file_name()
        .context("CSV path must name a file")?
        .to_string_lossy();
    match import_csv(&mut client, &source, &data)? {
        ImportOutcome::Imported(count) => println!("Imported {count} transactions."),
        ImportOutcome::AlreadyImported => println!("Already imported this exact file; no changes."),
    }
    Ok(())
}
