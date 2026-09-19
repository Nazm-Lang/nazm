//! Per-attempt PostgreSQL: a restricted role, fresh state, and a readiness check that
//! actually means something.
//!
//! # Never `POSTGRES_USER`
//!
//! The official image documents that the role named by `POSTGRES_USER` is created **with
//! superuser power**. Setting it to the application role would silently undo the entire
//! restricted-role decision, and nothing would look wrong. The role is created by an init
//! script instead, and its privileges are asserted per attempt rather than assumed.
//!
//! # Why superuser matters here
//!
//! A superuser can run `COPY … TO PROGRAM`, load untrusted procedural languages, read and
//! write server files, and set `archive_command` — every one of which is arbitrary code
//! execution inside the database container. Killing the candidate container does nothing
//! to a process started that way, because it lives in the database container's PID
//! namespace. The restriction is what makes the kill guarantee cover the whole attempt.

use std::path::{Path, PathBuf};

/// Credentials for one attempt's database. Generated per attempt, never reused.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub superuser_password: String,
    pub app_role: String,
    pub app_password: String,
    pub database: String,
}

impl Credentials {
    /// Generate from the OS random source.
    ///
    /// Not a cryptographic concern — the database is unreachable from outside its attempt
    /// network — but a fixed password would be shared across attempts, and shared state
    /// between attempts is the thing this whole layer exists to prevent.
    pub fn generate() -> std::io::Result<Self> {
        Ok(Self {
            superuser_password: random_hex()?,
            app_role: "app".to_owned(),
            app_password: random_hex()?,
            database: "appdb".to_owned(),
        })
    }

    /// The URL handed to the candidate. Points at the network alias, never a host port.
    #[must_use]
    pub fn candidate_url(&self) -> String {
        format!(
            "postgres://{}:{}@db:5432/{}",
            self.app_role, self.app_password, self.database
        )
    }
}

