package migrations

import (
	"context"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func init() {
	// Adds the _id type to the users validator and creates the servers collection,
	// linked to users through owner_id and members, with a unique invite_code.
	register(Migration{
		Version: 2,
		Name:    "create_server",
		Up: func(ctx context.Context, db *mongo.Database) error {
			// 1. users: aggiunge _id allo schema esistente
			usersValidator := bson.D{{Key: "$jsonSchema", Value: bson.D{
				{Key: "bsonType", Value: "object"},
				{Key: "required", Value: bson.A{"username", "email", "password_hash", "created_at", "updated_at"}},
				{Key: "properties", Value: bson.D{
					{Key: "_id", Value: bson.D{{Key: "bsonType", Value: "objectId"}}},
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
			if err := db.RunCommand(ctx, bson.D{
				{Key: "collMod", Value: "users"},
				{Key: "validator", Value: usersValidator},
			}).Err(); err != nil {
				return err
			}

			// 2. servers: crea la collection se non esiste
			names, err := db.ListCollectionNames(ctx, bson.D{{Key: "name", Value: "servers"}})
			if err != nil {
				return err
			}
			if len(names) == 0 {
				serversValidator := bson.D{{Key: "$jsonSchema", Value: bson.D{
					{Key: "bsonType", Value: "object"},
					{Key: "required", Value: bson.A{"name", "owner_id", "members", "invite_code", "created_at", "updated_at"}},
					{Key: "properties", Value: bson.D{
						{Key: "_id", Value: bson.D{{Key: "bsonType", Value: "objectId"}}},
						{Key: "name", Value: bson.D{
							{Key: "bsonType", Value: "string"},
							{Key: "minLength", Value: 3},
							{Key: "maxLength", Value: 32},
						}},
						// riferimento a users._id
						{Key: "owner_id", Value: bson.D{{Key: "bsonType", Value: "objectId"}}},
						// elenco di users._id che fanno parte del server
						{Key: "members", Value: bson.D{
							{Key: "bsonType", Value: "array"},
							{Key: "items", Value: bson.D{{Key: "bsonType", Value: "objectId"}}},
						}},
						// codice per entrare nel server
						{Key: "invite_code", Value: bson.D{
							{Key: "bsonType", Value: "string"},
							{Key: "minLength", Value: 6},
							{Key: "maxLength", Value: 32},
						}},
						{Key: "created_at", Value: bson.D{{Key: "bsonType", Value: "date"}}},
						{Key: "updated_at", Value: bson.D{{Key: "bsonType", Value: "date"}}},
					}},
				}}}
				if err := db.CreateCollection(ctx, "servers", options.CreateCollection().SetValidator(serversValidator)); err != nil {
					return err
				}
			}

			// 3. indici: server di un utente + lookup per invite code
			_, err = db.Collection("servers").Indexes().CreateMany(ctx, []mongo.IndexModel{
				{
					Keys:    bson.D{{Key: "owner_id", Value: 1}},
					Options: options.Index().SetName("owner_id_idx"),
				},
				{
					Keys:    bson.D{{Key: "members", Value: 1}},
					Options: options.Index().SetName("members_idx"),
				},
				{
					Keys:    bson.D{{Key: "invite_code", Value: 1}},
					Options: options.Index().SetName("invite_code_unique").SetUnique(true),
				},
			})
			return err
		},
	})
}
