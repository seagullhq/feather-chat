// Command server runs the feather-chat server: a TCP control plane plus a
// UDP media plane over PostgreSQL.
package main

import (
	"context"
	"database/sql"
	"flag"
	"log"
	"net"
	"time"

	"github.com/joho/godotenv"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/config"
	"github.com/seagullhq/feather-chat/server/internal/control"
	"github.com/seagullhq/feather-chat/server/internal/migrations"
	"github.com/seagullhq/feather-chat/server/internal/room"
	"github.com/seagullhq/feather-chat/server/internal/servers"

	// Driver PostgreSQL tramite pgx compatibile con database/sql
	_ "github.com/jackc/pgx/v5/stdlib"
)

func main() {
	if err := godotenv.Load(); err != nil {
		if err2 := godotenv.Load("../.env"); err2 != nil {
			log.Printf(".env not loaded (%v); using environment only", err)
		}
	}

	pgURI := flag.String("postgres", config.PostgresURI(), "PostgreSQL connection URI")

	// Define tcp and udp address
	tcpAddr := flag.String("tcp", ":7700", "TCP control address")
	udpAddr := flag.String("udp", ":7701", "UDP media address")
	flag.Parse()

	// 1. Connessione a PostgreSQL usando il driver pgx
	db, err := sql.Open("pgx", *pgURI)
	if err != nil {
		log.Fatalf("Errore apertura connessione DB: %v", err)
	}
	defer db.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	// Verifica la connessione
	if err := db.PingContext(ctx); err != nil {
		log.Fatalf("postgres ping fallito: %v", err)
	}
	log.Println("Connesso a PostgreSQL con successo")

	// 2. Esegui le migrazioni
	applied, err := migrations.Up(db)
	if err != nil {
		log.Fatalf("Errore esecuzione migrazioni: %v", err)
	}
	if applied {
		log.Println("Migrazioni applicate con successo!")
	} else {
		log.Println("Nessuna nuova migrazione da applicare.")
	}

	// 3. Inizializza gli store passandogli l'istanza *sql.DB
	users := accounts.NewStore(db)
	serverStore := servers.NewStore(db)

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

	// Server starting
	log.Printf("feather-chat server: control=%s media=%s", *tcpAddr, *udpAddr)
	rooms := room.NewManager()
	go rooms.ServeUDP(listenerUDP.(*net.UDPConn))
	control.New(users, serverStore, rooms).Serve(listenerTCP)
}
