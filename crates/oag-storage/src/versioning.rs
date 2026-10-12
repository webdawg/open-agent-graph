//! The generic mechanism behind spec/27-artifact-versioning.md: every
//! projected artifact row carries a `schema_version`, and every read applies
//! whatever migration chain is needed to serve it in its current shape
//! without touching disk (the "shim" access mode) -- `migrate_to_current`
//! repo functions additionally offer the "convert perm" access mode, which
//! rewrites the row so future reads skip the chain. Pure, no I/O, no
//! knowledge of any specific table -- that lives in each `repo::*` module.

/// A row that knows its own stored schema version.
pub trait HasSchemaVersion {
    fn schema_version(&self) -> i64;
}

/// One migration step: the version it applies from, and the function that
/// advances a row of type `T` past it (setting the row's own new
/// schema_version as part of what it returns).
pub type MigrationStep<T> = (i64, fn(T) -> T);

/// A row type with a declared current version and the chain of steps that
/// walk any older stored version forward to it. `MIGRATIONS` is empty for
/// every artifact type today -- no second shape exists yet for any of them --
/// which makes `migrate_to_current` a true no-op, the same opt-in-by-absence
/// shape as every other feature in this project.
pub trait Versioned: HasSchemaVersion + Sized + 'static {
    const CURRENT_VERSION: i64;
    const MIGRATIONS: &'static [MigrationStep<Self>];

    fn migrate_to_current(self) -> Self {
        migrate_to_version(self, Self::CURRENT_VERSION, Self::MIGRATIONS)
    }
}

/// Walks `row` forward through `steps` until its `schema_version()` reaches
/// `target_version` or no step matches its current version (meaning the
/// chain is incomplete for that row -- it's returned as-is rather than
/// panicking, since a half-migrated row read by old code is still better
/// than a crash). Each step is responsible for setting the row's own new
/// schema_version as part of what it returns.
pub fn migrate_to_version<T: HasSchemaVersion>(
    mut row: T,
    target_version: i64,
    steps: &[MigrationStep<T>],
) -> T {
    loop {
        let current = row.schema_version();
        if current >= target_version {
            return row;
        }
        match steps.iter().find(|(from, _)| *from == current) {
            Some((_, step)) => row = step(row),
            None => return row,
        }
    }
}

/// Reported by every `repo::*::migrate_to_current` function -- whether a
/// "convert perm" call actually rewrote the row or found it already current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationOutcome {
    AlreadyCurrent { version: i64 },
    Migrated { from: i64, to: i64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestRow {
        schema_version: i64,
        value: i32,
    }

    impl HasSchemaVersion for TestRow {
        fn schema_version(&self) -> i64 {
            self.schema_version
        }
    }

    impl Versioned for TestRow {
        const CURRENT_VERSION: i64 = 3;
        const MIGRATIONS: &'static [(i64, fn(TestRow) -> TestRow)] = &[
            (1, |r| TestRow { schema_version: 2, value: r.value * 10 }),
            (2, |r| TestRow { schema_version: 3, value: r.value + 1 }),
        ];
    }

    #[test]
    fn chain_applies_every_step_in_order() {
        let row = TestRow { schema_version: 1, value: 4 };
        let migrated = row.migrate_to_current();
        // 4 -> (*10) -> 40 -> (+1) -> 41, version 1 -> 2 -> 3
        assert_eq!(migrated, TestRow { schema_version: 3, value: 41 });
    }

    #[test]
    fn chain_starting_partway_through_only_applies_remaining_steps() {
        let row = TestRow { schema_version: 2, value: 4 };
        let migrated = row.migrate_to_current();
        assert_eq!(migrated, TestRow { schema_version: 3, value: 5 });
    }

    #[test]
    fn already_current_row_is_untouched() {
        let row = TestRow { schema_version: 3, value: 7 };
        let migrated = row.migrate_to_current();
        assert_eq!(migrated, TestRow { schema_version: 3, value: 7 });
    }

    #[test]
    fn missing_step_stops_the_chain_instead_of_panicking() {
        // schema_version 0 has no registered step -- an incomplete chain
        // should return the row as-is, not crash.
        let row = TestRow { schema_version: 0, value: 9 };
        let migrated = migrate_to_version(row.clone(), TestRow::CURRENT_VERSION, TestRow::MIGRATIONS);
        assert_eq!(migrated, row);
    }

    #[test]
    fn empty_migrations_is_a_true_no_op() {
        // The real shape every artifact type is in today: CURRENT_VERSION
        // equals every existing row's version, MIGRATIONS is empty.
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct RealisticRow {
            schema_version: i64,
        }
        impl HasSchemaVersion for RealisticRow {
            fn schema_version(&self) -> i64 {
                self.schema_version
            }
        }
        impl Versioned for RealisticRow {
            const CURRENT_VERSION: i64 = 1;
            const MIGRATIONS: &'static [(i64, fn(RealisticRow) -> RealisticRow)] = &[];
        }
        let row = RealisticRow { schema_version: 1 };
        assert_eq!(row.clone().migrate_to_current(), row);
    }
}
