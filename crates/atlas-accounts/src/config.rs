//! Configuration: database location, cookie flags, session lifetime, rate limits, hashing cost.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::auth::PasswordWork;

/// Where the accounts database lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Database {
    /// A SQLite file; parent directories are created on open.
    File(PathBuf),
    /// A private in-memory database (tests, throwaway demos).
    InMemory,
}

/// At most `max` events per `window` for one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub max: usize,
    pub window: Duration,
}

/// Argon2id cost. The default follows the OWASP recommendation (19 MiB, 2 passes, 1 lane).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasswordCost {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Default for PasswordCost {
    fn default() -> Self {
        Self {
            memory_kib: 19 * 1024,
            iterations: 2,
            parallelism: 1,
        }
    }
}

impl PasswordCost {
    /// Cheapest valid parameters; for tests only.
    pub fn insecure_fast() -> Self {
        Self {
            memory_kib: 64,
            iterations: 1,
            parallelism: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AccountsConfig {
    /// Production signup requires proof of email ownership. Disabled only by fixture configurations.
    pub require_email_verification: bool,
    pub email: Option<crate::email::EmailConfig>,
    pub verification_ttl: Duration,
    pub password_reset_ttl: Duration,
    pub email_per_client: Limit,
    pub database: Database,
    /// Mark the session cookie `Secure` (default). Turn off only for plain-http local development.
    pub secure_cookies: bool,
    /// Absolute session lifetime; sessions are not extended by activity.
    pub session_ttl: Duration,
    /// Use the first `X-Forwarded-For` address as the client key for rate limits. Right when the API
    /// sits behind the web server or a proxy that sets it; otherwise the socket address is used.
    pub trust_forwarded_for: bool,
    /// Failed sign-ins per email address.
    pub login_per_account: Limit,
    /// Sign-in attempts per client.
    pub login_per_client: Limit,
    /// Sign-ups per client.
    pub signup_per_client: Limit,
    pub password_cost: PasswordCost,
    /// Four concurrent password jobs, shared process-wide by default. Test configurations are isolated.
    pub password_work: PasswordWork,
}

impl Default for AccountsConfig {
    fn default() -> Self {
        Self {
            require_email_verification: true,
            email: None,
            verification_ttl: Duration::from_secs(24 * 3600),
            password_reset_ttl: Duration::from_secs(30 * 60),
            email_per_client: Limit {
                max: 20,
                window: Duration::from_secs(3600),
            },
            database: Database::File(default_db_path()),
            secure_cookies: true,
            session_ttl: Duration::from_secs(30 * 24 * 3600),
            trust_forwarded_for: false,
            login_per_account: Limit {
                max: 5,
                window: Duration::from_secs(15 * 60),
            },
            login_per_client: Limit {
                max: 30,
                window: Duration::from_secs(15 * 60),
            },
            signup_per_client: Limit {
                max: 10,
                window: Duration::from_secs(3600),
            },
            password_cost: PasswordCost::default(),
            password_work: PasswordWork::default(),
        }
    }
}

impl AccountsConfig {
    /// Defaults plus the environment:
    /// - `RARE_ATLAS_ACCOUNTS_DB`: database file (`:memory:` for an in-memory one);
    ///   otherwise `$RARE_ATLAS_DATA/app/accounts.sqlite`, otherwise `<nearest ./data>/app/accounts.sqlite`;
    /// - `RARE_ATLAS_INSECURE_COOKIES=1`: drop the `Secure` cookie flag (plain-http development);
    /// - `RARE_ATLAS_TRUST_FORWARDED_FOR=1`: explicitly trust a proxy that overwrites `X-Forwarded-For`.
    /// - `RESEND_API_KEY`, `ATLAS_ACCOUNT_EMAIL_FROM` (or `RESEND_FROM`), `ATLAS_ACCOUNT_PUBLIC_ORIGIN`: all three
    ///   configure verification/reset mail; the public origin must be a trusted HTTPS root origin.
    pub fn from_env() -> Self {
        let database = match std::env::var("RARE_ATLAS_ACCOUNTS_DB") {
            Ok(v) if v == ":memory:" => Database::InMemory,
            Ok(v) if !v.trim().is_empty() => Database::File(PathBuf::from(v)),
            _ => Database::File(default_db_path()),
        };
        let flag = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        };
        Self {
            database,
            email: crate::email::EmailConfig::from_env(),
            secure_cookies: !flag("RARE_ATLAS_INSECURE_COOKIES").unwrap_or(false),
            trust_forwarded_for: flag("RARE_ATLAS_TRUST_FORWARDED_FOR").unwrap_or(false),
            ..Self::default()
        }
    }

    /// An in-memory database with cheap hashing and an independent four-job gate: for tests.
    /// Cloning the returned configuration shares its gate, just as cloned routers share admission.
    pub fn for_tests() -> Self {
        Self {
            require_email_verification: false,
            database: Database::InMemory,
            trust_forwarded_for: true,
            password_cost: PasswordCost::insecure_fast(),
            password_work: PasswordWork::isolated(),
            ..Self::default()
        }
    }

    pub fn with_database(mut self, database: Database) -> Self {
        self.database = database;
        self
    }
}

/// `$RARE_ATLAS_DATA/app/accounts.sqlite`, else the nearest `data/` directory upwards from the current
/// directory, else `./data`. The `data/app/` folder is git-ignored.
pub fn default_db_path() -> PathBuf {
    data_root().join("app").join("accounts.sqlite")
}

fn data_root() -> PathBuf {
    if let Ok(v) = std::env::var("RARE_ATLAS_DATA")
        && !v.trim().is_empty()
    {
        return PathBuf::from(v);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    nearest_data_dir(&cwd).unwrap_or_else(|| cwd.join("data"))
}

fn nearest_data_dir(start: &Path) -> Option<PathBuf> {
    start.ancestors().map(|dir| dir.join("data")).find(|d| d.is_dir())
}
