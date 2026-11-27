// Command server runs the feather-chat server: a TCP control plane plus a
// UDP media plane over PostgreSQL.
package main

import (
	"context"
	"database/sql"
	"flag"
	"fmt"
	"log"
	"net"
	"os"
	"time"

	"github.com/joho/godotenv"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/config"
	"github.com/seagullhq/feather-chat/server/internal/control"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/migrations"
	"github.com/seagullhq/feather-chat/server/internal/room"
	"github.com/seagullhq/feather-chat/server/internal/servers"
	"github.com/seagullhq/feather-chat/server/internal/tlscert"

	// PostgreSQL driver via pgx, compatible with database/sql
	_ "github.com/jackc/pgx/v5/stdlib"
)

func main() {
	// Keep logs on stdout so launchers capturing only stdout still show them.
	log.SetOutput(os.Stdout)
	log.Println("feather-chat server: starting")
	if err := godotenv.Load(); err != nil {
		if err2 := godotenv.Load("../.env"); err2 != nil {
			log.Printf(".env not loaded (%v); using environment only", err)
		}
	}

	pgURI := flag.String("postgres", config.PostgresURI(), "PostgreSQL connection URI")

	// Define tcp and udp address
	tcpAddr := flag.String("tcp", ":7700", "TCP control address")
	udpAddr := flag.String("udp", ":7701", "UDP media address")
	tlsCert := flag.String("tls-cert", "", "PEM certificate of the control channel (with -tls-key)")
	tlsKey := flag.String("tls-key", "", "PEM private key of the control channel (with -tls-cert)")
	tlsDir := flag.String("tls-dir", config.TLSDir(), "directory of cert.pem and key.pem, generated if both are missing (env FEATHER_TLS_DIR)")
	resetUser := flag.String("reset-user", "", "reset the account of this username (new temporary password, keys retired) and exit")
	flag.Parse()

	// 1. Connect to PostgreSQL using the pgx driver
	db, err := sql.Open("pgx", *pgURI)
	if err != nil {
		log.Fatalf("failed to open DB connection: %v", err)
	}
	defer db.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	// Check the connection
	if err := db.PingContext(ctx); err != nil {
		log.Fatalf("postgres ping failed: %v", err)
	}
	log.Println("Connected to PostgreSQL")

	// 2. Run the migrations
	applied, err := migrations.Up(db)
	if err != nil {
		log.Fatalf("failed to run migrations: %v", err)
	}
	if applied {
		log.Println("Migrations applied")
	} else {
		log.Println("No new migrations to apply.")
	}

	// 3. Initialize the stores with the *sql.DB instance
	users := accounts.NewStore(db)
	if *resetUser != "" {
		resetCtx, resetCancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer resetCancel()
		password, err := users.Reset(resetCtx, *resetUser)
		if err != nil {
			log.Fatalf("failed to reset user %q: %v", *resetUser, err)
		}
		// Printed once, outside the log.
		fmt.Printf("temporary password of %q: %s\n", *resetUser, password)
		return
	}

	cert, err := tlscert.Load(*tlsCert, *tlsKey, *tlsDir)
	if err != nil {
		log.Fatalf("failed to load TLS certificate: %v", err)
	}
	log.Printf("TLS certificate SHA-256 fingerprint: %s", tlscert.Fingerprint(cert))

	serverStore := servers.NewStore(db)
	friendStore := friends.NewStore(db)
	msgStore := messages.NewStore(db)

	listenerTCP, err := net.Listen("tcp", *tcpAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer listenerTCP.Close()

	listenerUDP, err := net.ListenPacket("udp", *udpAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer listenerUDP.Close()

	// Server ready: both listeners are open, so clients can connect now.
	log.Printf("feather-chat server ready: control=%s media=%s", *tcpAddr, *udpAddr)
	rooms := room.NewManager()
	go rooms.ServeUDP(listenerUDP.(*net.UDPConn))
	control.New(users, serverStore, friendStore, msgStore, rooms).Serve(listenerTCP, tlscert.Config(cert))
}
