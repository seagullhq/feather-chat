package control

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"io"
	"strconv"
	"testing"
	"time"

	"golang.org/x/crypto/bcrypt"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// oneUser is a database whose every query returns the same account.
type oneUser struct{ hash string }

func (u oneUser) Connect(context.Context) (driver.Conn, error) { return u, nil }
func (u oneUser) Driver() driver.Driver                        { return nil }
func (u oneUser) Prepare(string) (driver.Stmt, error)          { return u, nil }
func (u oneUser) Close() error                                 { return nil }
func (u oneUser) Begin() (driver.Tx, error)                    { return nil, driver.ErrSkip }
func (u oneUser) NumInput() int                                { return -1 }
func (u oneUser) Exec([]driver.Value) (driver.Result, error)   { return nil, driver.ErrSkip }
func (u oneUser) Query([]driver.Value) (driver.Rows, error) {
	return &userRows{hash: u.hash}, nil
}

type userRows struct {
	hash string
	read bool
}

func (r *userRows) Columns() []string {
	return []string{"id", "username", "email", "password_hash", "created_at", "updated_at"}
}
func (r *userRows) Close() error { return nil }
func (r *userRows) Next(dest []driver.Value) error {
	if r.read {
		return io.EOF
	}
	r.read = true
	copy(dest, []driver.Value{int64(7), "victim", "victim@example.test", r.hash, time.Time{}, time.Time{}})
	return nil
}

// startUserServer serves a control server whose only account is "victim" with
// password "password123", and returns a function that sends LOGIN to it.
func startUserServer(t *testing.T) (*Server, func(identifier, password string, token ...string) protocol.Control) {
	t.Helper()
	hash, err := bcrypt.GenerateFromPassword([]byte("password123"), bcrypt.DefaultCost)
	if err != nil {
		t.Fatal(err)
	}
	db := sql.OpenDB(oneUser{string(hash)})
	t.Cleanup(func() { db.Close() })
	s, addr := startServer(t)
	s.users = accounts.NewStore(db) // set before the first connection
	c, err := dialTLSTest(t, addr, 0)
	if err != nil {
		t.Fatal(err)
	}
	c.SetDeadline(time.Now().Add(30 * time.Second))
	return s, func(identifier, password string, token ...string) protocol.Control {
		msg := protocol.Control{Type: "LOGIN", Identifier: identifier, Password: password}
		if len(token) > 0 {
			msg.DeviceToken = token[0]
		}
		sendFrame(t, c, msg)
		return recvFrame(t, c)
	}
}

// Username and email of one account share the failed-login budget.
func TestLoginLimitPerAccount(t *testing.T) {
	s, login := startUserServer(t)
	for i := 0; i < failedBurst; i++ {
		if m := login("victim", "wrong-password"); m.Error != loginRefused {
			t.Fatalf("failure %d = %+v", i, m)
		}
	}
	// The right password is refused, with the answer of an unknown identifier.
	if m := login("victim@example.test", "password123"); m.Type != "ERROR" || m.Error != loginRefused {
		t.Fatalf("LOGIN by email = %+v", m)
	}
	// Another source is not locked out.
	done, ok := s.reserveLogin("7", "owner")
	if !ok {
		t.Fatal("owner locked out")
	}
	done(false)
}

// A successful LOGIN frees the unauthenticated slot.
func TestLoginPromotes(t *testing.T) {
	s, login := startUserServer(t)
	if m := login("victim", "password123"); m.Type != "ACK" {
		t.Fatalf("LOGIN = %+v", m)
	}
	s.conns.mu.Lock()
	defer s.conns.mu.Unlock()
	if len(s.conns.unauth) != 0 || s.conns.total != 1 {
		t.Fatalf("unauth = %d, total = %d", len(s.conns.unauth), s.conns.total)
	}
}

// A device that logged in before is not locked out by others' failures.
func TestDeviceTokenBypassesAccountLimit(t *testing.T) {
	s, login := startUserServer(t)
	first := login("victim", "password123")
	if first.Type != "ACK" || first.DeviceToken == "" {
		t.Fatalf("LOGIN = %+v", first)
	}
	for i := 0; i < accountBurst; i++ {
		done, ok := s.reserveLogin("7", "attacker"+strconv.Itoa(i))
		if !ok {
			t.Fatalf("failure %d refused", i)
		}
		done(true)
	}
	if m := login("victim", "password123"); m.Type != "ERROR" || m.Error != loginRefused {
		t.Fatalf("LOGIN without token = %+v", m)
	}
	if m := login("victim", "password123", first.DeviceToken); m.Type != "ACK" {
		t.Fatalf("LOGIN with token = %+v", m)
	}
	// The token does not allow unlimited guessing.
	for i := 0; i < failedBurst; i++ {
		login("victim", "wrong-password", first.DeviceToken)
	}
	if m := login("victim", "password123", first.DeviceToken); m.Type != "ERROR" {
		t.Fatalf("LOGIN over the device budget = %+v", m)
	}
}

func TestDeviceToken(t *testing.T) {
	s := New(nil, nil, nil, nil, room.NewManager())
	now := time.Now()
	token := s.issueDeviceToken(7, "hash", now)
	if _, ok := s.checkDeviceToken(token, 7, "hash", now); !ok {
		t.Fatal("fresh token refused")
	}
	changed := "A" + token[1:]
	if changed == token {
		changed = "B" + token[1:]
	}
	for name, ok := range map[string]bool{
		"other user":     second(s.checkDeviceToken(token, 8, "hash", now)),
		"other password": second(s.checkDeviceToken(token, 7, "new hash", now)),
		"expired":        second(s.checkDeviceToken(token, 7, "hash", now.Add(deviceTokenLife+time.Second))),
		"altered":        second(s.checkDeviceToken(changed, 7, "hash", now)),
		"empty":          second(s.checkDeviceToken("", 7, "hash", now)),
	} {
		if ok {
			t.Errorf("%s: token accepted", name)
		}
	}
}

func second(_ string, ok bool) bool { return ok }
