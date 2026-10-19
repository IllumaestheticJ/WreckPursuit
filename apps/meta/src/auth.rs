//! Accounts, password login, session tokens, bans and the account-linking stub.
//!
//! OAuth-ready: logins are rows in `identities(provider, subject)`; the password provider is
//! just `provider = 'password'` with an Argon2id hash. Adding a provider means verifying its
//! token and looking up / inserting an identity row — sessions are provider-independent.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use std::sync::OnceLock;

use argon2::{Algorithm, Argon2, Params, Version};
use axum::http::StatusCode;
use rusqlite::{params, Connection, OptionalExtension};
use wp_contracts::PlayerId;
use wp_economy::Economy;

use crate::api::{AccountId, Profile, SessionResponse};
use crate::clock::UnixMs;
use crate::config::HashCost;
use crate::error::{ApiError, ApiResult};
use crate::store;
use crate::token;

/// Login providers accepted today.
pub const LOGIN_PROVIDERS: &[&str] = &["password"];
/// Providers that could be linked once configured (none yet).
pub const LINKABLE_PROVIDERS: &[&str] = &[];

fn argon(cost: HashCost) -> Argon2<'static> {
    match cost {
        HashCost::Production => Argon2::default(),
        HashCost::Fast => Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(Params::MIN_M_COST, 1, 1, None).expect("valid argon2 params"),
        ),
    }
}

/// Hash a password into a PHC string (salt and parameters embedded).
pub fn hash_password(password: &str, cost: HashCost) -> ApiResult<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| ApiError::internal(e.to_string()))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| ApiError::internal(e.to_string()))?;
    argon(cost)
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::internal(e.to_string()))
}

/// Check a password against a PHC string (parameters are read from the hash).
pub fn verify_password(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc)
        .map(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Normalise an email for lookup (trim + lowercase). Rejects obviously invalid addresses.
pub fn normalise_email(email: &str) -> ApiResult<String> {
    let e = email.trim().to_lowercase();
    let valid = e.len() <= 254
        && e.split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
        && !e.contains(char::is_whitespace);
    if valid {
        Ok(e)
    } else {
        Err(ApiError::bad_request("invalid email"))
    }
}

/// Validate a display name: 3..=24 chars of `[A-Za-z0-9_-]`.
pub fn validate_display_name(name: &str) -> ApiResult<()> {
    let ok = (3..=24).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "display_name must be 3-24 chars of A-Z a-z 0-9 _ -",
        ))
    }
}

fn validate_password(pw: &str) -> ApiResult<()> {
    if (8..=128).contains(&pw.chars().count()) {
        Ok(())
    } else {
        Err(ApiError::bad_request("password must be 8-128 characters"))
    }
}

/// Create an account with a password identity and a fresh economy account, then open a
/// session.
#[allow(clippy::too_many_arguments)]
pub fn signup(
    conn: &mut Connection,
    economy: &Economy,
    email: &str,
    password: &str,
    display_name: &str,
    cost: HashCost,
    session_ttl_ms: i64,
    now: UnixMs,
) -> ApiResult<SessionResponse> {
    let email = normalise_email(email)?;
    validate_display_name(display_name)?;
    validate_password(password)?;
    // Hash before taking the write transaction: hashing is the slow part.
    let phc = hash_password(password, cost)?;

    let tx = conn.transaction()?;
    let email_taken: bool = tx
        .query_row(
            "SELECT 1 FROM identities WHERE provider = 'password' AND subject = ?1",
            params![email],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if email_taken {
        return Err(ApiError::conflict(
            "email_taken",
            "email already registered",
        ));
    }
    let name_taken: bool = tx
        .query_row(
            "SELECT 1 FROM accounts WHERE display_name = ?1 COLLATE NOCASE",
            params![display_name],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if name_taken {
        return Err(ApiError::conflict(
            "name_taken",
            "display name already taken",
        ));
    }
    tx.execute(
        "INSERT INTO accounts (display_name, created_ms) VALUES (?1, ?2)",
        params![display_name, now],
    )?;
    let id = tx.last_insert_rowid();
    let account_id =
        AccountId::try_from(id).map_err(|_| ApiError::internal("account ids exhausted"))?;
    tx.execute(
        "INSERT INTO identities (provider, subject, account_id, secret_hash, created_ms)
         VALUES ('password', ?1, ?2, ?3, ?4)",
        params![email, account_id, phc, now],
    )?;
    let (acct, events) = economy.open_account(PlayerId(account_id));
    store::commit_tx(&tx, &acct, &events, now).map_err(store::store_error)?;
    let session = create_session(&tx, account_id, session_ttl_ms, now)?;
    tx.commit()?;
    Ok(session)
}

/// Verify email + password and open a session. Banned accounts are refused with 403.
pub fn login(
    conn: &Connection,
    email: &str,
    password: &str,
    cost: HashCost,
    session_ttl_ms: i64,
    now: UnixMs,
) -> ApiResult<SessionResponse> {
    let invalid = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "wrong email or password",
        )
    };
    let email = normalise_email(email).map_err(|_| invalid())?;
    let row: Option<(AccountId, Option<String>)> = conn
        .query_row(
            "SELECT account_id, secret_hash FROM identities
             WHERE provider = 'password' AND subject = ?1",
            params![email],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((account_id, Some(phc))) = row else {
        // Spend comparable time on unknown emails so response timing does not reveal them.
        let _ = verify_password(password, dummy_phc(cost));
        return Err(invalid());
    };
    if !verify_password(password, &phc) {
        return Err(invalid());
    }
    check_not_banned(conn, account_id, now)?;
    create_session(conn, account_id, session_ttl_ms, now)
}

/// A hash at the configured cost, verified against for unknown emails so login timing does
/// not reveal which emails exist.
fn dummy_phc(cost: HashCost) -> &'static str {
    static PROD: OnceLock<String> = OnceLock::new();
    static FAST: OnceLock<String> = OnceLock::new();
    let cell = match cost {
        HashCost::Production => &PROD,
        HashCost::Fast => &FAST,
    };
    cell.get_or_init(|| hash_password("dummy-password", cost).unwrap_or_default())
}

