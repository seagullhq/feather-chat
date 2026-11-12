package migrations

import (
	"database/sql"
	"errors"

	"github.com/golang-migrate/migrate/v4"
	"github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
)

// Up applies every pending migration, reporting whether anything changed.
//
// It deliberately does not close the migrate instance: the postgres driver
// wraps the shared *sql.DB and closing it would break the stores created
// right after.
func Up(db *sql.DB) (bool, error) {
	sourceDriver, err := iofs.New(FS, "sql")
	if err != nil {
		return false, err
	}

	driver, err := postgres.WithInstance(db, &postgres.Config{})
	if err != nil {
		return false, err
	}

	m, err := migrate.NewWithInstance("iofs", sourceDriver, "postgres", driver)
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
