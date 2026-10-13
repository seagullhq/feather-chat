package servers

import (
	"context"
	"errors"
	"math/rand"
	"time"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
)

var (
	ErrTaken          = errors.New("username or email already registered")
	ErrBadCredentials = errors.New("invalid credentials")
)

type ServerStore struct {
	servers *mongo.Collection
}

type Server struct {
	ID         bson.ObjectID
	Name       string
	OwnerID    bson.ObjectID
	Members    []bson.ObjectID
	InviteCode string
	CreatedAt  time.Time
	UpdatedAt  time.Time
}

const charset = "abcdefghijklmnopqrstuvwxyz" +
	"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"

var seededRand *rand.Rand = rand.New(
	rand.NewSource(time.Now().UnixNano()))

func StringWithCharset(length int, charset string) string {
	b := make([]byte, length)
	for i := range b {
		b[i] = charset[seededRand.Intn(len(charset))]
	}
	return string(b)
}

func String(length int) string {
	return StringWithCharset(length, charset)
}

func NewStore(db *mongo.Database) *ServerStore {
	return &ServerStore{servers: db.Collection("servers")}
}

func (s *ServerStore) CreateServer(ctx context.Context, ownerID bson.ObjectID, serverName string) (*Server, error) {

	now := time.Now().UTC()
	server := Server{
		ID:         bson.NewObjectID(),
		Name:       serverName,
		OwnerID:    ownerID,
		Members:    []bson.ObjectID{ownerID},
		InviteCode: String(7),
		CreatedAt:  now,
		UpdatedAt:  now,
	}

	_, err := s.servers.InsertOne(ctx, server)

	if err == nil {
		return nil, err
	}
	if !mongo.IsDuplicateKeyError(err) {
		return nil, err
	}
	return &server, nil
}