fn create_session(
    conn: &Connection,
    account_id: AccountId,
    ttl_ms: i64,
    now: UnixMs,
) -> ApiResult<SessionResponse> {
    let token = token::generate(token::prefix::SESSION);
    let expires_ms = now + ttl_ms;
    conn.execute(
        "INSERT INTO sessions (token_hash, account_id, created_ms, expires_ms) VALUES (?1, ?2, ?3, ?4)",
        params![token::hash(&token), account_id, now, expires_ms],
    )?;
    Ok(SessionResponse {
        account_id,
        token,
        expires_ms,
    })
}

/// Resolve a session token to its account. Expired, revoked or banned → 401/403.
pub fn authenticate(conn: &Connection, token: &str, now: UnixMs) -> ApiResult<AccountId> {
    let account: Option<AccountId> = conn
        .query_row(
            "SELECT account_id FROM sessions
             WHERE token_hash = ?1 AND revoked = 0 AND expires_ms > ?2",
            params![token::hash(token), now],
            |r| r.get(0),
        )
        .optional()?;
    let account = account.ok_or_else(ApiError::unauthorized)?;
    check_not_banned(conn, account, now)?;
    Ok(account)
}

/// Revoke one session.
pub fn logout(conn: &Connection, token: &str) -> ApiResult<()> {
    conn.execute(
        "UPDATE sessions SET revoked = 1 WHERE token_hash = ?1",
        params![token::hash(token)],
    )?;
    Ok(())
}

/// Revoke every session of an account (ban, password change).
pub fn revoke_all_sessions(conn: &Connection, account_id: AccountId) -> ApiResult<()> {
    conn.execute(
        "UPDATE sessions SET revoked = 1 WHERE account_id = ?1",
        params![account_id],
    )?;
    Ok(())
}

/// 403 `banned` if the account is banned at `now`; 404 if it does not exist.
pub fn check_not_banned(conn: &Connection, account_id: AccountId, now: UnixMs) -> ApiResult<()> {
    let row: Option<(Option<i64>, Option<String>)> = conn
        .query_row(
            "SELECT banned_until_ms, ban_reason FROM accounts WHERE id = ?1",
            params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(ApiError::not_found(format!("account {account_id}"))),
        Some((Some(until), reason)) if until > now => Err(ApiError::forbidden(
            "banned",
            format!(
                "account banned until {until}: {}",
                reason.unwrap_or_default()
            ),
        )),
        Some(_) => Ok(()),
    }
}

/// Record a link request for a provider. Always answers 501 until a provider is configured.
pub fn request_link(
    conn: &Connection,
    account_id: AccountId,
    provider: &str,
    now: UnixMs,
) -> ApiResult<()> {
    if provider.is_empty() || provider.len() > 32 {
        return Err(ApiError::bad_request("invalid provider"));
    }
    conn.execute(
        "INSERT INTO link_requests (account_id, provider, created_ms) VALUES (?1, ?2, ?3)",
        params![account_id, provider, now],
    )?;
    if LINKABLE_PROVIDERS.contains(&provider) {
        // Unreachable today; the OAuth redirect flow goes here.
        return Err(ApiError::internal("linking flow not implemented"));
    }
    Err(ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "provider_not_configured",
        format!("account linking with '{provider}' is not available yet"),
    ))
}

/// Public profile of an account.
pub fn profile(conn: &Connection, account_id: AccountId) -> ApiResult<Profile> {
    conn.query_row(
        "SELECT a.id, a.display_name, a.created_ms, cm.crew_id, pm.party_id,
                COALESCE(r.clean_rating, ?2)
         FROM accounts a
         LEFT JOIN crew_members cm ON cm.account_id = a.id
         LEFT JOIN party_members pm ON pm.account_id = a.id
         LEFT JOIN ratings r ON r.account_id = a.id
         WHERE a.id = ?1",
        params![account_id, crate::leaderboards::INITIAL_RATING],
        |r| {
            Ok(Profile {
                account_id: r.get(0)?,
                display_name: r.get(1)?,
                created_ms: r.get(2)?,
                crew_id: r.get(3)?,
                party_id: r.get(4)?,
                clean_rating: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| ApiError::not_found(format!("account {account_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let h = hash_password("correct horse", HashCost::Fast).unwrap();
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong horse", &h));
        assert!(!verify_password("x", "not a hash"));
        assert!(PasswordHash::new(dummy_phc(HashCost::Fast)).is_ok());
    }

    #[test]
    fn email_and_name_validation() {
        assert_eq!(normalise_email(" A@B.io ").unwrap(), "a@b.io");
        assert!(normalise_email("nope").is_err());
        assert!(normalise_email("@b.io").is_err());
        assert!(validate_display_name("Racer_01").is_ok());
        assert!(validate_display_name("ab").is_err());
        assert!(validate_display_name("bad name").is_err());
    }
}
