package servers

import (
	"context"
	"crypto/rand"
	"errors"
	"math/big"
	"time"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
)

// ErrTaken is returned when the unique index rejects a server creation.
var ErrTaken = errors.New("server already exists")

type ServerStore struct {
	servers *mongo.Collection
}

// The bson tags are mandatory: the servers collection has a $jsonSchema
// validator (migration 0002) requiring snake_case names, and without them the
// driver stores lowercased field names (ownerid, invitecode, ...) and every
// insert fails validation.
type Server struct {
	ID         bson.ObjectID   `bson:"_id"`
	Name       string          `bson:"name"`
	OwnerID    bson.ObjectID   `bson:"owner_id"`
	Members    []bson.ObjectID `bson:"members"`
	InviteCode string          `bson:"invite_code"`
	CreatedAt  time.Time       `bson:"created_at"`
	UpdatedAt  time.Time       `bson:"updated_at"`
}

const inviteCharset = "abcdefghijklmnopqrstuvwxyz" +
	"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"

// inviteCode returns a cryptographically random invite code. crypto/rand is
// safe for concurrent use; the shared math/rand.Rand this replaced was not, and
// CreateServer runs on one goroutine per connected client.
func inviteCode(length int) (string, error) {
	b := make([]byte, length)
	limit := big.NewInt(int64(len(inviteCharset)))
	for i := range b {
		n, err := rand.Int(rand.Reader, limit)
		if err != nil {
			return "", err
		}
		b[i] = inviteCharset[n.Int64()]
	}
	return string(b), nil
}

func NewStore(db *mongo.Database) *ServerStore {
	return &ServerStore{servers: db.Collection("servers")}
}

func (s *ServerStore) CreateServer(ctx context.Context, ownerID bson.ObjectID, serverName string) (*Server, error) {
	code, err := inviteCode(7)
	if err != nil {
		return nil, err
	}

	now := time.Now().UTC()
	server := Server{
		ID:         bson.NewObjectID(),
		Name:       serverName,
		OwnerID:    ownerID,
		Members:    []bson.ObjectID{ownerID},
		InviteCode: code,
		CreatedAt:  now,
		UpdatedAt:  now,
	}

	if _, err := s.servers.InsertOne(ctx, server); err != nil {
		if mongo.IsDuplicateKeyError(err) {
			return nil, ErrTaken
		}
		return nil, err
	}
	return &server, nil
}
