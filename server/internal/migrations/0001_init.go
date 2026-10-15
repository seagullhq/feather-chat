package migrations

import (
	"context"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func init() {
	// Creates the users collection with a JSON schema validator.
	// Call register to add this migration to the registry.
	register(Migration{
		Version: 1,
		Name:    "init",
		Up: func(ctx context.Context, db *mongo.Database) error {
			names, err := db.ListCollectionNames(ctx, bson.D{{Key: "name", Value: "users"}})
			if err != nil {
				return err
			}
			if len(names) == 0 {
				validator := bson.D{{Key: "$jsonSchema", Value: bson.D{
					{Key: "bsonType", Value: "object"},
					{Key: "required", Value: bson.A{"username", "email", "password_hash", "created_at", "updated_at"}},
					{Key: "properties", Value: bson.D{
						{Key: "username", Value: bson.D{
							{Key: "bsonType", Value: "string"},
							{Key: "minLength", Value: 3},
							{Key: "maxLength", Value: 32},
						}},
						{Key: "email", Value: bson.D{{Key: "bsonType", Value: "string"}}},
						{Key: "password_hash", Value: bson.D{{Key: "bsonType", Value: "string"}}},
						{Key: "created_at", Value: bson.D{{Key: "bsonType", Value: "date"}}},
						{Key: "updated_at", Value: bson.D{{Key: "bsonType", Value: "date"}}},
					}},
				}}}
				if err := db.CreateCollection(ctx, "users", options.CreateCollection().SetValidator(validator)); err != nil {
					return err
				}
			}
			caseInsensitive := &options.Collation{Locale: "en", Strength: 2}
			_, err = db.Collection("users").Indexes().CreateMany(ctx, []mongo.IndexModel{
				{
					Keys:    bson.D{{Key: "username", Value: 1}},
					Options: options.Index().SetName("username_unique").SetUnique(true).SetCollation(caseInsensitive),
				},
				{
					Keys:    bson.D{{Key: "email", Value: 1}},
					Options: options.Index().SetName("email_unique").SetUnique(true).SetCollation(caseInsensitive),
				},
			})
			return err
		},
	})
}
