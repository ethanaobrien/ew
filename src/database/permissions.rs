use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, object, JsonValue};

use crate::router::global;
use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("permissions.db", setup_tables);
}

// Scopes are flat dotted strings and imply everything below them: holding
// "card" grants "card.upload", and "*" grants everything. That hierarchy is
// the only notion of "level" - there is no rank integer, so a new capability
// is one line here and never a renumbering of anything else.
//
// Every call site must pass one of these consts, never a literal: an
// unrecognised scope string can only ever fail closed in has(), but grant()
// rejects it outright so a typo can't be persisted either.
pub const ALL: &str = "*";

pub const CARD: &str = "card";
// Create custom cards/characters, and edit or delete your OWN uploads
pub const CARD_UPLOAD: &str = "card.upload";
// Publish/unpublish and mark obtainable, on your OWN uploads
pub const CARD_PUBLISH: &str = "card.publish";
// Moderation: edit, delete, publish or unpublish ANYBODY's cards
pub const CARD_EDIT: &str = "card.edit";

pub const PERMISSION: &str = "permission";
pub const PERMISSION_GRANT: &str = "permission.grant";
pub const PERMISSION_REVOKE: &str = "permission.revoke";

// The whole grantable vocabulary, subtree roots included. Anything not in here
// cannot be written to the table
pub const SCOPES: &[&str] = &[
    ALL,
    CARD, CARD_UPLOAD, CARD_PUBLISH, CARD_EDIT,
    PERMISSION, PERMISSION_GRANT, PERMISSION_REVOKE
];

// Grants live in their own database rather than in userdata.db so that an
// account purge can never take administrative state with it
fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS grants (
    user_id     BIGINT NOT NULL,
    scope       TEXT NOT NULL,
    granted_by  BIGINT NOT NULL,
    granted_at  BIGINT NOT NULL,
    PRIMARY KEY (user_id, scope)
);
    ").unwrap();
}

// The owners are a process-level flag (--owner) rather than table rows: they
// are the bootstrap grantors, so they have to work on a fresh install with an
// empty (or hand-deleted) permissions.db, and they must not be revocable
// through the webui
fn is_owner(user_id: i64) -> bool {
    user_id > 0 && crate::runtime::get_owners().contains(&user_id)
}

// Every scope that would satisfy a request for `scope`: "*", each dotted
// ancestor, and the scope itself. Matching is on whole dot-separated segments,
// so "car" never satisfies "card.upload"
fn implied_by(scope: &str) -> Vec<String> {
    if scope == ALL {
        return vec![String::from(ALL)];
    }
    let mut rv = vec![String::from(ALL)];
    let mut prefix = String::new();
    for part in scope.split('.') {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        rv.push(prefix.clone());
    }
    rv
}

fn held_scopes(user_id: i64) -> Vec<String> {
    let rows = DATABASE.lock_and_select_all("SELECT scope FROM grants WHERE user_id=?1 ORDER BY scope", params!(user_id)).unwrap_or(array![]);
    rows.members().map(|scope| scope.to_string()).collect()
}

fn insert(user_id: i64, scope: &str, granted_by: i64) {
    DATABASE.lock_and_exec(
        "INSERT OR IGNORE INTO grants (user_id, scope, granted_by, granted_at) VALUES (?1, ?2, ?3, ?4)",
        params!(user_id, scope, granted_by, global::timestamp() as i64)
    );
}

// A user with no row holds nothing at all
pub fn has(user_id: i64, scope: &str) -> bool {
    if user_id <= 0 {
        return false;
    }
    if is_owner(user_id) {
        return true;
    }
    let held = held_scopes(user_id);
    implied_by(scope).iter().any(|candidate| held.contains(candidate))
}