fn random_hex() -> std::io::Result<String> {
    use std::io::Read;
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write the init script the image runs during bootstrap.
///
/// Returns the directory to mount read-only at `/docker-entrypoint-initdb.d`. The script
/// runs while the server is listening on a **Unix socket only**, so the role exists before
/// TCP is ever reachable.
pub fn write_init_script(dir: &Path, creds: &Credentials) -> std::io::Result<PathBuf> {
    let init_dir = dir.join("pginit");
    std::fs::create_dir_all(&init_dir)?;

    // The password is passed as a psql variable, never interpolated into a shell string
    // or into SQL text, so no quoting mistake can turn it into a statement.
    let sql = format!(
        r"-- Created by nazm-bench. The application role is deliberately NOT the superuser
-- the image would have created from POSTGRES_USER.
\set app_pw `echo $APP_PASSWORD`

CREATE ROLE {role} LOGIN PASSWORD :'app_pw'
  NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS
  CONNECTION LIMIT 50;

CREATE DATABASE {db} OWNER {role} ENCODING 'UTF8' TEMPLATE template0;

-- PUBLIC gets nothing by default; the app role gets exactly what it needs.
REVOKE ALL ON DATABASE {db} FROM PUBLIC;
REVOKE ALL ON DATABASE postgres FROM PUBLIC;
GRANT CONNECT, TEMPORARY ON DATABASE {db} TO {role};

\connect {db}
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE, CREATE ON SCHEMA public TO {role};
",
        role = creds.app_role,
        db = creds.database
    );

    std::fs::write(init_dir.join("10-role.sql"), sql)?;
    Ok(init_dir)
}

/// Drop and recreate the application database, as the superuser.
///
/// Run between the development phase and grading. Fresh state *between attempts* is not
/// the same property: within one attempt the candidate has spent its whole run writing to
/// this database, and grading it against that state would let a submission pass by having
/// left the right rows behind rather than by doing the work. The acceptance phase has to
/// start from the same empty database the task statement describes.
///
/// `WITH (FORCE)` because the candidate's connections are still open — it is not asked to
/// Returned as separate statements, not one string: `psql -c` wraps a multi-statement
/// argument in a single transaction, and `DROP DATABASE` cannot run inside one. Each has
/// to be sent on its own — found the direct way, by the reset failing with
/// "DROP DATABASE cannot run inside a transaction block".
#[must_use]
pub fn reset_statements(creds: &Credentials) -> Vec<String> {
    vec![
        format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", creds.database),
        format!(
            "CREATE DATABASE {db} OWNER {role} ENCODING 'UTF8' TEMPLATE template0",
            db = creds.database,
            role = creds.app_role
        ),
        // The grants the init script set up do not survive a recreate.
        format!("REVOKE ALL ON DATABASE {} FROM PUBLIC", creds.database),
        format!(
            "GRANT CONNECT, TEMPORARY ON DATABASE {db} TO {role}",
            db = creds.database,
            role = creds.app_role
        ),
    ]
}

/// The SQL that checks the role against the escalation routes this project tests for.
///
/// Run once per attempt after readiness. Every value must be `f`. This turns the central
/// security decision from a comment into a recorded per-attempt fact, and costs about
/// fifty milliseconds.
///
/// It covers the **tested** routes — the five role attributes and the three server-file /
/// program-execution predefined roles — not every conceivable one. A future PostgreSQL
/// release could add another, and this list would not know.
#[must_use]
pub fn privilege_assertion_sql(role: &str) -> String {
    format!(
        "SELECT rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls \
         AS too_much FROM pg_roles WHERE rolname = '{role}' \
         UNION ALL SELECT pg_has_role('{role}', 'pg_execute_server_program', 'member') \
         UNION ALL SELECT pg_has_role('{role}', 'pg_read_server_files', 'member') \
         UNION ALL SELECT pg_has_role('{role}', 'pg_write_server_files', 'member')"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_unique_per_attempt() {
        let a = Credentials::generate().unwrap();
        let b = Credentials::generate().unwrap();
        assert_ne!(a.app_password, b.app_password);
        assert_ne!(a.superuser_password, b.superuser_password);
        assert_eq!(a.app_password.len(), 32);
    }

    #[test]
    fn the_candidate_url_points_at_the_alias_not_a_host_port() {
        let c = Credentials::generate().unwrap();
        let url = c.candidate_url();
        assert!(url.contains("@db:5432/"), "{url}");
        assert!(
            !url.contains("localhost") && !url.contains("127.0.0.1"),
            "{url}"
        );
    }

    #[test]
    fn the_init_script_creates_a_role_that_is_not_a_superuser() {
        let dir = tempfile::tempdir().unwrap();
        let creds = Credentials::generate().unwrap();
        let init = write_init_script(dir.path(), &creds).unwrap();
        let sql = std::fs::read_to_string(init.join("10-role.sql")).unwrap();

        for required in [
            "NOSUPERUSER",
            "NOCREATEDB",
            "NOCREATEROLE",
            "NOREPLICATION",
            "NOBYPASSRLS",
        ] {
            assert!(sql.contains(required), "init script missing {required}");
        }
        // The password must not appear as literal text anywhere in the script.
        assert!(
            !sql.contains(&creds.app_password),
            "the password was interpolated into SQL"
        );
    }

    #[test]
    fn the_reset_forces_the_candidates_connections_closed() {
        // The candidate is not asked to disconnect, and would not be trusted to if it
        let c = Credentials::generate().unwrap();
        let stmts = reset_statements(&c);
        let joined = stmts.join(" | ");
        assert!(joined.contains("WITH (FORCE)"), "{joined}");
        assert!(
            joined.contains("TEMPLATE template0"),
            "must not inherit template1: {joined}"
        );
        assert!(joined.contains(&format!("OWNER {}", c.app_role)));
        // Separate statements, not one string: `psql -c` wraps a multi-statement argument
        // in a transaction, and DROP DATABASE cannot run inside one.
        assert!(
            stmts.iter().all(|s| !s.contains(';')),
            "statements must not be joined: {stmts:?}"
        );
        assert!(
            stmts.len() >= 4,
            "the reset must restore the grants too: {stmts:?}"
        );
    }

    #[test]
    fn the_privilege_assertion_covers_the_tested_escalation_routes() {
        let sql = privilege_assertion_sql("app");
        for route in [
            "rolsuper",
            "rolcreatedb",
            "rolcreaterole",
            "rolreplication",
            "rolbypassrls",
            "pg_execute_server_program",
            "pg_read_server_files",
            "pg_write_server_files",
        ] {
            assert!(sql.contains(route), "assertion does not cover {route}");
        }
    }
}
