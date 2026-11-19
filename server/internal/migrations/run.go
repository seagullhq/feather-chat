package migrations

import (
	"database/sql"
	"errors"

	"github.com/golang-migrate/migrate/v4"
	"github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
)

// newMigrate opens a migrate instance over an existing connection pool.
//
// It deliberately does not close the migrate instance: the postgres driver
// wraps the shared *sql.DB and closing it would break the stores created
// right after.
func newMigrate(db *sql.DB) (*migrate.Migrate, error) {
	sourceDriver, err := iofs.New(FS, "sql")
	if err != nil {
		return nil, err
	}

	driver, err := postgres.WithInstance(db, &postgres.Config{})
	if err != nil {
		return nil, err
	}
	return migrate.NewWithInstance("iofs", sourceDriver, "postgres", driver)
}

// Up applies every pending migration, reporting whether anything changed.
func Up(db *sql.DB) (bool, error) {
	m, err := newMigrate(db)
	if err != nil {
		return false, err
	}
	if err := m.Up(); err != nil {
		if errors.Is(err, migrate.ErrNoChange) {
			return false, nil
		}
		return false, err
	}
	return true, nil
}

// migrateTo moves the schema to a specific version in either direction. It is
// used by the reversibility test and never by the server at runtime.
func migrateTo(db *sql.DB, version uint) error {
	m, err := newMigrate(db)
	if err != nil {
		return err
	}
	if err := m.Migrate(version); err != nil {
		if errors.Is(err, migrate.ErrNoChange) {
			return nil
		}
		return err
	}
	return nil
}