// Everything this user holds, for the webui to hide what it can't use. An
// owner's implicit "*" is reported here even though it has no row
pub fn scopes_for(user_id: i64) -> JsonValue {
    if user_id <= 0 {
        return array![];
    }
    let mut scopes: Vec<String> = Vec::new();
    if is_owner(user_id) {
        scopes.push(String::from(ALL));
    }
    for scope in held_scopes(user_id) {
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    let mut rv = array![];
    for scope in scopes {
        rv.push(scope).unwrap();
    }
    rv
}

pub fn grants() -> JsonValue {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    let Ok(mut stmt) = conn.prepare("SELECT user_id, scope, granted_by, granted_at FROM grants ORDER BY user_id, scope") else {
        return array![];
    };
    let Ok(mapped) = stmt.query_map(params!(), |row| {
        Ok(object!{
            user_id: row.get::<usize, i64>(0)?,
            scope: row.get::<usize, String>(1)?,
            granted_by: row.get::<usize, i64>(2)?,
            granted_at: row.get::<usize, i64>(3)?
        })
    }) else {
        return array![];
    };
    let mut rv = array![];
    for row in mapped.flatten() {
        rv.push(row).unwrap();
    }
    rv
}

// The only way a row is ever written. Two conditions, both required:
//
//   1. the grantor holds permission.grant, and
//   2. the grantor holds the scope being granted
//
// (2) is what makes escalation impossible. has() only ever implies downwards,
// so holding a leaf never satisfies its parent - a user with card.upload can
// hand out card.upload and nothing else, and can no more grant themselves
// "card" (or "*") than they can grant it to anybody else. Both checks read the
// live table, so a revoked grantor loses the ability on their next request
pub fn grant(user_id: i64, scope: &str, granted_by: i64) -> Result<(), String> {
    if user_id <= 0 {
        return Err(String::from("Invalid user id"));
    }
    if !SCOPES.contains(&scope) {
        return Err(format!("Unknown scope '{}'", scope));
    }
    if !has(granted_by, PERMISSION_GRANT) {
        return Err(String::from("You do not have permission to grant scopes"));
    }
    if !has(granted_by, scope) {
        return Err(format!("You cannot grant '{}' because you do not hold it yourself", scope));
    }
    insert(user_id, scope, granted_by);
    Ok(())
}

// Revoking needs the same "you must hold it yourself" rule as granting, so a
// junior admin can't strip a senior one. An owner has no rows to delete, but
// the check is explicit so it stays true if that ever changes
pub fn revoke(user_id: i64, scope: &str, revoked_by: i64) -> Result<(), String> {
    if user_id <= 0 {
        return Err(String::from("Invalid user id"));
    }
    if !SCOPES.contains(&scope) {
        return Err(format!("Unknown scope '{}'", scope));
    }
    if !has(revoked_by, PERMISSION_REVOKE) {
        return Err(String::from("You do not have permission to revoke scopes"));
    }
    if !has(revoked_by, scope) {
        return Err(format!("You cannot revoke '{}' because you do not hold it yourself", scope));
    }
    if is_owner(user_id) {
        return Err(String::from("A server owner's scopes cannot be revoked"));
    }
    DATABASE.lock_and_exec("DELETE FROM grants WHERE user_id=?1 AND scope=?2", params!(user_id, scope));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every test holds the shared test data path lock, so uids are hand-picked
    // to not collide between tests rather than cleaned up between them
    fn wipe(user_id: i64) {
        DATABASE.lock_and_exec("DELETE FROM grants WHERE user_id=?1", params!(user_id));
    }

    #[test]
    fn subtree_implies_leaf_but_not_the_reverse() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(101);
        assert!(!has(101, CARD_UPLOAD));
        insert(101, CARD, 0);
        assert!(has(101, CARD_UPLOAD));
        assert!(has(101, CARD_EDIT));
        assert!(has(101, CARD));
        assert!(!has(101, PERMISSION_GRANT));
        assert!(!has(101, ALL));
        wipe(101);

        insert(101, CARD_UPLOAD, 0);
        assert!(has(101, CARD_UPLOAD));
        assert!(!has(101, CARD));
        assert!(!has(101, CARD_PUBLISH));
        wipe(101);
    }

    #[test]
    fn star_implies_everything() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(103);
        insert(103, ALL, 0);
        for scope in SCOPES {
            assert!(has(103, scope), "scope {}", scope);
        }
        wipe(103);
    }

    #[test]
    fn partial_segment_is_not_a_prefix() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(104);
        insert(104, "car", 0);
        assert!(!has(104, CARD_UPLOAD));
        assert!(!has(104, CARD));
        insert(104, "card.uplo", 0);
        assert!(!has(104, CARD_UPLOAD));
        wipe(104);
    }

    #[test]
    fn absent_user_holds_nothing() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(105);
        for scope in SCOPES {
            assert!(!has(105, scope), "scope {}", scope);
        }
        assert!(scopes_for(105).is_empty());
        assert!(!has(0, ALL));
        assert!(!has(-1, ALL));
        wipe(105);
    }

    #[test]
    fn grant_and_revoke_need_the_scope_and_the_ability() {
        let _lock = crate::runtime::lock_test_data_path();
        for uid in [110, 111, 112, 113] {
            wipe(uid);
        }
        // A grantor with permission.grant and card.upload can hand out
        // card.upload and nothing else - escalation is impossible
        insert(110, PERMISSION_GRANT, 0);
        insert(110, CARD_UPLOAD, 0);
        grant(111, CARD_UPLOAD, 110).unwrap();
        grant(111, CARD_UPLOAD, 110).unwrap(); // idempotent
        assert_eq!(held_scopes(111), vec![String::from(CARD_UPLOAD)]);
        assert!(grant(111, CARD_EDIT, 110).is_err());
        assert!(grant(111, CARD, 110).is_err());
        assert!(grant(110, ALL, 110).is_err());
        assert!(!has(111, CARD_EDIT));

        // No permission.grant / permission.revoke - no managing at all
        insert(112, ALL, 0);
        insert(113, CARD_UPLOAD, 0);
        assert!(grant(113, CARD_UPLOAD, 113).is_err());
        assert!(revoke(112, ALL, 113).is_err());

        // Revoking needs the revoked scope held too
        insert(110, PERMISSION_REVOKE, 0);
        assert!(revoke(112, ALL, 110).is_err());
        assert!(has(112, ALL));
        revoke(111, CARD_UPLOAD, 110).unwrap();
        assert!(!has(111, CARD_UPLOAD));

        for uid in [110, 111, 112, 113] {
            wipe(uid);
        }
    }

    #[test]
    fn unknown_scopes_are_rejected() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(116);
        wipe(117);
        insert(116, ALL, 0);
        assert!(grant(117, "card.explode", 116).is_err());
        assert!(revoke(117, "card.explode", 116).is_err());
        assert!(grant(117, "song.upload", 116).is_err());
        assert!(grant(0, CARD_UPLOAD, 116).is_err());
        wipe(116);
        wipe(117);
    }

    #[test]
    fn owners_hold_everything_without_a_row() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(118);
        wipe(119);
        crate::runtime::update_owners(&[118, 120]);
        for scope in SCOPES {
            assert!(has(118, scope), "scope {}", scope);
            assert!(has(120, scope), "scope {}", scope);
        }
        assert_eq!(scopes_for(118).len(), 1);
        assert_eq!(scopes_for(118)[0].to_string(), String::from(ALL));
        assert!(held_scopes(118).is_empty());
        // An owner can bootstrap-grant, and can't be revoked
        grant(119, ALL, 118).unwrap();
        assert!(has(119, ALL));
        insert(118, CARD_UPLOAD, 0);
        assert!(revoke(118, CARD_UPLOAD, 119).is_err());
        assert!(has(118, CARD_UPLOAD));
        // Dropping owner status drops the implicit "*"
        crate::runtime::update_owners(&[]);
        assert!(!has(118, CARD_EDIT));
        wipe(118);
        wipe(119);
    }

    #[test]
    fn the_vocabulary_is_wellformed() {
        for scope in SCOPES {
            assert!(!scope.is_empty());
            assert!(!scope.ends_with('.'));
        }
        for scope in [CARD_UPLOAD, CARD_PUBLISH, CARD_EDIT, PERMISSION_GRANT, PERMISSION_REVOKE] {
            assert!(SCOPES.contains(&scope), "scope {}", scope);
        }
    }
}
