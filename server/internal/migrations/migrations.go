package migrations

import (
	"context"
	"fmt"
	"sort"
	"time"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
)

type Migration struct {
	Version int
	Name    string
	Up      func(ctx context.Context, db *mongo.Database) error
}

var registry []Migration

// Register is called from the init() functions of individual migration files.
func register(m Migration) {
	for _, existing := range registry {
		if existing.Version == m.Version {
			panic(fmt.Sprintf("migrations: versione %d duplicata (%s, %s)", m.Version, existing.Name, m.Name))
		}
	}
	registry = append(registry, m)
}

func Run(ctx context.Context, db *mongo.Database) error {
	migs := append([]Migration(nil), registry...)
	sort.Slice(migs, func(i, j int) bool { return migs[i].Version < migs[j].Version })
	col := db.Collection("schema_migrations")

	for _, m := range migs {
		_, err := col.InsertOne(ctx, bson.D{
			{Key: "_id", Value: m.Version},
			{Key: "name", Value: m.Name},
			{Key: "applied_at", Value: time.Now()},
		})
		if mongo.IsDuplicateKeyError(err) {
			continue
		}
		if err != nil {
			return err
		}

		if err := m.Up(ctx, db); err != nil {
			_, _ = col.DeleteOne(ctx, bson.D{{Key: "_id", Value: m.Version}})
			return fmt.Errorf("migration %d (%s): %w", m.Version, m.Name, err)
		}
		fmt.Printf("applied %d %s\n", m.Version, m.Name)
	}
	return nil
}
