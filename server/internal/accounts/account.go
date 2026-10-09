package accounts

import (
	"context"
	"errors"
	"strings"
	"time"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
	"golang.org/x/crypto/bcrypt"
)

var (
	ErrTaken          = errors.New("username or email already registered")
	ErrBadCredentials = errors.New("invalid credentials")
)

type User struct {
	ID           bson.ObjectID `bson:"_id"`
	Username     string        `bson:"username"`
	Email        string        `bson:"email"`
	PasswordHash string        `bson:"password_hash"`
	CreatedAt    time.Time     `bson:"created_at"`
	UpdatedAt    time.Time     `bson:"updated_at"`
}

type Store struct {
	users *mongo.Collection
}

func NewStore(db *mongo.Database) *Store {
	return &Store{users: db.Collection("users")}
}

var caseInsensitive = options.Collation{Locale: "en", Strength: 2}
var dummyHash, _ = bcrypt.GenerateFromPassword([]byte("feather-dummy"), bcrypt.DefaultCost)

func (s *Store) Register(ctx context.Context, username, email, password string) error {
	hash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return err
	}
	now := time.Now()
	_, err = s.users.InsertOne(ctx, User{
		ID:           bson.NewObjectID(),
		Username:     strings.TrimSpace(username),
		Email:        strings.TrimSpace(email),
		PasswordHash: string(hash),
		CreatedAt:    now,
		UpdatedAt:    now,
	})
	if mongo.IsDuplicateKeyError(err) {
		return ErrTaken
	}
	return err
}

func (s *Store) Authenticate(ctx context.Context, identifier string, password string) (*User, error) {
	filter := bson.D{{Key: "$or", Value: bson.A{
		bson.D{{Key: "username", Value: strings.TrimSpace(identifier)}},
		bson.D{{Key: "email", Value: strings.TrimSpace(identifier)}},
	}}}
	var user User
	err := s.users.FindOne(ctx, filter, options.FindOne().SetCollation(&caseInsensitive)).Decode(&user)
	if errors.Is(err, mongo.ErrNoDocuments) {
		// Burn a comparison so a missing account is not measurably faster than
		// a wrong password, then answer with the same error.
		_ = bcrypt.CompareHashAndPassword(dummyHash, []byte(password))
		return nil, ErrBadCredentials
	}
	if err != nil {
		return nil, err
	}
	if bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(password)) != nil {
		return nil, ErrBadCredentials
	}
	return &user, nil
}
